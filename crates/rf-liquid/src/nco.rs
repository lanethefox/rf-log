use num_complex::Complex32;
use rf_liquid_sys as ffi;

/// Safe wrapper around LiquidDSP's NCO (numerically-controlled oscillator).
/// Used to frequency-shift IQ samples (mix down to baseband).
pub struct Nco {
    raw: ffi::nco_crcf,
}

unsafe impl Send for Nco {}

impl Nco {
    /// Create a new NCO with the given normalized angular frequency.
    /// `freq` is in radians/sample: 2*pi*f_offset/f_sample
    pub fn new(freq: f32) -> Self {
        let raw = unsafe { ffi::nco_crcf_create(ffi::liquid_ncotype_LIQUID_VCO) };
        assert!(!raw.is_null(), "nco_crcf_create returned null");
        unsafe { ffi::nco_crcf_set_frequency(raw, freq) };
        Self { raw }
    }

    /// Update the NCO frequency (radians/sample).
    pub fn set_frequency(&self, freq: f32) {
        unsafe { ffi::nco_crcf_set_frequency(self.raw, freq) };
    }

    /// Mix a block of samples down by the NCO frequency.
    /// `input` and `output` must be the same length.
    pub fn mix_block_down(&self, input: &[Complex32], output: &mut [Complex32]) {
        assert_eq!(input.len(), output.len());
        unsafe {
            ffi::nco_crcf_mix_block_down(
                self.raw,
                input.as_ptr() as *mut ffi::liquid_float_complex,
                output.as_mut_ptr() as *mut ffi::liquid_float_complex,
                input.len() as u32,
            );
        }
    }
}

impl Drop for Nco {
    fn drop(&mut self) {
        unsafe { ffi::nco_crcf_destroy(self.raw) };
    }
}
