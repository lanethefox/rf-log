//! rf-sigex — SIGEX (Signals Exploitation) Engine
//!
//! Transforms raw collection data into signals intelligence products.
//! Three tiers:
//!   - Tier 1: Passive spectrum analysis (traffic sessions, baselines, anomalies)
//!   - Tier 2: Targeted IQ exploitation (fingerprinting, protocol metadata, encryption)
//!   - Tier 3: Sustained collection (P25 control channel monitoring)

pub mod traffic;
pub mod encryption;
pub mod protocol;
pub mod network;

// Re-export key types for convenience
pub use traffic::{SessionTracker, SignalDetection, SessionEvent};
pub use encryption::{EncryptionTracker, CryptoEvent};
pub use protocol::{ProtocolTracker, ProtocolEvent};
pub use network::NetworkTracker;
