use num_complex::Complex32;
use rf_liquid_sys as ffi;

/// Safe wrapper around LiquidDSP's multi-stage arbitrary-rate resampler.
/// Used to decimate from SDR sample rate (2.4 MHz) to audio rate (48 kHz).
pub struct Resampler {
    raw: ffi::msresamp_crcf,
    rate: f32,
}

unsafe impl Send for Resampler {}

impl Resampler {
    /// Create a new resampler.
    /// - `rate`: output/input ratio (e.g., 0.02 for 2.4M→48k)
    /// - `stopband_atten`: stopband attenuation in dB (e.g., 60.0)
    pub fn new(rate: f32, stopband_atten: f32) -> Self {
        let raw = unsafe { ffi::msresamp_crcf_create(rate, stopband_atten) };
        assert!(!raw.is_null(), "msresamp_crcf_create returned null");
        Self { raw, rate }
    }

    /// Resample a block of input samples.
    /// Returns the number of output samples written to `output`.
    /// `output` must be large enough: at least `(input.len() as f32 * rate).ceil() + 16` samples.
    pub fn execute(&self, input: &[Complex32], output: &mut [Complex32]) -> usize {
        let mut n_written: u32 = 0;
        unsafe {
            ffi::msresamp_crcf_execute(
                self.raw,
                input.as_ptr() as *mut ffi::liquid_float_complex,
                input.len() as u32,
                output.as_mut_ptr() as *mut ffi::liquid_float_complex,
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

impl Drop for Resampler {
    fn drop(&mut self) {
        unsafe { ffi::msresamp_crcf_destroy(self.raw) };
    }
}
