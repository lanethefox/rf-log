//! RF-LOG v2 mission orchestrator.
//!
//! Owns the survey lifecycle: builds the [`SensorPool`], runs the [`SurveyDsp`] over
//! the dwells it produces, publishes PSD frames + detections to the [`Bus`], and
//! persists detections/sensors/phase to the [`Catalog`]. P0 drives the simulated
//! pool; the same wiring serves hardware once `rf-sensor` grows its SoapySDR backend.
//!
//! Threading: the pool and DSP run on std threads (blocking `mpsc`); the catalog
//! detection writer is a tokio task draining the bus's lossless path. `bus.publish`
//! is sync, so the std worker threads can feed it directly.

use rf_bus::Bus;
use rf_catalog::Catalog;
use rf_dsp::SurveyDsp;
use rf_sensor::{PoolConfig, PoolHandle, SensorPool, SimSensor, now_unix_ns};
use rf_types::{Band, BusEvent, Detection, MissionId, MissionPhase, SensorId, SensorRole};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tokio::sync::mpsc::UnboundedReceiver;

/// Survey tuning shared across missions.
pub struct MissionConfig {
    pub fft_size: usize,
    pub dwell_segments: usize,
    pub settle_samples: usize,
    pub sample_rate: f64,
    /// Number of simulated sweep sensors to spin up (P0).
    pub num_sensors: usize,
}

impl Default for MissionConfig {
    fn default() -> Self {
        Self {
            fft_size: 4096,
            dwell_segments: 8,
            settle_samples: 4096,
            sample_rate: 2_400_000.0,
            num_sensors: 2,
        }
    }
}

/// Spawn the long-lived catalog detection writer. It drains the bus's lossless
/// detection path and persists each detection against whatever mission is active
/// (`active` holds the active `MissionId.0`, or `-1` for none).
///
/// Runs on a std thread using `blocking_recv`, so it needs no surrounding tokio
/// runtime (Tauri's `setup` isn't in one).
pub fn spawn_detection_writer(
    catalog: Arc<Catalog>,
    active: Arc<AtomicI64>,
    mut det_rx: UnboundedReceiver<Detection>,
) {
    std::thread::Builder::new()
        .name("det-writer".into())
        .spawn(move || {
            while let Some(d) = det_rx.blocking_recv() {
                let v = active.load(Ordering::SeqCst);
                if v >= 0 {
                    if let Err(e) = catalog.insert_detection(MissionId(v), &d) {
                        tracing::warn!("detection persist failed: {e}");
                    }
                }
            }
        })
        .expect("spawn detection writer");
}

struct RunningMission {
    id: MissionId,
    pool: PoolHandle,
    workers: Vec<JoinHandle<()>>,
}

/// Creates, starts, and stops missions; one runs at a time in P0.
pub struct MissionManager {
    catalog: Arc<Catalog>,
    bus: Bus,
    active: Arc<AtomicI64>,
    cfg: MissionConfig,
    running: Mutex<Option<RunningMission>>,
}

impl MissionManager {
    pub fn new(
        catalog: Arc<Catalog>,
        bus: Bus,
        active: Arc<AtomicI64>,
        cfg: MissionConfig,
    ) -> Self {
        Self {
            catalog,
            bus,
            active,
            cfg,
            running: Mutex::new(None),
        }
    }

    pub fn catalog(&self) -> &Arc<Catalog> {
        &self.catalog
    }

    pub fn create_mission(&self, name: &str, bands: Vec<Band>) -> Result<MissionId, String> {
        self.catalog
            .create_mission(name, &bands, now_unix_ns())
            .map_err(|e| e.to_string())
    }

    pub fn active_mission(&self) -> Option<MissionId> {
        let v = self.active.load(Ordering::SeqCst);
        (v >= 0).then_some(MissionId(v))
    }

    /// Start surveying `id`. Builds a simulated pool over the union of the mission's
    /// bands, runs the DSP, and begins persisting detections.
    pub fn start(&self, id: MissionId) -> Result<(), String> {
        let mut guard = self.running.lock().unwrap();
        if guard.is_some() {
            return Err("a mission is already running".into());
        }
        let mission = self
            .catalog
            .get_mission(id)
            .map_err(|e| e.to_string())?
            .ok_or("mission not found")?;
        let bands = mission.bands;
        if bands.is_empty() {
            return Err("mission has no bands".into());
        }
        let low = bands.iter().map(|b| b.low_hz).fold(f64::INFINITY, f64::min);
        let high = bands
            .iter()
            .map(|b| b.high_hz)
            .fold(f64::NEG_INFINITY, f64::max);

        let mut pool = SensorPool::new();
        for s in 0..self.cfg.num_sensors.max(1) {
            pool.add(
                Box::new(SimSensor::new(
                    SensorId(s as u32),
                    low,
                    high,
                    self.cfg.sample_rate,
                )),
                SensorRole::SurveySweep,
            );
        }
        let (dwell_tx, dwell_rx) = channel();
        let (status_tx, status_rx) = channel();
        let pool_handle = pool.start(
            PoolConfig {
                bands: bands.clone(),
                fft_size: self.cfg.fft_size,
                dwell_segments: self.cfg.dwell_segments,
                settle_samples: self.cfg.settle_samples,
            },
            dwell_tx,
            status_tx,
        );

        // DSP worker: dwell -> (PSD frame, detections) -> bus
        let bus = self.bus.clone();
        let fft_size = self.cfg.fft_size;
        let dsp_handle = std::thread::Builder::new()
            .name("dsp".into())
            .spawn(move || {
                let mut dsp = SurveyDsp::new(fft_size);
                while let Ok(dwell) = dwell_rx.recv() {
                    let (frame, dets) = dsp.process_dwell(
                        &dwell.iq,
                        dwell.tile_center_hz,
                        dwell.sample_rate,
                        dwell.sensor,
                        dwell.t_unix_ns,
                    );
                    bus.publish(BusEvent::Psd(frame));
                    for d in dets {
                        bus.publish(BusEvent::Detection(d));
                    }
                }
            })
            .map_err(|e| e.to_string())?;

        // status worker: sensor state -> catalog + bus
        let bus2 = self.bus.clone();
        let catalog = self.catalog.clone();
        let status_handle = std::thread::Builder::new()
            .name("status".into())
            .spawn(move || {
                while let Ok((sid, state)) = status_rx.recv() {
                    let _ = catalog.upsert_sensor(id, sid, &format!("sim-{}", sid.0));
                    bus2.publish(BusEvent::SensorStatus { id: sid, state });
                }
            })
            .map_err(|e| e.to_string())?;

        self.active.store(id.0, Ordering::SeqCst);
        self.catalog
            .set_mission_phase(id, MissionPhase::Running)
            .map_err(|e| e.to_string())?;
        self.bus.publish(BusEvent::MissionState {
            id,
            phase: MissionPhase::Running,
        });

        *guard = Some(RunningMission {
            id,
            pool: pool_handle,
            workers: vec![dsp_handle, status_handle],
        });
        Ok(())
    }

    /// Stop the running mission (if any) and mark it Stopped.
    pub fn stop(&self) -> Result<(), String> {
        let mut guard = self.running.lock().unwrap();
        let Some(run) = guard.take() else {
            return Err("no mission running".into());
        };
        self.active.store(-1, Ordering::SeqCst);
        run.pool.stop(); // joins pool workers; dropping their senders disconnects the DSP/status loops
        for w in run.workers {
            let _ = w.join();
        }
        self.catalog
            .set_mission_phase(run.id, MissionPhase::Stopped)
            .map_err(|e| e.to_string())?;
        self.bus.publish(BusEvent::MissionState {
            id: run.id,
            phase: MissionPhase::Stopped,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn mission_runs_surveys_and_persists_detections() {
        let catalog = Arc::new(Catalog::open_in_memory().unwrap());
        let (bus, det_rx) = rf_bus::channel(256);
        let active = Arc::new(AtomicI64::new(-1));
        spawn_detection_writer(catalog.clone(), active.clone(), det_rx);

        let mgr = MissionManager::new(
            catalog.clone(),
            bus,
            active,
            MissionConfig {
                fft_size: 1024,
                dwell_segments: 4,
                settle_samples: 256,
                sample_rate: 2_400_000.0,
                num_sensors: 2,
            },
        );
        let bands = vec![Band {
            name: "VHF".into(),
            low_hz: 144e6,
            high_hz: 175e6,
        }];
        let id = mgr.create_mission("t", bands).unwrap();
        mgr.start(id).unwrap();

        let mut count = 0;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            count = catalog.detection_count(id).unwrap();
            if count > 0 {
                break;
            }
        }
        mgr.stop().unwrap();

        assert!(count > 0, "expected the survey to persist detections");
        assert!(matches!(
            catalog.get_mission(id).unwrap().unwrap().phase,
            MissionPhase::Stopped
        ));
        assert!(mgr.active_mission().is_none());
    }
}
