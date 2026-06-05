//! Lifecycle monitor — watches AppConfig state transitions and emits SIEM events.
//!
//! Runs as a 1 Hz async task, detecting changes in:
//! - Operation: active_operation_id (start/stop)
//! - Site: active_site_id + active_site_session_id (enter/exit)
//! - GPS: gps_source (source transitions)
//!
//! This avoids coupling rf-web to rf-events — the monitor observes state
//! changes from the egui side and emits LogRecords through the pipeline.

use std::sync::Arc;

use rf_events::pipeline::IngestionPipeline;
use rf_web::AppState;

use crate::event_ingestion;

/// Previous state snapshot for change detection.
struct PrevState {
    operation_id: Option<i64>,
    operation_name: Option<String>,
    operation_profile: Option<String>,
    site_id: Option<i64>,
    site_session_id: Option<i64>,
    gps_source: String,
}

/// Spawn the lifecycle monitor as a background task.
pub fn spawn(
    rt: &tokio::runtime::Runtime,
    state: AppState,
    pipeline: Arc<IngestionPipeline>,
) {
    rt.spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));

        // Initialize previous state from current config
        let config = state.config();
        let mut prev = PrevState {
            operation_id: config.active_operation_id,
            operation_name: config.active_operation_name.clone(),
            operation_profile: config.active_operation_profile.clone(),
            site_id: config.active_site_id,
            site_session_id: config.active_site_session_id,
            gps_source: config.gps_source.clone(),
        };

        tracing::info!("Lifecycle monitor started");

        loop {
            interval.tick().await;
            if state.is_shutdown() { break; }

            let config = state.config();

            // --- Operation transitions ---
            if config.active_operation_id != prev.operation_id {
                // Was there an old operation? → emit stop
                if let Some(old_id) = prev.operation_id {
                    let name = prev.operation_name.as_deref().unwrap_or("?");
                    let rec = event_ingestion::op_stop_record(old_id, name);
                    pipeline.ingest(rec);
                    tracing::info!("Lifecycle: operation {} stopped", old_id);
                }
                // Is there a new operation? → emit start
                if let Some(new_id) = config.active_operation_id {
                    let name = config.active_operation_name.as_deref().unwrap_or("?");
                    let profile = config.active_operation_profile.as_deref().unwrap_or("test");
                    let rec = event_ingestion::op_start_record(new_id, name, profile);
                    pipeline.ingest(rec);
                    tracing::info!("Lifecycle: operation {} started", new_id);
                }
                prev.operation_id = config.active_operation_id;
                prev.operation_name = config.active_operation_name.clone();
                prev.operation_profile = config.active_operation_profile.clone();
            }

            // --- Site transitions ---
            if config.active_site_id != prev.site_id {
                // Left a site? → emit exit
                if let Some(old_site_id) = prev.site_id {
                    let session_id = prev.site_session_id.unwrap_or(0);
                    let site_name = lookup_site_name(&state, old_site_id);
                    let rec = event_ingestion::site_exit_record(
                        old_site_id, &site_name, session_id,
                    );
                    pipeline.ingest(rec);
                    tracing::info!("Lifecycle: exited site {} ({})", old_site_id, site_name);
                }
                // Entered a site? → emit enter
                if let Some(new_site_id) = config.active_site_id {
                    let session_id = config.active_site_session_id.unwrap_or(0);
                    let site_name = lookup_site_name(&state, new_site_id);
                    let rec = event_ingestion::site_enter_record(
                        new_site_id, &site_name, session_id,
                    );
                    pipeline.ingest(rec);
                    tracing::info!("Lifecycle: entered site {} ({})", new_site_id, site_name);
                }
                prev.site_id = config.active_site_id;
                prev.site_session_id = config.active_site_session_id;
            }

            // --- GPS source transitions ---
            if config.gps_source != prev.gps_source {
                let rec = event_ingestion::gps_source_change_record(
                    &prev.gps_source,
                    &config.gps_source,
                );
                pipeline.ingest(rec);
                tracing::info!(
                    "Lifecycle: GPS source {} → {}",
                    prev.gps_source, config.gps_source,
                );
                prev.gps_source = config.gps_source.clone();
            }
        }

        tracing::info!("Lifecycle monitor stopped");
    });
}

/// Look up site name from DB. Returns "Unknown" if not found.
fn lookup_site_name(state: &AppState, site_id: i64) -> String {
    state
        .db()
        .list_intel_sites(500)
        .ok()
        .and_then(|sites| {
            sites.into_iter().find(|s| s.id == site_id).map(|s| s.name)
        })
        .unwrap_or_else(|| format!("Site #{}", site_id))
}
