use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};
use axum::extract::ws::{Message, WebSocket};
use serde_json::json;
use tokio::time::{interval, Duration};
use crate::AppState;

pub async fn handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut tick = interval(Duration::from_millis(250)); // 4 Hz
    loop {
        tick.tick().await;

        // Get the latest spectrum frame from the bridge
        let frame = {
            let bridge = state.bridge.lock().unwrap_or_else(|p| p.into_inner());
            bridge.latest_spectrum.clone()
        };

        let msg = match frame {
            Some(f) => json!({
                "type": "spectrum",
                "band": f.band,
                "freqs": f.freqs,
                "powers": f.powers,
                "noise_floor": f.noise_floor,
            }),
            None => json!({ "type": "spectrum_idle" }),
        };

        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
            break;
        }
    }
}
