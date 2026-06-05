//! RDS (Radio Data System) protocol decoder.
//!
//! Decodes RDS metadata from FM composite baseband at 192 kHz.
//! Chain: 57 kHz subcarrier extraction → BPSK clock recovery → differential decode
//!        → block sync + CRC-10 → group parser (0A/0B for PS, 2A/2B for RT).

use rf_liquid::RealFirFilter;

/// RDS block offset words (XOR'd into CRC per block position).
const OFFSET_A:  u16 = 0x0FC;
const OFFSET_B:  u16 = 0x198;
const OFFSET_C:  u16 = 0x168;
const OFFSET_CP: u16 = 0x350; // C' for type B groups
const OFFSET_D:  u16 = 0x1B4;

/// PTY code to name lookup (North America RBDS).
const PTY_NAMES: [&str; 32] = [
    "None", "News", "Information", "Sports",
    "Talk", "Rock", "Classic Rock", "Adult Hits",
    "Soft Rock", "Top 40", "Country", "Oldies",
    "Soft", "Nostalgia", "Jazz", "Classical",
    "R&B", "Soft R&B", "Language", "Religious Music",
    "Religious Talk", "Personality", "Public", "College",
    "Spanish Talk", "Spanish Music", "Hip Hop", "Unassigned",
    "Unassigned", "Weather", "Emergency Test", "Emergency",
];

/// Decoded RDS metadata snapshot.
#[derive(Debug, Clone)]
pub struct RdsMetadata {
    pub pi: u16,
    pub ps: String,
    pub rt: String,
    pub pty: u8,
    pub pty_name: &'static str,
}

/// RDS protocol decoder.
///
/// Feed FM-demodulated composite audio at 192 kHz via `process()`.
/// Check `take_update()` after each call for new metadata.
pub struct RdsDecoder {
    // Subcarrier extraction NCO tables (57 kHz at 192 kHz sample rate)
    nco_cos: Vec<f32>,
    nco_sin: Vec<f32>,
    nco_phase_idx: usize,
    nco_table_len: usize,

    // LPF for I/Q baseband (~2.4 kHz cutoff at 192 kHz)
    lpf_i: RealFirFilter,
    lpf_q: RealFirFilter,

    // Decimation: 192 kHz → 19.2 kHz (10:1) to ease clock recovery
    decim_factor: usize,
    decim_counter: usize,

    // Clock recovery (Gardner TED)
    samples_per_symbol: f32,
    symbol_counter: f32,
    prev_sample: f32,      // previous decision-point sample
    mid_sample: f32,       // mid-symbol sample (between decisions)
    prev_decision: f32,    // previous hard decision (+1/-1)
    got_mid: bool,         // whether we captured a mid-symbol sample this interval

    // Differential BPSK
    last_bit: bool,

    // Block sync + assembly
    shift_reg: u32,       // 26-bit sliding window
    bits_since_sync: u32,
    block_idx: usize,     // 0..3 within group (A, B, C/C', D)
    synced: bool,
    sync_miss_count: u32,
    group_buf: [u16; 4],  // 4 data words per group

    // Decoded metadata
    pi: u16,
    ps: [u8; 8],
    ps_seg_mask: u8,      // which 2-char segments we've received
    rt: [u8; 64],
    rt_seg_mask: u64,     // which segments we've received
    rt_ab: bool,          // A/B flag for RT clear
    pty: u8,
    updated: bool,

    // Diagnostics
    total_bits: u64,
    good_blocks: u64,
    bad_blocks: u64,

    // Internal scratch buffers
    mix_i_buf: Vec<f32>,
    mix_q_buf: Vec<f32>,
    filt_i_buf: Vec<f32>,
    filt_q_buf: Vec<f32>,
}

impl RdsDecoder {
    /// Create a new RDS decoder for the given composite sample rate (should be 192000).
    pub fn new(composite_sample_rate: f32) -> Self {
        // Precompute one full cycle of 57 kHz NCO at the sample rate.
        // Period = sample_rate / gcd(sample_rate, 57000).
        // gcd(192000, 57000) = 3000, so table_len = 192000/3000 = 64
        let table_len = (composite_sample_rate / gcd_f32(composite_sample_rate, 57000.0)) as usize;
        let table_len = table_len.min(19200);

        let mut nco_cos = Vec::with_capacity(table_len);
        let mut nco_sin = Vec::with_capacity(table_len);
        for n in 0..table_len {
            let phase = 2.0 * std::f32::consts::PI * 57000.0 * (n as f32) / composite_sample_rate;
            nco_cos.push(phase.cos());
            nco_sin.push(phase.sin());
        }

        // LPF: 2.4 kHz cutoff at 192 kHz → normalized = 2400 / 192000 = 0.0125
        // 128 taps for sharper rolloff, 60 dB stopband
        let lpf_i = RealFirFilter::new_kaiser(128, 0.0125, 60.0, 0.0);
        let lpf_q = RealFirFilter::new_kaiser(128, 0.0125, 60.0, 0.0);

        // Decimate 10:1 → 19.2 kHz
        let decim_factor = 10;
        let post_decim_rate = composite_sample_rate / decim_factor as f32;
        let samples_per_symbol = post_decim_rate / 1187.5;

        tracing::info!(
            "RDS decoder: table_len={}, decim={}:1, post_rate={}, sps={:.2}",
            table_len, decim_factor, post_decim_rate, samples_per_symbol
        );

        Self {
            nco_cos,
            nco_sin,
            nco_phase_idx: 0,
            nco_table_len: table_len,
            lpf_i,
            lpf_q,
            decim_factor,
            decim_counter: 0,
            samples_per_symbol,
            symbol_counter: 0.0,
            prev_sample: 0.0,
            mid_sample: 0.0,
            prev_decision: 1.0,
            got_mid: false,
            last_bit: false,
            shift_reg: 0,
            bits_since_sync: 0,
            block_idx: 0,
            synced: false,
            sync_miss_count: 0,
            group_buf: [0; 4],
            pi: 0,
            ps: [b' '; 8],
            ps_seg_mask: 0,
            rt: [b' '; 64],
            rt_seg_mask: 0,
            rt_ab: false,
            pty: 0,
            updated: false,
            total_bits: 0,
            good_blocks: 0,
            bad_blocks: 0,
            mix_i_buf: Vec::new(),
            mix_q_buf: Vec::new(),
            filt_i_buf: Vec::new(),
            filt_q_buf: Vec::new(),
        }
    }

    /// Feed FM-demodulated composite audio (real f32 at 192 kHz).
    pub fn process(&mut self, composite: &[f32]) {
        let n = composite.len();
        if n == 0 { return; }

        // Ensure scratch buffers are large enough
        if self.mix_i_buf.len() < n {
            self.mix_i_buf.resize(n, 0.0);
            self.mix_q_buf.resize(n, 0.0);
            self.filt_i_buf.resize(n, 0.0);
            self.filt_q_buf.resize(n, 0.0);
        }

        // 1. Multiply by cos/sin(2π·57000·t) to bring RDS subcarrier to baseband
        for i in 0..n {
            let idx = self.nco_phase_idx % self.nco_table_len;
            self.mix_i_buf[i] = composite[i] * self.nco_cos[idx] * 2.0; // *2 for mixer gain
            self.mix_q_buf[i] = composite[i] * self.nco_sin[idx] * 2.0;
            self.nco_phase_idx += 1;
            if self.nco_phase_idx >= self.nco_table_len {
                self.nco_phase_idx = 0;
            }
        }

        // 2. LPF to extract ±2.4 kHz around subcarrier
        self.lpf_i.execute_block(&self.mix_i_buf[..n], &mut self.filt_i_buf[..n]);
        self.lpf_q.execute_block(&self.mix_q_buf[..n], &mut self.filt_q_buf[..n]);

        // 3. Decimate and do clock recovery on I channel
        for i in 0..n {
            self.decim_counter += 1;
            if self.decim_counter < self.decim_factor {
                continue;
            }
            self.decim_counter = 0;

            let sample = self.filt_i_buf[i];
            self.clock_recover(sample);
        }
    }

    /// Gardner timing error detector with clock recovery and bit decision.
    fn clock_recover(&mut self, sample: f32) {
        self.symbol_counter += 1.0;

        // Capture mid-symbol sample for Gardner TED
        let half = self.samples_per_symbol / 2.0;
        if !self.got_mid && self.symbol_counter >= half && self.symbol_counter < half + 1.0 {
            self.mid_sample = sample;
            self.got_mid = true;
        }

        if self.symbol_counter >= self.samples_per_symbol {
            self.symbol_counter -= self.samples_per_symbol;

            // Hard decision on current sample
            let decision = if sample >= 0.0 { 1.0_f32 } else { -1.0 };

            // Gardner timing error detector:
            // e = mid_sample * (prev_decision - decision)
            if self.got_mid {
                let error = self.mid_sample * (self.prev_decision - decision);
                // Loop filter: adjust symbol counter phase
                let loop_gain = 0.005;
                self.symbol_counter += error * loop_gain;
                // Clamp adjustment
                self.symbol_counter = self.symbol_counter.clamp(-2.0, self.samples_per_symbol);
            }

            self.prev_sample = sample;
            self.prev_decision = decision;
            self.got_mid = false;

            // BPSK bit: positive = 1, negative = 0
            let raw_bit = decision > 0.0;

            // Differential decode: RDS uses differential encoding
            // data_bit = d(n) XOR d(n-1)
            let data_bit = raw_bit ^ self.last_bit;
            self.last_bit = raw_bit;

            self.push_bit(data_bit);
        }
    }

    /// Push one decoded bit into the block assembler.
    fn push_bit(&mut self, bit: bool) {
        self.shift_reg = ((self.shift_reg << 1) | (bit as u32)) & 0x03FF_FFFF; // 26 bits
        self.total_bits += 1;
        self.bits_since_sync += 1;

        if self.synced {
            // We're synced — collect 26 bits per block
            if self.bits_since_sync >= 26 {
                self.bits_since_sync = 0;
                let block = self.shift_reg;
                let info = ((block >> 10) & 0xFFFF) as u16;

                // Check CRC with expected offset for this block position
                let expected_offset = match self.block_idx {
                    0 => OFFSET_A,
                    1 => OFFSET_B,
                    3 => OFFSET_D,
                    2 => {
                        // Block C can be either C or C' — check both
                        let check = (block & 0x3FF) as u16;
                        let crc = compute_crc(info);
                        if (crc ^ check) == OFFSET_C {
                            OFFSET_C
                        } else {
                            OFFSET_CP
                        }
                    }
                    _ => OFFSET_A,
                };

                let check = (block & 0x3FF) as u16;
                let crc = compute_crc(info);
                let block_ok = (crc ^ check) == expected_offset;

                if block_ok {
                    self.group_buf[self.block_idx] = info;
                    self.good_blocks += 1;
                    self.block_idx += 1;
                    self.sync_miss_count = 0;

                    if self.block_idx >= 4 {
                        self.decode_group();
                        self.block_idx = 0;
                    }
                } else {
                    self.bad_blocks += 1;
                    self.sync_miss_count += 1;
                    if self.sync_miss_count > 10 {
                        tracing::debug!(
                            "RDS: lost sync after {} good, {} bad blocks",
                            self.good_blocks, self.bad_blocks
                        );
                        self.synced = false;
                        self.block_idx = 0;
                    } else {
                        // Skip this group, restart from block A
                        self.block_idx = 0;
                    }
                }
            }
        } else {
            // Not synced — scan every bit position for a valid block A
            if self.total_bits >= 26 {
                let block = self.shift_reg;
                let info = ((block >> 10) & 0xFFFF) as u16;
                let check = (block & 0x3FF) as u16;
                let crc = compute_crc(info);

                if (crc ^ check) == OFFSET_A {
                    // Found block A — enter sync
                    self.synced = true;
                    self.group_buf[0] = info;
                    self.block_idx = 1;
                    self.bits_since_sync = 0;
                    self.sync_miss_count = 0;
                    tracing::info!(
                        "RDS: sync acquired (PI=0x{:04X}) after {} bits",
                        info, self.total_bits
                    );
                }
            }
        }
    }

    /// Decode a complete RDS group (4 blocks = 4 × 16-bit data words).
    fn decode_group(&mut self) {
        let pi = self.group_buf[0];
        let b = self.group_buf[1];
        let c = self.group_buf[2];
        let d = self.group_buf[3];

        let group_type = (b >> 12) & 0x0F;
        let version = (b >> 11) & 0x01; // 0=A, 1=B
        let pty = ((b >> 5) & 0x1F) as u8;

        // Always update PI and PTY
        if pi != 0 {
            if self.pi != pi || self.pty != pty {
                self.updated = true;
            }
            self.pi = pi;
            self.pty = pty;
        }

        match (group_type, version) {
            // Type 0A/0B: Basic tuning + PS name
            (0, _) => {
                let seg = (b & 0x03) as usize; // 0-3, each carries 2 chars
                let c1 = (d >> 8) as u8;
                let c2 = (d & 0xFF) as u8;
                let pos = seg * 2;
                if pos + 1 < 8 {
                    let c1 = sanitize_rds_char(c1);
                    let c2 = sanitize_rds_char(c2);
                    if self.ps[pos] != c1 || self.ps[pos + 1] != c2 {
                        self.ps[pos] = c1;
                        self.ps[pos + 1] = c2;
                        self.ps_seg_mask |= 1 << seg;
                        self.updated = true;
                        tracing::debug!(
                            "RDS PS seg {}: '{}{}'  mask=0x{:02X}",
                            seg, c1 as char, c2 as char, self.ps_seg_mask
                        );
                    }
                }
            }
            // Type 2A: Radio Text (64 chars, 4 chars per group)
            (2, 0) => {
                let ab = (b >> 4) & 0x01;
                let seg = (b & 0x0F) as usize;
                // A/B flag toggle means clear RT
                if (ab == 1) != self.rt_ab {
                    self.rt_ab = ab == 1;
                    self.rt = [b' '; 64];
                    self.rt_seg_mask = 0;
                }
                let pos = seg * 4;
                if pos + 3 < 64 {
                    let chars = [
                        sanitize_rds_char((c >> 8) as u8),
                        sanitize_rds_char((c & 0xFF) as u8),
                        sanitize_rds_char((d >> 8) as u8),
                        sanitize_rds_char((d & 0xFF) as u8),
                    ];
                    for (j, &ch) in chars.iter().enumerate() {
                        if self.rt[pos + j] != ch {
                            self.rt[pos + j] = ch;
                            self.updated = true;
                        }
                    }
                    self.rt_seg_mask |= 1 << seg;
                }
            }
            // Type 2B: Radio Text (32 chars, 2 chars per group)
            (2, 1) => {
                let ab = (b >> 4) & 0x01;
                let seg = (b & 0x0F) as usize;
                if (ab == 1) != self.rt_ab {
                    self.rt_ab = ab == 1;
                    self.rt = [b' '; 64];
                    self.rt_seg_mask = 0;
                }
                let pos = seg * 2;
                if pos + 1 < 64 {
                    let c1 = sanitize_rds_char((d >> 8) as u8);
                    let c2 = sanitize_rds_char((d & 0xFF) as u8);
                    if self.rt[pos] != c1 || self.rt[pos + 1] != c2 {
                        self.rt[pos] = c1;
                        self.rt[pos + 1] = c2;
                        self.updated = true;
                    }
                    self.rt_seg_mask |= 1 << seg;
                }
            }
            _ => {}
        }
    }

    /// Take the current RDS metadata if it has been updated since last call.
    /// Resets the update flag.
    pub fn take_update(&mut self) -> Option<RdsMetadata> {
        if !self.updated {
            return None;
        }
        self.updated = false;

        // Only emit if we have at least PI
        if self.pi == 0 {
            return None;
        }

        let ps = if self.ps_seg_mask != 0 {
            String::from_utf8_lossy(&self.ps).trim().to_string()
        } else {
            String::new()
        };

        let rt = if self.rt_seg_mask != 0 {
            let end = self.rt.iter().position(|&c| c == b'\r').unwrap_or(64);
            String::from_utf8_lossy(&self.rt[..end]).trim().to_string()
        } else {
            String::new()
        };

        tracing::info!(
            "RDS update: PI=0x{:04X} PS='{}' PTY={} RT='{}'",
            self.pi, ps, self.pty, &rt[..rt.len().min(40)]
        );

        Some(RdsMetadata {
            pi: self.pi,
            ps,
            rt,
            pty: self.pty,
            pty_name: PTY_NAMES[self.pty as usize & 0x1F],
        })
    }

    /// Reset decoder state (call on frequency change).
    pub fn reset(&mut self) {
        self.nco_phase_idx = 0;
        self.lpf_i.reset();
        self.lpf_q.reset();
        self.decim_counter = 0;
        self.symbol_counter = 0.0;
        self.prev_sample = 0.0;
        self.mid_sample = 0.0;
        self.prev_decision = 1.0;
        self.got_mid = false;
        self.last_bit = false;
        self.shift_reg = 0;
        self.bits_since_sync = 0;
        self.block_idx = 0;
        self.synced = false;
        self.sync_miss_count = 0;
        self.group_buf = [0; 4];
        self.pi = 0;
        self.ps = [b' '; 8];
        self.ps_seg_mask = 0;
        self.rt = [b' '; 64];
        self.rt_seg_mask = 0;
        self.rt_ab = false;
        self.pty = 0;
        self.updated = false;
        self.total_bits = 0;
        self.good_blocks = 0;
        self.bad_blocks = 0;
    }
}

/// Compute RDS CRC-10 for a 16-bit info word.
/// Generator: g(x) = x^10 + x^8 + x^7 + x^5 + x^4 + x^3 + 1 = 0x5B9 (11 bits).
fn compute_crc(info: u16) -> u16 {
    // Pad info with 10 zero bits, then divide by g(x)
    let mut reg: u32 = (info as u32) << 10;
    for i in (10..26).rev() {
        if (reg >> i) & 1 != 0 {
            reg ^= 0x5B9 << (i - 10);
        }
    }
    (reg & 0x3FF) as u16
}

/// Sanitize an RDS character byte — replace non-printable with space.
fn sanitize_rds_char(c: u8) -> u8 {
    if c >= 0x20 && c < 0x7F {
        c
    } else {
        b' '
    }
}

/// GCD helper for integer values (used for NCO table sizing).
fn gcd_f32(a: f32, b: f32) -> f32 {
    let mut a = a as u32;
    let mut b = b as u32;
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a as f32
}
