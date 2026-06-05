use rf_liquid_sys as ffi;

/// Safe wrapper around LiquidDSP's FIR filter (real coefficients, real I/O).
/// Used for RDS subcarrier lowpass filtering.
pub struct RealFirFilter {
    raw: ffi::firfilt_rrrf,
}

unsafe impl Send for RealFirFilter {}

impl RealFirFilter {
    /// Create a Kaiser-windowed real-valued FIR filter.
    /// - `n`: filter length (number of taps)
    /// - `cutoff`: normalized cutoff frequency (0 to 0.5)
    /// - `stopband_atten`: stopband attenuation in dB (e.g., 60.0)
    /// - `fractional_offset`: fractional sample offset (usually 0.0)
    pub fn new_kaiser(n: u32, cutoff: f32, stopband_atten: f32, fractional_offset: f32) -> Self {
        let raw = unsafe {
            ffi::firfilt_rrrf_create_kaiser(n, cutoff, stopband_atten, fractional_offset)
        };
        assert!(!raw.is_null(), "firfilt_rrrf_create_kaiser returned null");
        Self { raw }
    }

    /// Filter a block of real-valued samples.
    /// `input` and `output` must be the same length.
    pub fn execute_block(&self, input: &[f32], output: &mut [f32]) {
        assert_eq!(input.len(), output.len());
        unsafe {
            ffi::firfilt_rrrf_execute_block(
                self.raw,
                input.as_ptr() as *mut f32,
                input.len() as u32,
                output.as_mut_ptr(),
            );
        }
    }

    /// Reset the filter state.
    pub fn reset(&self) {
        unsafe { ffi::firfilt_rrrf_reset(self.raw) };
    }
}

impl Drop for RealFirFilter {
    fn drop(&mut self) {
        unsafe { ffi::firfilt_rrrf_destroy(self.raw) };
    }
}
