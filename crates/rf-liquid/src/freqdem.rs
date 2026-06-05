use num_complex::Complex32;
use rf_liquid_sys as ffi;

/// Safe wrapper around LiquidDSP's FM frequency demodulator.
/// Used for NFM and WFM demodulation.
pub struct FreqDemod {
    raw: ffi::freqdem,
}

unsafe impl Send for FreqDemod {}

impl FreqDemod {
    /// Create a new FM demodulator.
    /// - `kf`: modulation factor (0.5 for NFM, 0.8 for WFM)
    pub fn new(kf: f32) -> Self {
        let raw = unsafe { ffi::freqdem_create(kf) };
        assert!(!raw.is_null(), "freqdem_create returned null");
        Self { raw }
    }

    /// Demodulate a block of IQ samples into audio.
    /// `input` (complex) and `output` (real) must be the same length.
    pub fn demod_block(&self, input: &[Complex32], output: &mut [f32]) {
        assert_eq!(input.len(), output.len());
        unsafe {
            ffi::freqdem_demodulate_block(
                self.raw,
                input.as_ptr() as *mut ffi::liquid_float_complex,
                input.len() as u32,
                output.as_mut_ptr(),
            );
        }
    }

    /// Reset the demodulator state.
    pub fn reset(&self) {
        unsafe { ffi::freqdem_reset(self.raw) };
    }
}

impl Drop for FreqDemod {
    fn drop(&mut self) {
        unsafe { ffi::freqdem_destroy(self.raw) };
    }
}
