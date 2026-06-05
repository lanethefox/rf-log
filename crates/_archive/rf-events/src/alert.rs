use serde::{Deserialize, Serialize};

use crate::query::{EventQuery, Filter};

// ── Comparison Operator ─────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmpOp {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
    Ne,
}

impl CmpOp {
    pub fn evaluate(&self, actual: f64, threshold: f64) -> bool {
        match self {
            CmpOp::Gt => actual > threshold,
            CmpOp::Gte => actual >= threshold,
            CmpOp::Lt => actual < threshold,
            CmpOp::Lte => actual <= threshold,
            CmpOp::Eq => (actual - threshold).abs() < f64::EPSILON,
            CmpOp::Ne => (actual - threshold).abs() >= f64::EPSILON,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            CmpOp::Gt => ">",
            CmpOp::Gte => ">=",
            CmpOp::Lt => "<",
            CmpOp::Lte => "<=",
            CmpOp::Eq => "=",
            CmpOp::Ne => "!=",
        }
    }
}

// ── Alert Condition ─────────────────────────────────────────

/// What must be true for an alert to fire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertCondition {
    /// Fire when count of matching events crosses threshold in time window.
    /// Example: "more than 5 encrypted events in 60 seconds"
    Threshold {
        op: CmpOp,
        value: f64,
        window_sec: u64,
    },

    /// Fire when no matching events arrive within time window.
    /// Example: "no spectrum data for 30 seconds" (dead-man switch)
    Absence {
        window_sec: u64,
    },

    /// Fire on rate change vs historical baseline.
    /// Example: "TG activity up 200% vs last hour"
    RateChange {
        percent: f64,
        window_sec: u64,
        baseline_sec: u64,
    },

    /// Fire on first occurrence of any matching event (ever, or since last reset).
    /// Example: "first time this emitter fingerprint is seen"
    FirstOccurrence,
}

impl AlertCondition {
    /// Human-readable description of this condition.
    pub fn describe(&self) -> String {
        match self {
            AlertCondition::Threshold { op, value, window_sec } => {
                format!("count {} {} in {}s", op.label(), value, window_sec)
            }
            AlertCondition::Absence { window_sec } => {
                format!("no events for {}s", window_sec)
            }
            AlertCondition::RateChange { percent, window_sec, baseline_sec } => {
                format!("rate change > {percent}% ({}s vs {}s baseline)", window_sec, baseline_sec)
            }
            AlertCondition::FirstOccurrence => "first occurrence".to_string(),
        }
    }
}

// ── Alert Action ────────────────────────────────────────────

/// What happens when an alert fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertAction {
    /// Insert a record into alert_firings (always happens).
    Log,
    /// Play an audio alert.
    Sound { file: String },
    /// Visual highlight in the UI (e.g., flash the status ribbon).
    Highlight { color: String },
    /// Send an OS notification.
    Notification { title: String, body: String },
}

// ── Alert Rule ──────────────────────────────────────────────

/// A persistent alert rule that watches the event stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    /// Database row ID (0 until persisted).
    pub id: i64,

    /// Human-readable name.
    pub name: String,

    /// Whether this rule is active.
    pub enabled: bool,

    /// The query filter that selects which events to watch.
    pub filter: Vec<Filter>,

    /// The condition that triggers the alert.
    pub condition: AlertCondition,

    /// Minimum seconds between firings.
    pub cooldown_sec: u64,

    /// Last time this rule fired (nanoseconds since epoch), if ever.
    pub last_fired_ns: Option<u64>,

    /// Actions to take when the alert fires.
    pub actions: Vec<AlertAction>,

    /// Priority level for UI ordering and notification importance.
    pub priority: AlertPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AlertPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl AlertPriority {
    pub fn label(&self) -> &'static str {
        match self {
            AlertPriority::Low => "LOW",
            AlertPriority::Medium => "MEDIUM",
            AlertPriority::High => "HIGH",
            AlertPriority::Critical => "CRITICAL",
        }
    }
}

impl Default for AlertRule {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            enabled: true,
            filter: Vec::new(),
            condition: AlertCondition::FirstOccurrence,
            cooldown_sec: 60,
            last_fired_ns: None,
            actions: vec![AlertAction::Log],
            priority: AlertPriority::Medium,
        }
    }
}

impl AlertRule {
    /// Check if this rule is in its cooldown period.
    pub fn in_cooldown(&self, now_ns: u64) -> bool {
        if let Some(last) = self.last_fired_ns {
            let elapsed_sec = (now_ns.saturating_sub(last)) / 1_000_000_000;
            elapsed_sec < self.cooldown_sec
        } else {
            false
        }
    }

    /// Build an EventQuery that matches this rule's filter over the given time window.
    pub fn to_query(&self, window_start_ns: u64, window_end_ns: u64) -> EventQuery {
        let mut q = EventQuery::new()
            .time_range(window_start_ns, window_end_ns)
            .limit(10_000);
        for f in &self.filter {
            q = q.filter(f.clone());
        }
        q
    }
}

// ── Alert Firing ────────────────────────────────────────────

/// A record of an alert having fired.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertFiring {
    pub id: i64,
    pub rule_id: i64,
    pub rule_name: String,
    pub fired_ns: u64,
    pub match_count: u64,
    pub sample_event_id: Option<u64>,
    pub acknowledged: bool,
    pub ack_ns: Option<u64>,
}
