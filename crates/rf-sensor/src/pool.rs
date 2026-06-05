use crate::{Dwell, IqSensor, now_unix_ns, plan_tiles};
use num_complex::Complex32;
use rf_types::{Band, Hz, SensorId, SensorRole, SensorState};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::Duration;

/// Survey parameters shared by all sweep workers. A dwell is `fft_size *
/// dwell_segments` samples — enough for the DSP's Welch averaging.
pub struct PoolConfig {
    pub bands: Vec<Band>,
    pub fft_size: usize,
    pub dwell_segments: usize,
    pub settle_samples: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            bands: Vec::new(),
            fft_size: 4096,
            dwell_segments: 8,
            settle_samples: 4096,
        }
    }
}

/// Sensors collected with their roles, awaiting [`start`](SensorPool::start).
pub struct SensorPool {
    sensors: Vec<(Box<dyn IqSensor>, SensorRole)>,
}

impl Default for SensorPool {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorPool {
    pub fn new() -> Self {
        Self {
            sensors: Vec::new(),
        }
    }

    pub fn add(&mut self, sensor: Box<dyn IqSensor>, role: SensorRole) {
        self.sensors.push((sensor, role));
    }

    pub fn len(&self) -> usize {
        self.sensors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sensors.is_empty()
    }

    pub fn ids(&self) -> Vec<SensorId> {
        self.sensors.iter().map(|(s, _)| s.id()).collect()
    }

    /// Spawn one worker per survey-sweep sensor. Tiles are split round-robin across
    /// the sweep sensors that can cover them. Dwells and status flow over the channels.
    pub fn start(
        self,
        cfg: PoolConfig,
        dwell_tx: Sender<Dwell>,
        status_tx: Sender<(SensorId, SensorState)>,
    ) -> PoolHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let dwell_len = cfg.fft_size * cfg.dwell_segments;
        let settle = cfg.settle_samples;

        let sweep: Vec<Box<dyn IqSensor>> = self
            .sensors
            .into_iter()
            .filter(|(_, r)| matches!(r, SensorRole::SurveySweep))
            .map(|(s, _)| s)
            .collect();
        let n = sweep.len().max(1);

        let mut handles = Vec::new();
        for (k, sensor) in sweep.into_iter().enumerate() {
            let bw = sensor.sample_rate();
            let tiles: Vec<Hz> = plan_tiles(&cfg.bands, bw)
                .into_iter()
                .filter(|&c| sensor.capabilities().covers(c, bw))
                .enumerate()
                .filter(|(i, _)| i % n == k) // round-robin split across sweep sensors
                .map(|(_, c)| c)
                .collect();
            let dwell_tx = dwell_tx.clone();
            let status_tx = status_tx.clone();
            let stop = stop.clone();
            let name = format!("sweep-{}", sensor.id().0);
            let h = std::thread::Builder::new()
                .name(name)
                .spawn(move || {
                    sweep_worker(sensor, tiles, dwell_len, settle, dwell_tx, status_tx, stop)
                })
                .expect("failed to spawn sweep worker");
            handles.push(h);
        }
        PoolHandle { stop, handles }
    }
}

/// Handle to a running pool. Call [`stop`](PoolHandle::stop) to wind down and join.
pub struct PoolHandle {
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

impl PoolHandle {
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        for h in self.handles {
            let _ = h.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sweep_worker(
    mut sensor: Box<dyn IqSensor>,
    tiles: Vec<Hz>,
    dwell_len: usize,
    settle: usize,
    dwell_tx: Sender<Dwell>,
    status_tx: Sender<(SensorId, SensorState)>,
    stop: Arc<AtomicBool>,
) {
    let id = sensor.id();
    let sr = sensor.sample_rate();
    let _ = status_tx.send((id, SensorState::Connected));

    if tiles.is_empty() {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = status_tx.send((id, SensorState::Disconnected));
        return;
    }

    let mut settle_buf = vec![Complex32::new(0.0, 0.0); settle.clamp(1, 1 << 16)];
    let mut dwell_buf = vec![Complex32::new(0.0, 0.0); dwell_len];

    'outer: while !stop.load(Ordering::Relaxed) {
        for &tile in &tiles {
            if stop.load(Ordering::Relaxed) {
                break 'outer;
            }
            if sensor.tune(tile).is_err() {
                let _ = status_tx.send((id, SensorState::Error));
                continue;
            }
            let _ = status_tx.send((id, SensorState::Sweeping));
            fill(&mut *sensor, &mut settle_buf, &stop); // settle: read & discard
            let filled = fill(&mut *sensor, &mut dwell_buf, &stop);
            if filled == dwell_len {
                let _ = dwell_tx.send(Dwell {
                    sensor: id,
                    tile_center_hz: tile,
                    sample_rate: sr,
                    iq: dwell_buf.clone(),
                    t_unix_ns: now_unix_ns(),
                });
                // Pace the simulated sweep so it doesn't run at raw thread speed
                // (real SDRs are rate-limited; keeps CPU/IPC sane). No-op for the
                // dwell-availability the tests assert.
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    let _ = status_tx.send((id, SensorState::Disconnected));
}

/// Read until `buf` is full or the sensor stalls/errors. Returns samples filled.
fn fill(sensor: &mut dyn IqSensor, buf: &mut [Complex32], stop: &AtomicBool) -> usize {
    let mut filled = 0;
    while filled < buf.len() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match sensor.read(&mut buf[filled..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => filled += n,
        }
    }
    filled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimSensor;
    use std::sync::mpsc::channel;

    #[test]
    fn pool_emits_dwells_for_a_sweep_sensor() {
        let mut pool = SensorPool::new();
        pool.add(
            Box::new(SimSensor::new(SensorId(1), 144e6, 174e6, 2.4e6)),
            SensorRole::SurveySweep,
        );
        let (dtx, drx) = channel();
        let (stx, _srx) = channel();
        let cfg = PoolConfig {
            bands: vec![Band {
                name: "VHF".into(),
                low_hz: 144e6,
                high_hz: 174e6,
            }],
            fft_size: 1024,
            dwell_segments: 2,
            settle_samples: 256,
        };
        let handle = pool.start(cfg, dtx, stx);
        let dwell = drx
            .recv_timeout(Duration::from_secs(5))
            .expect("expected a dwell");
        assert_eq!(dwell.iq.len(), 2048);
        assert_eq!(dwell.sensor, SensorId(1));
        assert!(dwell.tile_center_hz >= 144e6 && dwell.tile_center_hz <= 174e6);
        handle.stop();
    }
}
