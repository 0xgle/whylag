//! Development bridge: exposes `whylag_core::api::dispatch` over HTTP so the UI can
//! run in a normal browser (and in automated screenshot tests) without Tauri.
//! Binds to localhost only.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
use whylag_core::AppState;

fn is_local(value: &str) -> bool {
    let host = value.strip_prefix("http://").unwrap_or(value);
    let name = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    matches!(name, "localhost" | "127.0.0.1" | "[::1]")
}

/// Rejects requests whose Host or Origin is not local. CORS alone does not stop
/// a web page from sending a simple request, and a rebound DNS name would pass
/// as same-origin, so both headers are checked on every call.
fn allowed(headers: &HeaderMap) -> bool {
    let host_ok = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .map(is_local)
        .unwrap_or(false);
    let origin_ok = match headers.get("origin") {
        None => true,
        Some(o) => o
            .to_str()
            .map(|o| o.starts_with("http://") && is_local(o))
            .unwrap_or(false),
    };
    host_ok && origin_ok
}

async fn call(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    if !allowed(&headers) {
        return (
            StatusCode::FORBIDDEN,
            Json(
                json!({ "ok": false, "error": { "code": "forbidden", "message": "local origins only" } }),
            ),
        );
    }
    let method = body
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let params = body.get("params").cloned().unwrap_or(Value::Null);
    match whylag_core::api::dispatch(&state, &method, params).await {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({ "ok": true, "result": result })),
        ),
        Err(err) => (StatusCode::OK, Json(json!({ "ok": false, "error": err }))),
    }
}

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("WHYLAG_DEV_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);
    let data_dir = std::env::var("WHYLAG_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("whylag-dev"));
    let state = AppState::new(data_dir);
    // Only the local Vite dev server may call the bridge.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            origin
                .to_str()
                .map(|o| o.starts_with("http://localhost:") || o.starts_with("http://127.0.0.1:"))
                .unwrap_or(false)
        }))
        .allow_methods([axum::http::Method::POST])
        .allow_headers([axum::http::header::CONTENT_TYPE]);
    let app = Router::new()
        .route("/api", post(call))
        .layer(cors)
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind dev port");
    println!("whylag dev bridge on http://127.0.0.1:{port}/api");
    axum::serve(listener, app).await.expect("serve");
}
