use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarmonicGroup {
    pub fundamental_mhz: f64,
    pub harmonics: Vec<f64>,
    pub source_hypothesis: Option<String>,
}

/// Scan a list of signal peaks for harmonic relationships.
/// Returns groups where peaks are related by integer multiples within `tolerance_pct`.
pub fn find_harmonics(peaks_mhz: &[f64], tolerance_pct: f64) -> Vec<HarmonicGroup> {
    let mut groups: Vec<HarmonicGroup> = Vec::new();
    let mut used = vec![false; peaks_mhz.len()];

    for (i, &f0) in peaks_mhz.iter().enumerate() {
        if used[i] || f0 <= 0.0 {
            continue;
        }
        let mut harmonics = Vec::new();
        for (j, &fh) in peaks_mhz.iter().enumerate() {
            if i == j || used[j] {
                continue;
            }
            let ratio = fh / f0;
            let nearest_int = ratio.round();
            if nearest_int >= 2.0
                && (ratio - nearest_int).abs() / nearest_int < tolerance_pct / 100.0
            {
                harmonics.push(fh);
                used[j] = true;
            }
        }
        if !harmonics.is_empty() {
            used[i] = true;
            let source = guess_source(f0);
            groups.push(HarmonicGroup {
                fundamental_mhz: f0,
                harmonics,
                source_hypothesis: source,
            });
        }
    }

    groups
}

fn guess_source(fundamental_mhz: f64) -> Option<String> {
    // Common clock/oscillator fundamentals
    let known = [
        (0.012, "USB 1.1 (12 MHz)"),
        (0.025, "HDMI pixel clock (25 MHz)"),
        (0.027, "Crystal (27 MHz)"),
        (0.048, "USB 2.0 (48 MHz)"),
        (0.100, "Crystal (100 MHz)"),
        (0.480, "USB 2.0 HS (480 MHz)"),
    ];
    for (freq, label) in &known {
        if (fundamental_mhz - freq).abs() / freq < 0.01 {
            return Some(label.to_string());
        }
    }
    None
}
