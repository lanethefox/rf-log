use num_complex::Complex32;
use rf_liquid_sys as ffi;

/// Safe wrapper around LiquidDSP's FIR filter (complex coefficients, complex I/O).
/// Used for channel filtering before demodulation.
pub struct FirFilter {
    raw: ffi::firfilt_crcf,
}

unsafe impl Send for FirFilter {}

impl FirFilter {
    /// Create a Kaiser-windowed FIR filter.
    /// - `n`: filter length (number of taps)
    /// - `cutoff`: normalized cutoff frequency (0 to 0.5)
    /// - `stopband_atten`: stopband attenuation in dB (e.g., 60.0)
    /// - `fractional_offset`: fractional sample offset (usually 0.0)
    pub fn new_kaiser(n: u32, cutoff: f32, stopband_atten: f32, fractional_offset: f32) -> Self {
        let raw = unsafe {
            ffi::firfilt_crcf_create_kaiser(n, cutoff, stopband_atten, fractional_offset)
        };
        assert!(!raw.is_null(), "firfilt_crcf_create_kaiser returned null");
        Self { raw }
    }

    /// Filter a block of samples.
    /// `input` and `output` must be the same length.
    pub fn execute_block(&self, input: &[Complex32], output: &mut [Complex32]) {
        assert_eq!(input.len(), output.len());
        unsafe {
            ffi::firfilt_crcf_execute_block(
                self.raw,
                input.as_ptr() as *mut ffi::liquid_float_complex,
                input.len() as u32,
                output.as_mut_ptr() as *mut ffi::liquid_float_complex,
            );
        }
    }

    /// Reset the filter state.
    pub fn reset(&self) {
        unsafe { ffi::firfilt_crcf_reset(self.raw) };
    }
}

impl Drop for FirFilter {
    fn drop(&mut self) {
        unsafe { ffi::firfilt_crcf_destroy(self.raw) };
    }
}
