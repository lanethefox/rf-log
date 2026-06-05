use serde::{Deserialize, Serialize};

/// A reference library entry describing a known emitter type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmitterRef {
    pub id: i64,
    pub name: String,
    pub emitter_type: String,
    pub freq_min_mhz: f64,
    pub freq_max_mhz: f64,
    pub pri_min_us: Option<f64>,
    pub pri_max_us: Option<f64>,
    pub pw_min_us: Option<f64>,
    pub pw_max_us: Option<f64>,
    pub notes: Option<String>,
}

/// Match score result against a library entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub emitter_ref: EmitterRef,
    /// 0.0–1.0 confidence
    pub confidence: f64,
}

/// Score a measured emitter against a reference library entry.
/// Returns None if the emitter clearly doesn't match (freq out of range).
pub fn score_match(
    freq_mhz: f64,
    pri_us: Option<f64>,
    pw_us: Option<f64>,
    entry: &EmitterRef,
) -> Option<MatchResult> {
    if freq_mhz < entry.freq_min_mhz || freq_mhz > entry.freq_max_mhz {
        return None;
    }

    let mut score = 0.5; // base score for freq match
    let mut checks = 1;

    if let (Some(pri), Some(ref_min), Some(ref_max)) =
        (pri_us, entry.pri_min_us, entry.pri_max_us)
    {
        checks += 1;
        if pri >= ref_min && pri <= ref_max {
            score += 0.3;
        }
    }

    if let (Some(pw), Some(ref_min), Some(ref_max)) =
        (pw_us, entry.pw_min_us, entry.pw_max_us)
    {
        checks += 1;
        if pw >= ref_min && pw <= ref_max {
            score += 0.2;
        }
    }

    let confidence = score / checks as f64 * checks as f64; // normalise
    let confidence = (score / 1.0_f64).min(1.0);

    Some(MatchResult {
        emitter_ref: entry.clone(),
        confidence,
    })
}
