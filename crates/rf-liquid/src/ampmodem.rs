use num_complex::Complex32;
use rf_liquid_sys as ffi;

/// Amplitude modulation type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AmpModemType {
    Dsb,
    Usb,
    Lsb,
}

impl AmpModemType {
    fn to_ffi(self) -> ffi::liquid_ampmodem_type {
        match self {
            AmpModemType::Dsb => ffi::liquid_ampmodem_type_LIQUID_AMPMODEM_DSB,
            AmpModemType::Usb => ffi::liquid_ampmodem_type_LIQUID_AMPMODEM_USB,
            AmpModemType::Lsb => ffi::liquid_ampmodem_type_LIQUID_AMPMODEM_LSB,
        }
    }
}

/// Safe wrapper around LiquidDSP's AM/SSB demodulator.
/// Used for AM, USB, and LSB demodulation.
pub struct AmpModem {
    raw: ffi::ampmodem,
}

unsafe impl Send for AmpModem {}

impl AmpModem {
    /// Create a new AM/SSB demodulator.
    /// - `mod_index`: modulation index (e.g., 0.8 for AM)
    /// - `modem_type`: DSB, USB, or LSB
    /// - `suppressed_carrier`: true for suppressed carrier (SSB), false for AM
    pub fn new(mod_index: f32, modem_type: AmpModemType, suppressed_carrier: bool) -> Self {
        let raw = unsafe {
            ffi::ampmodem_create(
                mod_index,
                modem_type.to_ffi(),
                if suppressed_carrier { 1 } else { 0 },
            )
        };
        assert!(!raw.is_null(), "ampmodem_create returned null");
        Self { raw }
    }

    /// Demodulate a block of IQ samples into audio.
    /// `input` (complex) and `output` (real) must be the same length.
    pub fn demod_block(&self, input: &[Complex32], output: &mut [f32]) {
        assert_eq!(input.len(), output.len());
        unsafe {
            ffi::ampmodem_demodulate_block(
                self.raw,
                input.as_ptr() as *mut ffi::liquid_float_complex,
                input.len() as u32,
                output.as_mut_ptr(),
            );
        }
    }

    /// Reset the demodulator state.
    pub fn reset(&self) {
        unsafe { ffi::ampmodem_reset(self.raw) };
    }
}

impl Drop for AmpModem {
    fn drop(&mut self) {
        unsafe { ffi::ampmodem_destroy(self.raw) };
    }
}
