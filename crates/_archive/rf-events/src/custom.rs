use serde::{Deserialize, Serialize};

use crate::alert::CmpOp;
use crate::event::{EventSource, LogRecord, Severity};
use crate::query::{EventQuery, Field, Filter};

// ── Custom Event Condition ──────────────────────────────────

/// Trigger condition for a custom event rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CustomEventCondition {
    /// Count of matching events exceeds threshold in time window.
    Threshold {
        op: CmpOp,
        value: f64,
        window_sec: u64,
    },

    /// No matching events in time window (dead-man switch).
    Absence {
        window_sec: u64,
    },

    /// A field value appears that hasn't been seen in the lookback window.
    /// Example: "new encrypted TG not seen in last 24 hours"
    NewValue {
        field: Field,
        lookback_sec: u64,
    },

    /// N distinct values of a field appear within time window.
    /// Example: "same UID seen on 3+ different bands in 10 minutes"
    Cardinality {
        field: Field,
        op: CmpOp,
        value: u64,
        window_sec: u64,
    },

    /// Rate of matching events changed by more than N% compared to baseline.
    RateChange {
        percent: f64,
        window_sec: u64,
        baseline_sec: u64,
    },

    /// Two different event streams both produce matches within a time window,
    /// joined on a common field.
    /// Example: "same UID appears on both P25 voice and federal spectrum detection"
    Correlation {
        /// Second filter (the primary filter is on the rule itself).
        second_filter: Vec<Filter>,
        /// Field to join on (e.g., source_unit).
        join_field: Field,
        /// Both events must occur within this window.
        window_sec: u64,
    },

    /// Every matching event triggers a derived event (1:1 transform).
    /// Useful for enrichment, tagging, or reformatting.
    Every,
}

impl CustomEventCondition {
    /// Human-readable description.
    pub fn describe(&self) -> String {
        match self {
            CustomEventCondition::Threshold { op, value, window_sec } => {
                format!("count {} {} in {}s", op.label(), value, window_sec)
            }
            CustomEventCondition::Absence { window_sec } => {
                format!("no events for {}s", window_sec)
            }
            CustomEventCondition::NewValue { field, lookback_sec } => {
                format!("new {:?} value (lookback {}s)", field, lookback_sec)
            }
            CustomEventCondition::Cardinality { field, op, value, window_sec } => {
                format!("distinct {:?} {} {} in {}s", field, op.label(), value, window_sec)
            }
            CustomEventCondition::RateChange { percent, window_sec, baseline_sec } => {
                format!("rate change > {percent}% ({}s vs {}s)", window_sec, baseline_sec)
            }
            CustomEventCondition::Correlation { join_field, window_sec, .. } => {
                format!("correlation on {:?} within {}s", join_field, window_sec)
            }
            CustomEventCondition::Every => "every matching event".to_string(),
        }
    }

    /// The evaluation time window in seconds (how far back to look).
    pub fn window_sec(&self) -> u64 {
        match self {
            CustomEventCondition::Threshold { window_sec, .. } => *window_sec,
            CustomEventCondition::Absence { window_sec } => *window_sec,
            CustomEventCondition::NewValue { lookback_sec, .. } => *lookback_sec,
            CustomEventCondition::Cardinality { window_sec, .. } => *window_sec,
            CustomEventCondition::RateChange { baseline_sec, .. } => *baseline_sec,
            CustomEventCondition::Correlation { window_sec, .. } => *window_sec,
            CustomEventCondition::Every => 0,
        }
    }
}

// ── Custom Event Rule ───────────────────────────────────────

/// A user-defined rule that watches the event stream and produces derived events.
///
/// Unlike alerts (which notify), custom events CREATE new LogRecords that flow
/// back into the event log — queryable, alertable, and chainable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomEventRule {
    /// Database row ID (0 until persisted).
    pub id: i64,

    /// Human-readable name. Example: "Federal Activity Burst"
    pub name: String,

    /// The event_type of produced events. Must start with "custom.".
    /// Example: "custom.fedl_burst"
    pub event_type: String,

    /// Description of what this rule detects.
    pub description: String,

    /// Whether this rule is active.
    pub enabled: bool,

    /// The source filter — which events to watch.
    pub filter: Vec<Filter>,

    /// The trigger condition.
    pub condition: CustomEventCondition,

    /// Template for the body of produced events.
    /// Supports `{{count}}`, `{{freqs}}`, `{{talkgroups}}`, `{{uids}}` placeholders.
    pub body_template: String,

    /// Severity of produced events.
    pub severity: Severity,

    /// Whether to include triggering event IDs in the produced event's attributes.
    pub include_source_events: bool,

    /// Minimum seconds between firings (prevent spam).
    pub cooldown_sec: u64,

    /// Last time this rule fired (nanoseconds since epoch).
    pub last_fired_ns: Option<u64>,

    /// Chain depth: 0 = watches only raw events, 1+ = watches derived events too.
    /// A rule at depth N can see events from depth 0..N-1.
    pub chain_depth: u32,

    /// Maximum allowed chain depth (prevents infinite loops).
    pub max_chain_depth: u32,
}

impl Default for CustomEventRule {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            event_type: "custom.unnamed".to_string(),
            description: String::new(),
            enabled: true,
            filter: Vec::new(),
            condition: CustomEventCondition::Every,
            body_template: String::new(),
            severity: Severity::Info,
            include_source_events: true,
            cooldown_sec: 60,
            last_fired_ns: None,
            chain_depth: 0,
            max_chain_depth: 3,
        }
    }
}

impl CustomEventRule {
    /// Check if this rule is in its cooldown period.
    pub fn in_cooldown(&self, now_ns: u64) -> bool {
        if let Some(last) = self.last_fired_ns {
            let elapsed_sec = (now_ns.saturating_sub(last)) / 1_000_000_000;
            elapsed_sec < self.cooldown_sec
        } else {
            false
        }
    }

    /// Build an EventQuery for this rule's filter over a time window.
    pub fn to_query(&self, window_start_ns: u64, window_end_ns: u64) -> EventQuery {
        let mut q = EventQuery::new()
            .time_range(window_start_ns, window_end_ns)
            .limit(10_000);
        for f in &self.filter {
            q = q.filter(f.clone());
        }
        q
    }

    /// Render the body template with values from matched events.
    pub fn render_body(&self, context: &BodyContext) -> String {
        let mut body = self.body_template.clone();
        body = body.replace("{{count}}", &context.count.to_string());
        body = body.replace("{{freqs}}", &context.freqs.join(", "));
        body = body.replace("{{talkgroups}}", &context.talkgroups.join(", "));
        body = body.replace("{{uids}}", &context.uids.join(", "));
        body = body.replace("{{bands}}", &context.bands.join(", "));
        body
    }

    /// Produce a derived LogRecord from this rule's evaluation.
    pub fn produce_event(&self, context: &BodyContext, source_event_ids: &[u64]) -> LogRecord {
        let mut record = LogRecord::new(
            EventSource::Custom,
            self.severity,
            &self.event_type,
            self.render_body(context),
        );

        record = record
            .with_attr("rule_id".to_string(), crate::event::AttributeValue::Int(self.id))
            .with_attr("rule_name".to_string(), crate::event::AttributeValue::String(self.name.clone()))
            .with_attr("match_count".to_string(), crate::event::AttributeValue::Int(context.count as i64))
            .with_attr("chain_depth".to_string(), crate::event::AttributeValue::Int(self.chain_depth as i64));

        if self.include_source_events && !source_event_ids.is_empty() {
            record = record.with_attr(
                "source_event_ids".to_string(),
                crate::event::AttributeValue::IntArray(
                    source_event_ids.iter().map(|&id| id as i64).collect(),
                ),
            );
        }

        // Inherit the most common freq/tg/band from context
        if let Some(freq) = context.primary_freq {
            record.freq_mhz = Some(freq);
        }
        if let Some(tg) = context.primary_talkgroup {
            record.talkgroup = Some(tg);
        }
        if let Some(ref band) = context.primary_band {
            record.band = Some(band.clone());
        }

        record
    }
}

// ── Body Template Context ───────────────────────────────────

/// Values available for body template rendering, extracted from matched events.
#[derive(Debug, Clone, Default)]
pub struct BodyContext {
    pub count: usize,
    pub freqs: Vec<String>,
    pub talkgroups: Vec<String>,
    pub uids: Vec<String>,
    pub bands: Vec<String>,
    pub primary_freq: Option<f64>,
    pub primary_talkgroup: Option<u32>,
    pub primary_band: Option<String>,
}

impl BodyContext {
    /// Build a BodyContext from a set of matched LogRecords.
    pub fn from_events(events: &[LogRecord]) -> Self {
        use std::collections::HashMap;

        let mut freq_counts: HashMap<String, usize> = HashMap::new();
        let mut tg_counts: HashMap<u32, usize> = HashMap::new();
        let mut band_counts: HashMap<String, usize> = HashMap::new();
        let mut uid_set: Vec<String> = Vec::new();

        for evt in events {
            if let Some(f) = evt.freq_mhz {
                let key = format!("{:.4}", f);
                *freq_counts.entry(key).or_default() += 1;
            }
            if let Some(tg) = evt.talkgroup {
                *tg_counts.entry(tg).or_default() += 1;
            }
            if let Some(ref b) = evt.band {
                *band_counts.entry(b.clone()).or_default() += 1;
            }
            if let Some(uid) = evt.source_unit {
                let s = uid.to_string();
                if !uid_set.contains(&s) {
                    uid_set.push(s);
                }
            }
        }

        let freqs: Vec<String> = {
            let mut v: Vec<_> = freq_counts.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.into_iter().map(|(k, _)| k).collect()
        };
        let talkgroups: Vec<String> = {
            let mut v: Vec<_> = tg_counts.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1));
            v.iter().map(|(k, _)| k.to_string()).collect()
        };
        let bands: Vec<String> = {
            let mut v: Vec<_> = band_counts.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.into_iter().map(|(k, _)| k).collect()
        };

        let primary_freq = events.iter().find_map(|e| e.freq_mhz);
        let primary_talkgroup = tg_counts.iter().max_by_key(|(_, c)| *c).map(|(k, _)| *k);
        let primary_band = events.iter().find_map(|e| e.band.clone());

        Self {
            count: events.len(),
            freqs,
            talkgroups,
            uids: uid_set,
            bands,
            primary_freq,
            primary_talkgroup,
            primary_band,
        }
    }
}
