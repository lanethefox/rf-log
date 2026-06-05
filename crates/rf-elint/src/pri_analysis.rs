use crate::pdw::Pdw;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PriPattern {
    Stable,
    Stagger,
    Jitter,
    Agile,
    Insufficient,
}

/// Analyse a sequence of PDWs and classify their PRI pattern.
pub fn classify_pri(pdws: &[Pdw]) -> PriPattern {
    let pris: Vec<f64> = pdws.iter().filter_map(|p| p.pri_us).collect();
    if pris.len() < 3 {
        return PriPattern::Insufficient;
    }

    let mean = pris.iter().sum::<f64>() / pris.len() as f64;
    if mean == 0.0 {
        return PriPattern::Insufficient;
    }

    let variance = pris.iter().map(|&p| (p - mean).powi(2)).sum::<f64>() / pris.len() as f64;
    let std_dev = variance.sqrt();
    let cv = std_dev / mean; // coefficient of variation

    if cv < 0.01 {
        PriPattern::Stable
    } else if cv < 0.05 {
        PriPattern::Jitter
    } else if is_stagger(&pris) {
        PriPattern::Stagger
    } else {
        PriPattern::Agile
    }
}

/// Simple stagger detector: check if PRIs repeat in a short cycle (2-5 values).
fn is_stagger(pris: &[f64]) -> bool {
    for cycle_len in 2..=5usize {
        if pris.len() < cycle_len * 2 {
            continue;
        }
        let template: Vec<f64> = pris[..cycle_len].to_vec();
        let all_match = pris.chunks(cycle_len).all(|chunk| {
            chunk.iter().zip(&template).all(|(a, b)| (a - b).abs() / b < 0.02)
        });
        if all_match {
            return true;
        }
    }
    false
}
