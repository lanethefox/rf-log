use num_complex::Complex32;
use rf_liquid::{AmpModem, AmpModemType, FirFilter, FreqDemod, Nco, Resampler, RealResampler};
use crate::rds::{RdsDecoder, RdsMetadata};
use crate::cqpsk::CqpskDemod;
use crate::p25_baseband::{DcBlocker, BasebandFir, rrc_coefficients};
use crate::fingerprint::{RfFingerprint, FingerprintAccumulator};
use rf_p25::{P25Decoder, P25Result, P25Metadata, TsbkPayload};

/// Demodulation mode for the monitor pipeline.
#[derive(Debug, Clone, PartialEq)]
pub enum DemodMode {
    Nfm,
    Wfm,
    Am,
    Usb,
    Lsb,
    P25,
}

impl DemodMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "NFM" => DemodMode::Nfm,
            "WFM" => DemodMode::Wfm,
            "AM" => DemodMode::Am,
            "USB" => DemodMode::Usb,
            "LSB" => DemodMode::Lsb,
            "P25" => DemodMode::P25,
            _ => DemodMode::Nfm,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            DemodMode::Nfm => "NFM",
            DemodMode::Wfm => "WFM",
            DemodMode::Am => "AM",
            DemodMode::Usb => "USB",
            DemodMode::Lsb => "LSB",
            DemodMode::P25 => "P25",
        }
    }

    /// Normalized filter cutoff for this mode.
    /// For WFM at 192 kHz: 0.33 ≈ 63 kHz (passes stereo pilot + RDS subcarrier).
    /// For other modes at 48 kHz: narrower cutoffs.
    fn filter_cutoff(&self) -> f32 {
        match self {
            DemodMode::Nfm => 0.13,  // ~6.25 kHz at 48 kHz
            DemodMode::Wfm => 0.33,  // ~63 kHz at 192 kHz (passes 57 kHz RDS)
            DemodMode::Am => 0.10,   // ~4.8 kHz at 48 kHz
            DemodMode::Usb => 0.06,  // ~2.9 kHz at 48 kHz
            DemodMode::Lsb => 0.06,
            DemodMode::P25 => 0.26,  // ~12.5 kHz at 48 kHz (P25 C4FM bandwidth)
        }
    }

    /// Decimated sample rate for this mode.
    fn decimated_rate(&self) -> f32 {
        match self {
            DemodMode::Wfm => 192_000.0,
            _ => 48_000.0, // P25: 4800 baud = 10 samples/symbol at 48 kHz
        }
    }
}

/// Demodulator backend — wraps FreqDemod or AmpModem.
enum Demodulator {
    Fm(FreqDemod),
    Am(AmpModem),
}

impl Demodulator {
    fn new(mode: &DemodMode) -> Self {
        match mode {
            DemodMode::Nfm => Demodulator::Fm(FreqDemod::new(0.5)),
            DemodMode::Wfm => Demodulator::Fm(FreqDemod::new(0.8)),
            DemodMode::Am => Demodulator::Am(AmpModem::new(0.8, AmpModemType::Dsb, false)),
            DemodMode::Usb => Demodulator::Am(AmpModem::new(0.8, AmpModemType::Usb, true)),
            DemodMode::Lsb => Demodulator::Am(AmpModem::new(0.8, AmpModemType::Lsb, true)),
            DemodMode::P25 => Demodulator::Fm(FreqDemod::new(0.35)), // C4FM ±1.8 kHz deviation
        }
    }

    fn demod(&self, input: &[Complex32], output: &mut [f32]) {
        match self {
            Demodulator::Fm(d) => d.demod_block(input, output),
            Demodulator::Am(d) => d.demod_block(input, output),
        }
    }
}

/// Upsample 8 kHz PCM to 48 kHz using a RealResampler.
/// Returns an empty slice if input is empty or resampler is None.
fn upsample_8k_48k<'a>(
    pcm_8k: &[f32],
    buf: &'a mut Vec<f32>,
    resamp: &Option<RealResampler>,
) -> &'a [f32] {
    if pcm_8k.is_empty() {
        return &[];
    }
    if let Some(resamp) = resamp.as_ref() {
        let out_max = pcm_8k.len() * 6 + 64;
        if buf.len() < out_max {
            buf.resize(out_max, 0.0);
        }
        let n_audio = resamp.execute(pcm_8k, buf);
        return &buf[..n_audio];
    }
    &[]
}

/// The MonitorPipeline chains: NCO → Resampler → FirFilter → Squelch → Demodulator.
///
/// For WFM mode, uses 192 kHz intermediate rate to preserve the 57 kHz RDS subcarrier:
///   IQ (2.4M) → NCO → Resamp(192k) → Filter → Squelch → FM demod → composite (192k)
///     → RDS decoder (57 kHz extraction + BPSK decode)
///     → RealResampler (192k → 48k) → audio output
///
/// For NFM/AM/SSB, decimates directly to 48 kHz as before.
pub struct MonitorPipeline {
    nco: Nco,
    resampler: Resampler,
    filter: FirFilter,
    demod: Demodulator,
    mode: DemodMode,
    sample_rate: f32,

    // Squelch
    squelch_threshold: f32,
    squelch_open: bool,
    power_avg: f32,
    hang_counter: u32,
    hang_time: u32, // samples of audio to hold open after squelch closes

    // Internal buffers (reused across calls to avoid allocation)
    mixed_buf: Vec<Complex32>,
    resamp_buf: Vec<Complex32>,
    filtered_buf: Vec<Complex32>,
    audio_buf: Vec<f32>,

    // WFM-specific: composite demod output + audio decimation + RDS
    wfm_composite_buf: Vec<f32>,
    wfm_audio_resamp: Option<RealResampler>,
    wfm_audio_buf: Vec<f32>,
    rds_decoder: Option<RdsDecoder>,

    // P25-specific: CQPSK demod + digital voice decoder + 8k→48k resampler
    cqpsk_demod: Option<CqpskDemod>,
    p25_decoder: Option<P25Decoder>,
    p25_audio_resamp: Option<RealResampler>,
    p25_audio_buf: Vec<f32>,

    // P25 C4FM baseband conditioning (DC blocker + RRC matched filter)
    p25_dc_blocker: Option<DcBlocker>,
    p25_rrc: Option<BasebandFir>,

    // RF fingerprint extraction
    fp_accum: FingerprintAccumulator,
    fp_was_open: bool,
    fp_result: Option<RfFingerprint>,

    // P25 pipeline diagnostics
    pub p25_dibits_fed: u64,
    pub p25_events_out: u64,
    pub p25_tsbks_decoded: u64,
    p25_demod_min: f32,
    p25_demod_max: f32,
    p25_demod_sum: f64,
    p25_demod_count: u64,
}

impl MonitorPipeline {
    /// Create a new monitor pipeline.
    /// - `sample_rate`: SDR sample rate (e.g., 2_400_000.0)
    /// - `target_freq`: desired frequency in Hz
    /// - `center_freq`: current SDR center frequency in Hz
    /// - `mode`: demodulation mode string ("NFM", "WFM", "AM", "USB", "LSB")
    /// - `squelch_threshold`: squelch level in dB (e.g., -60.0)
    pub fn new(
        sample_rate: f64,
        target_freq: f64,
        center_freq: f64,
        mode: &str,
        squelch_threshold: f64,
    ) -> Self {
        let sample_rate_f = sample_rate as f32;
        let offset = (target_freq - center_freq) as f32;
        let nco_freq = 2.0 * std::f32::consts::PI * offset / sample_rate_f;

        let nco = Nco::new(nco_freq);

        let demod_mode = DemodMode::from_str(mode);

        // Decimate to mode-specific rate (192 kHz for WFM, 48 kHz otherwise)
        let decim_rate_target = demod_mode.decimated_rate();
        let decim_rate = decim_rate_target / sample_rate_f;
        let resampler = Resampler::new(decim_rate, 60.0);

        // Channel filter at decimated rate
        let filter = FirFilter::new_kaiser(64, demod_mode.filter_cutoff(), 60.0, 0.0);

        let demod = Demodulator::new(&demod_mode);

        // Pre-allocate buffers for a typical chunk size
        let max_input = 4096;
        let max_output = ((max_input as f32) * decim_rate).ceil() as usize + 64;

        // WFM-specific: second-stage resampler (192k → 48k) and RDS decoder
        let (wfm_audio_resamp, rds_decoder, wfm_composite_buf, wfm_audio_buf) =
            if demod_mode == DemodMode::Wfm {
                let resamp = RealResampler::new(48_000.0 / 192_000.0, 60.0);
                let rds = RdsDecoder::new(192_000.0);
                let wfm_max = max_output; // composite at 192 kHz
                let audio_max = ((wfm_max as f32) * 0.25).ceil() as usize + 64;
                (
                    Some(resamp),
                    Some(rds),
                    vec![0.0_f32; wfm_max],
                    vec![0.0_f32; audio_max],
                )
            } else {
                (None, None, Vec::new(), Vec::new())
            };

        // P25-specific: voice decoder + 8k→48k upsample resampler
        // C4FM path: FM demod → DC blocker → RRC matched filter → P25Decoder::feed(f32).
        let (cqpsk_demod, p25_decoder, p25_audio_resamp, p25_audio_buf, p25_dc_blocker, p25_rrc) =
            if demod_mode == DemodMode::P25 {
                let resamp = RealResampler::new(48_000.0 / 8_000.0, 60.0);
                let audio_max = max_output * 6 + 64; // 6x upsample from 8k→48k
                // DC blocker: ~5 Hz cutoff at 48 kHz — removes FM demod DC offset
                let dc = DcBlocker::new(5.0, 48_000.0);
                // RRC matched filter: α=0.2, 10 sps, 51 taps (±2.5 symbols)
                let rrc_taps = rrc_coefficients(10, 0.2, 51);
                let rrc = BasebandFir::new(rrc_taps);
                (
                    None, // C4FM uses FM demod, not CQPSK
                    Some(P25Decoder::new()),
                    Some(resamp),
                    vec![0.0_f32; audio_max],
                    Some(dc),
                    Some(rrc),
                )
            } else {
                (None, None, None, Vec::new(), None, None)
            };

        Self {
            nco,
            resampler,
            filter,
            demod,
            mode: demod_mode,
            sample_rate: sample_rate_f,
            squelch_threshold: squelch_threshold as f32,
            squelch_open: false,
            power_avg: -120.0,
            hang_counter: 0,
            hang_time: 240, // ~5ms at 48 kHz
            mixed_buf: vec![Complex32::new(0.0, 0.0); max_input],
            resamp_buf: vec![Complex32::new(0.0, 0.0); max_output],
            filtered_buf: vec![Complex32::new(0.0, 0.0); max_output],
            audio_buf: vec![0.0; max_output],
            wfm_composite_buf,
            wfm_audio_resamp,
            wfm_audio_buf,
            rds_decoder,
            cqpsk_demod,
            p25_decoder,
            p25_audio_resamp,
            p25_audio_buf,
            p25_dc_blocker,
            p25_rrc,
            fp_accum: FingerprintAccumulator::new(),
            fp_was_open: false,
            fp_result: None,
            p25_dibits_fed: 0,
            p25_events_out: 0,
            p25_tsbks_decoded: 0,
            p25_demod_min: f32::MAX,
            p25_demod_max: f32::MIN,
            p25_demod_sum: 0.0,
            p25_demod_count: 0,
        }
    }

    /// Process a chunk of raw IQ samples.
    /// Returns a slice of audio f32 samples at 48 kHz, or an empty slice if squelched.
    pub fn process(&mut self, iq: &[Complex32]) -> &[f32] {
        let n = iq.len();

        // Ensure buffers are large enough
        if self.mixed_buf.len() < n {
            self.mixed_buf.resize(n, Complex32::new(0.0, 0.0));
        }
        let max_out = ((n as f32) * self.resampler.rate()).ceil() as usize + 64;
        if self.resamp_buf.len() < max_out {
            self.resamp_buf.resize(max_out, Complex32::new(0.0, 0.0));
            self.filtered_buf.resize(max_out, Complex32::new(0.0, 0.0));
            self.audio_buf.resize(max_out, 0.0);
        }

        // 1. NCO mix down to baseband
        self.nco.mix_block_down(iq, &mut self.mixed_buf[..n]);

        // 2. Decimate to target rate (48 kHz or 192 kHz for WFM)
        let n_out = self.resampler.execute(
            &self.mixed_buf[..n],
            &mut self.resamp_buf,
        );

        if n_out == 0 {
            return &[];
        }

        // 3. Channel filter
        self.filter.execute_block(
            &self.resamp_buf[..n_out],
            &mut self.filtered_buf[..n_out],
        );

        // 4. Mode-specific demodulation
        match self.mode {
            DemodMode::P25 => self.process_p25(n_out),
            DemodMode::Wfm => self.process_wfm(n_out),
            _ => self.process_analog(n_out),
        }
    }

    /// P25 C4FM path: FM demod → DC blocker → RRC → digital decode → 8k→48k upsample.
    fn process_p25(&mut self, n_out: usize) -> &[f32] {
        let power = self.measure_power(&self.filtered_buf[..n_out]);
        self.update_squelch(power);
        self.accumulate_fingerprint(&self.filtered_buf[..n_out].to_vec());

        self.demod.demod(&self.filtered_buf[..n_out], &mut self.audio_buf[..n_out]);

        // Track FM demod output statistics for diagnostics
        for i in 0..n_out {
            let v = self.audio_buf[i];
            if v < self.p25_demod_min { self.p25_demod_min = v; }
            if v > self.p25_demod_max { self.p25_demod_max = v; }
            self.p25_demod_sum += v as f64;
            self.p25_demod_count += 1;
        }

        // Extract CFO from FM demod DC component (before DC blocker removes it)
        // FM demod output mean ≈ carrier offset; convert via: mean * decimated_rate * kf
        if self.squelch_open && n_out > 0 {
            let demod_mean = self.audio_buf[..n_out].iter()
                .map(|&s| s as f64).sum::<f64>() / n_out as f64;
            let cfo_hz = demod_mean * self.mode.decimated_rate() as f64 * 0.35;
            self.fp_accum.feed_cfo(cfo_hz);
        }

        // Apply DC blocker + RRC matched filter
        let dc = self.p25_dc_blocker.as_mut().unwrap();
        let rrc = self.p25_rrc.as_mut().unwrap();
        for i in 0..n_out {
            let dc_removed = dc.process(self.audio_buf[i]);
            self.audio_buf[i] = rrc.process(dc_removed);
        }

        let decoder = self.p25_decoder.as_mut().unwrap();
        let mut pcm_8k = Vec::new();
        for i in 0..n_out {
            self.p25_dibits_fed += 1;
            if let P25Result::Audio(frame) = decoder.feed(self.audio_buf[i]) {
                self.p25_events_out += 1;
                pcm_8k.extend_from_slice(&frame);
            }
        }
        self.p25_tsbks_decoded += decoder.peek_tsbk_count() as u64;

        upsample_8k_48k(&pcm_8k, &mut self.p25_audio_buf, &self.p25_audio_resamp)
    }

    /// WFM path: FM demod at 192 kHz → RDS + audio downsample to 48 kHz.
    fn process_wfm(&mut self, n_out: usize) -> &[f32] {
        let power = self.measure_power(&self.filtered_buf[..n_out]);
        self.update_squelch(power);

        // Always demod + feed RDS (even when squelched)
        if self.wfm_composite_buf.len() < n_out {
            self.wfm_composite_buf.resize(n_out, 0.0);
        }
        self.demod.demod(&self.filtered_buf[..n_out], &mut self.wfm_composite_buf[..n_out]);

        if let Some(rds) = self.rds_decoder.as_mut() {
            rds.process(&self.wfm_composite_buf[..n_out]);
        }

        if !self.squelch_open {
            return &[];
        }

        // Decimate composite 192k → 48k audio
        if let Some(resamp) = self.wfm_audio_resamp.as_ref() {
            let audio_max = ((n_out as f32) * 0.25).ceil() as usize + 64;
            if self.wfm_audio_buf.len() < audio_max {
                self.wfm_audio_buf.resize(audio_max, 0.0);
            }
            let n_audio = resamp.execute(
                &self.wfm_composite_buf[..n_out],
                &mut self.wfm_audio_buf,
            );
            return &self.wfm_audio_buf[..n_audio];
        }

        &[]
    }

    /// Analog path (NFM/AM/SSB): squelch gate → demod at 48 kHz.
    fn process_analog(&mut self, n_out: usize) -> &[f32] {
        let power = self.measure_power(&self.filtered_buf[..n_out]);
        self.update_squelch(power);
        self.accumulate_fingerprint(&self.filtered_buf[..n_out].to_vec());

        if !self.squelch_open {
            return &[];
        }

        self.demod.demod(&self.filtered_buf[..n_out], &mut self.audio_buf[..n_out]);

        // Extract CFO from FM demod DC component (NFM only — AM/SSB have no carrier offset)
        if n_out > 0 && (self.mode == DemodMode::Nfm || self.mode == DemodMode::Wfm) {
            let kf = if self.mode == DemodMode::Nfm { 0.5 } else { 0.8 };
            let demod_mean = self.audio_buf[..n_out].iter()
                .map(|&s| s as f64).sum::<f64>() / n_out as f64;
            let cfo_hz = demod_mean * self.mode.decimated_rate() as f64 * kf;
            self.fp_accum.feed_cfo(cfo_hz);
        }

        &self.audio_buf[..n_out]
    }

    /// Update the target frequency (re-tunes the NCO).
    /// Also resets digital demodulators (CQPSK, P25) since they carry
    /// carrier/timing state that is invalid at the new frequency.
    pub fn set_frequency(&mut self, target_freq: f64, center_freq: f64) {
        let offset = (target_freq - center_freq) as f32;
        let nco_freq = 2.0 * std::f32::consts::PI * offset / self.sample_rate;
        self.nco.set_frequency(nco_freq);

        // Reset CQPSK demod — stale carrier/timing state would delay re-acquisition
        if let Some(ref mut cqpsk) = self.cqpsk_demod {
            cqpsk.reset();
        }
        // Reset P25 decoder frame sync state and baseband filters
        if let Some(ref mut p25) = self.p25_decoder {
            p25.resync();
        }
        if let Some(ref mut dc) = self.p25_dc_blocker {
            dc.reset();
        }
        if let Some(ref mut rrc) = self.p25_rrc {
            rrc.reset();
        }
    }

    /// Switch demodulation mode. Rebuilds the pipeline components.
    pub fn set_mode(&mut self, mode: &str) {
        let new_mode = DemodMode::from_str(mode);
        if new_mode != self.mode {
            let old_is_wfm = self.mode == DemodMode::Wfm;
            let new_is_wfm = new_mode == DemodMode::Wfm;
            let old_is_p25 = self.mode == DemodMode::P25;
            let new_is_p25 = new_mode == DemodMode::P25;

            // Rebuild resampler if switching between WFM (192k) and other (48k)
            if old_is_wfm != new_is_wfm {
                let decim_rate = new_mode.decimated_rate() / self.sample_rate;
                self.resampler = Resampler::new(decim_rate, 60.0);
            }

            self.filter = FirFilter::new_kaiser(64, new_mode.filter_cutoff(), 60.0, 0.0);
            self.demod = Demodulator::new(&new_mode);

            // Set up or tear down WFM-specific components
            if new_is_wfm && !old_is_wfm {
                self.wfm_audio_resamp = Some(RealResampler::new(48_000.0 / 192_000.0, 60.0));
                self.rds_decoder = Some(RdsDecoder::new(192_000.0));
                self.wfm_composite_buf = vec![0.0; 2048];
                self.wfm_audio_buf = vec![0.0; 1024];
            } else if !new_is_wfm && old_is_wfm {
                self.wfm_audio_resamp = None;
                self.rds_decoder = None;
                self.wfm_composite_buf = Vec::new();
                self.wfm_audio_buf = Vec::new();
            }

            // Set up or tear down P25-specific components (C4FM path — no CQPSK)
            if new_is_p25 && !old_is_p25 {
                self.p25_decoder = Some(P25Decoder::new());
                self.p25_audio_resamp = Some(RealResampler::new(48_000.0 / 8_000.0, 60.0));
                self.p25_audio_buf = vec![0.0; 4096];
                self.p25_dc_blocker = Some(DcBlocker::new(5.0, 48_000.0));
                self.p25_rrc = Some(BasebandFir::new(rrc_coefficients(10, 0.2, 51)));
            } else if !new_is_p25 && old_is_p25 {
                self.p25_decoder = None;
                self.p25_audio_resamp = None;
                self.p25_audio_buf = Vec::new();
                self.p25_dc_blocker = None;
                self.p25_rrc = None;
            }

            self.mode = new_mode;
        }
    }

    /// Update squelch threshold (in dB).
    pub fn set_squelch(&mut self, threshold: f64) {
        self.squelch_threshold = threshold as f32;
    }

    /// Set demod filter bandwidth in Hz. Rebuilds the FIR filter.
    /// Pass 0 to reset to the default for the current mode.
    pub fn set_bandwidth(&mut self, bandwidth_hz: f64) {
        let decim_rate = self.mode.decimated_rate();
        let cutoff = if bandwidth_hz > 0.0 {
            // Normalized cutoff: bandwidth / (2 * decimated_rate)
            (bandwidth_hz as f32) / (2.0 * decim_rate)
        } else {
            self.mode.filter_cutoff()
        };
        // Clamp to valid range
        let cutoff = cutoff.clamp(0.01, 0.49);
        self.filter = FirFilter::new_kaiser(64, cutoff, 60.0, 0.0);
    }

    /// Returns true if the squelch is currently open (signal present).
    pub fn is_squelch_open(&self) -> bool {
        self.squelch_open
    }

    /// Current demod mode name.
    pub fn mode_name(&self) -> &'static str {
        self.mode.name()
    }

    /// Take RDS metadata update if available (WFM mode only).
    pub fn take_rds_update(&mut self) -> Option<RdsMetadata> {
        self.rds_decoder.as_mut().and_then(|rds| rds.take_update())
    }

    /// Take P25 metadata update if available (P25 mode only).
    pub fn take_p25_metadata(&mut self) -> Option<P25Metadata> {
        self.p25_decoder.as_mut().and_then(|d| d.take_metadata())
    }

    /// Take TSBK events if available (P25 mode only, control channel data).
    pub fn take_tsbk_events(&mut self) -> Vec<TsbkPayload> {
        self.p25_decoder.as_mut().map(|d| d.take_tsbk_events()).unwrap_or_default()
    }

    /// Take P25 FM demod output statistics (min, max, mean) and reset.
    pub fn take_p25_demod_stats(&mut self) -> Option<(f32, f32, f32)> {
        if self.p25_demod_count == 0 {
            return None;
        }
        let min = self.p25_demod_min;
        let max = self.p25_demod_max;
        let mean = (self.p25_demod_sum / self.p25_demod_count as f64) as f32;
        self.p25_demod_min = f32::MAX;
        self.p25_demod_max = f32::MIN;
        self.p25_demod_sum = 0.0;
        self.p25_demod_count = 0;
        Some((min, max, mean))
    }

    /// P25 decoder sync/error diagnostic counters.
    pub fn p25_decode_stats(&self) -> (u64, u64, [u64; 8]) {
        if let Some(ref dec) = self.p25_decoder {
            (dec.sync_nid_count, dec.decode_error_count, dec.duid_counts)
        } else {
            (0, 0, [0; 8])
        }
    }

    /// Whether the CQPSK demodulator has achieved carrier/timing lock (P25 mode only).
    pub fn cqpsk_locked(&self) -> bool {
        self.cqpsk_demod.as_ref().map_or(false, |d| d.is_locked())
    }

    /// CQPSK demodulator diagnostics (P25 mode only).
    pub fn cqpsk_diag(&self) -> Option<crate::cqpsk::CqpskDiag> {
        self.cqpsk_demod.as_ref().map(|d| d.diag.clone())
    }

    fn measure_power(&self, iq: &[Complex32]) -> f32 {
        if iq.is_empty() {
            return -120.0;
        }
        let sum: f32 = iq.iter().map(|s| s.norm_sqr()).sum();
        let avg = sum / iq.len() as f32;
        if avg > 0.0 {
            10.0 * avg.log10()
        } else {
            -120.0
        }
    }

    fn update_squelch(&mut self, power_db: f32) {
        // Exponential moving average of power
        let alpha = 0.3;
        self.power_avg = alpha * power_db + (1.0 - alpha) * self.power_avg;

        let was_open = self.squelch_open;

        if self.power_avg > self.squelch_threshold {
            self.squelch_open = true;
            self.hang_counter = self.hang_time;
        } else if self.hang_counter > 0 {
            self.hang_counter -= 1;
            // Keep squelch open during hang time
        } else {
            self.squelch_open = false;
        }

        // Fingerprint: finalize on squelch close (falling edge)
        if was_open && !self.squelch_open {
            if let Some(fp) = self.fp_accum.finalize() {
                self.fp_result = Some(fp);
            }
            self.fp_accum.reset();
        }
        self.fp_was_open = self.squelch_open;
    }

    /// Feed channel-filtered IQ into the fingerprint accumulator (call when squelch open).
    fn accumulate_fingerprint(&mut self, samples: &[Complex32]) {
        if self.squelch_open {
            self.fp_accum.feed_iq(samples);
        }
    }

    /// Take the completed fingerprint if available.
    pub fn take_fingerprint(&mut self) -> Option<RfFingerprint> {
        self.fp_result.take()
    }
}
