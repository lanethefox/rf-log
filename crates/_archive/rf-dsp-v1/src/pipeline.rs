use crate::fft::FftProcessor;
use crate::averaging::PsdAverager;
use num_complex::Complex32;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

/// Configuration for the DSP pipeline.
#[derive(Debug, Clone)]
pub struct DspConfig {
    pub fft_size: usize,
    pub averaging_alpha: f32,
}

impl Default for DspConfig {
    fn default() -> Self {
        Self {
            fft_size: 32768,
            averaging_alpha: 0.3,
        }
    }
}

/// A processed PSD frame output from the pipeline.
#[derive(Debug, Clone)]
pub struct PsdFrame {
    pub psd: Vec<f32>,
    pub fft_size: usize,
}

/// Consumer trait matching rf-sdr's IqConsumer interface.
pub trait IqSource: Send {
    fn pop_slice(&self, out: &mut [Complex32]) -> usize;
    fn available(&self) -> usize;
}

// Implement IqSource for anything that has the right methods
// We'll use a wrapper to avoid cross-crate trait impls

pub struct IqSourceWrapper<T> {
    inner: T,
}

impl<T> IqSourceWrapper<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: Send> IqSource for IqSourceWrapper<T>
where
    T: IqConsumerLike + Send,
{
    fn pop_slice(&self, out: &mut [Complex32]) -> usize {
        self.inner.pop_slice(out)
    }

    fn available(&self) -> usize {
        self.inner.available()
    }
}

pub trait IqConsumerLike {
    fn pop_slice(&self, out: &mut [Complex32]) -> usize;
    fn available(&self) -> usize;
}

/// Spawn the DSP pipeline thread.
/// Reads IQ chunks from `source`, runs FFT + averaging, sends PSD frames to `psd_tx`.
pub fn spawn_dsp_pipeline(
    source: Box<dyn IqSource>,
    psd_tx: mpsc::Sender<PsdFrame>,
    config: DspConfig,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("dsp_pipeline".into())
        .spawn(move || {
            tracing::info!("DSP pipeline started (fft_size={}, alpha={})", config.fft_size, config.averaging_alpha);

            let mut fft = FftProcessor::new(config.fft_size);
            let mut averager = PsdAverager::new(config.averaging_alpha, config.fft_size);
            let mut iq_buf = vec![Complex32::new(0.0, 0.0); config.fft_size];

            loop {
                // Wait until we have enough samples
                let available = source.available();
                if available < config.fft_size {
                    thread::sleep(std::time::Duration::from_micros(500));
                    continue;
                }

                // Read one FFT-size chunk
                let n = source.pop_slice(&mut iq_buf);
                if n < config.fft_size {
                    continue;
                }

                // FFT → PSD
                let psd = fft.process(&iq_buf);

                // Averaging
                averager.update(&psd);

                // Send averaged PSD
                let frame = PsdFrame {
                    psd: averager.get().to_vec(),
                    fft_size: config.fft_size,
                };

                if psd_tx.send(frame).is_err() {
                    tracing::info!("DSP pipeline: output channel closed, stopping");
                    break;
                }
            }

            tracing::info!("DSP pipeline exited");
        })
        .expect("failed to spawn dsp_pipeline thread")
}
