use crate::{IqProducer, IqSample, SdrCommand, SdrDevice, SoapySdr};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_CONSECUTIVE_ERRORS: u32 = 10;
const MAX_FREQ_ERRORS: u32 = 20;

/// Spawn the SDR reader thread with reconnection support.
///
/// When the device disconnects (consecutive read/command errors), the thread:
/// 1. Drops the device handle
/// 2. Sets `sdr_alive` to false
/// 3. Waits for `sdr_refresh` to be set to true
/// 4. Attempts to reopen the device via SoapySDR
/// 5. On success, sets `sdr_alive` back to true and resumes reading
///
/// When running in simulation mode, the thread periodically checks `sdr_refresh`
/// and will hot-swap to real hardware if available.
///
/// `device_label` is used for thread naming and log messages.
/// `reconnect_args` provides targeted reconnect via `open_by_args()`; if `None`,
/// falls back to `SoapySdr::open()` (simulation/single-device fallback).
pub fn spawn_sdr_reader(
    device: Box<dyn SdrDevice>,
    producer: IqProducer,
    cmd_rx: mpsc::Receiver<SdrCommand>,
    sdr_alive: Arc<AtomicBool>,
    sdr_refresh: Arc<AtomicBool>,
    sample_rate: f64,
    device_label: String,
    reconnect_args: Option<HashMap<String, String>>,
    sdr_quarantine: Arc<AtomicBool>,
) -> JoinHandle<()> {
    spawn_sdr_reader_with_iq_mirror(
        device, producer, cmd_rx, sdr_alive, sdr_refresh, sample_rate,
        device_label, reconnect_args, sdr_quarantine, None, None,
    )
}

/// Extended SDR reader with optional IQ mirror for recording.
/// When `iq_record_flag` is true, also pushes samples to `iq_mirror_producer`.
pub fn spawn_sdr_reader_with_iq_mirror(
    device: Box<dyn SdrDevice>,
    producer: IqProducer,
    cmd_rx: mpsc::Receiver<SdrCommand>,
    sdr_alive: Arc<AtomicBool>,
    sdr_refresh: Arc<AtomicBool>,
    sample_rate: f64,
    device_label: String,
    reconnect_args: Option<HashMap<String, String>>,
    sdr_quarantine: Arc<AtomicBool>,
    iq_mirror_producer: Option<IqProducer>,
    iq_record_flag: Option<Arc<AtomicBool>>,
) -> JoinHandle<()> {
    let thread_name = format!("sdr_reader_{}", device_label);
    thread::Builder::new()
        .name(thread_name.clone().into())
        .spawn(move || {
            tracing::info!("[{}] SDR reader thread started", device_label);
            let mut buf = vec![IqSample::new(0.0, 0.0); 2048];
            let mut current_device: Option<Box<dyn SdrDevice>> = Some(device);

            // Set sdr_alive based on whether this is real hardware or simulation
            let is_sim = current_device.as_ref().map_or(true, |d| d.is_simulated());
            sdr_alive.store(!is_sim, Ordering::Release);
            if is_sim {
                tracing::info!("SDR reader running in SIMULATION mode — sdr_alive=false");
            } else {
                tracing::info!("SDR reader running with HARDWARE — sdr_alive=true");
            }

            // Counter for periodic refresh checks during simulation
            let mut sim_refresh_counter: u32 = 0;
            // Freq errors persists across inner-loop breaks so quarantine can
            // trigger even when read-error disconnects break the inner loop
            let mut freq_errors: u32 = 0;

            loop {
                // === DISCONNECTED STATE: wait for refresh signal ===
                if current_device.is_none() {
                    // Check for stop/disconnect on command channel
                    match cmd_rx.try_recv() {
                        Ok(SdrCommand::Stop) => {
                            tracing::info!("SDR reader stopping (was disconnected)");
                            break;
                        }
                        Err(mpsc::TryRecvError::Disconnected) => {
                            tracing::info!("SDR command channel closed, stopping");
                            break;
                        }
                        _ => {}
                    }

                    // Check for refresh request
                    if sdr_refresh.compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                        tracing::info!("[{}] SDR refresh requested, attempting to reconnect...", device_label);
                        let result = if let Some(ref args) = reconnect_args {
                            SoapySdr::open_by_args(args, sample_rate)
                        } else {
                            SoapySdr::open(sample_rate)
                        };
                        match result {
                            Ok(dev) => {
                                tracing::info!("[{}] SDR reconnected successfully", device_label);
                                current_device = Some(Box::new(dev));
                                sdr_alive.store(true, Ordering::Release);
                                sim_refresh_counter = 0;
                                continue;
                            }
                            Err(e) => {
                                tracing::warn!("[{}] SDR reconnect failed: {}", device_label, e);
                            }
                        }
                    }

                    thread::sleep(Duration::from_millis(500));
                    continue;
                }

                // === CONNECTED STATE: read IQ data ===
                let device = current_device.as_mut().unwrap();
                let running_simulated = device.is_simulated();
                let mut consecutive_errors: u32 = 0;

                loop {
                    // Check for commands (non-blocking)
                    match cmd_rx.try_recv() {
                        Ok(SdrCommand::SetFreq(freq)) => {
                            if let Err(e) = device.set_freq(freq) {
                                freq_errors += 1;
                                // Do NOT increment consecutive_errors here — freq tune
                                // failures are not read errors and shouldn't trigger
                                // the "device disconnected" logic
                                // Only log first few, then every 100th to avoid spam
                                if freq_errors <= 3 || freq_errors % 100 == 0 {
                                    tracing::error!(
                                        "[{}] Failed to set freq ({} total failures): {}",
                                        device_label, freq_errors, e
                                    );
                                }
                                // Quarantine: stop assigning bands after threshold
                                if freq_errors >= MAX_FREQ_ERRORS
                                    && !sdr_quarantine.load(Ordering::Relaxed)
                                {
                                    sdr_quarantine.store(true, Ordering::Release);
                                    tracing::error!(
                                        "[{}] QUARANTINE: {} consecutive freq failures — bands will be redistributed",
                                        device_label, freq_errors
                                    );
                                }
                            } else {
                                if freq_errors > 0 {
                                    tracing::info!(
                                        "[{}] set_freq succeeded after {} previous failures",
                                        device_label, freq_errors
                                    );
                                }
                                freq_errors = 0;
                                // Lift quarantine if it was set
                                if sdr_quarantine.load(Ordering::Relaxed) {
                                    sdr_quarantine.store(false, Ordering::Release);
                                    tracing::info!(
                                        "[{}] Quarantine lifted — freq tuning recovered",
                                        device_label
                                    );
                                }
                            }
                        }
                        Ok(SdrCommand::SetGain(gain)) => {
                            tracing::info!("SDR gain set to {}", gain);
                            if let Err(e) = device.set_gain(gain) {
                                tracing::error!("Failed to set gain: {}", e);
                                consecutive_errors += 1;
                            }
                        }
                        Ok(SdrCommand::SetAgc(enabled)) => {
                            tracing::info!("SDR AGC set to {}", enabled);
                            if let Err(e) = device.set_agc(enabled) {
                                tracing::warn!("Failed to set AGC: {}", e);
                            }
                        }
                        Ok(SdrCommand::SetPpm(ppm)) => {
                            tracing::info!("SDR PPM correction set to {}", ppm);
                            if let Err(e) = device.set_ppm(ppm) {
                                tracing::warn!("Failed to set PPM: {}", e);
                            }
                        }
                        Ok(SdrCommand::SetOffsetTuning(enabled)) => {
                            tracing::info!("SDR offset tuning set to {}", enabled);
                            if let Err(e) = device.set_offset_tuning(enabled) {
                                tracing::warn!("Failed to set offset tuning: {}", e);
                            }
                        }
                        Ok(SdrCommand::Stop) => {
                            tracing::info!("SDR reader thread stopping");
                            drop(current_device.take());
                            sdr_alive.store(false, Ordering::Release);
                            return;
                        }
                        Err(mpsc::TryRecvError::Empty) => {}
                        Err(mpsc::TryRecvError::Disconnected) => {
                            tracing::info!("SDR command channel closed, stopping reader");
                            drop(current_device.take());
                            sdr_alive.store(false, Ordering::Release);
                            return;
                        }
                    }

                    // If running simulated, periodically check for hardware refresh
                    if running_simulated {
                        sim_refresh_counter += 1;
                        if sim_refresh_counter >= 500 {
                            sim_refresh_counter = 0;
                            if sdr_refresh.compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                                tracing::info!("[{}] SDR refresh during simulation — attempting hardware swap...", device_label);
                                let result = if let Some(ref args) = reconnect_args {
                                    SoapySdr::open_by_args(args, sample_rate)
                                } else {
                                    SoapySdr::open(sample_rate)
                                };
                                match result {
                                    Ok(dev) => {
                                        tracing::info!("[{}] Hardware detected! Swapping from simulation to real SDR", device_label);
                                        current_device = Some(Box::new(dev));
                                        sdr_alive.store(true, Ordering::Release);
                                        break; // break inner loop to restart with new device
                                    }
                                    Err(e) => {
                                        tracing::warn!("[{}] Hardware swap failed: {}", device_label, e);
                                    }
                                }
                            }
                        }
                    }

                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        tracing::error!(
                            "SDR device appears disconnected ({} consecutive errors)",
                            consecutive_errors
                        );
                        break;
                    }

                    // Read IQ samples from device
                    match device.read_iq(&mut buf) {
                        Ok(n) if n > 0 => {
                            producer.push_slice(&buf[..n]);
                            // Mirror to IQ recording buffer when active
                            if let (Some(mirror), Some(flag)) = (&iq_mirror_producer, &iq_record_flag) {
                                if flag.load(Ordering::Relaxed) {
                                    mirror.push_slice(&buf[..n]);
                                }
                            }
                            consecutive_errors = 0;
                        }
                        Ok(_) => {
                            // No data available, brief sleep
                            thread::sleep(Duration::from_micros(100));
                        }
                        Err(e) => {
                            consecutive_errors += 1;
                            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                tracing::error!(
                                    "SDR device disconnected ({} errors): {}",
                                    consecutive_errors, e
                                );
                                break;
                            }
                            tracing::error!(
                                "SDR read error ({}/{}): {}",
                                consecutive_errors, MAX_CONSECUTIVE_ERRORS, e
                            );
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                }

                // Check if we just swapped to a new device (break from sim refresh)
                if current_device.as_ref().map_or(false, |d| !d.is_simulated()) {
                    tracing::info!("Continuing with new hardware device");
                    continue;
                }

                // Device lost — forget (don't Drop) to avoid segfault in C cleanup
                // The USB resources are already gone, deactivate() would crash
                tracing::warn!("SDR device lost, entering disconnected state");
                if let Some(dev) = current_device.take() {
                    if dev.is_simulated() {
                        drop(dev); // SimulatedSdr can be safely dropped
                    } else {
                        std::mem::forget(dev);
                    }
                }
                sdr_alive.store(false, Ordering::Release);
            }

            tracing::info!("SDR reader thread exited");
        })
        .expect("failed to spawn sdr_reader thread")
}
