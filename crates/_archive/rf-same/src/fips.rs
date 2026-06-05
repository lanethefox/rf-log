//! FIPS county code resolution for Portland metro area.

use serde::{Deserialize, Serialize};

/// A resolved FIPS location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FipsLocation {
    pub code: String,
    pub county: String,
    pub state: String,
}

/// Resolve a 6-digit FIPS code (PSSCCC) to county + state.
/// P = part (0 = entire county), SS = state FIPS, CCC = county FIPS.
pub fn resolve_fips(code: &str) -> FipsLocation {
    if code.len() != 6 {
        return FipsLocation {
            code: code.to_string(),
            county: "Unknown".into(),
            state: "??".into(),
        };
    }

    // SS = digits 1-2, CCC = digits 3-5 (P = digit 0 is county subdivision)
    let state_county = &code[1..]; // SSCCC
    let (county, state) = lookup_county(state_county);

    FipsLocation {
        code: code.to_string(),
        county: county.to_string(),
        state: state.to_string(),
    }
}

/// Look up a state+county FIPS (5 digits: SSCCC) to (county_name, state_abbrev).
fn lookup_county(ssccc: &str) -> (&'static str, &'static str) {
    match ssccc {
        // Oregon (41)
        "41001" => ("Baker", "OR"),
        "41003" => ("Benton", "OR"),
        "41005" => ("Clackamas", "OR"),
        "41007" => ("Clatsop", "OR"),
        "41009" => ("Columbia", "OR"),
        "41011" => ("Coos", "OR"),
        "41013" => ("Crook", "OR"),
        "41015" => ("Curry", "OR"),
        "41017" => ("Deschutes", "OR"),
        "41019" => ("Douglas", "OR"),
        "41021" => ("Gilliam", "OR"),
        "41023" => ("Grant", "OR"),
        "41025" => ("Harney", "OR"),
        "41027" => ("Hood River", "OR"),
        "41029" => ("Jackson", "OR"),
        "41031" => ("Jefferson", "OR"),
        "41033" => ("Josephine", "OR"),
        "41035" => ("Klamath", "OR"),
        "41037" => ("Lake", "OR"),
        "41039" => ("Lane", "OR"),
        "41041" => ("Lincoln", "OR"),
        "41043" => ("Linn", "OR"),
        "41045" => ("Malheur", "OR"),
        "41047" => ("Marion", "OR"),
        "41049" => ("Morrow", "OR"),
        "41051" => ("Multnomah", "OR"),
        "41053" => ("Polk", "OR"),
        "41055" => ("Sherman", "OR"),
        "41057" => ("Tillamook", "OR"),
        "41059" => ("Umatilla", "OR"),
        "41061" => ("Union", "OR"),
        "41063" => ("Wallowa", "OR"),
        "41065" => ("Wasco", "OR"),
        "41067" => ("Washington", "OR"),
        "41069" => ("Wheeler", "OR"),
        "41071" => ("Yamhill", "OR"),

        // Washington (53) — Portland metro
        "53011" => ("Clark", "WA"),
        "53015" => ("Cowlitz", "WA"),
        "53039" => ("Klickitat", "WA"),
        "53049" => ("Pacific", "WA"),
        "53059" => ("Skamania", "WA"),
        "53069" => ("Wahkiakum", "WA"),

        // Fallback
        _ => {
            // Try to at least identify the state
            if ssccc.len() >= 2 {
                match &ssccc[..2] {
                    "41" => ("Unknown County", "OR"),
                    "53" => ("Unknown County", "WA"),
                    "06" => ("Unknown County", "CA"),
                    "16" => ("Unknown County", "ID"),
                    _ => ("Unknown", "??"),
                }
            } else {
                ("Unknown", "??")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_multnomah() {
        let loc = resolve_fips("041051");
        assert_eq!(loc.county, "Multnomah");
        assert_eq!(loc.state, "OR");
        assert_eq!(loc.code, "041051");
    }

    #[test]
    fn test_resolve_clark_wa() {
        let loc = resolve_fips("053011");
        assert_eq!(loc.county, "Clark");
        assert_eq!(loc.state, "WA");
    }

    #[test]
    fn test_resolve_with_part() {
        // Part digit 1 = partial county
        let loc = resolve_fips("141051");
        assert_eq!(loc.county, "Multnomah");
        assert_eq!(loc.state, "OR");
    }

    #[test]
    fn test_unknown_fips() {
        let loc = resolve_fips("099999");
        assert_eq!(loc.county, "Unknown");
        assert_eq!(loc.state, "??");
    }

    #[test]
    fn test_short_code() {
        let loc = resolve_fips("041");
        assert_eq!(loc.county, "Unknown");
        assert_eq!(loc.state, "??");
    }
}
