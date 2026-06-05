use rf_scan::ScanStatus;
use rf_sdr::SdrDeviceInfo;
use rf_web::SdrSlotStatus;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, RwLock};

use crate::dsp_bridge;

/// Per-device slot holding all channels and state for one SDR device.
pub struct DeviceSlot {
    /// Stable key: "rtlsdr:00000001"
    pub device_key: String,
    pub info: SdrDeviceInfo,
    pub alive: Arc<AtomicBool>,
    /// Refresh flag: set to trigger SDR reader reconnection attempts
    #[allow(dead_code)]
    pub refresh: Arc<AtomicBool>,
    /// Quarantine flag: set by SDR reader on freq tune failures, cleared on recovery
    pub quarantine: Arc<AtomicBool>,
    /// Streaming flag: set by DSP pipeline when first PSD frame is produced.
    /// Indicates the device is actually delivering IQ data (not just "alive").
    pub streaming: Arc<AtomicBool>,
    /// WX IQ forwarding flag: when true, DSP includes raw IQ in PsdInput for SAME decode.
    /// Arc is cloned to DSP/scan threads before storage — struct field held for lifetime management.
    #[allow(dead_code)]
    pub wx_iq_flag: Arc<AtomicBool>,
    pub sdr_cmd_tx: mpsc::Sender<rf_sdr::SdrCommand>,
    pub dsp_cmd_tx: mpsc::Sender<dsp_bridge::DspCommand>,
    pub scan_cmd_tx: mpsc::Sender<rf_scan::ScanCommand>,
    pub scan_status: Arc<RwLock<ScanStatus>>,
    pub assigned_bands: Vec<String>,
    pub role: String,
    pub sample_rate: f64,
    /// IQ recording mirror flag: when true, SDR reader copies samples to mirror buffer
    #[allow(dead_code)] // Wired in Phase 1 (Recording)
    pub iq_record_flag: Arc<AtomicBool>,
}

/// Pool of all active SDR device slots.
pub struct DevicePool {
    pub slots: Vec<DeviceSlot>,
    /// Shared shutdown flag — propagated to DSP threads at slot creation time
    #[allow(dead_code)]
    pub shutdown: Arc<AtomicBool>,
    /// Band definitions for segment-balanced distribution
    pub band_defs: Vec<rf_scan::BandDef>,
    /// Sample rate used for segment calculation (typically 2_400_000.0)
    pub sample_rate: f64,
    /// Antenna frequency constraints: serial → (freq_min_mhz, freq_max_mhz)
    pub antenna_map: HashMap<String, (f64, f64)>,
}

/// Build a stable device key from SdrDeviceInfo.
/// Uses driver-specific index (e.g. "rtl" for SoapyRTLSDR) to disambiguate
/// devices that share the same serial number (common for RTL-SDR dongles).
pub fn device_key(info: &SdrDeviceInfo) -> String {
    // Driver-specific device index keys (SoapyRTLSDR uses "rtl")
    let dev_index = info.args.get("rtl")
        .or_else(|| info.args.get("index"));

    match (info.serial.is_empty(), dev_index) {
        // Has serial + index: use both to disambiguate duplicate serials
        (false, Some(idx)) => format!("{}:{}:{}", info.driver, info.serial, idx),
        // Has serial, no index: serial alone
        (false, None) => format!("{}:{}", info.driver, info.serial),
        // No serial: use label
        (true, _) => format!("{}:{}", info.driver, info.label),
    }
}

/// Calculate number of 2.4 MHz segments needed to cover a band.
fn band_segment_count(band: &rf_scan::BandDef, sample_rate: f64) -> usize {
    let bw = (band.end - band.start) * 1e6;
    if bw <= sample_rate { 1 } else { (bw / sample_rate).ceil() as usize }
}

/// Segment-balanced band distribution across N devices using greedy LPT scheduling.
/// Sorts bands by segment count (largest first), assigns each to the device
/// with the lowest current total — minimizes sweep time imbalance.
#[allow(dead_code)] // Used in tests; will be called from band reassignment logic
pub fn distribute_bands(
    enabled: &[String],
    n: usize,
    band_defs: &[rf_scan::BandDef],
    sample_rate: f64,
) -> Vec<Vec<String>> {
    if n == 0 {
        return Vec::new();
    }
    let mut result: Vec<Vec<String>> = (0..n).map(|_| Vec::new()).collect();
    let mut totals: Vec<usize> = vec![0; n];

    // Build (band_key, segment_count) pairs, sorted largest-first
    let mut items: Vec<(&String, usize)> = enabled.iter().map(|key| {
        let segs = band_defs.iter()
            .find(|b| b.key == *key)
            .map(|b| band_segment_count(b, sample_rate))
            .unwrap_or(1);
        (key, segs)
    }).collect();
    items.sort_by(|a, b| b.1.cmp(&a.1));

    // Greedy: assign each band to the device with the lowest current total
    for (key, segs) in items {
        let min_idx = totals.iter().enumerate()
            .min_by_key(|&(_, t)| t)
            .map(|(i, _)| i)
            .unwrap_or(0);
        result[min_idx].push(key.clone());
        totals[min_idx] += segs;
    }

    result
}

/// Segment-balanced distribution with band splitting.
/// Large bands that have more segments than total/n_devices are split into sub-ranges
/// so multiple devices can share the load. Returns per-device Vec<BandRange>.
pub fn distribute_segments(
    enabled: &[String],
    n: usize,
    band_defs: &[rf_scan::BandDef],
    sample_rate: f64,
) -> Vec<Vec<rf_scan::BandRange>> {
    if n == 0 {
        return Vec::new();
    }

    // Calculate total segments and per-device target
    let mut band_items: Vec<(&rf_scan::BandDef, usize)> = enabled.iter()
        .filter_map(|key| {
            band_defs.iter().find(|b| b.key == *key)
                .map(|b| (b, band_segment_count(b, sample_rate)))
        })
        .collect();

    let total_segs: usize = band_items.iter().map(|(_, s)| *s).sum();
    let split_threshold = if total_segs > 0 && n > 1 {
        (total_segs + n - 1) / n  // ceil(total/n)
    } else {
        usize::MAX
    };

    // Build items: split large bands, keep small ones whole
    let mut items: Vec<(rf_scan::BandRange, usize)> = Vec::new();
    // Sort bands largest-first for consistent LPT scheduling
    band_items.sort_by(|a, b| b.1.cmp(&a.1));

    for (band, segs) in &band_items {
        if *segs > split_threshold && *segs > 1 {
            // Split this band into roughly equal sub-ranges
            let n_splits = ((*segs + split_threshold - 1) / split_threshold).min(n);
            let band_bw = band.end - band.start;
            let split_bw = band_bw / n_splits as f64;
            for i in 0..n_splits {
                let start = band.start + split_bw * i as f64;
                let end = if i == n_splits - 1 { band.end } else { band.start + split_bw * (i + 1) as f64 };
                let sub_bw = (end - start) * 1e6;
                let sub_segs = if sub_bw <= sample_rate { 1 } else { (sub_bw / sample_rate).ceil() as usize };
                items.push((rf_scan::BandRange {
                    key: band.key.clone(),
                    start_mhz: start,
                    end_mhz: end,
                }, sub_segs));
            }
        } else {
            items.push((rf_scan::BandRange {
                key: band.key.clone(),
                start_mhz: band.start,
                end_mhz: band.end,
            }, *segs));
        }
    }

    // Items are already sorted largest-first from band_items sort + split order
    // Re-sort to ensure LPT scheduling works correctly
    items.sort_by(|a, b| b.1.cmp(&a.1));

    // Greedy LPT bin-packing
    let mut result: Vec<Vec<rf_scan::BandRange>> = (0..n).map(|_| Vec::new()).collect();
    let mut totals: Vec<usize> = vec![0; n];

    for (range, segs) in items {
        let min_idx = totals.iter().enumerate()
            .min_by_key(|&(_, t)| t)
            .map(|(i, _)| i)
            .unwrap_or(0);
        result[min_idx].push(range);
        totals[min_idx] += segs;
    }

    result
}

impl DevicePool {
    /// Create an empty pool with no device slots.
    /// Used during deferred init — the config poller and event bridges
    /// start with this empty pool and operate on it once real devices are swapped in.
    pub fn empty() -> Self {
        DevicePool {
            slots: Vec::new(),
            shutdown: Arc::new(AtomicBool::new(false)),
            band_defs: rf_scan::bands::default_bands(),
            sample_rate: 2_400_000.0,
            antenna_map: HashMap::new(),
        }
    }

    /// Open all enumerated devices, creating per-device pipelines.
    /// Devices that fail to open are logged and skipped.
    ///
    /// Shared outputs (frame_tx, audio_tx, rds_tx, p25_tx) are cloned per device.
    /// Each device gets its own: reader, ring buffer, DSP pipeline, scan controller, tune bridge.
    pub fn open_all_devices(
        enumerated: &[SdrDeviceInfo],
        sample_rate: f64,
        frame_tx: &mpsc::Sender<rf_scan::SpectrumFrame>,
        audio_tx: &mpsc::Sender<rf_recorder::AudioChunk>,
        rds_tx: &mpsc::Sender<String>,
        p25_tx: &mpsc::Sender<String>,
        enabled_bands: &[String],
        freq_json_path: &str,
        shutdown: Arc<AtomicBool>,
        wx_alert_tx: Option<mpsc::Sender<rf_scan::WxAlertEvent>>,
        rec_tx: Option<mpsc::Sender<rf_recorder::RecorderCommand>>,
    ) -> Self {
        let bands = rf_scan::bands::default_bands();
        let seg_distribution = distribute_segments(enabled_bands, enumerated.len(), &bands, sample_rate);

        // Phase 1: Open all devices WITHOUT activating streams.
        // Opening multiple RTL-SDR devices disrupts USB bulk transfers on
        // already-streaming devices. Deferring activation until all devices
        // are claimed prevents this.
        let mut opened: Vec<(String, SdrDeviceInfo, rf_sdr::SoapySdr, Vec<String>, Vec<rf_scan::BandRange>)> = Vec::new();

        for (idx, info) in enumerated.iter().enumerate() {
            let key = device_key(info);
            let ranges = seg_distribution.get(idx).cloned().unwrap_or_default();
            // Derive unique band keys for display
            let assigned: Vec<String> = {
                let mut keys: Vec<String> = ranges.iter().map(|r| r.key.clone()).collect();
                keys.sort();
                keys.dedup();
                keys
            };

            tracing::info!(
                "[pool] Opening device {} ({}) — assigned bands: {:?}",
                key, info.label, assigned
            );

            match rf_sdr::SoapySdr::open_by_args_deferred(&info.args, sample_rate) {
                Ok(dev) => {
                    tracing::info!("[pool] Device {} opened (stream deferred)", key);
                    opened.push((key, info.clone(), dev, assigned, ranges));
                }
                Err(e) => {
                    tracing::warn!(
                        "[pool] Failed to open device {} ({}): {} — skipping",
                        key, info.label, e
                    );
                }
            }
        }

        // Phase 2: Activate all streams now that every device is claimed.
        // Activate in REVERSE order — the last-activated device gets USB priority
        // on shared USB controllers, so activate the first device last.
        for (key, _, dev, _, _) in opened.iter_mut().rev() {
            if let Err(e) = dev.activate_stream() {
                tracing::error!("[pool] Failed to activate stream for {}: {}", key, e);
            } else {
                tracing::info!("[pool] Stream activated for {}", key);
            }
        }
        tracing::info!("[pool] All {} streams activated", opened.len());

        // Phase 3: Create slots (reader, DSP, scan controller threads).
        let mut slots = Vec::new();
        for (key, info, dev, assigned, ranges) in opened {
            let reconnect_args = info.args.clone();
            let slot = Self::create_slot(
                key,
                info,
                Box::new(dev),
                sample_rate,
                assigned,
                frame_tx,
                audio_tx,
                rds_tx,
                p25_tx,
                &bands,
                freq_json_path,
                Some(reconnect_args),
                Arc::clone(&shutdown),
                wx_alert_tx.clone(),
                rec_tx.clone(),
            );
            // Send initial ranges (segment-balanced) instead of full band keys
            if !ranges.is_empty() {
                let _ = slot.scan_cmd_tx.send(rf_scan::ScanCommand::SetBandRanges(ranges));
            }
            slots.push(slot);
        }

        DevicePool { slots, shutdown, band_defs: bands, sample_rate, antenna_map: HashMap::new() }
    }

    /// Create a simulation-mode pool with one simulated device.
    pub fn create_simulated_slot(
        freq_json_path: &str,
        sample_rate: f64,
        frame_tx: &mpsc::Sender<rf_scan::SpectrumFrame>,
        audio_tx: &mpsc::Sender<rf_recorder::AudioChunk>,
        rds_tx: &mpsc::Sender<String>,
        p25_tx: &mpsc::Sender<String>,
        enabled_bands: &[String],
        shutdown: Arc<AtomicBool>,
        wx_alert_tx: Option<mpsc::Sender<rf_scan::WxAlertEvent>>,
        rec_tx: Option<mpsc::Sender<rf_recorder::RecorderCommand>>,
    ) -> Self {
        let sim = rf_sdr::SimulatedSdr::new(freq_json_path)
            .expect("Failed to create simulated SDR");
        let bands = rf_scan::bands::default_bands();

        let info = SdrDeviceInfo {
            driver: "simulated".into(),
            label: "Simulation".into(),
            serial: String::new(),
            args: HashMap::new(),
        };

        let slot = Self::create_slot(
            "simulated:0".into(),
            info,
            Box::new(sim),
            sample_rate,
            enabled_bands.to_vec(),
            frame_tx,
            audio_tx,
            rds_tx,
            p25_tx,
            &bands,
            freq_json_path,
            None, // no reconnect args for sim
            Arc::clone(&shutdown),
            wx_alert_tx,
            rec_tx,
        );

        DevicePool { slots: vec![slot], shutdown, band_defs: bands, sample_rate, antenna_map: HashMap::new() }
    }

    /// Create a single device slot with all its threads/channels.
    fn create_slot(
        key: String,
        info: SdrDeviceInfo,
        device: Box<dyn rf_sdr::SdrDevice>,
        sample_rate: f64,
        assigned_bands: Vec<String>,
        frame_tx: &mpsc::Sender<rf_scan::SpectrumFrame>,
        _audio_tx: &mpsc::Sender<rf_recorder::AudioChunk>,
        _rds_tx: &mpsc::Sender<String>,
        _p25_tx: &mpsc::Sender<String>,
        bands: &[rf_scan::BandDef],
        freq_json_path: &str,
        reconnect_args: Option<HashMap<String, String>>,
        shutdown: Arc<AtomicBool>,
        wx_alert_tx: Option<mpsc::Sender<rf_scan::WxAlertEvent>>,
        rec_tx: Option<mpsc::Sender<rf_recorder::RecorderCommand>>,
    ) -> DeviceSlot {
        let is_sim = device.is_simulated();
        let alive = Arc::new(AtomicBool::new(!is_sim));
        let refresh = Arc::new(AtomicBool::new(false));
        let quarantine = Arc::new(AtomicBool::new(false));
        let streaming = Arc::new(AtomicBool::new(false));
        let wx_iq_flag = Arc::new(AtomicBool::new(false));

        // Per-device channels
        let (sdr_cmd_tx, sdr_cmd_rx) = mpsc::channel::<rf_sdr::SdrCommand>();
        let (psd_tx, psd_rx) = mpsc::channel::<rf_scan::PsdInput>();
        let (scan_cmd_tx, scan_cmd_rx) = mpsc::channel::<rf_scan::ScanCommand>();
        let (tune_cmd_tx, tune_cmd_rx) = mpsc::channel::<rf_scan::SdrTuneCommand>();
        let (dsp_cmd_tx, dsp_cmd_rx) = mpsc::channel::<dsp_bridge::DspCommand>();

        // Per-device IQ ring buffer
        let (producer, consumer) = rf_sdr::IqRingBuffer::new(262144);

        // IQ recording mirror: second ring buffer for forwarding to recorder
        let iq_record_flag = Arc::new(AtomicBool::new(false));
        let (iq_mirror_producer, iq_mirror_consumer) = rf_sdr::IqRingBuffer::new(262144);

        // Spawn SDR reader thread with IQ mirror
        let _reader = rf_sdr::spawn_sdr_reader_with_iq_mirror(
            device,
            producer,
            sdr_cmd_rx,
            Arc::clone(&alive),
            Arc::clone(&refresh),
            sample_rate,
            key.clone(),
            reconnect_args,
            Arc::clone(&quarantine),
            Some(iq_mirror_producer),
            Some(Arc::clone(&iq_record_flag)),
        );

        // Spawn IQ forwarder thread: reads from mirror buffer, sends to recorder
        if let Some(rec_tx_clone) = rec_tx {
            let fwd_flag = Arc::clone(&iq_record_flag);
            let fwd_shutdown = Arc::clone(&shutdown);
            let fwd_key = key.clone();
            std::thread::Builder::new()
                .name(format!("iq_fwd_{}", key))
                .spawn(move || {
                    let mut buf = vec![rf_sdr::IqSample::new(0.0, 0.0); 16384];
                    loop {
                        if fwd_shutdown.load(Ordering::Relaxed) { break; }
                        if !fwd_flag.load(Ordering::Relaxed) {
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            continue;
                        }
                        let n = iq_mirror_consumer.pop_slice(&mut buf);
                        if n > 0 {
                            let _ = rec_tx_clone.send(rf_recorder::RecorderCommand::IqData(buf[..n].to_vec()));
                        } else {
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                    }
                    tracing::info!("[{}] IQ forwarder thread exited", fwd_key);
                })
                .expect("failed to spawn IQ forwarder");
        }

        // Spawn DSP pipeline thread
        // Per-device DSP threads all produce PSD data for their scan controllers.
        let audio_tx_for_dsp = _audio_tx.clone();
        let rds_tx_for_dsp = _rds_tx.clone();
        let p25_tx_for_dsp = _p25_tx.clone();
        let iq_source = dsp_bridge::IqConsumerAdapter(consumer);
        let dsp_config = rf_dsp::DspConfig::default();
        let dsp_label = key.clone();
        let dsp_device_label = key.clone();
        let dsp_shutdown = Arc::clone(&shutdown);
        let dsp_streaming = Arc::clone(&streaming);
        let dsp_wx_iq_flag = Arc::clone(&wx_iq_flag);
        let _dsp = std::thread::Builder::new()
            .name(format!("dsp_{}", key))
            .spawn(move || {
                dsp_bridge::run_dsp_pipeline(
                    iq_source, psd_tx, dsp_config, dsp_cmd_rx,
                    audio_tx_for_dsp, rds_tx_for_dsp, p25_tx_for_dsp, sample_rate,
                    dsp_shutdown,
                    dsp_device_label,
                    dsp_streaming,
                    Some(dsp_wx_iq_flag),
                );
                tracing::info!("[{}] DSP pipeline exited", dsp_label);
            })
            .expect("failed to spawn DSP pipeline");

        // Spawn scan controller thread
        let scan_status = Arc::new(RwLock::new(ScanStatus::default()));
        let freq_db = rf_scan::FreqDb::load(freq_json_path);
        let per_device_frame_tx = frame_tx.clone();
        let scan_device_key = key.clone();
        let scan_wx_iq_flag = Arc::clone(&wx_iq_flag);
        let _scan = rf_scan::spawn_scan_controller(
            bands.to_vec(),
            freq_db,
            tune_cmd_tx,
            psd_rx,
            per_device_frame_tx,
            scan_cmd_rx,
            Arc::clone(&scan_status),
            scan_device_key,
            wx_alert_tx,
            Some(scan_wx_iq_flag),
        );

        // Tune bridge: scan controller → SDR reader
        let sdr_cmd_tx_for_tune = sdr_cmd_tx.clone();
        let tune_label = key.clone();
        let _tune = std::thread::Builder::new()
            .name(format!("tune_{}", key))
            .spawn(move || {
                while let Ok(cmd) = tune_cmd_rx.recv() {
                    match cmd {
                        rf_scan::SdrTuneCommand::SetFreq(freq) => {
                            let _ = sdr_cmd_tx_for_tune
                                .send(rf_sdr::SdrCommand::SetFreq(freq));
                        }
                    }
                }
                tracing::info!("[{}] Tune bridge exited", tune_label);
            })
            .expect("failed to spawn tune bridge");

        // Set initial bands on scan controller
        if !assigned_bands.is_empty() {
            let _ = scan_cmd_tx.send(rf_scan::ScanCommand::SetBands(assigned_bands.clone()));
        }

        DeviceSlot {
            device_key: key,
            info,
            alive,
            refresh,
            quarantine,
            streaming,
            wx_iq_flag,
            sdr_cmd_tx,
            dsp_cmd_tx,
            scan_cmd_tx,
            scan_status,
            assigned_bands,
            role: "idle".into(),
            sample_rate,
            iq_record_flag,
        }
    }

    /// Send a ScanCommand to all slot scan controllers.
    pub fn broadcast_scan_cmd(&self, cmd_fn: impl Fn() -> rf_scan::ScanCommand) {
        for slot in &self.slots {
            let _ = slot.scan_cmd_tx.send(cmd_fn());
        }
    }

    /// Send an SdrCommand to all slot readers.
    pub fn broadcast_sdr_cmd(&self, cmd_fn: impl Fn() -> rf_sdr::SdrCommand) {
        for slot in &self.slots {
            let _ = slot.sdr_cmd_tx.send(cmd_fn());
        }
    }

    /// Set the antenna constraint map (serial → freq_min/max MHz).
    pub fn set_antenna_map(&mut self, map: HashMap<String, (f64, f64)>) {
        self.antenna_map = map;
    }

    /// Redistribute enabled bands across all alive, non-quarantined, streaming slots.
    /// Uses segment-balanced distribution with band splitting for optimal load balancing.
    /// If no device has started streaming yet (warmup phase), streaming filter is skipped.
    /// When antenna constraints are set, bands outside the antenna's range are excluded per-device.
    pub fn redistribute(&mut self, enabled_bands: &[String]) {
        let any_streaming = self.slots.iter()
            .any(|s| s.streaming.load(Ordering::Relaxed));

        let alive_indices: Vec<usize> = self.slots.iter()
            .enumerate()
            .filter(|(_, s)| {
                s.alive.load(Ordering::Relaxed)
                    && !s.quarantine.load(Ordering::Relaxed)
                    && (!any_streaming || s.streaming.load(Ordering::Relaxed))
            })
            .map(|(i, _)| i)
            .collect();

        if alive_indices.is_empty() {
            return;
        }

        // If no antenna constraints, use the standard distribution
        if self.antenna_map.is_empty() {
            let distribution = distribute_segments(
                enabled_bands, alive_indices.len(), &self.band_defs, self.sample_rate,
            );

            for slot in &mut self.slots {
                slot.assigned_bands.clear();
            }

            for (dist_idx, &slot_idx) in alive_indices.iter().enumerate() {
                if let Some(ranges) = distribution.get(dist_idx) {
                    let band_keys: Vec<String> = ranges.iter()
                        .map(|r| r.key.clone())
                        .collect::<std::collections::HashSet<_>>()
                        .into_iter()
                        .collect();
                    self.slots[slot_idx].assigned_bands = band_keys;
                    let _ = self.slots[slot_idx].scan_cmd_tx.send(
                        rf_scan::ScanCommand::SetBandRanges(ranges.clone()),
                    );
                }
            }
            return;
        }

        // Antenna-aware distribution: filter bands per device based on antenna range,
        // then distribute only eligible bands to each device.
        // Build per-device eligible band lists.
        let mut per_device_bands: Vec<Vec<String>> = Vec::new();
        for &slot_idx in &alive_indices {
            let serial = &self.slots[slot_idx].info.serial;
            if let Some(&(ant_min, ant_max)) = self.antenna_map.get(serial) {
                // Filter to bands whose center falls within antenna range
                let eligible: Vec<String> = enabled_bands.iter().filter(|key| {
                    if let Some(bdef) = self.band_defs.iter().find(|b| b.key == **key) {
                        let center = (bdef.start + bdef.end) / 2.0;
                        center >= ant_min && center <= ant_max
                    } else {
                        false
                    }
                }).cloned().collect();
                if eligible.len() < enabled_bands.len() {
                    let skipped = enabled_bands.len() - eligible.len();
                    tracing::info!(
                        "[pool] Antenna constraint for {} ({:.1}–{:.1} MHz): {}/{} bands eligible, {} skipped",
                        self.slots[slot_idx].device_key, ant_min, ant_max,
                        eligible.len(), enabled_bands.len(), skipped,
                    );
                }
                per_device_bands.push(eligible);
            } else {
                // No antenna constraint — all bands eligible
                per_device_bands.push(enabled_bands.to_vec());
            }
        }

        // Simple approach: distribute each band only to devices that can receive it.
        // For each band, find which alive devices can handle it, then use LPT to balance.
        for slot in &mut self.slots {
            slot.assigned_bands.clear();
        }

        // Collect all band ranges for each alive device
        let mut device_ranges: Vec<Vec<rf_scan::BandRange>> = vec![Vec::new(); alive_indices.len()];
        let mut device_totals: Vec<usize> = vec![0; alive_indices.len()];

        // Sort bands largest-first for better LPT scheduling
        let mut band_items: Vec<(&String, usize)> = enabled_bands.iter().map(|key| {
            let segs = self.band_defs.iter()
                .find(|b| b.key == *key)
                .map(|b| band_segment_count(b, self.sample_rate))
                .unwrap_or(1);
            (key, segs)
        }).collect();
        band_items.sort_by(|a, b| b.1.cmp(&a.1));

        for (band_key, segs) in band_items {
            // Find eligible devices for this band
            let eligible: Vec<usize> = (0..alive_indices.len())
                .filter(|&di| per_device_bands[di].contains(band_key))
                .collect();

            if eligible.is_empty() {
                tracing::warn!("[pool] Band {} has no eligible device (antenna constraints), skipping", band_key);
                continue;
            }

            // Assign to the eligible device with lowest current total (LPT)
            let best = eligible.iter()
                .min_by_key(|&&di| device_totals[di])
                .copied()
                .unwrap_or(0);

            if let Some(bdef) = self.band_defs.iter().find(|b| b.key == *band_key) {
                device_ranges[best].push(rf_scan::BandRange {
                    key: band_key.clone(),
                    start_mhz: bdef.start,
                    end_mhz: bdef.end,
                });
                device_totals[best] += segs;
            }
        }

        // Dispatch to slots
        for (dist_idx, &slot_idx) in alive_indices.iter().enumerate() {
            let ranges = &device_ranges[dist_idx];
            let band_keys: Vec<String> = ranges.iter()
                .map(|r| r.key.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            self.slots[slot_idx].assigned_bands = band_keys;
            let _ = self.slots[slot_idx].scan_cmd_tx.send(
                rf_scan::ScanCommand::SetBandRanges(ranges.clone()),
            );
        }
    }

    /// Generate SdrSlotStatus snapshot for AppState.
    /// `device_names` maps serial → user_name (from DB sdr_devices table).
    pub fn to_slot_statuses(&self, device_names: &HashMap<String, String>) -> Vec<SdrSlotStatus> {
        self.slots.iter().map(|s| SdrSlotStatus {
            device_key: s.device_key.clone(),
            label: s.info.label.clone(),
            driver: s.info.driver.clone(),
            serial: s.info.serial.clone(),
            user_name: device_names.get(&s.info.serial).cloned().unwrap_or_default(),
            role: s.role.clone(),
            alive: s.alive.load(Ordering::Relaxed),
            quarantined: s.quarantine.load(Ordering::Relaxed),
            streaming: s.streaming.load(Ordering::Relaxed),
            assigned_bands: s.assigned_bands.clone(),
            sample_rate: s.sample_rate,
        }).collect()
    }

    /// Get scan_statuses for registering with AppState.
    pub fn scan_statuses(&self) -> Vec<(String, Arc<RwLock<ScanStatus>>)> {
        self.slots.iter().map(|s| {
            (s.device_key.clone(), Arc::clone(&s.scan_status))
        }).collect()
    }

    /// Clear the quarantine flag on a specific device by key.
    #[allow(dead_code)] // Will be wired to SDR device recovery UI
    pub fn unquarantine(&self, device_key: &str) -> bool {
        if let Some(slot) = self.slots.iter().find(|s| s.device_key == device_key) {
            slot.quarantine.store(false, Ordering::Release);
            tracing::info!("[pool] Quarantine cleared for {}", device_key);
            true
        } else {
            false
        }
    }

    /// Get the first slot (primary device for monitor mode).
    pub fn primary(&self) -> Option<&DeviceSlot> {
        self.slots.first()
    }

    /// Get the first slot mutably.
    pub fn primary_mut(&mut self) -> Option<&mut DeviceSlot> {
        self.slots.first_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> Vec<rf_scan::BandDef> {
        rf_scan::bands::default_bands()
    }
    const SR: f64 = 2_400_000.0;

    #[test]
    fn test_distribute_bands_single() {
        let bands = vec!["VHF".into(), "UHF".into(), "P25".into()];
        let result = distribute_bands(&bands, 1, &defs(), SR);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 3);
    }

    #[test]
    fn test_distribute_bands_two() {
        let bands: Vec<String> = vec!["AM", "HF", "FM", "VHF", "FEDV", "BIII", "UHF", "GMRS", "P25"]
            .into_iter().map(String::from).collect();
        let result = distribute_bands(&bands, 2, &defs(), SR);
        // Should balance by segment count, not just band count
        let seg_counts: Vec<usize> = result.iter().map(|device_bands| {
            device_bands.iter().map(|k| {
                defs().iter().find(|b| b.key == *k)
                    .map(|b| band_segment_count(b, SR))
                    .unwrap_or(1)
            }).sum()
        }).collect();
        // Both devices should have similar segment counts
        let diff = (seg_counts[0] as i64 - seg_counts[1] as i64).unsigned_abs();
        assert!(diff <= 15, "Segment imbalance too large: {:?} (diff={})", seg_counts, diff);
    }

    #[test]
    fn test_distribute_bands_three_balanced() {
        // The key scenario: VHF(16) + UHF(47) + P25(38) on 3 devices
        let bands: Vec<String> = vec!["VHF", "UHF", "P25"]
            .into_iter().map(String::from).collect();
        let result = distribute_bands(&bands, 3, &defs(), SR);
        // Each device should get exactly one band (UHF=47, P25=38, VHF=16)
        assert_eq!(result.len(), 3);
        for device_bands in &result {
            assert_eq!(device_bands.len(), 1);
        }
        // UHF (largest) should be first assigned
        assert!(result.iter().any(|b| b.contains(&"UHF".to_string())));
        assert!(result.iter().any(|b| b.contains(&"P25".to_string())));
        assert!(result.iter().any(|b| b.contains(&"VHF".to_string())));
    }

    #[test]
    fn test_distribute_all_bands_three_devices() {
        // All 8 bands on 3 devices — should balance ~50 segments each
        let bands: Vec<String> = vec!["AM", "HF", "FM", "VHF", "FEDV", "BIII", "UHF", "GMRS", "P25"]
            .into_iter().map(String::from).collect();
        let result = distribute_bands(&bands, 3, &defs(), SR);
        let seg_counts: Vec<usize> = result.iter().map(|device_bands| {
            device_bands.iter().map(|k| {
                defs().iter().find(|b| b.key == *k)
                    .map(|b| band_segment_count(b, SR))
                    .unwrap_or(1)
            }).sum()
        }).collect();
        let max = *seg_counts.iter().max().unwrap();
        let min = *seg_counts.iter().min().unwrap();
        // Max imbalance should be much less than 2x
        assert!(max <= min * 2, "Imbalance too large: {:?}", seg_counts);
    }

    #[test]
    fn test_distribute_bands_empty() {
        let result = distribute_bands(&[], 2, &defs(), SR);
        assert_eq!(result, vec![Vec::<String>::new(), Vec::<String>::new()]);
    }

    #[test]
    fn test_distribute_bands_zero_devices() {
        let bands = vec!["VHF".into()];
        let result = distribute_bands(&bands, 0, &defs(), SR);
        assert!(result.is_empty());
    }

    #[test]
    fn test_distribute_segments_uhf_splits() {
        // UHF alone on 3 devices — should split into ~3 sub-ranges
        let bands = vec!["UHF".into()];
        let result = distribute_segments(&bands, 3, &defs(), SR);
        // Each device should get at least one sub-range
        let total_ranges: usize = result.iter().map(|v| v.len()).sum();
        assert!(total_ranges >= 2, "Expected UHF to split, got {} ranges", total_ranges);
        // All ranges should have key "UHF"
        for device_ranges in &result {
            for r in device_ranges {
                assert_eq!(r.key, "UHF");
            }
        }
    }

    #[test]
    fn test_distribute_segments_balanced() {
        // All bands on 3 devices — segments should be balanced
        let bands: Vec<String> = vec!["AM", "HF", "FM", "VHF", "FEDV", "BIII", "UHF", "GMRS", "P25"]
            .into_iter().map(String::from).collect();
        let result = distribute_segments(&bands, 3, &defs(), SR);
        let seg_counts: Vec<usize> = result.iter().map(|device_ranges| {
            device_ranges.iter().map(|r| {
                let bw = (r.end_mhz - r.start_mhz) * 1e6;
                if bw <= SR { 1 } else { (bw / SR).ceil() as usize }
            }).sum()
        }).collect();
        let max = *seg_counts.iter().max().unwrap();
        let min = *seg_counts.iter().min().unwrap();
        assert!(max <= min * 2, "Segment imbalance too large: {:?}", seg_counts);
    }

    #[test]
    fn test_distribute_segments_small_band_no_split() {
        // AM (1 segment) on 3 devices — should NOT split
        let bands = vec!["AM".into()];
        let result = distribute_segments(&bands, 3, &defs(), SR);
        let total_ranges: usize = result.iter().map(|v| v.len()).sum();
        assert_eq!(total_ranges, 1, "AM should not split");
    }
}
