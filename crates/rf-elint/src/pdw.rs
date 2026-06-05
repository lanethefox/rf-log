use serde::{Deserialize, Serialize};

/// Pulse Descriptor Word — measured parameters of a single detected RF pulse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pdw {
    /// Time of arrival in seconds (relative to collection start)
    pub toa: f64,
    /// Pulse width in microseconds
    pub pw_us: f64,
    /// Carrier frequency in MHz
    pub freq_mhz: f64,
    /// Peak amplitude in dBFS
    pub amplitude_dbfs: f32,
    /// Pulse repetition interval to previous pulse in microseconds (None for first)
    pub pri_us: Option<f64>,
}
