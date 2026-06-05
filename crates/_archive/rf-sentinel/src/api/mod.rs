pub mod emitters;
pub mod baselines;
pub mod surveys;
pub mod reports;
pub mod drones;
pub mod config;
pub mod anomalies;
pub mod pdw;
pub mod harmonics;

use axum::{routing::{get, post, put, delete}, Router};
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        // Emitters
        .route("/api/emitters", get(emitters::list).post(emitters::create))
        .route("/api/emitters/{id}", get(emitters::get_one).put(emitters::update).delete(emitters::delete))
        // Baselines — capture (specific before wildcard)
        .route("/api/baselines/capture/start",  post(baselines::capture_start))
        .route("/api/baselines/capture/stop",   post(baselines::capture_stop))
        .route("/api/baselines/capture/status", get(baselines::capture_status))
        .route("/api/baselines",        get(baselines::list).post(baselines::create))
        .route("/api/baselines/{id}",    get(baselines::get_one))
        .route("/api/baselines/{id}/bins",     get(baselines::get_bins))
        .route("/api/baselines/{id}/activate", post(baselines::activate))
        // Anomalies
        .route("/api/anomalies", get(anomalies::list))
        .route("/api/anomalies/{id}/acknowledge", post(anomalies::acknowledge))
        // PDW / Pulses
        .route("/api/pdw",     get(pdw::list))
        .route("/api/pdw/pri", get(pdw::pri_stats))
        // Harmonics
        .route("/api/harmonics", get(harmonics::list))
        // Surveys
        .route("/api/surveys",    get(surveys::list).post(surveys::create))
        .route("/api/surveys/{id}", get(surveys::get_one))
        // Reports
        .route("/api/reports",    get(reports::list).post(reports::create))
        .route("/api/reports/{id}", get(reports::get_one))
        // Drones
        .route("/api/drones/detections", get(drones::list_detections))
        .route("/api/drones/tracks",     get(drones::list_tracks))
        .route("/api/drones/remote-id",  get(drones::list_remote_id))
        .route("/api/drones/signatures", get(drones::list_signatures))
        .route("/api/drones/whitelist",  get(drones::list_whitelist).post(drones::add_whitelist))
        // Config
        .route("/api/config", get(config::get_config).post(config::update_config))
        .with_state(state)
}
