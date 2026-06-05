use serde::{Deserialize, Serialize};

/// Per-frequency-bin statistics accumulated over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinStats {
    pub freq_mhz: f64,
    pub mean: f32,
    pub std_dev: f32,
    pub min: f32,
    pub max: f32,
    pub sample_count: u64,
}

/// Accumulates running statistics for a spectrum baseline.
pub struct BaselineAccumulator {
    /// Bins indexed by bin index; each holds (sum, sum_sq, min, max, count)
    bins: Vec<(f64, f64, f32, f32, u64)>,
    pub freq_start_mhz: f64,
    pub freq_step_mhz: f64,
}

impl BaselineAccumulator {
    pub fn new(bin_count: usize, freq_start_mhz: f64, freq_step_mhz: f64) -> Self {
        Self {
            bins: vec![(0.0, 0.0, f32::MAX, f32::MIN, 0); bin_count],
            freq_start_mhz,
            freq_step_mhz,
        }
    }

    /// Ingest a spectrum frame (power values in dBFS per bin).
    pub fn update(&mut self, powers: &[f32]) {
        for (i, &p) in powers.iter().enumerate() {
            if i >= self.bins.len() {
                break;
            }
            let (sum, sum_sq, min, max, count) = &mut self.bins[i];
            *sum += p as f64;
            *sum_sq += (p as f64).powi(2);
            *min = min.min(p);
            *max = max.max(p);
            *count += 1;
        }
    }

    /// Number of spectrum frames fed in so far (from the first bin).
    pub fn sample_count(&self) -> u64 {
        self.bins.first().map(|(_, _, _, _, c)| *c).unwrap_or(0)
    }

    /// Compute final BinStats for each bin.
    pub fn finalize(&self) -> Vec<BinStats> {
        self.bins
            .iter()
            .enumerate()
            .map(|(i, (sum, sum_sq, min, max, count))| {
                let freq_mhz = self.freq_start_mhz + i as f64 * self.freq_step_mhz;
                if *count == 0 {
                    return BinStats {
                        freq_mhz,
                        mean: 0.0,
                        std_dev: 0.0,
                        min: 0.0,
                        max: 0.0,
                        sample_count: 0,
                    };
                }
                let n = *count as f64;
                let mean = (sum / n) as f32;
                let variance = ((sum_sq / n) - (sum / n).powi(2)).max(0.0);
                BinStats {
                    freq_mhz,
                    mean,
                    std_dev: (variance as f32).sqrt(),
                    min: *min,
                    max: *max,
                    sample_count: *count,
                }
            })
            .collect()
    }
}
