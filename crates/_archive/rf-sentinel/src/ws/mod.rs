pub mod spectrum;
pub mod events;

use axum::{
    Router,
    routing::get,
};
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/ws/spectrum", get(spectrum::handler))
        .route("/ws/events", get(events::handler))
        .with_state(state)
}
