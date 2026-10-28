//! Development bridge: exposes `whylag_core::api::dispatch` over HTTP so the UI can
//! run in a normal browser (and in automated screenshot tests) without Tauri.
//! Binds to localhost only.

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
use whylag_core::AppState;

async fn call(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> (StatusCode, Json<Value>) {
    let method = body.get("method").and_then(Value::as_str).unwrap_or_default().to_string();
    let params = body.get("params").cloned().unwrap_or(Value::Null);
    match whylag_core::api::dispatch(&state, &method, params).await {
        Ok(result) => (StatusCode::OK, Json(json!({ "ok": true, "result": result }))),
        Err(err) => (StatusCode::OK, Json(json!({ "ok": false, "error": err }))),
    }
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("WHYLAG_DEV_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8787);
    let data_dir = std::env::var("WHYLAG_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("whylag-dev"));
    let state = AppState::new(data_dir);
    // Only the local Vite dev server may call the bridge.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            origin.to_str().map(|o| o.starts_with("http://localhost:") || o.starts_with("http://127.0.0.1:")).unwrap_or(false)
        }))
        .allow_methods([axum::http::Method::POST])
        .allow_headers([axum::http::header::CONTENT_TYPE]);
    let app = Router::new().route("/api", post(call)).layer(cors).with_state(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.expect("bind dev port");
    println!("whylag dev bridge on http://127.0.0.1:{port}/api");
    axum::serve(listener, app).await.expect("serve");
}
