//! Continuous device-management service, decoupled from missions.
//!
//! A background thread re-enumerates SDRs (~every 2s, so hotplug works), keyed by
//! hardware serial. Each device walks `Detected → Opening → Ready` (or `Error`), is
//! held open but idle in `Ready`, and is `InUse` while a mission streams it. Missions
//! [`allocate`](DeviceManager::allocate) ready devices (≥1) and [`release`] them on stop.
//! On any change a [`DeviceInfo`] snapshot is pushed to the `on_change` callback (the app
//! forwards it to the bus → status bar).
//!
//! Sim build (no `soapy` feature) presents a fixed set of simulated devices so the whole
//! flow — status bar, settings, allocation — works without hardware.
//!
//! [`release`]: DeviceManager::release

use crate::IqSensor;
#[cfg(not(feature = "soapy"))]
use crate::SimSensor;
use rf_types::{DeviceInfo, DeviceState, SensorId};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const RTL_FREQ_MIN: f64 = 24e6;
const RTL_FREQ_MAX: f64 = 1.766e9;

/// One enumerated source (serial + open args + descriptors).
struct Source {
    serial: String,
    label: String,
    driver: String,
    args: String,
}

struct Managed {
    info: DeviceInfo,
    args: String,
    #[cfg(feature = "soapy")]
    handle: Option<crate::soapy::SoapyDevice>,
}

impl Managed {
    fn new(info: DeviceInfo, args: String) -> Self {
        Self {
            info,
            args,
            #[cfg(feature = "soapy")]
            handle: None,
        }
    }
}

struct Registry {
    devices: Vec<Managed>,
    next_id: u32,
}

type OnChange = Box<dyn Fn(Vec<DeviceInfo>) + Send + Sync>;

/// Owns the device registry and the background detection loop.
pub struct DeviceManager {
    inner: Mutex<Registry>,
    sample_rate: f64,
    sim_count: usize,
    on_change: OnChange,
}

impl DeviceManager {
    pub fn new(
        sample_rate: f64,
        sim_count: usize,
        on_change: impl Fn(Vec<DeviceInfo>) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Mutex::new(Registry {
                devices: Vec::new(),
                next_id: 0,
            }),
            sample_rate,
            sim_count,
            on_change: Box::new(on_change),
        }
    }

    /// Spawn the background detection/open loop. Returns immediately so the UI renders
    /// while devices come up.
    pub fn start(self: &Arc<Self>) {
        let me = self.clone();
        thread::Builder::new()
            .name("device-manager".into())
            .spawn(move || {
                loop {
                    me.detect();
                    thread::sleep(Duration::from_millis(2000));
                }
            })
            .expect("spawn device-manager thread");
    }

    /// Run one detect/open cycle now (manual refresh; the background loop also does this).
    pub fn refresh(&self) {
        self.detect();
    }

    /// Current registry snapshot (for the `list_devices` command).
    pub fn snapshot(&self) -> Vec<DeviceInfo> {
        self.inner
            .lock()
            .unwrap()
            .devices
            .iter()
            .map(|m| m.info.clone())
            .collect()
    }

    fn emit(&self) {
        let snap = self.snapshot();
        (self.on_change)(snap);
    }

    /// Diff the live device list against the registry: drop missing, add new, revive
    /// reconnected — then open anything `Detected`.
    fn detect(&self) {
        let sources = enumerate_sources(self.sim_count);
        {
            let mut reg = self.inner.lock().unwrap();
            // gone → Disconnected
            for m in reg.devices.iter_mut() {
                let present = sources.iter().any(|s| s.serial == m.info.serial);
                if !present && m.info.state != DeviceState::Disconnected {
                    m.info.state = DeviceState::Disconnected;
                    #[cfg(feature = "soapy")]
                    {
                        m.handle = None;
                    }
                }
            }
            // new / revived
            for s in &sources {
                match reg.devices.iter_mut().find(|m| m.info.serial == s.serial) {
                    Some(m) => {
                        if m.info.state == DeviceState::Disconnected {
                            m.info.state = DeviceState::Detected;
                        }
                    }
                    None => {
                        let id = SensorId(reg.next_id);
                        reg.next_id += 1;
                        let info = DeviceInfo {
                            id,
                            serial: s.serial.clone(),
                            label: if s.label.is_empty() {
                                s.serial.clone()
                            } else {
                                s.label.clone()
                            },
                            driver: s.driver.clone(),
                            freq_min_hz: RTL_FREQ_MIN,
                            freq_max_hz: RTL_FREQ_MAX,
                            sample_rate_hz: self.sample_rate,
                            gain_db: 0.0,
                            auto_gain: true,
                            enabled: true,
                            state: DeviceState::Detected,
                            simulated: cfg!(not(feature = "soapy")),
                        };
                        reg.devices.push(Managed::new(info, s.args.clone()));
                    }
                }
            }
        }
        self.emit();
        self.open_detected();
    }

    /// Open every `Detected` device to `Ready` (or `Error`), one at a time, reporting
    /// progress. The slow open happens without the registry lock held.
    fn open_detected(&self) {
        loop {
            // claim the next Detected device
            let next = {
                let mut reg = self.inner.lock().unwrap();
                match reg
                    .devices
                    .iter_mut()
                    .find(|m| m.info.state == DeviceState::Detected)
                {
                    Some(m) => {
                        m.info.state = DeviceState::Opening;
                        Some((
                            m.info.id,
                            m.args.clone(),
                            m.info.sample_rate_hz,
                            m.info.auto_gain,
                            m.info.gain_db,
                        ))
                    }
                    None => None,
                }
            };
            let Some((id, _args, _rate, _auto, _gain)) = next else {
                break;
            };
            self.emit();

            // open (slow) without the lock
            #[cfg(feature = "soapy")]
            let opened = crate::soapy::SoapyDevice::open(id, &_args, _rate);
            #[cfg(not(feature = "soapy"))]
            let opened: Result<(), ()> = Ok(()); // sim: always "opens"

            let mut reg = self.inner.lock().unwrap();
            if let Some(m) = reg.devices.iter_mut().find(|m| m.info.id == id) {
                match opened {
                    Ok(_dev) => {
                        m.info.state = DeviceState::Ready;
                        #[cfg(feature = "soapy")]
                        {
                            let _ = _dev.set_gain_config(_auto, _gain);
                            m.info.freq_min_hz = _dev.capabilities().freq_min_hz;
                            m.info.freq_max_hz = _dev.capabilities().freq_max_hz;
                            m.handle = Some(_dev);
                        }
                    }
                    #[allow(unreachable_patterns)]
                    Err(_) => {
                        m.info.state = DeviceState::Error;
                    }
                }
            }
            drop(reg);
            self.emit();
        }
    }

    /// Allocate up to `max` enabled, ready devices for a mission, activating their
    /// streams. Returns the streaming sensors (their `id()` matches the registry).
    pub fn allocate(&self, max: usize) -> Vec<Box<dyn IqSensor>> {
        let mut out: Vec<Box<dyn IqSensor>> = Vec::new();
        {
            let mut reg = self.inner.lock().unwrap();
            for m in reg.devices.iter_mut() {
                if out.len() >= max {
                    break;
                }
                if !(m.info.enabled && m.info.state == DeviceState::Ready) {
                    continue;
                }
                #[cfg(feature = "soapy")]
                {
                    if let Some(dev) = m.handle.take() {
                        match dev.activate() {
                            Ok(s) => {
                                m.info.state = DeviceState::InUse;
                                out.push(Box::new(s));
                            }
                            Err(_) => m.info.state = DeviceState::Error,
                        }
                    }
                }
                #[cfg(not(feature = "soapy"))]
                {
                    let s = SimSensor::new(
                        m.info.id,
                        m.info.freq_min_hz,
                        m.info.freq_max_hz,
                        m.info.sample_rate_hz,
                    );
                    m.info.state = DeviceState::InUse;
                    out.push(Box::new(s));
                }
            }
        }
        self.emit();
        out
    }

    /// Return in-use devices to the pool (re-opened to `Ready` on the next detect cycle).
    pub fn release(&self, ids: &[SensorId]) {
        {
            let mut reg = self.inner.lock().unwrap();
            for m in reg.devices.iter_mut() {
                if ids.contains(&m.info.id) && m.info.state == DeviceState::InUse {
                    m.info.state = DeviceState::Detected; // re-opened to Ready by detect()
                }
            }
        }
        self.emit();
        self.open_detected();
    }

    /// Count of devices available to allocate right now.
    pub fn ready_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap()
            .devices
            .iter()
            .filter(|m| m.info.enabled && m.info.state == DeviceState::Ready)
            .count()
    }

    /// Update a device's config. Gain is applied live; a sample-rate change re-opens it.
    pub fn set_config(
        &self,
        id: SensorId,
        enabled: Option<bool>,
        auto_gain: Option<bool>,
        gain_db: Option<f32>,
        sample_rate_hz: Option<f64>,
    ) {
        {
            let mut reg = self.inner.lock().unwrap();
            if let Some(m) = reg.devices.iter_mut().find(|m| m.info.id == id) {
                if let Some(e) = enabled {
                    m.info.enabled = e;
                }
                if let Some(a) = auto_gain {
                    m.info.auto_gain = a;
                }
                if let Some(g) = gain_db {
                    m.info.gain_db = g;
                }
                let mut reopen = false;
                if let Some(sr) = sample_rate_hz {
                    if (sr - m.info.sample_rate_hz).abs() > 1.0 {
                        m.info.sample_rate_hz = sr;
                        reopen = true;
                    }
                }
                #[cfg(feature = "soapy")]
                {
                    if reopen && m.info.state == DeviceState::Ready {
                        m.handle = None;
                        m.info.state = DeviceState::Detected; // re-open with new rate
                    } else if let Some(h) = &m.handle {
                        let _ = h.set_gain_config(m.info.auto_gain, m.info.gain_db);
                    }
                }
                #[cfg(not(feature = "soapy"))]
                {
                    let _ = reopen;
                }
            }
        }
        self.emit();
        self.open_detected();
    }
}

/// Enumerate the devices visible right now.
fn enumerate_sources(sim_count: usize) -> Vec<Source> {
    #[cfg(feature = "soapy")]
    {
        let _ = sim_count;
        crate::soapy::enumerate()
            .into_iter()
            .map(|d| Source {
                serial: d.serial,
                label: d.label,
                driver: d.driver,
                args: d.args,
            })
            .collect()
    }
    #[cfg(not(feature = "soapy"))]
    {
        (0..sim_count)
            .map(|i| Source {
                serial: format!("SIM-{i}"),
                label: format!("Simulated SDR {i}"),
                driver: "sim".into(),
                args: String::new(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn sim_devices_detect_open_allocate_release() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let dm = Arc::new(DeviceManager::new(2_400_000.0, 2, move |_devs| {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        // one detect cycle (without the background thread)
        dm.detect();
        let snap = dm.snapshot();
        assert_eq!(snap.len(), 2, "two simulated devices");
        assert!(
            snap.iter().all(|d| d.state == DeviceState::Ready),
            "opened to ready"
        );
        assert_eq!(dm.ready_count(), 2);
        assert!(calls.load(Ordering::SeqCst) > 0, "emitted change events");

        // allocate one, the other stays ready
        let sensors = dm.allocate(1);
        assert_eq!(sensors.len(), 1);
        assert_eq!(dm.ready_count(), 1);
        let id = sensors[0].id();

        // release it back
        dm.release(&[id]);
        assert_eq!(dm.ready_count(), 2, "released device returns to ready");
    }

    #[test]
    fn disabled_devices_are_not_allocated() {
        let dm = Arc::new(DeviceManager::new(2_400_000.0, 2, |_| {}));
        dm.detect();
        let id = dm.snapshot()[0].id;
        dm.set_config(id, Some(false), None, None, None);
        let sensors = dm.allocate(usize::MAX);
        assert_eq!(sensors.len(), 1, "only the enabled device allocates");
    }
}
