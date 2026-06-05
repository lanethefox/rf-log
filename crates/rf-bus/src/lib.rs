//! RF-LOG v2 event bus.
//!
//! Two delivery semantics, matching `docs/RF-LOG-v2.md` §4.5:
//! - **lossy broadcast** ([`Bus::subscribe`]) for live telemetry (PSD frames, sensor
//!   and mission status) — a slow UI subscriber lags and drops, never blocks producers;
//! - **lossless detection path** (the receiver returned by [`channel`]) — every
//!   [`Detection`](rf_types::Detection) reaches the catalog writer, independent of the
//!   broadcast.
//!
//! Producers are sync worker threads; consumers are async tasks (Tauri bridge, DB
//! writer). [`Bus`] is cheap to clone and `Send`.

use rf_types::{BusEvent, Detection};
use tokio::sync::{broadcast, mpsc};

/// Clonable publish handle.
#[derive(Clone)]
pub struct Bus {
    events: broadcast::Sender<BusEvent>,
    detections: mpsc::UnboundedSender<Detection>,
}

impl Bus {
    /// Publish an event: always broadcast (lossy); detections additionally go to the
    /// lossless persistence path.
    pub fn publish(&self, ev: BusEvent) {
        if let BusEvent::Detection(d) = &ev {
            let _ = self.detections.send(d.clone());
        }
        let _ = self.events.send(ev); // Err only means "no subscribers" — fine for telemetry
    }

    /// A new live telemetry subscription. Lagged subscribers drop oldest events.
    pub fn subscribe(&self) -> broadcast::Receiver<BusEvent> {
        self.events.subscribe()
    }
}

/// Build a bus. `broadcast_cap` bounds the live telemetry backlog per subscriber.
/// Returns the bus plus the lossless detection receiver (hand to the catalog writer).
pub fn channel(broadcast_cap: usize) -> (Bus, mpsc::UnboundedReceiver<Detection>) {
    let (events, _initial) = broadcast::channel(broadcast_cap.max(1));
    let (det_tx, det_rx) = mpsc::unbounded_channel();
    (
        Bus {
            events,
            detections: det_tx,
        },
        det_rx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rf_types::{SensorId, SensorState};

    fn sample_detection() -> Detection {
        Detection {
            center_hz: 162.55e6,
            bandwidth_hz: 12.5e3,
            power_dbfs: -38.0,
            snr_db: 22.0,
            t_unix_ns: 1,
            tile_center_hz: 162.0e6,
            sensor: SensorId(0),
        }
    }

    #[tokio::test]
    async fn detection_reaches_both_paths() {
        let (bus, mut det_rx) = channel(16);
        let mut sub = bus.subscribe();
        bus.publish(BusEvent::Detection(sample_detection()));

        // lossless path
        let d = det_rx.recv().await.expect("detection on lossless path");
        assert_eq!(d.center_hz, 162.55e6);

        // broadcast path
        match sub.recv().await.expect("event on broadcast") {
            BusEvent::Detection(d) => assert_eq!(d.snr_db, 22.0),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn telemetry_is_broadcast_only() {
        let (bus, mut det_rx) = channel(16);
        let mut sub = bus.subscribe();
        bus.publish(BusEvent::SensorStatus {
            id: SensorId(3),
            state: SensorState::Sweeping,
        });
        assert!(matches!(
            sub.recv().await.unwrap(),
            BusEvent::SensorStatus { .. }
        ));
        // non-detection events do not hit the lossless path
        assert!(det_rx.try_recv().is_err());
    }
}
