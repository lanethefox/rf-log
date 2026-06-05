/// A detected signal in the spectrum.
#[derive(Debug, Clone)]
pub struct Detection {
    pub freq: f64,
    pub power: f32,
    pub bandwidth: f32,
}

/// Detect signals in a PSD by finding peaks above a threshold.
///
/// - `psd`: power spectral density in dBFS, DC-centered
/// - `freq_start`: frequency of the first bin (Hz)
/// - `freq_step`: frequency step per bin (Hz)
/// - `threshold`: minimum power (dBFS) for a detection
pub fn detect_signals(
    psd: &[f32],
    freq_start: f64,
    freq_step: f64,
    threshold: f64,
) -> Vec<Detection> {
    if psd.len() < 3 {
        return Vec::new();
    }

    // Estimate noise floor as the median
    let noise_floor = estimate_noise_floor(psd);
    let effective_threshold = threshold.max(noise_floor as f64 + 6.0) as f32;

    let mut detections = Vec::new();
    let min_rise = 3.0f32; // must be at least 3dB above neighbors

    for i in 1..psd.len() - 1 {
        let cur = psd[i];
        let left = psd[i - 1];
        let right = psd[i + 1];

        // Local maximum check
        if cur > left && cur > right && cur > effective_threshold {
            // Must rise 3dB above at least one neighbor
            if (cur - left) >= min_rise || (cur - right) >= min_rise {
                // Parabolic interpolation for sub-bin frequency precision
                let alpha = left;
                let beta = cur;
                let gamma = right;
                let denom = alpha - 2.0 * beta + gamma;
                let delta = if denom.abs() > 1e-10 {
                    0.5 * (alpha - gamma) / denom
                } else {
                    0.0
                };

                let freq = freq_start + (i as f64 + delta as f64) * freq_step;
                let power = beta - 0.25 * (alpha - gamma) * delta;

                // Estimate bandwidth: count bins above noise_floor + 3dB around peak
                let bw_threshold = noise_floor + 3.0;
                let mut bw_bins = 1;
                let mut j = i.wrapping_sub(1);
                while j < psd.len() && psd[j] > bw_threshold {
                    bw_bins += 1;
                    if j == 0 { break; }
                    j = j.wrapping_sub(1);
                }
                let mut j = i + 1;
                while j < psd.len() && psd[j] > bw_threshold {
                    bw_bins += 1;
                    j += 1;
                }

                let bandwidth = bw_bins as f32 * freq_step as f32;

                detections.push(Detection {
                    freq,
                    power,
                    bandwidth,
                });
            }
        }
    }

    detections
}

/// Estimate noise floor as the median of PSD bins.
fn estimate_noise_floor(psd: &[f32]) -> f32 {
    let mut sorted: Vec<f32> = psd.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted[sorted.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_single_peak() {
        let n = 512;
        let mut psd = vec![-100.0f32; n];
        // Insert a peak at bin 200
        psd[199] = -70.0;
        psd[200] = -50.0;
        psd[201] = -70.0;

        let dets = detect_signals(&psd, 136.0e6, 74218.75, -60.0);
        assert_eq!(dets.len(), 1);
        assert!((dets[0].power - (-50.0)).abs() < 1.0);
    }

    #[test]
    fn no_detection_below_threshold() {
        let psd = vec![-100.0f32; 512];
        let dets = detect_signals(&psd, 136.0e6, 74218.75, -60.0);
        assert!(dets.is_empty());
    }
}
