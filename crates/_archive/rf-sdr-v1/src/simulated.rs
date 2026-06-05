use crate::{IqSample, SdrDevice};
use serde::Deserialize;
use std::f32::consts::PI;

#[derive(Debug, Deserialize)]
struct FreqDbFile {
    conventional: Vec<FreqEntry>,
}

#[derive(Debug, Deserialize)]
struct FreqEntry {
    freq: f64,
    #[allow(dead_code)]
    name: String,
    cls: String,
    #[allow(dead_code)]
    band: String,
    #[allow(dead_code)]
    mode: Option<String>,
}

struct SimSignal {
    freq_hz: f64,
    amplitude: f32,
    active: bool,
    toggle_counter: u32,
    toggle_period: u32,
}

/// Simulated SDR device that generates synthetic IQ data.
pub struct SimulatedSdr {
    center_freq: f64,
    sample_rate: f64,
    signals: Vec<SimSignal>,
    phase_accum: f64,
    sample_count: u64,
}

impl SimulatedSdr {
    /// Create a new simulated SDR, optionally loading signals from portland-frequencies.json.
    pub fn new(freq_db_path: &str) -> Result<Self, String> {
        let mut signals = Vec::new();

        // Try to load frequency database
        if let Ok(data) = std::fs::read_to_string(freq_db_path) {
            if let Ok(db) = serde_json::from_str::<FreqDbFile>(&data) {
                for entry in &db.conventional {
                    // Convert MHz to Hz
                    let freq_hz = entry.freq * 1e6;
                    // Assign amplitude based on signal class
                    let amplitude = match entry.cls.as_str() {
                        "WX" => 0.08,
                        "PUBS" => 0.05,
                        "AMAT" => 0.03,
                        "MARN" => 0.02,
                        "GMRS" => 0.02,
                        "COMM" => 0.04,
                        "FEDL" => 0.03,
                        "BCST" => 0.06,
                        _ => 0.03,
                    };
                    let toggle_period = 30 + (signals.len() as u32 * 7) % 50;
                    signals.push(SimSignal {
                        freq_hz,
                        amplitude,
                        active: signals.len() % 3 != 2, // ~2/3 start active
                        toggle_counter: 0,
                        toggle_period,
                    });
                }
                tracing::info!("Simulated SDR loaded {} signals from {}", signals.len(), freq_db_path);
            }
        }

        // If no signals loaded, use hardcoded defaults matching frontend simulation
        if signals.is_empty() {
            tracing::info!("Using hardcoded simulation signals");
            let defaults = [
                (146.52e6, 0.04, "2m Call"),
                (155.01e6, 0.06, "PPB"),
                (162.40e6, 0.08, "WX1"),
                (156.80e6, 0.03, "Mar16"),
                (453.45e6, 0.05, "PF&R"),
                (460.53e6, 0.04, "MCSO"),
                (462.5625e6, 0.02, "FRS1"),
                (769.50e6, 0.05, "OWIN"),
            ];
            for (i, &(freq_hz, amplitude, _name)) in defaults.iter().enumerate() {
                signals.push(SimSignal {
                    freq_hz,
                    amplitude,
                    active: true,
                    toggle_counter: 0,
                    toggle_period: 20 + (i as u32 * 5),
                });
            }
        }

        Ok(Self {
            center_freq: 155.0e6,
            sample_rate: 2_400_000.0,
            signals,
            phase_accum: 0.0,
            sample_count: 0,
        })
    }
}

impl SdrDevice for SimulatedSdr {
    fn is_simulated(&self) -> bool { true }

    fn set_freq(&mut self, freq: f64) -> Result<(), String> {
        self.center_freq = freq;
        self.phase_accum = 0.0;
        Ok(())
    }

    fn set_gain(&mut self, _gain: f64) -> Result<(), String> {
        Ok(())
    }

    fn read_iq(&mut self, buf: &mut [IqSample]) -> Result<usize, String> {
        let dt = 1.0 / self.sample_rate;
        let half_bw = self.sample_rate / 2.0;

        // Toggle signal activity periodically
        let sweep_tick = (self.sample_count / 2048) as u32;
        for sig in &mut self.signals {
            sig.toggle_counter = sig.toggle_counter.wrapping_add(1);
            if sig.toggle_counter >= sig.toggle_period * (sweep_tick % 3 + 1) {
                sig.toggle_counter = 0;
                sig.active = !sig.active;
            }
        }

        for (i, sample) in buf.iter_mut().enumerate() {
            let t = (self.sample_count + i as u64) as f64 * dt;

            // Noise floor
            let noise_i = (pseudo_random(self.sample_count + i as u64) - 0.5) * 0.005;
            let noise_q = (pseudo_random(self.sample_count + i as u64 + 1_000_000) - 0.5) * 0.005;

            let mut re = noise_i as f32;
            let mut im = noise_q as f32;

            // Add each active signal as a complex tone at its offset frequency
            for sig in &self.signals {
                if !sig.active {
                    continue;
                }
                let offset = sig.freq_hz - self.center_freq;
                // Only generate if within our bandwidth
                if offset.abs() > half_bw {
                    continue;
                }
                let phase = 2.0 * PI as f64 * offset * t;
                re += sig.amplitude * phase.cos() as f32;
                im += sig.amplitude * phase.sin() as f32;
            }

            *sample = IqSample::new(re, im);
        }

        self.sample_count += buf.len() as u64;
        Ok(buf.len())
    }
}

/// Simple deterministic pseudo-random number generator (0.0..1.0)
fn pseudo_random(seed: u64) -> f64 {
    let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    (x as f64) / (u64::MAX as f64)
}
