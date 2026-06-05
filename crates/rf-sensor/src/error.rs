use thiserror::Error;

/// Typed sensor failures, so the pool can branch (e.g. skip an unsupported tile vs
/// tear down a disconnected device). v1 used stringly-typed errors everywhere.
#[derive(Debug, Error)]
pub enum SensorError {
    #[error("read timed out")]
    Timeout,
    #[error("frequency {0} Hz is out of range / unsupported")]
    UnsupportedFreq(f64),
    #[error("sensor disconnected")]
    Disconnected,
    #[error("sensor io: {0}")]
    Io(String),
}
