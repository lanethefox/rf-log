use serde::{Deserialize, Serialize};

/// A frequency database entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreqEntry {
    pub freq: f64,
    pub name: String,
    pub cls: String,
    pub band: String,
    pub mode: Option<String>,
    pub tag: Option<String>,
}

#[derive(Deserialize)]
struct FreqDbFile {
    conventional: Vec<RawEntry>,
}

#[derive(Deserialize)]
struct RawEntry {
    freq: f64,
    name: String,
    cls: String,
    band: String,
    mode: Option<String>,
    tag: Option<String>,
}

/// Frequency database for signal classification.
pub struct FreqDb {
    entries: Vec<FreqEntry>,
}

impl FreqDb {
    /// Load from portland-frequencies.json.
    pub fn load(path: &str) -> Self {
        let entries = match std::fs::read_to_string(path) {
            Ok(data) => {
                match serde_json::from_str::<FreqDbFile>(&data) {
                    Ok(db) => {
                        let entries: Vec<FreqEntry> = db.conventional.into_iter().map(|e| {
                            FreqEntry {
                                freq: e.freq,
                                name: e.name,
                                cls: e.cls,
                                band: e.band,
                                mode: e.mode,
                                tag: e.tag,
                            }
                        }).collect();
                        tracing::info!("Loaded {} frequencies from {}", entries.len(), path);
                        entries
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse freq db: {}", e);
                        Vec::new()
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read freq db from {}: {}", path, e);
                Vec::new()
            }
        };

        Self { entries }
    }

    /// Look up a frequency within a tolerance (in MHz).
    pub fn lookup(&self, freq_mhz: f64, tolerance: f64) -> Option<&FreqEntry> {
        self.entries.iter().find(|e| (e.freq - freq_mhz).abs() < tolerance)
    }

    /// Get all entries.
    pub fn entries(&self) -> &[FreqEntry] {
        &self.entries
    }

    /// Get all entries in a specific band.
    pub fn entries_in_band(&self, band: &str) -> Vec<&FreqEntry> {
        self.entries.iter().filter(|e| e.band == band).collect()
    }
}
