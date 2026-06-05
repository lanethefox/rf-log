use num_complex::Complex32;
use serde::{Deserialize, Serialize};

/// IQ-level RF fingerprint features extracted from a signal burst.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfFingerprint {
    /// Carrier frequency offset in Hz (deviation from nominal)
    pub cfo_hz: f32,
    /// I/Q power imbalance in dB
    pub iq_imbalance_db: f32,
    /// RMS amplitude
    pub rms_amplitude: f32,
}

/// Extract fingerprint features from a burst of IQ samples.
pub fn extract_fingerprint(samples: &[Complex32], sample_rate: f64) -> RfFingerprint {
    let n = samples.len() as f32;

    // RMS amplitude
    let rms_amplitude = (samples.iter().map(|s| s.norm_sqr()).sum::<f32>() / n).sqrt();

    // I/Q power imbalance
    let i_power = samples.iter().map(|s| s.re * s.re).sum::<f32>() / n;
    let q_power = samples.iter().map(|s| s.im * s.im).sum::<f32>() / n;
    let iq_imbalance_db = if q_power > 0.0 {
        10.0 * (i_power / q_power).log10()
    } else {
        0.0
    };

    // CFO estimate via angle of autocorrelation at lag 1
    let mut auto_corr = Complex32::new(0.0, 0.0);
    for i in 0..samples.len().saturating_sub(1) {
        auto_corr += samples[i] * samples[i + 1].conj();
    }
    let cfo_hz = (auto_corr.arg() * sample_rate as f32) / (2.0 * std::f32::consts::PI);

    RfFingerprint {
        cfo_hz,
        iq_imbalance_db,
        rms_amplitude,
    }
}

/// Cosine similarity between two fingerprint feature vectors.
pub fn similarity(a: &RfFingerprint, b: &RfFingerprint) -> f32 {
    let va = [a.cfo_hz, a.iq_imbalance_db, a.rms_amplitude];
    let vb = [b.cfo_hz, b.iq_imbalance_db, b.rms_amplitude];
    let dot: f32 = va.iter().zip(vb.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = va.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = vb.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}
