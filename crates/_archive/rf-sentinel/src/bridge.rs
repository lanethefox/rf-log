use std::sync::Mutex;

/// Latest spectrum frame from the DSP pipeline, cached for WebSocket broadcast.
#[derive(Debug, Clone)]
pub struct SpectrumFrame {
    pub band: String,
    pub freqs: Vec<f64>,
    pub powers: Vec<f64>,
    pub noise_floor: f64,
}

/// Shared state polled by the WebSocket spectrum handler.
pub struct SentinelBridge {
    pub latest_spectrum: Option<SpectrumFrame>,
}

impl SentinelBridge {
    pub fn new() -> Mutex<Self> {
        Mutex::new(Self {
            latest_spectrum: None,
        })
    }

    pub fn update_spectrum(&mut self, frame: SpectrumFrame) {
        self.latest_spectrum = Some(frame);
    }
}
