//! EAS event code lookup table (47 CFR Part 11).

use serde::{Deserialize, Serialize};

/// Severity level for an EAS event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Extreme,
    Severe,
    Moderate,
    Minor,
    Test,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Extreme => write!(f, "Extreme"),
            Self::Severe => write!(f, "Severe"),
            Self::Moderate => write!(f, "Moderate"),
            Self::Minor => write!(f, "Minor"),
            Self::Test => write!(f, "Test"),
            Self::Info => write!(f, "Info"),
        }
    }
}

/// Look up an EAS event code. Returns (event_name, severity).
pub fn lookup_event(code: &str) -> Option<(&'static str, Severity)> {
    Some(match code {
        // Extreme
        "EAN" => ("Emergency Action Notification", Severity::Extreme),
        "EAT" => ("Emergency Action Termination", Severity::Extreme),
        "NIC" => ("National Information Center", Severity::Extreme),
        "TOR" => ("Tornado Warning", Severity::Extreme),
        "EWW" => ("Extreme Wind Warning", Severity::Extreme),
        "TSW" => ("Tsunami Warning", Severity::Extreme),

        // Severe — weather
        "SVR" => ("Severe Thunderstorm Warning", Severity::Severe),
        "FFW" => ("Flash Flood Warning", Severity::Severe),
        "WSW" => ("Winter Storm Warning", Severity::Severe),
        "BZW" => ("Blizzard Warning", Severity::Severe),
        "HWW" => ("High Wind Warning", Severity::Severe),
        "HUW" => ("Hurricane Warning", Severity::Severe),
        "FRW" => ("Fire Warning", Severity::Severe),
        "SSW" => ("Storm Surge Warning", Severity::Severe),
        "SMW" => ("Special Marine Warning", Severity::Severe),
        "DSW" => ("Dust Storm Warning", Severity::Severe),
        "SQW" => ("Snow Squall Warning", Severity::Severe),
        "ISW" => ("Ice Storm Warning", Severity::Severe),

        // Severe — civil
        "CEM" => ("Civil Emergency Message", Severity::Severe),
        "LAE" => ("Local Area Emergency", Severity::Severe),
        "LEW" => ("Law Enforcement Warning", Severity::Severe),
        "CDW" => ("Civil Danger Warning", Severity::Severe),
        "EVI" => ("Evacuation Immediate", Severity::Severe),
        "SPW" => ("Shelter in Place Warning", Severity::Severe),
        "NUW" => ("Nuclear Power Plant Warning", Severity::Severe),
        "RHW" => ("Radiological Hazard Warning", Severity::Severe),
        "VOW" => ("Volcano Warning", Severity::Severe),
        "HMW" => ("Hazardous Materials Warning", Severity::Severe),
        "CAE" => ("Child Abduction Emergency", Severity::Severe),
        "BLU" => ("Blue Alert", Severity::Severe),

        // Moderate
        "FLW" => ("Flood Warning", Severity::Moderate),
        "SVA" => ("Severe Thunderstorm Watch", Severity::Moderate),
        "TOA" => ("Tornado Watch", Severity::Moderate),
        "WNT" | "WSA" => ("Winter Storm Watch", Severity::Moderate),
        "HUA" => ("Hurricane Watch", Severity::Moderate),
        "SSA" => ("Storm Surge Watch", Severity::Moderate),
        "FFA" => ("Flash Flood Watch", Severity::Moderate),
        "FLA" => ("Flood Watch", Severity::Moderate),
        "HWA" => ("High Wind Watch", Severity::Moderate),
        "FZW" => ("Freeze Warning", Severity::Moderate),
        "WCW" => ("Wind Chill Warning", Severity::Moderate),
        "EHW" => ("Excessive Heat Warning", Severity::Moderate),

        // Minor / Advisory
        "HEA" => ("Heat Advisory", Severity::Minor),
        "FOG" | "DFA" => ("Dense Fog Advisory", Severity::Minor),
        "WCA" => ("Wind Chill Advisory", Severity::Minor),
        "FZA" => ("Freeze Watch", Severity::Minor),
        "WIY" => ("Wind Advisory", Severity::Minor),
        "FRY" => ("Frost Advisory", Severity::Minor),
        "HTA" => ("Heat Advisory", Severity::Minor),
        "UPY" => ("Heavy Freezing Spray Warning", Severity::Minor),
        "SPS" => ("Special Weather Statement", Severity::Minor),
        "FLS" => ("Flood Statement", Severity::Minor),
        "FFS" => ("Flash Flood Statement", Severity::Minor),
        "AVW" => ("Avalanche Warning", Severity::Minor),
        "AVA" => ("Avalanche Watch", Severity::Minor),
        "BHW" => ("Biological Hazard Warning", Severity::Minor),
        "CFA" => ("Coastal Flood Watch", Severity::Minor),
        "CFW" => ("Coastal Flood Warning", Severity::Minor),
        "DSA" => ("Dust Advisory", Severity::Minor),
        "EQW" => ("Earthquake Warning", Severity::Minor),
        "TSA" => ("Tsunami Watch", Severity::Minor),

        // Test / Info
        "RWT" => ("Required Weekly Test", Severity::Test),
        "RMT" => ("Required Monthly Test", Severity::Test),
        "NPT" => ("National Periodic Test", Severity::Test),
        "DMO" => ("Practice/Demo Warning", Severity::Test),
        "ADR" => ("Administrative Message", Severity::Info),
        "NMN" => ("Network Message Notification", Severity::Info),

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_known() {
        let (name, sev) = lookup_event("TOR").unwrap();
        assert_eq!(name, "Tornado Warning");
        assert_eq!(sev, Severity::Extreme);
    }

    #[test]
    fn test_lookup_unknown() {
        assert!(lookup_event("ZZZ").is_none());
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(Severity::Extreme.to_string(), "Extreme");
    }
}
