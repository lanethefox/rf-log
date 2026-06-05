//! E-SIEM-6: EventManager — background evaluation loop for custom event rules.
//!
//! The EventManager watches the event stream and periodically evaluates
//! custom event rules against the event_log. When conditions are met,
//! it produces derived LogRecords that flow back into the pipeline —
//! queryable, alertable, and chainable.
//!
//! Two evaluation modes:
//! - **Streaming (`Every`):** subscribes to EventBus, evaluates each event inline
//! - **Windowed (all others):** periodic DB queries (every 5s) over time windows

use std::collections::HashSet;
use std::sync::Arc;

use rf_events::{
    CustomEventRule, EventBus, LogRecord,
    custom::{BodyContext, CustomEventCondition},
    event::{now_ns, EventSource},
    pipeline::IngestionPipeline,
    query::{Field, Filter, FilterValue},
};

/// Evaluation interval for windowed conditions (Threshold, Absence, etc.).
const EVAL_INTERVAL_SEC: u64 = 5;

/// Maximum events to fetch per rule evaluation (prevents OOM on broad rules).
const MAX_QUERY_EVENTS: usize = 10_000;

// ── EventManager Task ──────────────────────────────────────────

/// Background task that evaluates custom event rules.
///
/// - Subscribes to EventBus for `Every` (streaming) rules
/// - Polls DB every 5 seconds for windowed rules (Threshold, Absence, etc.)
/// - Produces derived events back through the pipeline
/// - Reloads rules from DB every cycle to pick up user edits
pub async fn event_manager_task(
    bus: EventBus,
    db: rf_db::Db,
    pipeline: Arc<IngestionPipeline>,
) {
    let mut rx = bus.subscribe();
    let mut eval_timer = tokio::time::interval(
        std::time::Duration::from_secs(EVAL_INTERVAL_SEC),
    );
    eval_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Cache rules, reload from DB each eval cycle
    let mut rules: Vec<CustomEventRule> = Vec::new();
    let mut last_rule_load: u64 = 0;

    tracing::info!("EventManager started (eval interval: {}s)", EVAL_INTERVAL_SEC);

    loop {
        tokio::select! {
            // Streaming path: evaluate `Every` rules on each event
            msg = rx.recv() => {
                match msg {
                    Ok(record) => {
                        evaluate_streaming(&mut rules, &record, &pipeline, &db);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("EventManager lagged — skipped {} events", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("EventManager: EventBus closed, shutting down");
                        break;
                    }
                }
            }

            // Periodic path: evaluate windowed rules via DB queries
            _ = eval_timer.tick() => {
                // Reload rules from DB (picks up user edits)
                let now = now_ns();
                if now.saturating_sub(last_rule_load) >= 5_000_000_000 {
                    match db.list_custom_event_rules() {
                        Ok(loaded) => {
                            if loaded.len() != rules.len() {
                                tracing::debug!("EventManager: loaded {} custom event rules", loaded.len());
                            }
                            rules = loaded;
                        }
                        Err(e) => {
                            tracing::error!("EventManager: failed to load rules: {}", e);
                        }
                    }
                    last_rule_load = now;
                }

                // Evaluate windowed rules
                evaluate_windowed(&rules, &db, &pipeline);
            }
        }
    }
}

// ── Streaming Evaluation (Every) ───────────────────────────────

/// Evaluate `Every` rules against a single incoming event.
/// Rules are mutable so we can update `last_fired_ns` in the cache
/// (prevents re-firing before the next DB reload cycle).
fn evaluate_streaming(
    rules: &mut [CustomEventRule],
    event: &LogRecord,
    pipeline: &IngestionPipeline,
    db: &rf_db::Db,
) {
    // Don't evaluate custom events against custom event rules to prevent
    // infinite loops (unless chain_depth allows it)
    let event_chain_depth = event.attributes.get("chain_depth")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as u32;

    for rule in rules {
        if !rule.enabled {
            continue;
        }
        if !matches!(rule.condition, CustomEventCondition::Every) {
            continue;
        }
        // Chain depth check: rule must accept events at this depth
        if event.source == EventSource::Custom && event_chain_depth >= rule.max_chain_depth {
            continue;
        }
        if rule.in_cooldown(now_ns()) {
            continue;
        }
        // Check if event matches the rule's filter
        if !event_matches_filters(event, &rule.filter) {
            continue;
        }

        // Fire: produce derived event
        let context = BodyContext::from_events(&[event.clone()]);
        let source_ids = if event.id > 0 { vec![event.id] } else { vec![] };
        let derived = rule.produce_event(&context, &source_ids);

        pipeline.ingest(derived);

        // Update last_fired_ns in both cache and DB
        let fired_ns = now_ns();
        rule.last_fired_ns = Some(fired_ns);
        if let Err(e) = db.mark_custom_event_fired(rule.id, fired_ns) {
            tracing::warn!("EventManager: failed to mark rule {} fired: {}", rule.id, e);
        }

        tracing::debug!(
            "EventManager: Every rule '{}' fired on event_type={}",
            rule.name, event.event_type,
        );
    }
}

/// Check if a LogRecord matches all filters in a rule.
/// This is an in-memory filter evaluation for streaming mode —
/// avoids DB round-trip for `Every` rules.
pub fn event_matches_filters(event: &LogRecord, filters: &[Filter]) -> bool {
    filters.iter().all(|f| filter_matches(event, f))
}

fn filter_matches(event: &LogRecord, filter: &Filter) -> bool {
    match filter {
        Filter::Eq(field, value) => field_value_str(event, field)
            .is_some_and(|v| v == filter_value_str(value)),
        Filter::Ne(field, value) => field_value_str(event, field)
            .is_none_or(|v| v != filter_value_str(value)),
        Filter::Contains(field, substring) => field_value_str(event, field)
            .is_some_and(|v| v.to_lowercase().contains(&substring.to_lowercase())),
        Filter::NotContains(field, substring) => field_value_str(event, field)
            .is_none_or(|v| !v.to_lowercase().contains(&substring.to_lowercase())),
        Filter::Like(field, pattern) => field_value_str(event, field)
            .is_some_and(|v| sql_like_match(&v, pattern)),
        Filter::In(field, values) => field_value_str(event, field)
            .is_some_and(|v| values.iter().any(|fv| v == filter_value_str(fv))),
        Filter::NotIn(field, values) => field_value_str(event, field)
            .is_none_or(|v| !values.iter().any(|fv| v == filter_value_str(fv))),
        Filter::Exists(field) => field_value_str(event, field).is_some(),
        Filter::NotExists(field) => field_value_str(event, field).is_none(),
        Filter::Gt(field, value) => field_value_f64(event, field)
            .is_some_and(|v| v > filter_value_f64(value)),
        Filter::Gte(field, value) => field_value_f64(event, field)
            .is_some_and(|v| v >= filter_value_f64(value)),
        Filter::Lt(field, value) => field_value_f64(event, field)
            .is_some_and(|v| v < filter_value_f64(value)),
        Filter::Lte(field, value) => field_value_f64(event, field)
            .is_some_and(|v| v <= filter_value_f64(value)),
        Filter::Not(inner) => !filter_matches(event, inner),
        Filter::And(filters) => filters.iter().all(|f| filter_matches(event, f)),
        Filter::Or(filters) => filters.iter().any(|f| filter_matches(event, f)),
        Filter::Regex(_, _) => true, // Skip regex in streaming mode (too expensive)
    }
}

/// Extract a field value from a LogRecord as a string (for comparison).
pub fn field_value_str(event: &LogRecord, field: &Field) -> Option<String> {
    match field {
        Field::EventType => Some(event.event_type.clone()),
        Field::Body => Some(event.body.clone()),
        Field::Source => Some(event.source.as_u8().to_string()),
        Field::Severity => Some(event.severity.as_u8().to_string()),
        Field::FreqMhz => event.freq_mhz.map(|f| format!("{f:.4}")),
        Field::Talkgroup => event.talkgroup.map(|t| t.to_string()),
        Field::SourceUnit => event.source_unit.map(|u| u.to_string()),
        Field::Nac => event.nac.map(|n| n.to_string()),
        Field::Encrypted => event.encrypted.map(|e| if e { "1" } else { "0" }.to_string()),
        Field::Band => event.band.clone(),
        Field::DeviceKey => event.device_key.clone(),
        Field::Classification => event.classification.clone(),
        Field::TraceId => event.trace_id.map(|t| t.to_string()),
        Field::SpanId => event.span_id.map(|s| s.to_string()),
        Field::OperationId => event.operation_id.map(|o| o.to_string()),
        Field::SiteSessionId => event.site_session_id.map(|s| s.to_string()),
        Field::Timestamp => Some(event.timestamp_ns.to_string()),
        Field::ReceiverLat => event.receiver_lat.map(|l| format!("{l:.7}")),
        Field::ReceiverLon => event.receiver_lon.map(|l| format!("{l:.7}")),
        Field::Attribute(key) => event.attributes.get(key).map(|v| v.to_string()),
    }
}

/// Extract a field value as f64 (for numeric comparisons).
fn field_value_f64(event: &LogRecord, field: &Field) -> Option<f64> {
    match field {
        Field::FreqMhz => event.freq_mhz,
        Field::Talkgroup => event.talkgroup.map(|t| t as f64),
        Field::SourceUnit => event.source_unit.map(|u| u as f64),
        Field::Nac => event.nac.map(|n| n as f64),
        Field::Severity => Some(event.severity.as_u8() as f64),
        Field::Timestamp => Some(event.timestamp_ns as f64),
        Field::ReceiverLat => event.receiver_lat,
        Field::ReceiverLon => event.receiver_lon,
        Field::Attribute(key) => event.attributes.get(key).and_then(|v| v.as_f64()),
        _ => field_value_str(event, field).and_then(|s| s.parse::<f64>().ok()),
    }
}

fn filter_value_str(v: &FilterValue) -> String {
    match v {
        FilterValue::String(s) => s.clone(),
        FilterValue::Int(n) => n.to_string(),
        FilterValue::Float(f) => format!("{f:.4}"),
        FilterValue::Bool(b) => if *b { "1" } else { "0" }.to_string(),
    }
}

fn filter_value_f64(v: &FilterValue) -> f64 {
    match v {
        FilterValue::Int(n) => *n as f64,
        FilterValue::Float(f) => *f,
        FilterValue::Bool(b) => if *b { 1.0 } else { 0.0 },
        FilterValue::String(s) => s.parse().unwrap_or(0.0),
    }
}

/// Simple SQL LIKE pattern matching (% and _ wildcards).
fn sql_like_match(value: &str, pattern: &str) -> bool {
    let val = value.to_lowercase();
    let pat = pattern.to_lowercase();
    like_match(val.as_bytes(), pat.as_bytes())
}

fn like_match(val: &[u8], pat: &[u8]) -> bool {
    match (val, pat) {
        (_, []) => val.is_empty(),
        (_, [b'%', rest @ ..]) => {
            // % matches zero or more characters
            (0..=val.len()).any(|i| like_match(&val[i..], rest))
        }
        ([], _) => pat.iter().all(|&c| c == b'%'),
        ([v, vrest @ ..], [b'_', prest @ ..]) => {
            // _ matches exactly one character
            let _ = v;
            like_match(vrest, prest)
        }
        ([v, vrest @ ..], [p, prest @ ..]) => {
            *v == *p && like_match(vrest, prest)
        }
    }
}

// ── Windowed Evaluation ────────────────────────────────────────

/// Evaluate all windowed rules by querying the DB.
fn evaluate_windowed(
    rules: &[CustomEventRule],
    db: &rf_db::Db,
    pipeline: &IngestionPipeline,
) {
    let now = now_ns();

    for rule in rules {
        if !rule.enabled {
            continue;
        }
        if matches!(rule.condition, CustomEventCondition::Every) {
            continue; // handled in streaming path
        }
        if rule.in_cooldown(now) {
            continue;
        }

        let fired = match &rule.condition {
            CustomEventCondition::Threshold { op, value, window_sec } => {
                eval_threshold(rule, db, now, *op, *value, *window_sec)
            }
            CustomEventCondition::Absence { window_sec } => {
                eval_absence(rule, db, now, *window_sec)
            }
            CustomEventCondition::NewValue { field, lookback_sec } => {
                eval_new_value(rule, db, now, field, *lookback_sec)
            }
            CustomEventCondition::Cardinality { field, op, value, window_sec } => {
                eval_cardinality(rule, db, now, field, *op, *value, *window_sec)
            }
            CustomEventCondition::RateChange { percent, window_sec, baseline_sec } => {
                eval_rate_change(rule, db, now, *percent, *window_sec, *baseline_sec)
            }
            CustomEventCondition::Correlation { second_filter, join_field, window_sec } => {
                eval_correlation(rule, db, now, second_filter, join_field, *window_sec)
            }
            CustomEventCondition::Every => unreachable!(),
        };

        if let Some(derived) = fired {
            pipeline.ingest(derived);
            if let Err(e) = db.mark_custom_event_fired(rule.id, now) {
                tracing::warn!("EventManager: failed to mark rule {} fired: {}", rule.id, e);
            }
            tracing::debug!("EventManager: windowed rule '{}' fired", rule.name);
        }
    }
}

/// Threshold: count of matching events exceeds threshold in window.
fn eval_threshold(
    rule: &CustomEventRule,
    db: &rf_db::Db,
    now: u64,
    op: rf_events::alert::CmpOp,
    value: f64,
    window_sec: u64,
) -> Option<LogRecord> {
    let window_start = now.saturating_sub(window_sec * 1_000_000_000);
    let query = rule.to_query(window_start, now);

    let count = db.count_events(&query).ok()? as f64;
    if !op.evaluate(count, value) {
        return None;
    }

    // Fetch events for context (limited)
    let events = db.query_events(&query.clone().limit(100)).ok()?;
    let context = BodyContext::from_events(&events);
    let source_ids: Vec<u64> = events.iter().filter(|e| e.id > 0).map(|e| e.id).collect();

    Some(rule.produce_event(&context, &source_ids))
}

/// Absence: no matching events in window (dead-man switch).
fn eval_absence(
    rule: &CustomEventRule,
    db: &rf_db::Db,
    now: u64,
    window_sec: u64,
) -> Option<LogRecord> {
    let window_start = now.saturating_sub(window_sec * 1_000_000_000);
    let query = rule.to_query(window_start, now);

    let count = db.count_events(&query).ok()?;
    if count > 0 {
        return None; // events exist, no absence
    }

    let context = BodyContext {
        count: 0,
        ..Default::default()
    };
    Some(rule.produce_event(&context, &[]))
}

/// NewValue: a field value appears that hasn't been seen in the lookback window.
fn eval_new_value(
    rule: &CustomEventRule,
    db: &rf_db::Db,
    now: u64,
    field: &Field,
    lookback_sec: u64,
) -> Option<LogRecord> {
    let recent_window = 5_000_000_000u64; // last 5 seconds
    let recent_start = now.saturating_sub(recent_window);
    let lookback_start = now.saturating_sub(lookback_sec * 1_000_000_000);

    // Get recent events (last eval interval)
    let recent_query = rule.to_query(recent_start, now).limit(MAX_QUERY_EVENTS);
    let recent_events = db.query_events(&recent_query).ok()?;
    if recent_events.is_empty() {
        return None;
    }

    // Get distinct values from lookback period (before recent window)
    let field_name = match field {
        Field::Attribute(key) => key.as_str(),
        other => {
            // Use the SQL column name for built-in fields
            return eval_new_value_builtin(rule, db, now, other, lookback_sec, &recent_events);
        }
    };

    let historical = db.event_facets(field_name, Some(lookback_start), Some(recent_start), 10_000).ok()?;
    let known_values: HashSet<String> = historical.into_iter().map(|(v, _)| v).collect();

    // Find values in recent events not in historical set
    let mut new_values: Vec<String> = Vec::new();
    for evt in &recent_events {
        if let Some(val) = field_value_str(evt, field) {
            if !known_values.contains(&val) && !new_values.contains(&val) {
                new_values.push(val);
            }
        }
    }

    if new_values.is_empty() {
        return None;
    }

    let mut context = BodyContext::from_events(&recent_events);
    context.count = new_values.len();
    let source_ids: Vec<u64> = recent_events.iter().filter(|e| e.id > 0).map(|e| e.id).collect();

    Some(rule.produce_event(&context, &source_ids))
}

/// NewValue for built-in Field types (uses event_facets with field SQL name).
fn eval_new_value_builtin(
    rule: &CustomEventRule,
    db: &rf_db::Db,
    now: u64,
    field: &Field,
    lookback_sec: u64,
    recent_events: &[LogRecord],
) -> Option<LogRecord> {
    let recent_window = 5_000_000_000u64;
    let recent_start = now.saturating_sub(recent_window);
    let lookback_start = now.saturating_sub(lookback_sec * 1_000_000_000);

    // Map Field enum back to a facet-compatible field name
    let facet_name = match field {
        Field::EventType => "event_type",
        Field::Band => "band",
        Field::Classification => "classification",
        Field::DeviceKey => "device_key",
        Field::Talkgroup => "talkgroup",
        Field::SourceUnit => "source_unit",
        Field::Nac => "nac",
        _ => return None, // unsupported field for NewValue
    };

    let historical = db.event_facets(facet_name, Some(lookback_start), Some(recent_start), 10_000).ok()?;
    let known_values: HashSet<String> = historical.into_iter().map(|(v, _)| v).collect();

    let mut new_values: Vec<String> = Vec::new();
    for evt in recent_events {
        if let Some(val) = field_value_str(evt, field) {
            if !known_values.contains(&val) && !new_values.contains(&val) {
                new_values.push(val);
            }
        }
    }

    if new_values.is_empty() {
        return None;
    }

    let mut context = BodyContext::from_events(recent_events);
    context.count = new_values.len();
    let source_ids: Vec<u64> = recent_events.iter().filter(|e| e.id > 0).map(|e| e.id).collect();

    Some(rule.produce_event(&context, &source_ids))
}

/// Cardinality: N distinct values of a field in window.
fn eval_cardinality(
    rule: &CustomEventRule,
    db: &rf_db::Db,
    now: u64,
    field: &Field,
    op: rf_events::alert::CmpOp,
    value: u64,
    window_sec: u64,
) -> Option<LogRecord> {
    let window_start = now.saturating_sub(window_sec * 1_000_000_000);
    let query = rule.to_query(window_start, now).limit(MAX_QUERY_EVENTS);

    let events = db.query_events(&query).ok()?;
    if events.is_empty() {
        return None;
    }

    // Count distinct values of the target field
    let mut distinct: HashSet<String> = HashSet::new();
    for evt in &events {
        if let Some(val) = field_value_str(evt, field) {
            distinct.insert(val);
        }
    }

    if !op.evaluate(distinct.len() as f64, value as f64) {
        return None;
    }

    let context = BodyContext::from_events(&events);
    let source_ids: Vec<u64> = events.iter().filter(|e| e.id > 0).map(|e| e.id).collect();

    Some(rule.produce_event(&context, &source_ids))
}

/// RateChange: event rate changed by more than N% vs baseline.
fn eval_rate_change(
    rule: &CustomEventRule,
    db: &rf_db::Db,
    now: u64,
    percent: f64,
    window_sec: u64,
    baseline_sec: u64,
) -> Option<LogRecord> {
    let window_start = now.saturating_sub(window_sec * 1_000_000_000);
    let baseline_start = now.saturating_sub(baseline_sec * 1_000_000_000);

    // Count events in current window
    let current_query = rule.to_query(window_start, now);
    let current_count = db.count_events(&current_query).ok()? as f64;

    // Count events in baseline period (excluding current window)
    let baseline_query = rule.to_query(baseline_start, window_start);
    let baseline_count = db.count_events(&baseline_query).ok()? as f64;

    // Normalize baseline to same duration as window
    let baseline_rate = if baseline_sec > window_sec && baseline_sec > 0 {
        baseline_count * (window_sec as f64 / (baseline_sec - window_sec) as f64)
    } else {
        baseline_count
    };

    // Calculate percent change
    if baseline_rate < 1.0 {
        // Not enough baseline data
        return None;
    }

    let change_pct = ((current_count - baseline_rate) / baseline_rate * 100.0).abs();
    if change_pct < percent {
        return None;
    }

    // Fetch events for context
    let events = db.query_events(&current_query.clone().limit(100)).ok()?;
    let mut context = BodyContext::from_events(&events);
    context.count = current_count as usize;
    let source_ids: Vec<u64> = events.iter().filter(|e| e.id > 0).map(|e| e.id).collect();

    Some(rule.produce_event(&context, &source_ids))
}

/// Correlation: two event streams both match within a window, joined on a field.
fn eval_correlation(
    rule: &CustomEventRule,
    db: &rf_db::Db,
    now: u64,
    second_filter: &[Filter],
    join_field: &Field,
    window_sec: u64,
) -> Option<LogRecord> {
    let window_start = now.saturating_sub(window_sec * 1_000_000_000);

    // Query primary filter
    let primary_query = rule.to_query(window_start, now).limit(MAX_QUERY_EVENTS);
    let primary_events = db.query_events(&primary_query).ok()?;
    if primary_events.is_empty() {
        return None;
    }

    // Collect join values from primary events
    let primary_join_values: HashSet<String> = primary_events.iter()
        .filter_map(|e| field_value_str(e, join_field))
        .collect();

    if primary_join_values.is_empty() {
        return None;
    }

    // Query secondary filter
    let mut secondary_query = rf_events::EventQuery::new()
        .time_range(window_start, now)
        .limit(MAX_QUERY_EVENTS);
    for f in second_filter {
        secondary_query = secondary_query.filter(f.clone());
    }
    let secondary_events = db.query_events(&secondary_query).ok()?;

    // Find matching join values
    let mut matched_events: Vec<LogRecord> = Vec::new();
    for evt in &secondary_events {
        if let Some(val) = field_value_str(evt, join_field) {
            if primary_join_values.contains(&val) {
                matched_events.push(evt.clone());
            }
        }
    }

    if matched_events.is_empty() {
        return None;
    }

    // Combine primary + matched secondary events for context
    let mut all_events = primary_events;
    all_events.extend(matched_events);
    let context = BodyContext::from_events(&all_events);
    let source_ids: Vec<u64> = all_events.iter().filter(|e| e.id > 0).map(|e| e.id).collect();

    Some(rule.produce_event(&context, &source_ids))
}
