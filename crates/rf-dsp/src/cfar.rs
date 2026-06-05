use rf_types::{Detection, Hz, SensorId, UnixNanos};

/// CA-CFAR (cell-averaging constant false-alarm rate) detector configuration.
#[derive(Debug, Clone)]
pub struct CfarConfig {
    /// Training cells per side (noise estimate window).
    pub num_train: usize,
    /// Guard cells per side (excluded around the cell under test).
    pub num_guard: usize,
    /// Target probability of false alarm.
    pub pfa: f64,
    /// Reject detections weaker than this SNR (dB) above the local noise.
    pub min_snr_db: f32,
}

impl Default for CfarConfig {
    fn default() -> Self {
        Self {
            num_train: 24,
            num_guard: 4,
            pfa: 1e-4,
            min_snr_db: 6.0,
        }
    }
}

/// Run CA-CFAR over a dBFS PSD frame and return grouped detections.
///
/// Works in linear power: for each cell under test, the noise is the mean of the
/// training cells on both sides (skipping guard cells), and the threshold is
/// `mean * alpha` where `alpha` is derived from `pfa`. Contiguous detected bins are
/// grouped into one detection; center is the power-weighted centroid and bandwidth
/// is the 99%-energy occupied bandwidth of the cluster.
pub fn detect(
    psd_dbfs: &[f32],
    cfg: &CfarConfig,
    bin_hz: Hz,
    tile_center_hz: Hz,
    sensor: SensorId,
    t_unix_ns: UnixNanos,
) -> Vec<Detection> {
    let n = psd_dbfs.len();
    let win = cfg.num_train + cfg.num_guard;
    if cfg.num_train == 0 || n < 2 * win + 1 {
        return Vec::new();
    }

    let lin: Vec<f64> = psd_dbfs
        .iter()
        .map(|&d| 10f64.powf(d as f64 / 10.0))
        .collect();
    let ntrain_total = 2 * cfg.num_train;
    let alpha = ntrain_total as f64 * (cfg.pfa.powf(-1.0 / ntrain_total as f64) - 1.0);

    let mut flagged = vec![false; n];
    let mut noise_lin = vec![0.0f64; n];
    for i in win..n - win {
        let mut sum = 0.0f64;
        for k in 1..=cfg.num_train {
            sum += lin[i - cfg.num_guard - k];
            sum += lin[i + cfg.num_guard + k];
        }
        let mean = sum / ntrain_total as f64;
        noise_lin[i] = mean;
        if lin[i] > mean * alpha {
            flagged[i] = true;
        }
    }

    let mut dets = Vec::new();
    let mut i = win;
    while i < n - win {
        if !flagged[i] {
            i += 1;
            continue;
        }
        // Extend the cluster, tolerating gaps of up to 2 unflagged bins.
        let start = i;
        let mut end = i;
        let mut gap = 0;
        let mut j = i + 1;
        while j < n - win {
            if flagged[j] {
                end = j;
                gap = 0;
            } else {
                gap += 1;
                if gap > 2 {
                    break;
                }
            }
            j += 1;
        }

        let cluster = &lin[start..=end];
        // peak
        let (peak_off, &peak_db) = psd_dbfs[start..=end]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap();
        let peak_idx = start + peak_off;
        let noise_db = 10.0 * noise_lin[peak_idx].max(1e-24).log10();
        let snr_db = (peak_db as f64 - noise_db) as f32;

        if snr_db >= cfg.min_snr_db {
            // power-weighted centroid (absolute bin index)
            let mut wsum = 0.0f64;
            let mut isum = 0.0f64;
            for (k, &p) in cluster.iter().enumerate() {
                wsum += p;
                isum += p * (start + k) as f64;
            }
            let centroid = if wsum > 0.0 {
                isum / wsum
            } else {
                peak_idx as f64
            };
            let (lo_off, hi_off) = obw_99(cluster);
            let bw = (hi_off - lo_off + 1) as f64 * bin_hz;
            dets.push(Detection {
                center_hz: bin_freq(centroid, n, tile_center_hz, bin_hz),
                bandwidth_hz: bw.max(bin_hz),
                power_dbfs: peak_db,
                snr_db,
                t_unix_ns,
                tile_center_hz,
                sensor,
            });
        }
        i = end + 1;
    }
    dets
}

fn bin_freq(i: f64, n: usize, center: Hz, bin_hz: Hz) -> Hz {
    let low_edge = center - (n as f64 / 2.0) * bin_hz;
    low_edge + (i + 0.5) * bin_hz
}

/// Returns the (lo, hi) offsets within `lin` bounding 99% of the energy.
fn obw_99(lin: &[f64]) -> (usize, usize) {
    let total: f64 = lin.iter().sum();
    if total <= 0.0 || lin.len() == 1 {
        return (0, lin.len().saturating_sub(1));
    }
    let lo_t = total * 0.005;
    let hi_t = total * 0.995;
    let mut cum = 0.0;
    let mut lo = 0;
    for (k, &p) in lin.iter().enumerate() {
        cum += p;
        if cum >= lo_t {
            lo = k;
            break;
        }
    }
    cum = 0.0;
    let mut hi = lin.len() - 1;
    for (k, &p) in lin.iter().enumerate() {
        cum += p;
        if cum >= hi_t {
            hi = k;
            break;
        }
    }
    (lo, hi.max(lo))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_noise(n: usize, floor_db: f32) -> Vec<f32> {
        vec![floor_db; n]
    }

    #[test]
    fn detects_a_clear_peak_at_the_right_frequency() {
        let n = 1024;
        let mut psd = flat_noise(n, -90.0);
        // a 5-bin signal around bin 700
        psd[698..=702].fill(-40.0);
        let cfg = CfarConfig::default();
        let bin_hz = 1_000.0; // 1 kHz/bin
        let center = 100e6;
        let dets = detect(&psd, &cfg, bin_hz, center, SensorId(0), 0);
        assert_eq!(dets.len(), 1, "exactly one detection");
        let d = &dets[0];
        // expected center: bin 700
        let expect = center - (n as f64 / 2.0) * bin_hz + (700.0 + 0.5) * bin_hz;
        assert!(
            (d.center_hz - expect).abs() < 2.0 * bin_hz,
            "center {} vs {}",
            d.center_hz,
            expect
        );
        assert!(d.snr_db > 40.0, "snr {}", d.snr_db);
        assert!(d.bandwidth_hz >= bin_hz && d.bandwidth_hz <= 12.0 * bin_hz);
    }

    #[test]
    fn flat_noise_does_not_false_alarm() {
        let n = 2048;
        let psd = flat_noise(n, -85.0);
        let dets = detect(&psd, &CfarConfig::default(), 1_000.0, 100e6, SensorId(0), 0);
        assert!(
            dets.is_empty(),
            "CFAR fired on flat noise: {} detections",
            dets.len()
        );
    }

    #[test]
    fn resolves_two_separate_signals() {
        let n = 1024;
        let mut psd = flat_noise(n, -95.0);
        psd[300..=303].fill(-50.0);
        psd[700..=703].fill(-45.0);
        let dets = detect(&psd, &CfarConfig::default(), 1_000.0, 100e6, SensorId(0), 0);
        assert_eq!(dets.len(), 2);
    }
}
