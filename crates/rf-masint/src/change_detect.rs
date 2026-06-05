use crate::baseline::BinStats;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnomalyKind {
    NewSignal,
    PowerIncrease,
    PowerDecrease,
    SignalGone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    pub freq_mhz: f64,
    pub kind: AnomalyKind,
    pub delta_db: f32,
    /// Z-score (standard deviations from baseline mean)
    pub z_score: f32,
}

/// Compare a live spectrum frame against a stored baseline.
/// Returns anomalies where power deviates beyond `z_threshold` standard deviations.
pub fn detect_anomalies(
    live_powers: &[f32],
    baseline: &[BinStats],
    z_threshold: f32,
) -> Vec<Anomaly> {
    let mut anomalies = Vec::new();

    for (live, stats) in live_powers.iter().zip(baseline.iter()) {
        if stats.sample_count == 0 || stats.std_dev < 0.1 {
            continue;
        }
        let delta = live - stats.mean;
        let z = delta / stats.std_dev;

        if z.abs() < z_threshold {
            continue;
        }

        let kind = if *live > stats.max + 3.0 {
            AnomalyKind::NewSignal
        } else if delta > 0.0 {
            AnomalyKind::PowerIncrease
        } else if *live < stats.min - 3.0 {
            AnomalyKind::SignalGone
        } else {
            AnomalyKind::PowerDecrease
        };

        anomalies.push(Anomaly {
            freq_mhz: stats.freq_mhz,
            kind,
            delta_db: delta,
            z_score: z,
        });
    }

    anomalies
}
