use rf_types::{Hz, PsdFrame};

/// Wideband occupancy map: stitches per-tile PSD frames onto one frequency grid
/// spanning the whole surveyed range. Feeds the wideband spectrum view and the
/// waterfall.
pub struct OccupancyMap {
    f_lo: Hz,
    f_hi: Hz,
    bins: usize,
    bin_hz: Hz,
    data: Vec<f32>,
}

impl OccupancyMap {
    pub fn new(f_lo: Hz, f_hi: Hz, bins: usize) -> Self {
        assert!(f_hi > f_lo && bins > 0);
        let bin_hz = (f_hi - f_lo) / bins as f64;
        Self {
            f_lo,
            f_hi,
            bins,
            bin_hz,
            data: vec![-240.0; bins],
        }
    }

    /// Map a tile's bins onto the grid (latest value wins).
    pub fn accept(&mut self, frame: &PsdFrame) {
        let n = frame.psd_dbfs.len();
        let low_edge = frame.tile_center_hz - (n as f64 / 2.0) * frame.bin_hz;
        for (i, &db) in frame.psd_dbfs.iter().enumerate() {
            let f = low_edge + (i as f64 + 0.5) * frame.bin_hz;
            if f < self.f_lo || f >= self.f_hi {
                continue;
            }
            let g = ((f - self.f_lo) / self.bin_hz) as usize;
            if g < self.bins {
                self.data[g] = db;
            }
        }
    }

    pub fn snapshot(&self) -> &[f32] {
        &self.data
    }

    pub fn range(&self) -> (Hz, Hz) {
        (self.f_lo, self.f_hi)
    }

    pub fn bins(&self) -> usize {
        self.bins
    }

    /// A max-pooled row downsampled to `width` for the waterfall.
    pub fn row(&self, width: usize) -> Vec<f32> {
        if width == 0 {
            return Vec::new();
        }
        if width >= self.bins {
            return self.data.clone();
        }
        let mut out = vec![-240.0f32; width];
        for (i, slot) in out.iter_mut().enumerate() {
            let lo = i * self.bins / width;
            let hi = ((i + 1) * self.bins / width).max(lo + 1).min(self.bins);
            *slot = self.data[lo..hi].iter().cloned().fold(f32::MIN, f32::max);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rf_types::SensorId;

    #[test]
    fn places_a_tile_at_the_correct_grid_bins() {
        // 100–200 MHz grid, 1000 bins (100 kHz each)
        let mut map = OccupancyMap::new(100e6, 200e6, 1000);
        // a tile centered at 150 MHz, 2 MHz wide, with a hot bin at its center
        let mut psd = vec![-120.0f32; 20];
        psd[10] = -20.0;
        let frame = PsdFrame {
            tile_center_hz: 150e6,
            bin_hz: 100e3,
            psd_dbfs: psd,
            t_unix_ns: 0,
            sensor: SensorId(0),
        };
        map.accept(&frame);
        // the hot bin sits at ~150 MHz → grid bin 500
        let snap = map.snapshot();
        let hottest = snap
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .unwrap()
            .0;
        assert!(
            (hottest as i64 - 500).abs() <= 1,
            "hot grid bin {hottest} ~ 500"
        );
    }
}
