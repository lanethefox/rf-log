use rf_liquid_sys as ffi;

/// Safe wrapper around LiquidDSP's multi-stage arbitrary-rate resampler (real-valued).
/// Used for decimating FM composite audio (192 kHz → 48 kHz) in the WFM path.
pub struct RealResampler {
    raw: ffi::msresamp_rrrf,
    rate: f32,
}

unsafe impl Send for RealResampler {}

impl RealResampler {
    /// Create a new real-valued resampler.
    /// - `rate`: output/input ratio (e.g., 0.25 for 192k→48k)
    /// - `stopband_atten`: stopband attenuation in dB (e.g., 60.0)
    pub fn new(rate: f32, stopband_atten: f32) -> Self {
        let raw = unsafe { ffi::msresamp_rrrf_create(rate, stopband_atten) };
        assert!(!raw.is_null(), "msresamp_rrrf_create returned null");
        Self { raw, rate }
    }

    /// Resample a block of real-valued input samples.
    /// Returns the number of output samples written.
    /// `output` must be large enough: at least `(input.len() as f32 * rate).ceil() + 16`.
    pub fn execute(&self, input: &[f32], output: &mut [f32]) -> usize {
        let mut n_written: u32 = 0;
        unsafe {
            ffi::msresamp_rrrf_execute(
                self.raw,
                input.as_ptr() as *mut f32,
                input.len() as u32,
                output.as_mut_ptr(),
                &mut n_written,
            );
        }
        n_written as usize
    }

    /// Get the resampling rate.
    pub fn rate(&self) -> f32 {
        self.rate
    }
}

impl Drop for RealResampler {
    fn drop(&mut self) {
        unsafe { ffi::msresamp_rrrf_destroy(self.raw) };
    }
}
