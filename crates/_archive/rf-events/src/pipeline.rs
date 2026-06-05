use std::sync::Arc;

use tokio::sync::broadcast;

use crate::event::LogRecord;

/// Capacity of the event bus broadcast channel.
const EVENT_BUS_CAPACITY: usize = 8192;

// ── EventBus ────────────────────────────────────────────────

/// Central event bus for RF-LOG. All events — raw and derived — flow through here.
///
/// Producers call `emit()` to publish events. Consumers subscribe via `subscribe()`.
/// The bus is a tokio broadcast channel; lagging receivers skip old events.
///
/// Subscribers:
/// - **EventStore** (batch writer) — persists to SQLite
/// - **AlertEngine** — evaluates alert rules
/// - **EventManager** — evaluates custom event rules
/// - **SessionCorrelator** — assigns trace_id/span_id
/// - **Live Tail UI** — real-time display
pub struct EventBus {
    tx: broadcast::Sender<Arc<LogRecord>>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        Self { tx }
    }

    /// Publish an event to the bus. Returns the number of active subscribers.
    pub fn emit(&self, record: LogRecord) -> usize {
        // Ignore send errors (no subscribers = events dropped silently)
        self.tx.send(Arc::new(record)).unwrap_or(0)
    }

    /// Publish a pre-wrapped Arc event.
    pub fn emit_arc(&self, record: Arc<LogRecord>) -> usize {
        self.tx.send(record).unwrap_or(0)
    }

    /// Get a new receiver for subscribing to the event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<LogRecord>> {
        self.tx.subscribe()
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

// ── Enrichment ──────────────────────────────────────────────

/// Trait for event enrichment stages in the ingestion pipeline.
/// Each enricher can modify a LogRecord before it's persisted.
pub trait Enricher: Send + Sync {
    /// Enrich a log record in place. Return false to drop the event.
    fn enrich(&self, record: &mut LogRecord) -> bool;
}

/// GPS enricher — stamps receiver coordinates on every event.
pub struct GpsEnricher {
    lat: std::sync::atomic::AtomicI64, // lat * 1e7
    lon: std::sync::atomic::AtomicI64, // lon * 1e7
}

impl GpsEnricher {
    pub fn new() -> Self {
        Self {
            lat: std::sync::atomic::AtomicI64::new(0),
            lon: std::sync::atomic::AtomicI64::new(0),
        }
    }

    /// Update the current GPS position.
    pub fn update(&self, lat: f64, lon: f64) {
        self.lat.store((lat * 1e7) as i64, std::sync::atomic::Ordering::Relaxed);
        self.lon.store((lon * 1e7) as i64, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Default for GpsEnricher {
    fn default() -> Self {
        Self::new()
    }
}

impl Enricher for GpsEnricher {
    fn enrich(&self, record: &mut LogRecord) -> bool {
        if record.receiver_lat.is_none() {
            let lat = self.lat.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e7;
            let lon = self.lon.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e7;
            if lat.abs() > 0.01 {
                record.receiver_lat = Some(lat);
                record.receiver_lon = Some(lon);
            }
        }
        true // never drop
    }
}

/// Operation enricher — stamps current operation context on every event.
pub struct OperationEnricher {
    operation_id: std::sync::atomic::AtomicI64,
    site_session_id: std::sync::atomic::AtomicI64,
}

impl OperationEnricher {
    pub fn new() -> Self {
        Self {
            operation_id: std::sync::atomic::AtomicI64::new(0),
            site_session_id: std::sync::atomic::AtomicI64::new(0),
        }
    }

    /// Update the current operation and site session.
    pub fn update(&self, operation_id: i64, site_session_id: i64) {
        self.operation_id.store(operation_id, std::sync::atomic::Ordering::Relaxed);
        self.site_session_id.store(site_session_id, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Default for OperationEnricher {
    fn default() -> Self {
        Self::new()
    }
}

impl Enricher for OperationEnricher {
    fn enrich(&self, record: &mut LogRecord) -> bool {
        if record.operation_id.is_none() {
            let op = self.operation_id.load(std::sync::atomic::Ordering::Relaxed);
            if op > 0 {
                record.operation_id = Some(op);
            }
        }
        if record.site_session_id.is_none() {
            let ss = self.site_session_id.load(std::sync::atomic::Ordering::Relaxed);
            if ss > 0 {
                record.site_session_id = Some(ss);
            }
        }
        true
    }
}

// ── Session Enricher ────────────────────────────────────────

/// Enricher that wraps a `SessionCorrelator` behind a Mutex.
/// Assigns `trace_id` and `span_id` to protocol events that belong
/// to P25 transmission sessions (grants → voice frames → TLC).
pub struct SessionEnricher {
    correlator: std::sync::Mutex<crate::session::SessionCorrelator>,
}

impl SessionEnricher {
    pub fn new(gap_timeout_sec: u64) -> Self {
        Self {
            correlator: std::sync::Mutex::new(
                crate::session::SessionCorrelator::new(gap_timeout_sec),
            ),
        }
    }

    /// Sweep timed-out sessions and drain all finalized sessions.
    /// Call this periodically (e.g., every second) from a background task.
    /// Returns finalized sessions that need to be persisted to DB.
    pub fn sweep_and_drain(&self) -> Vec<crate::session::TransmissionSession> {
        let mut correlator = self.correlator.lock().unwrap();
        correlator.sweep();
        correlator.drain_finalized()
    }

    /// Number of currently active sessions.
    pub fn active_count(&self) -> usize {
        self.correlator.lock().unwrap().active_count()
    }

    /// Link a recording to the active session for the given talkgroup/freq.
    pub fn link_recording(&self, talkgroup: Option<u32>, freq_mhz: Option<f64>, recording_id: i64) {
        self.correlator.lock().unwrap().link_recording(talkgroup, freq_mhz, recording_id);
    }

    /// Link a fingerprint to the active session for the given talkgroup/freq.
    pub fn link_fingerprint(&self, talkgroup: Option<u32>, freq_mhz: Option<f64>, fingerprint_id: i64) {
        self.correlator.lock().unwrap().link_fingerprint(talkgroup, freq_mhz, fingerprint_id);
    }
}

impl Enricher for SessionEnricher {
    fn enrich(&self, record: &mut LogRecord) -> bool {
        // Only attempt correlation for protocol events
        if record.source == crate::event::EventSource::Protocol {
            let mut correlator = self.correlator.lock().unwrap();
            correlator.correlate(record);
        }
        true // never drop
    }
}

// ── Ingestion Pipeline ──────────────────────────────────────

/// The ingestion pipeline: enrichers → session correlator → event bus.
///
/// Events flow through enrichment stages before being published.
/// This struct is the main interface for producers to submit raw events.
pub struct IngestionPipeline {
    enrichers: Vec<Box<dyn Enricher>>,
    bus: EventBus,
}

impl IngestionPipeline {
    pub fn new(bus: EventBus) -> Self {
        Self {
            enrichers: Vec::new(),
            bus,
        }
    }

    /// Add an enrichment stage to the pipeline.
    pub fn add_enricher(&mut self, enricher: Box<dyn Enricher>) {
        self.enrichers.push(enricher);
    }

    /// Submit an event through the pipeline.
    /// The event passes through all enrichers before being published to the bus.
    /// Returns None if an enricher dropped the event.
    pub fn ingest(&self, mut record: LogRecord) -> Option<Arc<LogRecord>> {
        for enricher in &self.enrichers {
            if !enricher.enrich(&mut record) {
                return None; // dropped
            }
        }
        let arc = Arc::new(record);
        self.bus.emit_arc(arc.clone());
        Some(arc)
    }

    /// Get a reference to the underlying EventBus.
    pub fn bus(&self) -> &EventBus {
        &self.bus
    }
}
