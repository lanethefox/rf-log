//! SAME header parser + 3-transmission majority voting.

use crate::codes::{self, Severity};
use crate::fips::{self, FipsLocation};
use serde::{Deserialize, Serialize};

/// A fully decoded SAME alert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SameAlert {
    pub originator: String,
    pub event_code: String,
    pub event_name: String,
    pub severity: Severity,
    pub locations: Vec<FipsLocation>,
    pub duration_mins: u16,
    pub issued_utc: String,
    pub station: String,
    pub raw_header: String,
    pub confidence: f32,
}

/// State machine for SAME header accumulation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseState {
    /// Searching for ZCZC header start.
    Idle,
    /// Accumulating header bytes after ZCZC.
    Accumulating,
    /// Waiting for end-of-message (NNNN).
    WaitingForEnd,
}

/// SAME parser with 3-transmission majority voting.
pub struct SameParser {
    state: ParseState,
    buf: Vec<u8>,
    headers: [Option<Vec<u8>>; 3],
    header_count: usize,
    nnnn_count: usize,
    /// Bytes since last ZCZC or NNNN, for timeout detection
    idle_bytes: usize,
}

impl SameParser {
    pub fn new() -> Self {
        Self {
            state: ParseState::Idle,
            buf: Vec::with_capacity(256),
            headers: [None, None, None],
            header_count: 0,
            nnnn_count: 0,
            idle_bytes: 0,
        }
    }

    pub fn state(&self) -> &ParseState {
        &self.state
    }

    /// Feed a decoded byte to the parser.
    /// Returns Some(SameAlert) when a complete message is decoded.
    pub fn feed_byte(&mut self, byte: u8) -> Option<SameAlert> {
        self.buf.push(byte);
        self.idle_bytes += 1;

        // Timeout: if we've seen 500 bytes without finding anything useful, reset
        if self.idle_bytes > 500 && self.state == ParseState::Idle {
            self.buf.clear();
            self.idle_bytes = 0;
        }

        // Look for ZCZC in buffer tail
        if self.state == ParseState::Idle || self.state == ParseState::WaitingForEnd {
            if self.buf.len() >= 4 {
                let tail = &self.buf[self.buf.len() - 4..];
                if tail == b"ZCZC" {
                    // Start accumulating a new header
                    self.buf.clear();
                    self.buf.extend_from_slice(b"ZCZC");
                    self.state = ParseState::Accumulating;
                    self.idle_bytes = 0;
                    return None;
                }
            }
        }

        // Look for NNNN in buffer tail
        if self.state == ParseState::WaitingForEnd {
            if self.buf.len() >= 4 {
                let tail = &self.buf[self.buf.len() - 4..];
                if tail == b"NNNN" {
                    self.nnnn_count += 1;
                    self.idle_bytes = 0;
                    if self.nnnn_count >= 3 {
                        // End of message — try to produce alert from collected headers
                        let result = self.resolve();
                        self.full_reset();
                        return result;
                    }
                    self.buf.clear();
                }
            }
            // Timeout: 2000 bytes without NNNN after header collection
            if self.idle_bytes > 2000 {
                let result = self.resolve();
                self.full_reset();
                return result;
            }
            return None;
        }

        // Accumulating header bytes
        if self.state == ParseState::Accumulating {
            // Headers end with '-' after the station callsign field
            // Max header length is ~268 bytes (ZCZC + 31 locations × 7 + overhead)
            if self.buf.len() > 300 {
                // Too long, probably garbage — discard and go back to idle
                self.buf.clear();
                self.state = ParseState::Idle;
                return None;
            }

            // A complete header looks like: ZCZC-ORG-EEE-PSSCCC[-PSSCCC...]+TTTT-JJJHHMM-LLLLLLLL-
            // It ends with the station callsign padded to 8 chars + trailing dash
            // We detect completion by finding the pattern after the '+' (duration field)
            let s = String::from_utf8_lossy(&self.buf);
            if s.len() >= 20 && s.ends_with('-') {
                // Format: ZCZC-ORG-EEE-PSSCCC+TTTT-JJJHHMM-LLLLLLLL-
                // After the duration field (containing '+'), need time + station fields.
                let after_zczc = &s[4..]; // skip "ZCZC"
                let fields: Vec<&str> = after_zczc.split('-').collect();
                // Find the field containing '+' (duration marker)
                let dur_idx = fields.iter().position(|f| f.contains('+'));
                // After the duration field, we need at least 2 more non-empty fields
                // (time=JJJHHMM + station=LLLLLLLL), plus the trailing empty from the final '-'
                let complete = if let Some(di) = dur_idx {
                    let after_dur: Vec<&&str> = fields[di + 1..].iter()
                        .filter(|f| !f.is_empty())
                        .collect();
                    // Need time (7 digits) + station (callsign)
                    after_dur.len() >= 2
                        && after_dur[0].len() == 7
                        && after_dur[0].chars().all(|c| c.is_ascii_digit())
                } else {
                    false
                };

                if complete {
                    // Complete header
                    let header = self.buf.clone();
                    self.headers[self.header_count % 3] = Some(header);
                    self.header_count += 1;
                    self.buf.clear();
                    self.idle_bytes = 0;
                    self.state = ParseState::WaitingForEnd;

                    // If we have 3 headers, we can try to resolve early
                    if self.header_count >= 3 {
                        let result = self.resolve();
                        self.full_reset();
                        return result;
                    }
                    return None;
                }
            }
        }

        None
    }

    /// Reset everything for a new message.
    fn full_reset(&mut self) {
        self.state = ParseState::Idle;
        self.buf.clear();
        self.headers = [None, None, None];
        self.header_count = 0;
        self.nnnn_count = 0;
        self.idle_bytes = 0;
    }

    pub fn reset(&mut self) {
        self.full_reset();
    }

    /// Resolve collected headers into an alert using majority voting.
    fn resolve(&self) -> Option<SameAlert> {
        let valid_headers: Vec<&[u8]> = self.headers.iter()
            .filter_map(|h| h.as_deref())
            .collect();

        if valid_headers.is_empty() {
            return None;
        }

        let confidence = valid_headers.len() as f32 / 3.0;

        // Use majority voting: pick the header that appears most often,
        // or fall back to the first one
        let best = majority_vote(&valid_headers);
        let raw = String::from_utf8_lossy(best).to_string();

        parse_header(&raw, confidence)
    }

    /// Returns true if we're in a state where we've found at least one header
    /// (i.e., we detected a SAME transmission in progress).
    pub fn has_partial_decode(&self) -> bool {
        self.header_count > 0
    }
}

/// Majority vote on byte sequences — returns the one that matches the most.
fn majority_vote<'a>(headers: &[&'a [u8]]) -> &'a [u8] {
    if headers.len() <= 1 {
        return headers.first().copied().unwrap_or(&[]);
    }

    let mut best = headers[0];
    let mut best_count = 0;

    for (i, h) in headers.iter().enumerate() {
        let count = headers.iter().filter(|other| *other == h).count();
        if count > best_count || (count == best_count && i == 0) {
            best = h;
            best_count = count;
        }
    }

    best
}

/// Parse a raw SAME header string into a SameAlert.
/// Format: ZCZC-ORG-EEE-PSSCCC[-PSSCCC...]+TTTT-JJJHHMM-LLLLLLLL-
pub fn parse_header(raw: &str, confidence: f32) -> Option<SameAlert> {
    // Strip leading/trailing whitespace and dashes
    let trimmed = raw.trim().trim_end_matches('-');

    // Must start with ZCZC
    if !trimmed.starts_with("ZCZC") {
        return None;
    }

    let after_zczc = &trimmed[4..];
    // Split on '-' to get fields
    let mut parts: Vec<&str> = after_zczc.split('-').collect();
    // Remove empty first element (from leading '-')
    if parts.first() == Some(&"") {
        parts.remove(0);
    }

    // Need at least: ORG, EEE, location+duration, time, station
    if parts.len() < 4 {
        return None;
    }

    let originator = parts[0].to_string();
    let event_code = parts[1].to_string();

    // Everything between EEE and the time field contains locations + duration
    // The '+' separates the last location from the duration (TTTT)
    // Find the field containing '+'
    let mut location_codes = Vec::new();
    let mut duration_mins: u16 = 0;
    let mut time_field_idx = None;

    for (i, part) in parts[2..].iter().enumerate() {
        let actual_idx = i + 2;
        if let Some(plus_pos) = part.find('+') {
            // This field has the last location + duration
            let loc = &part[..plus_pos];
            if loc.len() == 6 {
                location_codes.push(loc.to_string());
            }
            let dur = &part[plus_pos + 1..];
            duration_mins = parse_duration(dur);
            time_field_idx = Some(actual_idx + 1);
            break;
        } else if part.len() == 6 && part.chars().all(|c| c.is_ascii_digit()) {
            location_codes.push(part.to_string());
        }
    }

    let time_idx = time_field_idx.unwrap_or(parts.len().saturating_sub(2));

    let issued_utc = if time_idx < parts.len() {
        parse_time_field(parts[time_idx])
    } else {
        String::new()
    };

    let station = if time_idx + 1 < parts.len() {
        parts[time_idx + 1].trim().to_string()
    } else {
        String::new()
    };

    // Resolve event code
    let (event_name, severity) = codes::lookup_event(&event_code)
        .unwrap_or(("Unknown Event", Severity::Info));

    // Resolve FIPS locations
    let locations: Vec<FipsLocation> = location_codes.iter()
        .map(|c| fips::resolve_fips(c))
        .collect();

    Some(SameAlert {
        originator,
        event_code,
        event_name: event_name.to_string(),
        severity,
        locations,
        duration_mins,
        issued_utc,
        station,
        raw_header: raw.to_string(),
        confidence,
    })
}

/// Parse TTTT duration field (HHMM format) to minutes.
fn parse_duration(tttt: &str) -> u16 {
    if tttt.len() != 4 {
        return 0;
    }
    let hours: u16 = tttt[..2].parse().unwrap_or(0);
    let mins: u16 = tttt[2..].parse().unwrap_or(0);
    hours * 60 + mins
}

/// Parse JJJHHMM time field to a human-readable string.
/// JJJ = Julian day (001-366), HH = hour, MM = minute (UTC).
fn parse_time_field(field: &str) -> String {
    if field.len() != 7 {
        return field.to_string();
    }
    let jday: u16 = field[..3].parse().unwrap_or(0);
    let hour: u8 = field[3..5].parse().unwrap_or(0);
    let min: u8 = field[5..7].parse().unwrap_or(0);
    format!("J{:03} {:02}:{:02}Z", jday, hour, min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_header_basic() {
        let raw = "ZCZC-WXR-WSW-041051-041067+0600-0451430-KIG77---";
        let alert = parse_header(raw, 1.0).unwrap();
        assert_eq!(alert.originator, "WXR");
        assert_eq!(alert.event_code, "WSW");
        assert_eq!(alert.event_name, "Winter Storm Warning");
        assert_eq!(alert.severity, Severity::Severe);
        assert_eq!(alert.locations.len(), 2);
        assert_eq!(alert.locations[0].county, "Multnomah");
        assert_eq!(alert.locations[1].county, "Washington");
        assert_eq!(alert.duration_mins, 360);
        assert_eq!(alert.station, "KIG77");
        assert_eq!(alert.confidence, 1.0);
    }

    #[test]
    fn test_parse_header_single_location() {
        let raw = "ZCZC-WXR-TOR-041005+0100-0451830-KIG77---";
        let alert = parse_header(raw, 0.67).unwrap();
        assert_eq!(alert.event_code, "TOR");
        assert_eq!(alert.severity, Severity::Extreme);
        assert_eq!(alert.locations.len(), 1);
        assert_eq!(alert.locations[0].county, "Clackamas");
        assert_eq!(alert.duration_mins, 60);
        assert_eq!(alert.confidence, 0.67);
    }

    #[test]
    fn test_parse_header_test() {
        let raw = "ZCZC-WXR-RWT-041051-041067-041005+0000-0440200-KIG77---";
        let alert = parse_header(raw, 1.0).unwrap();
        assert_eq!(alert.event_code, "RWT");
        assert_eq!(alert.severity, Severity::Test);
        assert_eq!(alert.locations.len(), 3);
        assert_eq!(alert.duration_mins, 0);
    }

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("0600"), 360);
        assert_eq!(parse_duration("0100"), 60);
        assert_eq!(parse_duration("0030"), 30);
        assert_eq!(parse_duration("0000"), 0);
    }

    #[test]
    fn test_parse_time_field() {
        assert_eq!(parse_time_field("0451430"), "J045 14:30Z");
    }

    #[test]
    fn test_parser_single_header() {
        let mut parser = SameParser::new();
        let header = b"ZCZC-WXR-WSW-041051+0600-0451430-KIG77---";
        for &byte in header {
            let _ = parser.feed_byte(byte);
        }
        assert!(parser.has_partial_decode());
    }

    #[test]
    fn test_parser_three_headers() {
        let mut parser = SameParser::new();
        let header = b"ZCZC-WXR-TOR-041051+0100-0451830-KIG77---";
        let mut result = None;

        // Feed 3 copies of the header
        for _ in 0..3 {
            // Add some preamble bytes between headers
            for _ in 0..16 {
                let _ = parser.feed_byte(0xAB);
            }
            for &byte in header.iter() {
                if let Some(alert) = parser.feed_byte(byte) {
                    result = Some(alert);
                }
            }
        }

        let alert = result.unwrap();
        assert_eq!(alert.event_code, "TOR");
        assert_eq!(alert.confidence, 1.0);
    }

    #[test]
    fn test_parser_reset() {
        let mut parser = SameParser::new();
        let header = b"ZCZC-WXR-WSW-041051+0600-0451430-KIG77---";
        for &byte in header {
            let _ = parser.feed_byte(byte);
        }
        assert!(parser.has_partial_decode());
        parser.reset();
        assert!(!parser.has_partial_decode());
    }
}
