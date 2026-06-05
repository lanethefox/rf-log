use rf_dsp::pipeline::IqConsumerLike;
use rf_recorder::{AudioChunk, P25ClipEvent};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

/// Commands sent to the DSP pipeline thread to switch modes.
#[allow(dead_code)]
pub enum DspCommand {
    ScanMode,
    MonitorMode {
        center_freq: f64,
        target_freq: f64,
        mode: String,
        squelch: f64,
    },
    SetSquelch(f64),
    SetModulation(String),
    SetFrequency {
        center_freq: f64,
        target_freq: f64,
    },
    /// Set demod filter bandwidth in Hz (0 = mode default).
    SetBandwidth(f64),
}

/// Adapter to bridge rf_sdr::IqConsumer → rf_dsp::IqSource.
pub struct IqConsumerAdapter(pub rf_sdr::IqConsumer);

impl IqConsumerLike for IqConsumerAdapter {
    fn pop_slice(&self, out: &mut [num_complex::Complex32]) -> usize {
        self.0.pop_slice(out)
    }
    fn available(&self) -> usize {
        self.0.available()
    }
}

/// Run the DSP pipeline with scan/monitor mode switching.
pub fn run_dsp_pipeline(
    source: IqConsumerAdapter,
    psd_tx: mpsc::Sender<rf_scan::PsdInput>,
    config: rf_dsp::DspConfig,
    cmd_rx: mpsc::Receiver<DspCommand>,
    audio_tx: mpsc::Sender<AudioChunk>,
    rds_tx: mpsc::Sender<String>,
    p25_tx: mpsc::Sender<String>,
    sample_rate: f64,
    shutdown: Arc<AtomicBool>,
    device_label: String,
    streaming: Arc<AtomicBool>,
    wx_iq_flag: Option<Arc<AtomicBool>>,
) {
    let mut fft = rf_dsp::FftProcessor::new(config.fft_size);
    let mut averager =
        rf_dsp::PsdAverager::new(config.averaging_alpha, config.fft_size);
    let mut iq_buf =
        vec![num_complex::Complex32::new(0.0, 0.0); config.fft_size];
    let mut monitor: Option<rf_dsp::MonitorPipeline> = None;
    let mut psd_frame_count: u64 = 0;
    let mut starvation_count: u64 = 0;
    let dsp_start = std::time::Instant::now();
    let mut last_cqpsk_status = std::time::Instant::now();
    let mut last_monitor_diag = std::time::Instant::now();
    let mut monitor_chunks: u64 = 0;
    let mut monitor_samples: u64 = 0;
    // Throttle P25 metadata: only send when content (NAC/TG/UID/enc) changes
    let mut last_p25_content: (u16, Option<u32>, Option<u32>, bool) = (0, None, None, false);
    // Clip recording state: track current freq/mode for AudioChunk tagging
    let mut current_freq_mhz: f64 = 0.0;
    let mut current_mode: String = String::new();
    // P25 boundary detection: track last duid for transmission start/end
    let mut last_duid: String = String::new();
    // Cached P25 metadata for AudioChunk tagging
    let mut cached_talkgroup: Option<u32> = None;
    let mut cached_source_unit: Option<u32> = None;
    let mut cached_encrypted: bool = false;


    tracing::info!(
        "[{}] DSP pipeline started (scan mode, fft_size={})",
        device_label, config.fft_size
    );

    loop {
        if shutdown.load(Ordering::Relaxed) {
            tracing::info!("DSP pipeline: shutdown signal received");
            break;
        }

        // Check for mode switch commands (non-blocking)
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                DspCommand::ScanMode => {
                    monitor = None;
                    averager = rf_dsp::PsdAverager::new(
                        config.averaging_alpha,
                        config.fft_size,
                    );
                    tracing::info!("DSP: switched to scan mode");
                }
                DspCommand::MonitorMode {
                    center_freq,
                    target_freq,
                    mode,
                    squelch,
                } => {
                    current_freq_mhz = target_freq / 1_000_000.0;
                    current_mode = mode.clone();
                    monitor = Some(rf_dsp::MonitorPipeline::new(
                        sample_rate,
                        target_freq,
                        center_freq,
                        &mode,
                        squelch,
                    ));
                    tracing::info!(
                        "DSP: switched to monitor mode (target={:.6} MHz)",
                        target_freq / 1_000_000.0
                    );
                }
                DspCommand::SetSquelch(level) => {
                    if let Some(ref mut m) = monitor {
                        m.set_squelch(level);
                    }
                }
                DspCommand::SetModulation(mode) => {
                    if let Some(ref mut m) = monitor {
                        m.set_mode(&mode);
                        current_mode = mode.clone();
                        tracing::info!("DSP: modulation changed to {}", mode);
                    }
                }
                DspCommand::SetFrequency {
                    center_freq,
                    target_freq,
                } => {
                    current_freq_mhz = target_freq / 1_000_000.0;
                    if let Some(ref mut m) = monitor {
                        m.set_frequency(target_freq, center_freq);
                    }
                }
                DspCommand::SetBandwidth(bw_hz) => {
                    if let Some(ref mut m) = monitor {
                        m.set_bandwidth(bw_hz);
                        tracing::info!("DSP: bandwidth set to {} Hz", bw_hz);
                    }
                }
            }
        }

        // Wait for enough samples
        if source.available() < config.fft_size {
            starvation_count += 1;
            // Log starvation every 10 seconds (20000 iterations at 500us each)
            if starvation_count == 1 || starvation_count % 200000 == 0 {
                tracing::warn!(
                    "[{}] DSP starved: available={}, need={}, starvation_count={}",
                    device_label, source.available(), config.fft_size, starvation_count
                );
            }
            std::thread::sleep(std::time::Duration::from_micros(500));
            continue;
        }

        let n = source.pop_slice(&mut iq_buf);
        if n < config.fft_size {
            continue;
        }

        if let Some(ref mut mon) = monitor {
            // Monitor mode: demod IQ → audio
            // IMPORTANT: Send audio FIRST, then process metadata (matches original working flow).
            let audio_vec = mon.process(&iq_buf[..n]).to_vec();
            if !audio_vec.is_empty() {
                monitor_chunks += 1;
                monitor_samples += audio_vec.len() as u64;
                let squelch_open = mon.is_squelch_open();
                let _ = audio_tx.send(AudioChunk {
                    samples: audio_vec,
                    device_key: device_label.clone(),
                    freq_mhz: current_freq_mhz,
                    modulation: current_mode.clone(),
                    squelch_open,
                    p25_event: None,
                    talkgroup: cached_talkgroup,
                    source_unit: cached_source_unit,
                    encrypted: cached_encrypted,
                });
            }
            // Monitor diagnostics every 5s
            if last_monitor_diag.elapsed().as_secs() >= 5 {
                let (dibits, audio_events, duids) = mon.p25_decode_stats();
                tracing::info!(
                    "[{}] MONITOR: freq={:.4}MHz mode={} chunks={} samples={} squelch={} p25[syncs={} errors={} HDU={} LDU1={} LDU2={} TLC={} TSBK={}]",
                    device_label, current_freq_mhz, current_mode,
                    monitor_chunks, monitor_samples, mon.is_squelch_open(),
                    dibits, audio_events, duids[0], duids[1], duids[2], duids[3], duids[4]
                );
                last_monitor_diag = std::time::Instant::now();
            }


            // Check for P25 metadata updates (P25 mode)
            if let Some(p25) = mon.take_p25_metadata() {
                // Update cached P25 metadata for AudioChunk tagging
                cached_talkgroup = p25.talkgroup;
                cached_source_unit = p25.source_unit;
                cached_encrypted = p25.encrypted;

                // P25 boundary detection: send clip events as separate AudioChunks
                if p25.duid != last_duid {
                    let clip_event = match p25.duid.as_str() {
                        "HDU" => Some(P25ClipEvent::TransmissionStart),
                        "TLC" => Some(P25ClipEvent::TransmissionEnd),
                        "LDU1" | "LDU2" => Some(P25ClipEvent::VoiceFrame),
                        _ => None,
                    };
                    if let Some(evt) = clip_event {
                        let _ = audio_tx.send(AudioChunk {
                            samples: Vec::new(),
                            device_key: device_label.clone(),
                            freq_mhz: current_freq_mhz,
                            modulation: current_mode.clone(),
                            squelch_open: false,
                            p25_event: Some(evt),
                            talkgroup: cached_talkgroup,
                            source_unit: cached_source_unit,
                            encrypted: cached_encrypted,
                        });
                    }
                    // Send boundary DUIDs (TLC/HDU) un-throttled so the SIEM
                    // session correlator sees transmission start/end events.
                    // The content-change throttle below may suppress these
                    // since TG/UID/enc don't change at TLC boundaries.
                    if matches!(p25.duid.as_str(), "TLC" | "HDU") {
                        let boundary_msg = serde_json::json!({
                            "type": "p25",
                            "nac": p25.nac,
                            "duid": p25.duid,
                            "talkgroup": p25.talkgroup,
                            "source_unit": p25.source_unit,
                            "encrypted": p25.encrypted,
                            "freq_mhz": current_freq_mhz,
                        });
                        let _ = p25_tx.send(boundary_msg.to_string());
                    }
                    last_duid = p25.duid.clone();
                }

                // Throttle: only send JSON when content changes (TG/UID/enc), not on every
                // duid change. CC SDR fires metadata ~30x/sec on NID decodes which
                // floods the broadcast channel if sent unconditionally.
                let content_key = (p25.nac, p25.talkgroup, p25.source_unit, p25.encrypted);
                if content_key != last_p25_content {
                    last_p25_content = content_key;
                    let mut msg = serde_json::json!({
                        "type": "p25",
                        "nac": p25.nac,
                        "duid": p25.duid,
                        "talkgroup": p25.talkgroup,
                        "source_unit": p25.source_unit,
                        "encrypted": p25.encrypted,
                        "algorithm": p25.algorithm,
                        "key_id": p25.key_id,
                        "freq_mhz": current_freq_mhz,
                    });
                    // Include CQPSK demod diagnostics
                    if let Some(diag) = mon.cqpsk_diag() {
                        msg["cqpsk_locked"] = serde_json::json!(diag.locked);
                        msg["cqpsk_freq_offset"] = serde_json::json!(diag.carrier_freq_hz);
                        msg["cqpsk_phase_err"] = serde_json::json!(diag.phase_err_rms);
                        msg["cqpsk_symbols"] = serde_json::json!(diag.symbols_out);
                    }
                    let _ = p25_tx.send(msg.to_string());
                }
            }

            // Check for RDS metadata updates (WFM mode)
            if let Some(rds) = mon.take_rds_update() {
                let msg = serde_json::json!({
                    "type": "rds",
                    "pi": rds.pi,
                    "ps": rds.ps,
                    "rt": rds.rt,
                    "pty": rds.pty,
                    "pty_name": rds.pty_name,
                });
                let _ = rds_tx.send(msg.to_string());
            }
            // Check for RF fingerprint (finalized on squelch close)
            if let Some(fp) = mon.take_fingerprint() {
                let msg = serde_json::json!({
                    "type": "fingerprint",
                    "cfo_hz": fp.cfo_hz,
                    "iq_amplitude_imbal": fp.iq_amplitude_imbal,
                    "iq_phase_imbal": fp.iq_phase_imbal,
                    "avg_power_db": fp.avg_power_db,
                    "power_variance": fp.power_variance,
                    "sample_count": fp.sample_count,
                    "freq_mhz": current_freq_mhz,
                });
                let _ = p25_tx.send(msg.to_string());
            }
            // Check for TSBK events (P25 control channel data)
            for tsbk in mon.take_tsbk_events() {
                let msg = serde_json::json!({
                    "type": "tsbk",
                    "opcode": tsbk.opcode,
                    "opcode_raw": tsbk.opcode_raw,
                    "protected": tsbk.protected,
                    "crc_valid": tsbk.crc_valid,
                    "payload": tsbk.payload,
                });
                let _ = p25_tx.send(msg.to_string());
            }
            // Periodic P25 decode status (1 Hz) for CC hunt diagnostics
            if last_cqpsk_status.elapsed().as_secs_f64() >= 1.0 {
                // Log FM demod output levels and sync/error stats for P25 signal diagnostics
                let (syncs, errors, duids) = mon.p25_decode_stats();
                if let Some((min, max, mean)) = mon.take_p25_demod_stats() {
                    tracing::info!(
                        "[{}] P25 C4FM demod: min={:.4}, max={:.4}, mean={:.4} | syncs={}, errors={}, duids=[HDU={},ST={},LCT={},LCFG={},CCFG={},PDU={},TSBK={}]",
                        device_label, min, max, mean, syncs, errors,
                        duids[0], duids[1], duids[2], duids[3], duids[4], duids[5], duids[6]
                    );
                }
                let msg = if let Some(diag) = mon.cqpsk_diag() {
                    // CQPSK demod path
                    serde_json::json!({
                        "type": "cqpsk_status",
                        "locked": diag.locked,
                        "freq_offset_hz": diag.carrier_freq_hz,
                        "phase_err_rms": diag.phase_err_rms,
                        "symbols": diag.symbols_out,
                        "dibits_fed": mon.p25_dibits_fed,
                        "p25_events": mon.p25_events_out,
                        "tsbks_decoded": mon.p25_tsbks_decoded,
                    })
                } else {
                    // C4FM mode — report sync count for CC hunt decisions
                    let (sync_count, _, _) = mon.p25_decode_stats();
                    serde_json::json!({
                        "type": "cqpsk_status",
                        "locked": mon.p25_tsbks_decoded > 0,
                        "freq_offset_hz": 0.0,
                        "phase_err_rms": 0.0,
                        "symbols": 0u64,
                        "dibits_fed": mon.p25_dibits_fed,
                        "p25_events": mon.p25_events_out,
                        "tsbks_decoded": mon.p25_tsbks_decoded,
                        "sync_nid_count": sync_count,
                    })
                };
                let _ = p25_tx.send(msg.to_string());
                last_cqpsk_status = std::time::Instant::now();
            }
        } else {
            // Scan mode: FFT → PSD → scan controller
            let psd = fft.process(&iq_buf);
            averager.update(&psd);

            // Include raw IQ when WX check is active (for SAME decode)
            let iq_for_wx = wx_iq_flag.as_ref()
                .filter(|f| f.load(Ordering::Acquire))
                .map(|_| iq_buf[..n].to_vec());

            let frame = rf_scan::PsdInput {
                psd: averager.get().to_vec(),
                fft_size: config.fft_size,
                iq_samples: iq_for_wx,
            };

            if psd_tx.send(frame).is_err() {
                tracing::info!(
                    "[{}] DSP pipeline: PSD channel closed, stopping",
                    device_label
                );
                break;
            }
            psd_frame_count += 1;
            if psd_frame_count == 1 {
                streaming.store(true, Ordering::Release);
            }
            if psd_frame_count == 1 || psd_frame_count % 5000 == 0 {
                let elapsed = dsp_start.elapsed().as_secs_f64();
                tracing::info!(
                    "[{}] DSP: {} PSD frames in {:.1}s ({:.1} fps)",
                    device_label, psd_frame_count, elapsed,
                    psd_frame_count as f64 / elapsed
                );
            }
        }
    }
}
