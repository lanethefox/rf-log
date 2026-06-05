/// Exponential moving average for PSD bins.
pub struct PsdAverager {
    alpha: f32,
    data: Vec<f32>,
    initialized: bool,
}

impl PsdAverager {
    pub fn new(alpha: f32, size: usize) -> Self {
        Self {
            alpha,
            data: vec![-120.0; size],
            initialized: false,
        }
    }

    /// Blend a new PSD frame into the running average.
    pub fn update(&mut self, psd: &[f32]) {
        if !self.initialized || self.data.len() != psd.len() {
            self.data = psd.to_vec();
            self.initialized = true;
            return;
        }

        for (avg, &new) in self.data.iter_mut().zip(psd.iter()) {
            *avg = self.alpha * new + (1.0 - self.alpha) * *avg;
        }
    }

    /// Get the current averaged PSD.
    pub fn get(&self) -> &[f32] {
        &self.data
    }

    /// Reset the averager (e.g., on retune).
    pub fn reset(&mut self) {
        self.initialized = false;
        self.data.fill(-120.0);
    }
}
