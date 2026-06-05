use crate::bands::BandDef;
use crate::freq_db::FreqDb;
use num_complex::Complex32;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// A detected signal with classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanDetection {
    pub freq: f64,
    pub power: f32,
    pub cls: String,
    pub name: String,
    pub band: String,
    pub mode: Option<String>,
    pub timestamp: String,
}

/// A complete spectrum frame for one band sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectrumFrame {
    pub band: String,
    pub band_start: f64,
    pub band_end: f64,
    pub freqs: Vec<f64>,
    pub powers: Vec<f32>,
    pub signals: Vec<ScanDetection>,
    pub noise_floor: f32,
    /// Per-segment noise floors: (start_mhz, end_mhz, noise_floor_db)
    #[serde(default)]
    pub segment_noise_floors: Vec<(f64, f64, f32)>,
    /// Device key identifying which SDR produced this frame (e.g. "rtlsdr:00000001")
    #[serde(default)]
    pub device_key: String,
}

/// Commands to control the scan controller.
#[derive(Debug)]
pub enum ScanCommand {
    Start,
    Stop,
    SetBands(Vec<String>),
    SetBandRanges(Vec<BandRange>),
    SetThreshold(f64),
    SetBandThresholds(HashMap<String, f64>),
    SetSnrMargin(f64),
    SetPersistence { min_hits: u32, window: u32 },
    SetDwellRange { min_ms: u64, max_ms: u64 },
    /// Enable/disable WX_CHECK interleaving after each sweep cycle.
    SetWxEnabled(bool),
}

/// A decoded SAME weather alert forwarded from the scan controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WxAlertEvent {
    pub alert_json: String,
    pub freq_mhz: f64,
}

/// A sub-range of a band, used for splitting large bands across devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandRange {
    /// Parent band key (e.g. "UHF") — used for SpectrumFrame tagging
    pub key: String,
    /// Sub-range start in MHz
    pub start_mhz: f64,
    /// Sub-range end in MHz
    pub end_mhz: f64,
}

/// Live scan status shared with the heartbeat for per-band indicators.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanStatus {
    /// The band key currently being swept (e.g. "VHF"), or empty if idle.
    pub current_band: String,
    /// Whether the scan controller is actively scanning.
    pub scanning: bool,
    /// The band index position (for save/restore).
    pub band_idx: usize,
    /// Per-band last sweep duration in milliseconds.
    pub band_sweep_ms: HashMap<String, u64>,
}

/// Commands the scan controller sends to the SDR reader.
#[derive(Debug)]
pub enum SdrTuneCommand {
    SetFreq(f64),
}

/// Scan controller configuration.
#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub active_bands: Vec<String>,
    /// Explicit band ranges (set by SetBandRanges). When non-empty, overrides active_bands.
    pub active_ranges: Vec<BandRange>,
    pub threshold: f64,
    pub band_thresholds: HashMap<String, f64>,
    pub settle_ms: u64,
    pub num_points: usize,
    pub sample_rate: f64,
    pub snr_margin: f64,
    pub min_bandwidth_khz: f64,
    pub persist_min_hits: u32,
    pub persist_window: u32,
    /// Adaptive dwell: minimum dwell for quiet segments (ms)
    pub min_dwell_ms: u64,
    /// Adaptive dwell: maximum dwell for active segments (ms)
    pub max_dwell_ms: u64,
    /// Adaptive dwell: default dwell for unknown/new segments (ms)
    pub default_dwell_ms: u64,
    /// Flag: set when SetBandRanges is received, cleared by outer loop
    pub ranges_changed: bool,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            active_bands: vec!["VHF".into()],
            active_ranges: Vec::new(),
            threshold: -90.0,
            band_thresholds: HashMap::new(),
            settle_ms: 20,
            num_points: 2048,
            sample_rate: 2_400_000.0,
            snr_margin: 8.0,
            min_bandwidth_khz: 1.0,
            persist_min_hits: 1,
            persist_window: 3,
            min_dwell_ms: 50,
            max_dwell_ms: 150,
            default_dwell_ms: 100,
            ranges_changed: false,
        }
    }
}

/// PSD frame input from the DSP pipeline.
pub struct PsdInput {
    pub psd: Vec<f32>,
    pub fft_size: usize,
    /// Raw IQ samples, present only when wx_iq_flag is set (WX_CHECK mode).
    pub iq_samples: Option<Vec<Complex32>>,
}

/// Drain all pending scan commands, updating config and scanning state.
/// Returns false if the command channel is disconnected (shutdown).
fn drain_commands(
    scan_cmd_rx: &mpsc::Receiver<ScanCommand>,
    config: &mut ScanConfig,
    scanning: &mut bool,
    wx_enabled: &mut bool,
) -> bool {
    loop {
        match scan_cmd_rx.try_recv() {
            Ok(ScanCommand::Start) => {
                *scanning = true;
                tracing::info!("Scan started");
            }
            Ok(ScanCommand::Stop) => {
                *scanning = false;
                tracing::info!("Scan stopped");
            }
            Ok(ScanCommand::SetBands(b)) => {
                config.active_bands = b;
                config.active_ranges.clear(); // SetBands clears explicit ranges
            }
            Ok(ScanCommand::SetBandRanges(ranges)) => {
                config.active_ranges = ranges;
                config.ranges_changed = true;
            }
            Ok(ScanCommand::SetThreshold(t)) => config.threshold = t,
            Ok(ScanCommand::SetBandThresholds(bt)) => config.band_thresholds = bt,
            Ok(ScanCommand::SetSnrMargin(m)) => config.snr_margin = m,
            Ok(ScanCommand::SetPersistence { min_hits, window }) => {
                config.persist_min_hits = min_hits;
                config.persist_window = window;
            }
            Ok(ScanCommand::SetDwellRange { min_ms, max_ms }) => {
                config.min_dwell_ms = min_ms;
                config.max_dwell_ms = max_ms;
            }
            Ok(ScanCommand::SetWxEnabled(enabled)) => {
                *wx_enabled = enabled;
                tracing::info!("WX check: {}", if enabled { "enabled" } else { "disabled" });
            }
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => {
                tracing::info!("Scan command channel closed, stopping");
                return false;
            }
        }
    }
}

/// Spawn the scan controller thread.
///
/// Sweeps each active band by stepping through 2.4 MHz segments,
/// stitching the PSD data together with correct frequency mapping.
pub fn spawn_scan_controller(
    bands: Vec<BandDef>,
    freq_db: FreqDb,
    sdr_tune_tx: mpsc::Sender<SdrTuneCommand>,
    psd_rx: mpsc::Receiver<PsdInput>,
    frame_tx: mpsc::Sender<SpectrumFrame>,
    scan_cmd_rx: mpsc::Receiver<ScanCommand>,
    scan_status: Arc<RwLock<ScanStatus>>,
    device_key: String,
    wx_alert_tx: Option<mpsc::Sender<WxAlertEvent>>,
    wx_iq_flag: Option<Arc<AtomicBool>>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("scan_controller".into())
        .spawn(move || {
            tracing::info!("Scan controller started");

            let mut config = ScanConfig::default();
            let mut scanning = false;
            let mut band_idx = 0usize;

            // Persistence tracking: freq bin key → ring buffer of hit/miss per sweep
            // Key = (freq_mhz * 200.0).round() as i64, giving 5 kHz bins
            let mut persist_history: HashMap<i64, VecDeque<bool>> = HashMap::new();

            // Adaptive dwell: per-segment detection history (last 8 sweeps)
            // Key = center_khz, Value = ring buffer of had_signal bools
            let mut segment_activity: HashMap<i64, VecDeque<bool>> = HashMap::new();

            // WX_CHECK state
            let mut wx_enabled = true; // enabled by default
            let wx_freqs = [162_400_000.0f64, 162_550_000.0]; // Portland NWR
            let mut wx_freq_idx = 0usize;
            let mut wx_decoder: Option<rf_same::SameDecoder> = None;

            loop {
                // Process pending commands
                if !drain_commands(&scan_cmd_rx, &mut config, &mut scanning, &mut wx_enabled) {
                    return;
                }

                // Handle range reassignment: clear stale sweep times, reset position
                if config.ranges_changed {
                    config.ranges_changed = false;
                    band_idx = 0;
                    if let Ok(mut st) = scan_status.write() {
                        st.band_sweep_ms.clear();
                    }
                }

                if !scanning {
                    // Update shared status: not scanning
                    if let Ok(mut st) = scan_status.write() {
                        st.scanning = false;
                        st.current_band.clear();
                    }
                    // Drain any pending PSD frames
                    while psd_rx.try_recv().is_ok() {}
                    thread::sleep(std::time::Duration::from_millis(50));
                    continue;
                }

                // Resolve what to sweep: explicit ranges (from SetBandRanges) or
                // active_bands converted to ranges via BandDef lookup.
                let sweep_ranges: Vec<BandRange> = if !config.active_ranges.is_empty() {
                    config.active_ranges.clone()
                } else {
                    bands.iter()
                        .filter(|b| config.active_bands.contains(&b.key))
                        .map(|b| BandRange {
                            key: b.key.clone(),
                            start_mhz: b.start,
                            end_mhz: b.end,
                        })
                        .collect()
                };

                if sweep_ranges.is_empty() {
                    if let Ok(mut st) = scan_status.write() {
                        st.scanning = true;
                        st.current_band.clear();
                    }
                    thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }

                let range = &sweep_ranges[band_idx % sweep_ranges.len()];
                let sweep_start = Instant::now();

                // Parent BandDef is used implicitly via range.key for threshold lookup

                // Calculate segment centers for this range
                let range_start_hz = range.start_mhz * 1e6;
                let range_end_hz = range.end_mhz * 1e6;
                let range_bw_hz = range_end_hz - range_start_hz;

                let segment_centers: Vec<f64> = if range_bw_hz <= config.sample_rate {
                    vec![(range_start_hz + range_end_hz) / 2.0]
                } else {
                    let n = (range_bw_hz / config.sample_rate).ceil() as usize;
                    let step = range_bw_hz / n as f64;
                    (0..n).map(|i| range_start_hz + step * (i as f64 + 0.5)).collect()
                };

                let num_segments = segment_centers.len();
                tracing::debug!(
                    "[{}] Sweep #{}: band={} ({:.1}-{:.1} MHz, {} segments)",
                    device_key, band_idx, range.key, range.start_mhz, range.end_mhz, num_segments
                );

                // Update shared status: currently sweeping this band
                if let Ok(mut st) = scan_status.write() {
                    st.scanning = true;
                    st.current_band = range.key.clone();
                    st.band_idx = band_idx;
                }

                // Sweep all segments, collecting (freq_mhz, power) pairs
                let mut raw_bins: Vec<(f64, f32)> = Vec::new();
                let mut segment_noise_floors: Vec<(f64, f64, f32)> = Vec::new();
                let mut aborted = false;

                for &center_hz in &segment_centers {
                    // Check for commands between segments
                    if !drain_commands(&scan_cmd_rx, &mut config, &mut scanning, &mut wx_enabled) {
                        return;
                    }
                    if !scanning {
                        aborted = true;
                        break;
                    }

                    // Adaptive dwell: look up this segment's history
                    let seg_key = (center_hz / 1000.0).round() as i64;
                    let per_seg_dwell = compute_adaptive_dwell(&segment_activity, seg_key, &config);

                    // RETUNE to segment center
                    let _ = sdr_tune_tx.send(SdrTuneCommand::SetFreq(center_hz));

                    // SETTLE: wait for PLL lock, discard stale PSD
                    thread::sleep(std::time::Duration::from_millis(config.settle_ms));
                    while psd_rx.try_recv().is_ok() {}

                    // DWELL: collect PSD frames, max-hold
                    let dwell_start = Instant::now();
                    let mut seg_psd: Option<Vec<f32>> = None;

                    while dwell_start.elapsed().as_millis() < per_seg_dwell as u128 {
                        match psd_rx.recv_timeout(std::time::Duration::from_millis(10)) {
                            Ok(frame) => {
                                match &mut seg_psd {
                                    None => seg_psd = Some(frame.psd),
                                    Some(acc) => {
                                        if acc.len() == frame.psd.len() {
                                            for (a, &b) in acc.iter_mut().zip(frame.psd.iter()) {
                                                *a = a.max(b);
                                            }
                                        }
                                    }
                                }
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }

                    // Diagnostic: log when segment has no PSD data
                    if seg_psd.is_none() {
                        tracing::warn!(
                            "[{}] Segment {:.3} MHz: no PSD data after {}ms dwell",
                            device_key, center_hz / 1e6, per_seg_dwell
                        );
                    }

                    // Track whether this segment had any signal (for adaptive dwell)
                    let mut seg_had_signal = false;

                    // Map PSD bins to correct absolute frequencies (with DC offset nulling)
                    if let Some(mut psd) = seg_psd {
                        let num_bins = psd.len();
                        nullify_dc_offset(&mut psd, num_bins);

                        let bin_width_hz = config.sample_rate / num_bins as f64;
                        let freq_start_hz = center_hz - config.sample_rate / 2.0;

                        // Compute per-segment noise floor (median of this segment's bins)
                        let mut seg_powers: Vec<f32> = Vec::with_capacity(num_bins);
                        for i in 0..num_bins {
                            let freq_hz = freq_start_hz + i as f64 * bin_width_hz;
                            let freq_mhz = freq_hz / 1e6;
                            // Only keep bins within the range boundaries
                            if freq_mhz >= range.start_mhz && freq_mhz <= range.end_mhz {
                                raw_bins.push((freq_mhz, psd[i]));
                                seg_powers.push(psd[i]);
                            }
                        }
                        if !seg_powers.is_empty() {
                            seg_powers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                            let seg_nf = seg_powers[seg_powers.len() / 2];
                            let seg_start = (center_hz - config.sample_rate / 2.0) / 1e6;
                            let seg_end = (center_hz + config.sample_rate / 2.0) / 1e6;

                            // Check if any bin exceeds local noise floor + margin
                            let sig_threshold = seg_nf + config.snr_margin as f32;
                            if seg_powers.iter().any(|&p| p > sig_threshold) {
                                seg_had_signal = true;
                            }

                            segment_noise_floors.push((
                                seg_start.max(range.start_mhz),
                                seg_end.min(range.end_mhz),
                                seg_nf,
                            ));
                        }
                    }

                    // Update adaptive dwell history for this segment
                    let history = segment_activity.entry(seg_key).or_default();
                    history.push_back(seg_had_signal);
                    if history.len() > 8 { history.pop_front(); }
                }

                if aborted || raw_bins.is_empty() {
                    if !aborted && raw_bins.is_empty() {
                        tracing::warn!(
                            "[{}] Band {} sweep empty — 0 PSD bins collected across {} segments",
                            device_key, range.key, num_segments
                        );
                    }
                    band_idx += 1;
                    continue;
                }

                // Sort raw bins by frequency
                raw_bins.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

                // Resample to uniform output grid
                let out_step = (range.end_mhz - range.start_mhz) / config.num_points as f64;
                let freqs: Vec<f64> = (0..config.num_points)
                    .map(|i| range.start_mhz + (i as f64 + 0.5) * out_step)
                    .collect();

                let powers: Vec<f32> = freqs.iter().map(|&f| {
                    interpolate_raw(&raw_bins, f)
                }).collect();

                // Compute noise floor once as median of resampled powers
                let noise_floor = {
                    let mut sorted: Vec<f32> = powers.to_vec();
                    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    sorted[sorted.len() / 2]
                };

                // Use per-band threshold if set, otherwise global
                let band_thr = config.band_thresholds
                    .get(&range.key)
                    .copied()
                    .unwrap_or(config.threshold);

                // DETECT signals on the resampled spectrum (with bandwidth filter)
                let mut detections = detect_and_classify(
                    &powers,
                    range.start_mhz,
                    out_step,
                    band_thr,
                    config.snr_margin,
                    config.min_bandwidth_khz,
                    noise_floor,
                    &raw_bins,
                    &freq_db,
                    &range.key,
                    &segment_noise_floors,
                );

                // Apply persistence filter
                if config.persist_min_hits > 1 {
                    apply_persistence(
                        &mut detections,
                        &mut persist_history,
                        config.persist_min_hits,
                        config.persist_window as usize,
                    );
                }

                // Send spectrum frame — use range boundaries for sub-range support
                let frame = SpectrumFrame {
                    band: range.key.clone(),
                    band_start: range.start_mhz,
                    band_end: range.end_mhz,
                    freqs,
                    powers,
                    signals: detections,
                    noise_floor,
                    segment_noise_floors,
                    device_key: device_key.clone(),
                };

                if frame_tx.send(frame).is_err() {
                    tracing::info!("Scan output channel closed, stopping");
                    return;
                }

                // Record sweep duration for this band
                let sweep_ms = sweep_start.elapsed().as_millis() as u64;
                if let Ok(mut st) = scan_status.write() {
                    st.band_sweep_ms.insert(range.key.clone(), sweep_ms);
                }

                // ADVANCE to next band
                band_idx += 1;

                // ── WX_CHECK: interleave SAME decode after each sweep cycle ──
                let sweep_complete = band_idx > 0 && band_idx % sweep_ranges.len() == 0;
                if wx_enabled && sweep_complete {
                    if let Some(ref flag) = wx_iq_flag {
                        let wx_freq = wx_freqs[wx_freq_idx % wx_freqs.len()];
                        wx_freq_idx += 1;

                        if !wx_check_cycle(
                            wx_freq, &mut wx_decoder, flag,
                            &wx_alert_tx, &sdr_tune_tx, &psd_rx,
                            &config, &device_key,
                        ) {
                            return; // channel disconnected
                        }
                    }
                }
            }
        })
        .expect("failed to spawn scan_controller thread")
}

/// Interpolate a power value from sorted raw bins at a target frequency.
fn interpolate_raw(raw_bins: &[(f64, f32)], freq: f64) -> f32 {
    if raw_bins.is_empty() {
        return -120.0;
    }

    match raw_bins.binary_search_by(|b| b.0.partial_cmp(&freq).unwrap_or(std::cmp::Ordering::Equal)) {
        Ok(idx) => raw_bins[idx].1,
        Err(idx) => {
            if idx == 0 {
                raw_bins[0].1
            } else if idx >= raw_bins.len() {
                raw_bins[raw_bins.len() - 1].1
            } else {
                let lo = &raw_bins[idx - 1];
                let hi = &raw_bins[idx];
                let span = hi.0 - lo.0;
                if span < 1e-9 {
                    lo.1
                } else {
                    let t = ((freq - lo.0) / span) as f32;
                    lo.1 * (1.0 - t) + hi.1 * t
                }
            }
        }
    }
}

/// Measure bandwidth of a peak using the raw bin data.
/// Returns bandwidth in kHz, measured at snr_margin/2 below peak.
fn measure_bandwidth_raw(raw_bins: &[(f64, f32)], freq_mhz: f64, peak_power: f32, snr_margin: f64) -> f64 {
    if raw_bins.is_empty() {
        return 0.0;
    }

    // Find the closest raw bin to the peak frequency
    let center_idx = match raw_bins.binary_search_by(|b| {
        b.0.partial_cmp(&freq_mhz).unwrap_or(std::cmp::Ordering::Equal)
    }) {
        Ok(idx) => idx,
        Err(idx) => idx.min(raw_bins.len() - 1),
    };

    // Threshold: half the SNR margin below peak
    let bw_threshold = peak_power - (snr_margin as f32 / 2.0);

    // Walk left from peak
    let mut left_freq = raw_bins[center_idx].0;
    for i in (0..center_idx).rev() {
        if raw_bins[i].1 < bw_threshold {
            left_freq = raw_bins[i].0;
            break;
        }
        left_freq = raw_bins[i].0;
    }

    // Walk right from peak
    let mut right_freq = raw_bins[center_idx].0;
    for i in (center_idx + 1)..raw_bins.len() {
        if raw_bins[i].1 < bw_threshold {
            right_freq = raw_bins[i].0;
            break;
        }
        right_freq = raw_bins[i].0;
    }

    // Return bandwidth in kHz
    (right_freq - left_freq) * 1000.0
}

/// Detect signals on raw FFT bins and classify them using the frequency database.
/// Runs peak detection directly on full-resolution raw bins (73 Hz at 32768 FFT)
/// rather than the coarse resampled display grid, so narrow signals aren't missed.
fn detect_and_classify(
    _powers: &[f32],
    _freq_start: f64,
    _freq_step: f64,
    threshold: f64,
    snr_margin: f64,
    min_bandwidth_khz: f64,
    _noise_floor: f32,
    raw_bins: &[(f64, f32)],
    freq_db: &FreqDb,
    band_key: &str,
    segment_noise_floors: &[(f64, f64, f32)],
) -> Vec<ScanDetection> {
    let mut detections = Vec::new();

    if raw_bins.len() < 3 {
        return detections;
    }

    // Extract power values from raw bins for detection
    let raw_powers: Vec<f32> = raw_bins.iter().map(|b| b.1).collect();

    // Compute global noise floor from raw bins (median) as fallback
    let global_noise_floor = {
        let mut sorted = raw_powers.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sorted[sorted.len() / 2]
    };

    // Look up local noise floor for a frequency using per-segment data.
    // Returns (noise_floor, has_segment_data).
    let local_nf = |freq_mhz: f64| -> (f32, bool) {
        for &(start, end, nf) in segment_noise_floors {
            if freq_mhz >= start && freq_mhz <= end {
                return (nf, true);
            }
        }
        (global_noise_floor, false)
    };

    for i in 1..raw_powers.len() - 1 {
        let cur = raw_powers[i];
        let left = raw_powers[i - 1];
        let right = raw_powers[i + 1];

        // Use per-segment noise floor for this bin's frequency.
        // When segment data is available, the local NF + margin is authoritative —
        // the band-wide threshold is only used as fallback when no segment data exists.
        // This prevents one hot segment from suppressing detection across the entire band.
        let bin_freq = raw_bins[i].0;
        let (nf, has_seg) = local_nf(bin_freq);
        let effective_thr = if has_seg {
            nf + snr_margin as f32
        } else {
            (threshold as f32).max(nf + snr_margin as f32)
        };

        if cur > left && cur > right && cur > effective_thr && (cur - left >= 6.0 || cur - right >= 6.0) {
            // Parabolic interpolation for sub-bin frequency precision
            let alpha = left;
            let beta = cur;
            let gamma = right;
            let denom = alpha - 2.0 * beta + gamma;
            let delta = if denom.abs() > 1e-10 {
                0.5 * (alpha - gamma) / denom
            } else {
                0.0
            };

            let freq_mhz = raw_bins[i].0 + delta as f64 * (raw_bins[1].0 - raw_bins[0].0);
            let peak_power = beta - 0.25 * (alpha - gamma) * delta;

            // Minimum bandwidth filter: reject narrow spurs
            if min_bandwidth_khz > 0.0 {
                let bw = measure_bandwidth_raw(raw_bins, freq_mhz, peak_power, snr_margin);
                if bw < min_bandwidth_khz {
                    continue;
                }
            }

            // Classify from frequency database (tolerance ~15 kHz)
            let (name, cls, mode) = match freq_db.lookup(freq_mhz, 0.015) {
                Some(entry) => (entry.name.clone(), entry.cls.clone(), entry.mode.clone()),
                None => {
                    if is_likely_repeater(freq_mhz) {
                        (format!("Repeater {:.4}", freq_mhz), "RPTR".into(), Some("NFM".into()))
                    } else {
                        ("Unknown".into(), "UNK".into(), None)
                    }
                }
            };

            let now = timestamp_now();

            detections.push(ScanDetection {
                freq: freq_mhz,
                power: peak_power,
                cls,
                name,
                band: band_key.into(),
                mode,
                timestamp: now,
            });
        }
    }

    detections
}

/// Check if a frequency (in MHz) falls in a known repeater output range.
/// These are standard band plans where signals are almost certainly repeater outputs.
fn is_likely_repeater(freq_mhz: f64) -> bool {
    // VHF 2m repeater outputs (standard -600 kHz offset pairs)
    // 145.100-145.500 output (input 144.500-144.900)
    if freq_mhz >= 145.1 && freq_mhz <= 145.5 { return true; }
    // 146.610-146.970 output (input 146.010-146.370)
    if freq_mhz >= 146.61 && freq_mhz <= 146.97 { return true; }
    // 147.000-147.390 output (input 147.600-147.990, +600 kHz)
    if freq_mhz >= 147.0 && freq_mhz <= 147.39 { return true; }

    // UHF 70cm repeater outputs (standard +5 MHz offset)
    // 440.000-444.975 output (input 445.000-449.975)
    if freq_mhz >= 440.0 && freq_mhz <= 444.975 { return true; }

    // GMRS repeater output channels (462.5500-462.7250)
    if freq_mhz >= 462.55 && freq_mhz <= 462.725 { return true; }

    // UHF commercial/public safety repeater output ranges
    // 453-454 MHz output (input 458-459 MHz, +5 MHz offset)
    if freq_mhz >= 453.0 && freq_mhz <= 454.0 { return true; }
    // 460-461 MHz output (input 465-466 MHz, +5 MHz offset)
    if freq_mhz >= 460.0 && freq_mhz <= 461.0 { return true; }

    // 800 MHz public safety repeater outputs (851-869 MHz)
    if freq_mhz >= 851.0 && freq_mhz <= 869.0 { return true; }

    false
}

/// Zero out the DC spike (center bins) in a PSD array.
/// RTL-SDR has a hardware DC offset that creates a false peak at center frequency.
fn nullify_dc_offset(psd: &mut [f32], num_bins: usize) {
    let dc_idx = num_bins / 2;
    let dc_radius = 3usize;
    if num_bins > dc_radius * 2 + 2 {
        let left = psd[dc_idx.saturating_sub(dc_radius + 1)];
        let right = psd[(dc_idx + dc_radius + 1).min(num_bins - 1)];
        let avg = (left + right) / 2.0;
        let start = dc_idx.saturating_sub(dc_radius);
        let end = (dc_idx + dc_radius + 1).min(num_bins);
        for bin in &mut psd[start..end] {
            *bin = avg;
        }
    }
}

/// Compute adaptive dwell time for a segment based on its detection history.
fn compute_adaptive_dwell(
    segment_activity: &HashMap<i64, VecDeque<bool>>,
    seg_key: i64,
    config: &ScanConfig,
) -> u64 {
    match segment_activity.get(&seg_key) {
        None => config.default_dwell_ms,
        Some(h) if h.is_empty() => config.default_dwell_ms,
        Some(h) => {
            let active_count = h.iter().filter(|&&v| v).count();
            if active_count == 0 {
                config.min_dwell_ms
            } else if active_count >= h.len() / 2 {
                config.max_dwell_ms
            } else {
                config.default_dwell_ms
            }
        }
    }
}

/// Apply persistence filter: require signals to appear min_hits times within a sliding window.
fn apply_persistence(
    detections: &mut Vec<ScanDetection>,
    persist_history: &mut HashMap<i64, VecDeque<bool>>,
    min_hits: u32,
    window: usize,
) {
    let detected_keys: Vec<i64> = detections.iter()
        .map(|d| (d.freq * 200.0).round() as i64)
        .collect();

    // Update history for all tracked frequencies
    let mut keys_to_remove = Vec::new();
    for (key, history) in persist_history.iter_mut() {
        let hit = detected_keys.contains(key);
        history.push_back(hit);
        while history.len() > window {
            history.pop_front();
        }
        if history.iter().all(|&h| !h) {
            keys_to_remove.push(*key);
        }
    }
    for key in keys_to_remove {
        persist_history.remove(&key);
    }

    // Add new entries for newly detected frequencies
    for &key in &detected_keys {
        persist_history.entry(key).or_insert_with(|| {
            let mut dq = VecDeque::with_capacity(window);
            dq.push_back(true);
            dq
        });
    }

    // Filter: only keep detections that meet persistence threshold
    detections.retain(|d| {
        let key = (d.freq * 200.0).round() as i64;
        persist_history.get(&key)
            .map(|h| h.iter().filter(|&&v| v).count() as u32 >= min_hits)
            .unwrap_or(false)
    });
}

/// Run one WX_CHECK cycle: retune to a NWR frequency, collect IQ, decode SAME.
fn wx_check_cycle(
    wx_freq: f64,
    wx_decoder: &mut Option<rf_same::SameDecoder>,
    wx_iq_flag: &Arc<AtomicBool>,
    wx_alert_tx: &Option<mpsc::Sender<WxAlertEvent>>,
    sdr_tune_tx: &mpsc::Sender<SdrTuneCommand>,
    psd_rx: &mpsc::Receiver<PsdInput>,
    config: &ScanConfig,
    device_key: &str,
) -> bool {
    // Initialize decoder on first use
    if wx_decoder.is_none() {
        *wx_decoder = Some(rf_same::SameDecoder::new(
            config.sample_rate, wx_freq, wx_freq,
        ));
    }

    // Retune SDR to WX frequency
    let _ = sdr_tune_tx.send(SdrTuneCommand::SetFreq(wx_freq));

    // Enable IQ forwarding
    wx_iq_flag.store(true, Ordering::Release);

    // Settle
    thread::sleep(std::time::Duration::from_millis(config.settle_ms));
    while psd_rx.try_recv().is_ok() {}

    // Update decoder frequency
    if let Some(dec) = wx_decoder {
        dec.set_frequencies(wx_freq, wx_freq);
    }

    // Dwell: collect IQ for ~200ms, check for preamble
    let wx_dwell_start = Instant::now();
    let wx_dwell_ms = 200u64;
    let mut preamble_found = false;
    let mut wx_alert = None;

    while wx_dwell_start.elapsed().as_millis() < wx_dwell_ms as u128 {
        match psd_rx.recv_timeout(std::time::Duration::from_millis(10)) {
            Ok(frame) => {
                if let Some(iq) = frame.iq_samples.as_deref() {
                    if let Some(dec) = wx_decoder {
                        if let Some(alert) = dec.feed_iq(iq) {
                            wx_alert = Some(alert);
                        }
                        if dec.preamble_detected() {
                            preamble_found = true;
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                wx_iq_flag.store(false, Ordering::Release);
                return false; // signal shutdown
            }
        }
    }

    // If preamble detected, extend dwell for full 3-transmission decode (~8s)
    if preamble_found && wx_alert.is_none() {
        tracing::info!("[{}] WX: SAME preamble on {:.3} MHz — extending dwell",
            device_key, wx_freq / 1e6);

        let ext_start = Instant::now();
        let ext_dwell_ms = 8000u64;

        while ext_start.elapsed().as_millis() < ext_dwell_ms as u128 {
            match psd_rx.recv_timeout(std::time::Duration::from_millis(20)) {
                Ok(frame) => {
                    if let Some(iq) = frame.iq_samples.as_deref() {
                        if let Some(dec) = wx_decoder {
                            if let Some(alert) = dec.feed_iq(iq) {
                                wx_alert = Some(alert);
                                break;
                            }
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    wx_iq_flag.store(false, Ordering::Release);
                    return false;
                }
            }
        }
    }

    // Disable IQ forwarding
    wx_iq_flag.store(false, Ordering::Release);

    // Reset decoder for next check
    if let Some(dec) = wx_decoder {
        dec.reset();
    }

    // Forward decoded alert
    if let Some(alert) = wx_alert {
        tracing::info!("[{}] WX: decoded SAME alert: {} ({})",
            device_key, alert.event_name, alert.event_code);
        if let Some(tx) = wx_alert_tx {
            let event = WxAlertEvent {
                alert_json: serde_json::to_string(&alert).unwrap_or_default(),
                freq_mhz: wx_freq / 1e6,
            };
            let _ = tx.send(event);
        }
    }

    true // continue scanning
}

fn timestamp_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}
