//! E-SIEM-7: AlertEngine — background evaluation loop for alert rules.
//!
//! The AlertEngine watches the event stream and periodically evaluates
//! alert rules against the event_log. When conditions are met, it creates
//! AlertFiring records, logs events, and executes actions:
//! - **Log**: AlertFiring DB record + system.alert.fired pipeline event (always)
//! - **Sound**: Procedural alert tone mixed into cpal audio output
//! - **Highlight**: Pulsing glow on the status ribbon (via AppState)
//! - **Notification**: OS toast notification (rate-limited)
//!
//! Two evaluation modes:
//! - **Streaming (`FirstOccurrence`):** subscribes to EventBus, evaluates each event inline
//! - **Windowed (Threshold, Absence, RateChange):** periodic DB queries (every 5s)

use rf_events::{
    AlertFiring, AlertRule, EventBus, LogRecord,
    alert::{AlertAction, AlertCondition, AlertPriority},
    event::{now_ns, EventSource, Severity, event_types},
    pipeline::IngestionPipeline,
};
use std::sync::{mpsc, Arc};

use crate::event_manager::event_matches_filters;

/// Evaluation interval for windowed conditions.
const EVAL_INTERVAL_SEC: u64 = 5;

/// Default highlight duration in seconds.
const HIGHLIGHT_DURATION_SEC: u64 = 5;

/// Minimum interval between OS notifications (seconds) to prevent toast spam.
const NOTIFICATION_MIN_INTERVAL_SEC: u64 = 3;

// ── Alert Sound Generation ────────────────────────────────────

/// Generate a procedural alert tone based on priority.
/// Returns mono f32 samples at 48kHz.
///
/// - Low: single 440Hz beep, 200ms
/// - Medium: two 660Hz beeps, 150ms each with 100ms gap
/// - High: three 880Hz beeps, 100ms each with 80ms gap
/// - Critical: continuous 1kHz + 500Hz two-tone warble, 600ms
fn generate_alert_tone(priority: AlertPriority) -> Vec<f32> {
    const SAMPLE_RATE: f32 = 48000.0;
    let amplitude = 0.25_f32; // Keep alert tones at reasonable volume

    match priority {
        AlertPriority::Low => {
            // Single 440Hz beep, 200ms
            let samples = (SAMPLE_RATE * 0.2) as usize;
            (0..samples)
                .map(|i| {
                    let t = i as f32 / SAMPLE_RATE;
                    let env = fade_envelope(i, samples, 400);
                    amplitude * env * (2.0 * std::f32::consts::PI * 440.0 * t).sin()
                })
                .collect()
        }
        AlertPriority::Medium => {
            // Two 660Hz beeps
            let beep = (SAMPLE_RATE * 0.15) as usize;
            let gap = (SAMPLE_RATE * 0.1) as usize;
            let mut out = Vec::with_capacity(beep * 2 + gap);
            for _ in 0..2 {
                for i in 0..beep {
                    let t = i as f32 / SAMPLE_RATE;
                    let env = fade_envelope(i, beep, 300);
                    out.push(amplitude * env * (2.0 * std::f32::consts::PI * 660.0 * t).sin());
                }
                if out.len() < beep * 2 + gap {
                    out.extend(std::iter::repeat_n(0.0_f32, gap));
                }
            }
            out
        }
        AlertPriority::High => {
            // Three 880Hz beeps
            let beep = (SAMPLE_RATE * 0.1) as usize;
            let gap = (SAMPLE_RATE * 0.08) as usize;
            let mut out = Vec::with_capacity(beep * 3 + gap * 2);
            for n in 0..3 {
                for i in 0..beep {
                    let t = i as f32 / SAMPLE_RATE;
                    let env = fade_envelope(i, beep, 200);
                    out.push(amplitude * env * (2.0 * std::f32::consts::PI * 880.0 * t).sin());
                }
                if n < 2 {
                    out.extend(std::iter::repeat_n(0.0_f32, gap));
                }
            }
            out
        }
        AlertPriority::Critical => {
            // Two-tone warble: alternating 1kHz and 500Hz, 600ms
            let samples = (SAMPLE_RATE * 0.6) as usize;
            let warble_rate = 8.0; // switches per second
            (0..samples)
                .map(|i| {
                    let t = i as f32 / SAMPLE_RATE;
                    let freq = if ((t * warble_rate) as u32) % 2 == 0 { 1000.0 } else { 500.0 };
                    let env = fade_envelope(i, samples, 600);
                    amplitude * 1.3 * env * (2.0 * std::f32::consts::PI * freq * t).sin()
                })
                .collect()
        }
    }
}

/// Fade-in/fade-out envelope to prevent clicks.
fn fade_envelope(sample: usize, total: usize, fade_samples: usize) -> f32 {
    if sample < fade_samples {
        sample as f32 / fade_samples as f32
    } else if sample > total.saturating_sub(fade_samples) {
        (total - sample) as f32 / fade_samples as f32
    } else {
        1.0
    }
}

// ── AlertEngine Task ──────────────────────────────────────────

/// Background task that evaluates alert rules.
///
/// - Subscribes to EventBus for `FirstOccurrence` (streaming) rules
/// - Polls DB every 5 seconds for windowed rules (Threshold, Absence, RateChange)
/// - Creates AlertFiring records and executes AlertActions
/// - Reloads rules from DB every cycle to pick up user edits
pub async fn alert_engine_task(
    bus: EventBus,
    db: rf_db::Db,
    pipeline: Arc<IngestionPipeline>,
    app_state: rf_web::AppState,
    alert_sound_tx: mpsc::Sender<Vec<f32>>,
) {
    let mut rx = bus.subscribe();
    let mut eval_timer = tokio::time::interval(
        std::time::Duration::from_secs(EVAL_INTERVAL_SEC),
    );
    eval_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Cache rules, reload from DB each eval cycle
    let mut rules: Vec<AlertRule> = Vec::new();
    let mut last_rule_load: u64 = 0;

    // Rate-limit OS notifications
    let mut last_notification_ns: u64 = 0;

    tracing::info!("AlertEngine started (eval interval: {}s)", EVAL_INTERVAL_SEC);

    loop {
        tokio::select! {
            // Streaming path: evaluate `FirstOccurrence` rules on each event
            msg = rx.recv() => {
                match msg {
                    Ok(record) => {
                        evaluate_streaming(
                            &mut rules, &record, &db, &pipeline,
                            &app_state, &alert_sound_tx, &mut last_notification_ns,
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("AlertEngine lagged — skipped {} events", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("AlertEngine: EventBus closed, shutting down");
                        break;
                    }
                }
            }

            // Periodic path: evaluate windowed rules via DB queries
            _ = eval_timer.tick() => {
                // Reload rules from DB (picks up user edits)
                let now = now_ns();
                if now.saturating_sub(last_rule_load) >= 5_000_000_000 {
                    match db.list_alert_rules() {
                        Ok(loaded) => {
                            if loaded.len() != rules.len() {
                                tracing::debug!("AlertEngine: loaded {} alert rules", loaded.len());
                            }
                            rules = loaded;
                        }
                        Err(e) => {
                            tracing::error!("AlertEngine: failed to load rules: {}", e);
                        }
                    }
                    last_rule_load = now;
                }

                // Evaluate windowed rules
                evaluate_windowed(
                    &mut rules, &db, &pipeline,
                    &app_state, &alert_sound_tx, &mut last_notification_ns,
                );
            }
        }
    }
}

// ── Streaming Evaluation (FirstOccurrence) ────────────────────

/// Evaluate `FirstOccurrence` rules against a single incoming event.
fn evaluate_streaming(
    rules: &mut [AlertRule],
    event: &LogRecord,
    db: &rf_db::Db,
    pipeline: &IngestionPipeline,
    app_state: &rf_web::AppState,
    alert_sound_tx: &mpsc::Sender<Vec<f32>>,
    last_notification_ns: &mut u64,
) {
    // Don't evaluate alert firing events — prevents recursive loops
    if event.event_type == event_types::SYSTEM_ALERT_FIRED {
        return;
    }

    let now = now_ns();

    for rule in rules.iter_mut() {
        if !rule.enabled {
            continue;
        }
        if !matches!(rule.condition, AlertCondition::FirstOccurrence) {
            continue;
        }
        if rule.in_cooldown(now) {
            continue;
        }
        if !event_matches_filters(event, &rule.filter) {
            continue;
        }

        let firing = AlertFiring {
            id: 0,
            rule_id: rule.id,
            rule_name: rule.name.clone(),
            fired_ns: now,
            match_count: 1,
            sample_event_id: if event.id > 0 { Some(event.id) } else { None },
            acknowledged: false,
            ack_ns: None,
        };

        fire_alert(rule, &firing, db, pipeline, app_state, alert_sound_tx, last_notification_ns);
        rule.last_fired_ns = Some(now);

        tracing::info!(
            "AlertEngine: FirstOccurrence '{}' [{}] fired on event_type={}",
            rule.name, rule.priority.label(), event.event_type,
        );
    }
}

// ── Windowed Evaluation ───────────────────────────────────────

/// Evaluate all windowed alert rules by querying the DB.
fn evaluate_windowed(
    rules: &mut [AlertRule],
    db: &rf_db::Db,
    pipeline: &IngestionPipeline,
    app_state: &rf_web::AppState,
    alert_sound_tx: &mpsc::Sender<Vec<f32>>,
    last_notification_ns: &mut u64,
) {
    let now = now_ns();

    for rule in rules.iter_mut() {
        if !rule.enabled {
            continue;
        }
        if matches!(rule.condition, AlertCondition::FirstOccurrence) {
            continue;
        }
        if rule.in_cooldown(now) {
            continue;
        }

        let result = match &rule.condition {
            AlertCondition::Threshold { op, value, window_sec } => {
                eval_threshold(rule, db, now, *op, *value, *window_sec)
            }
            AlertCondition::Absence { window_sec } => {
                eval_absence(rule, db, now, *window_sec)
            }
            AlertCondition::RateChange { percent, window_sec, baseline_sec } => {
                eval_rate_change(rule, db, now, *percent, *window_sec, *baseline_sec)
            }
            AlertCondition::FirstOccurrence => unreachable!(),
        };

        if let Some((match_count, sample_event_id)) = result {
            let firing = AlertFiring {
                id: 0,
                rule_id: rule.id,
                rule_name: rule.name.clone(),
                fired_ns: now,
                match_count,
                sample_event_id,
                acknowledged: false,
                ack_ns: None,
            };

            fire_alert(rule, &firing, db, pipeline, app_state, alert_sound_tx, last_notification_ns);
            rule.last_fired_ns = Some(now);

            tracing::info!(
                "AlertEngine: {} '{}' [{}] fired (match_count={})",
                condition_label(&rule.condition), rule.name,
                rule.priority.label(), match_count,
            );
        }
    }
}

// ── Condition Evaluators ──────────────────────────────────────

fn eval_threshold(
    rule: &AlertRule,
    db: &rf_db::Db,
    now: u64,
    op: rf_events::alert::CmpOp,
    value: f64,
    window_sec: u64,
) -> Option<(u64, Option<u64>)> {
    let window_start = now.saturating_sub(window_sec * 1_000_000_000);
    let query = rule.to_query(window_start, now);

    let count = db.count_events(&query).ok()? as f64;
    if !op.evaluate(count, value) {
        return None;
    }

    let sample_id = db.query_events(&query.clone().limit(1)).ok()
        .and_then(|events| events.first().filter(|e| e.id > 0).map(|e| e.id));

    Some((count as u64, sample_id))
}

fn eval_absence(
    rule: &AlertRule,
    db: &rf_db::Db,
    now: u64,
    window_sec: u64,
) -> Option<(u64, Option<u64>)> {
    let window_start = now.saturating_sub(window_sec * 1_000_000_000);
    let query = rule.to_query(window_start, now);

    let count = db.count_events(&query).ok()?;
    if count > 0 {
        return None;
    }

    Some((0, None))
}

fn eval_rate_change(
    rule: &AlertRule,
    db: &rf_db::Db,
    now: u64,
    percent: f64,
    window_sec: u64,
    baseline_sec: u64,
) -> Option<(u64, Option<u64>)> {
    let window_start = now.saturating_sub(window_sec * 1_000_000_000);
    let baseline_start = now.saturating_sub(baseline_sec * 1_000_000_000);

    let current_query = rule.to_query(window_start, now);
    let current_count = db.count_events(&current_query).ok()? as f64;

    let baseline_query = rule.to_query(baseline_start, window_start);
    let baseline_count = db.count_events(&baseline_query).ok()? as f64;

    let baseline_rate = if baseline_sec > window_sec && baseline_sec > 0 {
        baseline_count * (window_sec as f64 / (baseline_sec - window_sec) as f64)
    } else {
        baseline_count
    };

    if baseline_rate < 1.0 {
        return None;
    }

    let change_pct = ((current_count - baseline_rate) / baseline_rate * 100.0).abs();
    if change_pct < percent {
        return None;
    }

    let sample_id = db.query_events(&current_query.clone().limit(1)).ok()
        .and_then(|events| events.first().filter(|e| e.id > 0).map(|e| e.id));

    Some((current_count as u64, sample_id))
}

// ── Fire Alert ────────────────────────────────────────────────

fn fire_alert(
    rule: &AlertRule,
    firing: &AlertFiring,
    db: &rf_db::Db,
    pipeline: &IngestionPipeline,
    app_state: &rf_web::AppState,
    alert_sound_tx: &mpsc::Sender<Vec<f32>>,
    last_notification_ns: &mut u64,
) {
    // 1. Insert firing record
    match db.insert_alert_firing(firing) {
        Ok(id) => {
            tracing::debug!("AlertEngine: firing record id={} for rule '{}'", id, rule.name);
        }
        Err(e) => {
            tracing::error!("AlertEngine: failed to insert firing for '{}': {}", rule.name, e);
        }
    }

    // 2. Mark rule as fired in DB
    if let Err(e) = db.mark_alert_fired(rule.id, firing.fired_ns) {
        tracing::warn!("AlertEngine: failed to mark rule {} fired: {}", rule.id, e);
    }

    // 3. Emit alert event through pipeline
    let severity = match rule.priority {
        AlertPriority::Low => Severity::Notice,
        AlertPriority::Medium => Severity::Warn,
        AlertPriority::High => Severity::Error,
        AlertPriority::Critical => Severity::Fatal,
    };

    let mut rec = LogRecord::new(
        EventSource::System,
        severity,
        event_types::SYSTEM_ALERT_FIRED,
        format!(
            "Alert [{}] '{}': {} (matched {})",
            rule.priority.label(), rule.name,
            rule.condition.describe(), firing.match_count,
        ),
    );
    rec.attributes.insert(
        "alert_rule_id".into(),
        rf_events::AttributeValue::Int(rule.id),
    );
    rec.attributes.insert(
        "alert_priority".into(),
        rf_events::AttributeValue::String(rule.priority.label().to_string()),
    );
    rec.attributes.insert(
        "match_count".into(),
        rf_events::AttributeValue::Int(firing.match_count as i64),
    );

    pipeline.ingest(rec);

    // 4. Execute actions
    for action in &rule.actions {
        execute_action(action, rule, firing, app_state, alert_sound_tx, last_notification_ns);
    }
}

fn execute_action(
    action: &AlertAction,
    rule: &AlertRule,
    _firing: &AlertFiring,
    app_state: &rf_web::AppState,
    alert_sound_tx: &mpsc::Sender<Vec<f32>>,
    last_notification_ns: &mut u64,
) {
    match action {
        AlertAction::Log => {
            // Already handled by fire_alert (AlertFiring insert + pipeline event)
        }
        AlertAction::Sound { file: _ } => {
            // Generate procedural alert tone based on priority
            // (file field reserved for future custom WAV support)
            let tone = generate_alert_tone(rule.priority);
            if alert_sound_tx.send(tone).is_err() {
                tracing::warn!("AlertEngine: alert sound channel closed");
            }
        }
        AlertAction::Highlight { color } => {
            app_state.push_alert_highlight(rf_web::AlertHighlight {
                rule_name: rule.name.clone(),
                color: color.clone(),
                priority: rule.priority.label().to_string(),
                created_ns: now_ns(),
                duration_sec: HIGHLIGHT_DURATION_SEC,
            });
        }
        AlertAction::Notification { title, body } => {
            let now = now_ns();
            let elapsed = now.saturating_sub(*last_notification_ns) / 1_000_000_000;
            if elapsed < NOTIFICATION_MIN_INTERVAL_SEC {
                tracing::debug!(
                    "AlertEngine: notification rate-limited ({elapsed}s < {NOTIFICATION_MIN_INTERVAL_SEC}s)"
                );
                return;
            }
            *last_notification_ns = now;

            let title = title.clone();
            let body = body.clone();
            // Send notification on a blocking thread to avoid async overhead
            std::thread::Builder::new()
                .name("alert-notify".into())
                .spawn(move || {
                    match notify_rust::Notification::new()
                        .appname("RF-LOG")
                        .summary(&title)
                        .body(&body)
                        .timeout(notify_rust::Timeout::Milliseconds(5000))
                        .show()
                    {
                        Ok(_) => {
                            tracing::debug!("AlertEngine: OS notification sent: {}", title);
                        }
                        Err(e) => {
                            tracing::warn!("AlertEngine: OS notification failed: {}", e);
                        }
                    }
                })
                .ok();
        }
    }
}

fn condition_label(cond: &AlertCondition) -> &'static str {
    match cond {
        AlertCondition::Threshold { .. } => "Threshold",
        AlertCondition::Absence { .. } => "Absence",
        AlertCondition::RateChange { .. } => "RateChange",
        AlertCondition::FirstOccurrence => "FirstOccurrence",
    }
}
