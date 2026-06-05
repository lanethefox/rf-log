//! rf-recorder — Multi-slot WAV/IQ recording engine.
//!
//! Receives audio (f32) and IQ (Complex32) data via mpsc commands,
//! writes to disk as WAV (hound) or raw IQ files, and reports status.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write as IoWrite};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use serde::Serialize;

/// Tagged audio chunk from the DSP pipeline.
/// Carries metadata needed for automatic clip recording.
#[derive(Clone, Debug)]
pub struct AudioChunk {
    pub samples: Vec<f32>,       // 48 kHz f32 PCM
    pub device_key: String,
    pub freq_mhz: f64,
    pub modulation: String,
    pub squelch_open: bool,
    pub p25_event: Option<P25ClipEvent>,
    pub talkgroup: Option<u32>,
    pub source_unit: Option<u32>,
    pub encrypted: bool,
}

/// P25 transmission boundary events for clip segmentation.
#[derive(Clone, Debug, PartialEq)]
pub enum P25ClipEvent {
    /// HDU received — start of a new voice transmission.
    TransmissionStart,
    /// TLC received — end of voice transmission.
    TransmissionEnd,
    /// LDU1/LDU2 — voice frame in progress.
    VoiceFrame,
}

/// Commands sent to the recorder thread.
pub enum RecorderCommand {
    /// Start recording audio to WAV file.
    StartAudio {
        db_id: i64,
        freq_mhz: f64,
        file_path: PathBuf,
    },
    /// Start recording raw IQ to file.
    StartIq {
        db_id: i64,
        freq_mhz: f64,
        file_path: PathBuf,
        sample_rate: u32,
    },
    /// Stop a specific recording slot.
    Stop { db_id: i64 },
    /// Stop all active recordings.
    StopAll,
    /// Audio data from the DSP pipeline (forwarded to all active audio slots).
    AudioData(Vec<f32>),
    /// Audio data targeted to a specific recording slot (by db_id).
    /// Used by the clip manager to route audio to individual clip slots.
    AudioDataFor { db_id: i64, samples: Vec<f32> },
    /// IQ data from the DSP pipeline (forwarded to all active IQ slots).
    IqData(Vec<num_complex::Complex32>),
    /// Shutdown the recorder thread.
    Shutdown,
}

/// Status of a single active recording slot.
#[derive(Debug, Clone, Serialize)]
pub struct ActiveSlot {
    pub db_id: i64,
    pub freq_mhz: f64,
    pub duration_sec: f64,
    pub file_size_bytes: u64,
}

/// Aggregated status of the recorder engine.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RecorderStatus {
    pub active_audio: Vec<ActiveSlot>,
    pub active_iq: Vec<ActiveSlot>,
}

/// Result from finalizing a recording slot.
#[derive(Debug, Clone)]
pub struct FinalizeResult {
    pub db_id: i64,
    pub file_size_bytes: u64,
    pub duration_sec: f64,
}

enum SlotWriter {
    Wav(hound::WavWriter<BufWriter<File>>),
    Iq(BufWriter<File>),
}

struct RecordingSlot {
    db_id: i64,
    freq_mhz: f64,
    slot_type: SlotType,
    writer: SlotWriter,
    samples_written: u64,
    #[allow(dead_code)] // stored for future file_size_bytes calculation
    sample_rate: u32,
    started: Instant,
    /// Consecutive write errors — slot is removed after MAX_WRITE_ERRORS.
    write_errors: u32,
}

const MAX_WRITE_ERRORS: u32 = 5;

#[derive(Clone, Copy, PartialEq)]
enum SlotType {
    Audio,
    Iq,
}

impl RecordingSlot {
    fn duration_sec(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn file_size_bytes(&self) -> u64 {
        match self.slot_type {
            SlotType::Audio => {
                // WAV header (44 bytes) + samples * 4 bytes (f32)
                44 + self.samples_written * 4
            }
            SlotType::Iq => {
                // Interleaved f32 pairs: samples * 8 bytes
                self.samples_written * 8
            }
        }
    }

    fn to_active_slot(&self) -> ActiveSlot {
        ActiveSlot {
            db_id: self.db_id,
            freq_mhz: self.freq_mhz,
            duration_sec: self.duration_sec(),
            file_size_bytes: self.file_size_bytes(),
        }
    }
}

/// Run the recorder engine loop. Call from a dedicated thread.
///
/// Returns a vec of finalize results for any slots that were active at shutdown.
pub fn run_recorder(
    cmd_rx: mpsc::Receiver<RecorderCommand>,
    status_tx: mpsc::Sender<RecorderStatus>,
    finalize_tx: mpsc::Sender<FinalizeResult>,
) {
    let mut slots: HashMap<i64, RecordingSlot> = HashMap::new();
    let mut last_status = Instant::now();

    tracing::info!("Recorder thread started");

    loop {
        // Use try_recv with a small sleep for responsiveness
        let cmd = match cmd_rx.recv_timeout(std::time::Duration::from_millis(10)) {
            Ok(cmd) => Some(cmd),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::info!("Recorder command channel disconnected, shutting down");
                break;
            }
        };

        if let Some(cmd) = cmd {
            match cmd {
                RecorderCommand::StartAudio { db_id, freq_mhz, file_path } => {
                    // Ensure parent directory exists
                    if let Some(parent) = file_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    let spec = hound::WavSpec {
                        channels: 1,
                        sample_rate: 48000,
                        bits_per_sample: 32,
                        sample_format: hound::SampleFormat::Float,
                    };
                    match hound::WavWriter::create(&file_path, spec) {
                        Ok(writer) => {
                            tracing::info!(
                                "Recording audio: db_id={}, freq={:.4} MHz, path={}",
                                db_id, freq_mhz, file_path.display()
                            );
                            slots.insert(db_id, RecordingSlot {
                                db_id,
                                freq_mhz,
                                slot_type: SlotType::Audio,
                                writer: SlotWriter::Wav(writer),
                                samples_written: 0,
                                sample_rate: 48000,
                                started: Instant::now(),
                                write_errors: 0,
                            });
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to create WAV file {}: {}",
                                file_path.display(), e
                            );
                        }
                    }
                }
                RecorderCommand::StartIq { db_id, freq_mhz, file_path, sample_rate } => {
                    if let Some(parent) = file_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    match File::create(&file_path) {
                        Ok(file) => {
                            tracing::info!(
                                "Recording IQ: db_id={}, freq={:.4} MHz, sr={}, path={}",
                                db_id, freq_mhz, sample_rate, file_path.display()
                            );
                            slots.insert(db_id, RecordingSlot {
                                db_id,
                                freq_mhz,
                                slot_type: SlotType::Iq,
                                writer: SlotWriter::Iq(BufWriter::new(file)),
                                samples_written: 0,
                                sample_rate,
                                started: Instant::now(),
                                write_errors: 0,
                            });
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to create IQ file {}: {}",
                                file_path.display(), e
                            );
                        }
                    }
                }
                RecorderCommand::Stop { db_id } => {
                    if let Some(slot) = slots.remove(&db_id) {
                        let result = finalize_slot(slot);
                        let _ = finalize_tx.send(result);
                    }
                }
                RecorderCommand::StopAll => {
                    let ids: Vec<i64> = slots.keys().copied().collect();
                    for id in ids {
                        if let Some(slot) = slots.remove(&id) {
                            let result = finalize_slot(slot);
                            let _ = finalize_tx.send(result);
                        }
                    }
                }
                RecorderCommand::AudioData(samples) => {
                    for slot in slots.values_mut() {
                        if slot.slot_type != SlotType::Audio { continue; }
                        if let SlotWriter::Wav(ref mut writer) = slot.writer {
                            let mut batch_err = false;
                            for &sample in &samples {
                                if writer.write_sample(sample).is_err() {
                                    batch_err = true;
                                    break;
                                }
                                slot.samples_written += 1;
                            }
                            if batch_err {
                                slot.write_errors += 1;
                                tracing::warn!(
                                    "Recording db_id={}: write error ({}/{})",
                                    slot.db_id, slot.write_errors, MAX_WRITE_ERRORS
                                );
                            } else {
                                slot.write_errors = 0;
                            }
                        }
                    }
                    // Remove slots that exceeded error threshold
                    let failed: Vec<i64> = slots.values()
                        .filter(|s| s.write_errors >= MAX_WRITE_ERRORS)
                        .map(|s| s.db_id)
                        .collect();
                    for id in failed {
                        tracing::error!("Recording db_id={}: too many write errors, stopping", id);
                        if let Some(slot) = slots.remove(&id) {
                            let _ = finalize_tx.send(finalize_slot(slot));
                        }
                    }
                }
                RecorderCommand::AudioDataFor { db_id, samples } => {
                    if let Some(slot) = slots.get_mut(&db_id) {
                        if slot.slot_type == SlotType::Audio {
                            if let SlotWriter::Wav(ref mut writer) = slot.writer {
                                let mut batch_err = false;
                                for &sample in &samples {
                                    if writer.write_sample(sample).is_err() {
                                        batch_err = true;
                                        break;
                                    }
                                    slot.samples_written += 1;
                                }
                                if batch_err {
                                    slot.write_errors += 1;
                                    if slot.write_errors >= MAX_WRITE_ERRORS {
                                        tracing::error!("Clip db_id={}: too many write errors, stopping", db_id);
                                        if let Some(slot) = slots.remove(&db_id) {
                                            let _ = finalize_tx.send(finalize_slot(slot));
                                        }
                                    }
                                } else {
                                    slot.write_errors = 0;
                                }
                            }
                        }
                    }
                }
                RecorderCommand::IqData(samples) => {
                    for slot in slots.values_mut() {
                        if slot.slot_type != SlotType::Iq { continue; }
                        if let SlotWriter::Iq(ref mut writer) = slot.writer {
                            let mut batch_err = false;
                            for sample in &samples {
                                let i_bytes = sample.re.to_le_bytes();
                                let q_bytes = sample.im.to_le_bytes();
                                if writer.write_all(&i_bytes).is_err()
                                    || writer.write_all(&q_bytes).is_err()
                                {
                                    batch_err = true;
                                    break;
                                }
                                slot.samples_written += 1;
                            }
                            if batch_err {
                                slot.write_errors += 1;
                                tracing::warn!(
                                    "Recording db_id={}: IQ write error ({}/{})",
                                    slot.db_id, slot.write_errors, MAX_WRITE_ERRORS
                                );
                            } else {
                                slot.write_errors = 0;
                            }
                        }
                    }
                    let failed: Vec<i64> = slots.values()
                        .filter(|s| s.write_errors >= MAX_WRITE_ERRORS)
                        .map(|s| s.db_id)
                        .collect();
                    for id in failed {
                        tracing::error!("Recording db_id={}: too many IQ write errors, stopping", id);
                        if let Some(slot) = slots.remove(&id) {
                            let _ = finalize_tx.send(finalize_slot(slot));
                        }
                    }
                }
                RecorderCommand::Shutdown => {
                    tracing::info!("Recorder shutdown requested, finalizing {} slots", slots.len());
                    let ids: Vec<i64> = slots.keys().copied().collect();
                    for id in ids {
                        if let Some(slot) = slots.remove(&id) {
                            let result = finalize_slot(slot);
                            let _ = finalize_tx.send(result);
                        }
                    }
                    break;
                }
            }
        }

        // Send status update every ~1 second
        if last_status.elapsed().as_secs_f64() >= 1.0 {
            let status = RecorderStatus {
                active_audio: slots.values()
                    .filter(|s| s.slot_type == SlotType::Audio)
                    .map(|s| s.to_active_slot())
                    .collect(),
                active_iq: slots.values()
                    .filter(|s| s.slot_type == SlotType::Iq)
                    .map(|s| s.to_active_slot())
                    .collect(),
            };
            let _ = status_tx.send(status);
            last_status = Instant::now();
        }
    }

    tracing::info!("Recorder thread exited");
}

fn finalize_slot(slot: RecordingSlot) -> FinalizeResult {
    let duration_sec = slot.duration_sec();
    let file_size_bytes = slot.file_size_bytes();
    let db_id = slot.db_id;

    match slot.writer {
        SlotWriter::Wav(writer) => {
            if let Err(e) = writer.finalize() {
                tracing::error!("Failed to finalize WAV for db_id={}: {}", db_id, e);
            }
        }
        SlotWriter::Iq(mut writer) => {
            let _ = writer.flush();
        }
    }

    tracing::info!(
        "Finalized recording db_id={}: {:.1}s, {} bytes",
        db_id, duration_sec, file_size_bytes
    );

    FinalizeResult {
        db_id,
        file_size_bytes,
        duration_sec,
    }
}
