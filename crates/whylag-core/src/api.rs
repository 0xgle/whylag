//! The single entry point used by the Tauri shell and the dev HTTP bridge.
//!
//! Every UI action is a `method` with JSON `params`, answered with JSON. Keeping
//! one dispatcher means the desktop app and browser preview can never drift apart.

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

fn params<T: DeserializeOwned>(value: Value) -> ApiResult<T> {
    let value = if value.is_null() { json!({}) } else { value };
    serde_json::from_value(value).map_err(|e| ApiError::bad_params(e.to_string()))
}

fn to_value<T: serde::Serialize>(v: T) -> ApiResult<Value> {
    serde_json::to_value(v).map_err(|e| ApiError::internal(e.to_string()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PositionPx {
    x_px: f64,
    y_px: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WaitMs {
    wait_ms: Option<u64>,
}

#[derive(Deserialize)]
struct Enable {
    enable: bool,
}

#[derive(Deserialize)]
struct Host {
    host: String,
}

#[derive(Deserialize)]
struct Content {
    content: String,
}

#[derive(Deserialize)]
struct WriteText {
    path: String,
    content: String,
}

/// Runs CPU-heavy work off the async executor.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> ApiResult<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| ApiError::internal(e.to_string()))
}

pub async fn dispatch(state: &Arc<AppState>, method: &str, p: Value) -> ApiResult<Value> {
    match method {
        "app.info" => Ok(json!({
            "version": crate::VERSION,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "dataDir": state.data_dir.display().to_string(),
        })),
        "settings.get" => to_value(state.settings()),
        "settings.set" => to_value(state.update_settings(p)?),

        "sim.info" => to_value(state.sim.lock().info()),
        "sim.setPosition" => {
            let pos: PositionPx = params(p)?;
            let mut sim = state.sim.lock();
            sim.set_position_px(pos.x_px, pos.y_px);
            to_value(sim.position)
        }

        "wifi.scan" => to_value(crate::wifi::scan(state).await?),
        "wifi.scanFresh" => {
            let w: WaitMs = params(p)?;
            to_value(crate::wifi::scan_fresh(state, w.wait_ms.unwrap_or(3500)).await?)
        }
        "sim.aps" => to_value(state.sim.lock().virtual_aps()),

        "project.autosave" => {
            let c: Content = params(p)?;
            let path = state.data_dir.join("autosave.whylag");
            std::fs::create_dir_all(&state.data_dir)?;
            // Write then rename so a crash never leaves a half-written project.
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, c.content)?;
            std::fs::rename(&tmp, &path)?;
            Ok(Value::Null)
        }
        "project.loadAutosave" => match std::fs::read_to_string(state.data_dir.join("autosave.whylag")) {
            Ok(text) => Ok(Value::String(text)),
            Err(_) => Ok(Value::Null),
        },

        "survey.heatmap" => {
            let req: crate::survey::HeatmapRequest = params(p)?;
            to_value(blocking(move || crate::survey::heatmap(&req)).await?)
        }
        "survey.channelPlan" => {
            let req: crate::survey::ChannelPlanRequest = params(p)?;
            to_value(blocking(move || crate::survey::channel_plan(&req)).await?)
        }
        "survey.suggestAp" => {
            let req: crate::survey::PlacementRequest = params(p)?;
            to_value(blocking(move || crate::survey::suggest_placement(&req)).await?)
        }
        "survey.materials" => to_value(crate::survey::materials()),

        "net.info" => {
            if state.use_simulator() {
                return to_value(crate::sim::lan::local());
            }
            to_value(blocking(crate::net::local_net).await?)
        }

        "discovery.start" => to_value(crate::discovery::start(state.clone())),
        "discovery.status" => to_value(state.discovery.lock().clone()),
        "discovery.stop" => {
            crate::discovery::stop(state);
            Ok(Value::Null)
        }

        "perf.start" => {
            let req: crate::perf::PerfRequest = params(p)?;
            to_value(crate::perf::start(state.clone(), req))
        }
        "perf.status" => to_value(state.perf.lock().clone()),
        "perf.stop" => {
            crate::perf::stop(state);
            Ok(Value::Null)
        }
        "perf.peerServer" => {
            let r: Enable = params(p)?;
            let mut status = crate::perf::peer::PeerServerStatus { port: crate::perf::peer::PORT, ..Default::default() };
            let running = state.peer_server.lock().is_some();
            if r.enable && !running {
                match crate::perf::peer::start_server().await {
                    Ok(handle) => *state.peer_server.lock() = Some(handle),
                    Err(e) => status.error = Some(e.to_string()),
                }
            } else if !r.enable {
                if let Some(h) = state.peer_server.lock().take() {
                    h.abort();
                }
            }
            status.listening = state.peer_server.lock().is_some();
            status.addresses = crate::perf::peer::local_addresses();
            to_value(status)
        }
        "perf.peerTest" => {
            let r: Host = params(p)?;
            to_value(crate::perf::peer::run_test(&r.host).await.map_err(|e| ApiError::new(e, "peer test failed"))?)
        }

        "path.start" => {
            let req: crate::path::PathRequest = params(p)?;
            to_value(crate::path::start(state.clone(), req))
        }
        "path.status" => to_value(state.path.lock().clone()),
        "path.stop" => {
            crate::path::stop(state);
            Ok(Value::Null)
        }

        "fs.writeText" => {
            // Only reached with a path the user picked in a native save dialog.
            let w: WriteText = params(p)?;
            std::fs::write(&w.path, w.content)?;
            Ok(Value::Null)
        }

        "fs.writeBase64" => {
            let w: WriteText = params(p)?;
            let bytes = crate::util::base64_decode(&w.content).ok_or_else(|| ApiError::bad_params("invalid base64"))?;
            std::fs::write(&w.path, bytes)?;
            Ok(Value::Null)
        }

        _ => Err(ApiError::unknown_method(method)),
    }
}

/// Compile-time guard: the dispatcher future must stay `Send`, because both the
/// Tauri command and the HTTP bridge run it on a multi-threaded runtime.
#[allow(dead_code)]
fn dispatch_future_is_send(state: &Arc<AppState>) {
    fn assert_send<T: Send>(_: T) {}
    assert_send(dispatch(state, "", Value::Null));
}
