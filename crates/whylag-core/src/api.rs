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
struct LiveReq {
    target: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoTestReq {
    profile: Option<crate::autotest::Profile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tz {
    tz_offset_min: Option<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanTrigger {
    /// Ask the adapter for a fresh scan (Wi-Fi and Survey screens only).
    trigger: Option<bool>,
    /// Minimum gap between scan requests; the survey walk asks more often.
    min_gap_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WaitMs {
    wait_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Interval {
    interval_s: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Range {
    from_ms: u64,
    to_ms: u64,
    points: Option<usize>,
}

#[derive(Deserialize)]
struct Enable {
    /// `None` only reports the current status.
    enable: Option<bool>,
}

#[derive(Deserialize)]
struct Host {
    host: String,
    /// WhyLag pairing code (peer tests).
    code: Option<String>,
    /// "peer" (default) or "iperf3".
    kind: Option<String>,
    port: Option<u16>,
}

#[derive(Deserialize)]
struct Content {
    content: String,
}

/// Runs CPU-heavy work off the async executor.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> ApiResult<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))
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
        "settings.set" => {
            let before = state.settings().data_source;
            let updated = state.update_settings(p)?;
            // Monitoring writes to a demo or a live database: switching the
            // data source mid-run would mix them, so it stops.
            if updated.data_source != before {
                crate::monitor::stop(state);
                *state.sim_fallback.write() = false;
            }
            to_value(updated)
        }

        "sim.info" => to_value(state.sim.lock().info()),
        "sim.setPosition" => {
            let pos: PositionPx = params(p)?;
            let mut sim = state.sim.lock();
            sim.set_position_px(pos.x_px, pos.y_px);
            to_value(sim.position)
        }

        "wifi.scan" => {
            let r: ScanTrigger = params(p)?;
            let t = if r.trigger.unwrap_or(false) {
                crate::wifi::Trigger::Throttled(r.min_gap_ms.unwrap_or(crate::wifi::TRIGGER_GAP_MS))
            } else {
                crate::wifi::Trigger::CacheOnly
            };
            to_value(crate::wifi::scan(state, t).await?)
        }
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
        "project.loadAutosave" => {
            match std::fs::read_to_string(state.data_dir.join("autosave.whylag")) {
                Ok(text) => Ok(Value::String(text)),
                Err(_) => Ok(Value::Null),
            }
        }

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
        "survey.locateAps" => {
            let req: crate::survey::LocateRequest = params(p)?;
            to_value(blocking(move || crate::survey::locate_aps(&req)).await?)
        }

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
            let mut status = crate::perf::peer::PeerServerStatus {
                port: crate::perf::peer::PORT,
                ..Default::default()
            };
            // Forget a listener that switched itself off.
            let running = {
                let mut guard = state.peer_server.lock();
                if guard.as_ref().is_some_and(|s| !s.is_running()) {
                    *guard = None;
                }
                guard.is_some()
            };
            match r.enable {
                Some(true) if !running => match crate::perf::peer::start_server().await {
                    Ok(server) => *state.peer_server.lock() = Some(server),
                    Err(e) => status.error = Some(e.to_string()),
                },
                Some(false) => {
                    if let Some(s) = state.peer_server.lock().take() {
                        s.handle.abort();
                    }
                }
                _ => {}
            }
            if let Some(s) = state.peer_server.lock().as_ref() {
                status.listening = true;
                status.code = Some(s.code.clone());
                status.expires_ms = Some(s.expires_ms);
            }
            status.addresses = crate::perf::peer::local_addresses();
            to_value(status)
        }
        "perf.peerTest" => {
            let r: Host = params(p)?;
            let res = if r.kind.as_deref() == Some("iperf3") {
                crate::perf::iperf3::run_test(&r.host, r.port, 5).await
            } else {
                crate::perf::peer::run_test(&r.host, r.code.as_deref().unwrap_or("")).await
            };
            to_value(res.map_err(|e| ApiError::new(e, "peer test failed"))?)
        }
        // Short throughput probe for survey points: peer test or simulated.
        "survey.throughput" => {
            let r: Host = params(p)?;
            if state.use_simulator() || r.host.is_empty() && state.sim_active() {
                let mbps = state.sim.lock().throughput_here();
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                return to_value(crate::perf::peer::PeerResult {
                    host: "simulator".into(),
                    download_mbps: mbps,
                    upload_mbps: mbps * 0.8,
                    connect_ms: 2.0,
                });
            }
            let res = if r.kind.as_deref() == Some("iperf3") {
                crate::perf::iperf3::run_download(&r.host, r.port, 2).await
            } else {
                crate::perf::peer::run_test_for(
                    &r.host,
                    r.code.as_deref().unwrap_or(""),
                    std::time::Duration::from_secs(2),
                )
                .await
            };
            to_value(res.map_err(|e| ApiError::new(e, "peer test failed"))?)
        }

        "live.sample" => {
            let r: LiveReq = params(p)?;
            to_value(crate::live::sample(state, r.target).await)
        }

        "roam.start" => to_value(crate::roaming::start(state.clone())),
        "roam.status" => to_value(state.roam.lock().clone()),
        "roam.stop" => {
            crate::roaming::stop(state);
            Ok(Value::Null)
        }

        "autotest.start" => {
            // Explicit profile from the UI, else the selected saved one.
            let req: AutoTestReq = params(p)?;
            let settings = state.settings();
            let profile = req
                .profile
                .or_else(|| {
                    settings
                        .autotest_profiles
                        .iter()
                        .find(|x| x.name == settings.autotest_profile)
                        .cloned()
                })
                .unwrap_or_default();
            to_value(crate::autotest::start(state.clone(), profile))
        }
        "autotest.status" => to_value(state.autotest.lock().clone()),
        "autotest.stop" => {
            crate::autotest::stop(state);
            Ok(Value::Null)
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

        "security.scan" => {
            let scan = crate::wifi::scan_for_analysis(state).await;
            let discovery = state.discovery.lock().clone();
            let perf = state.perf.lock().clone();
            let report = crate::security::report(scan.as_ref(), Some(&discovery), Some(&perf));
            *state.security.lock() = Some(report.clone());
            to_value(report)
        }
        "security.last" => to_value(state.security.lock().clone()),

        "monitor.start" => {
            let r: Interval = params(p)?;
            to_value(
                crate::monitor::start(state.clone(), r.interval_s.unwrap_or(5))
                    .map_err(|e| ApiError::new("monitor", e))?,
            )
        }
        "monitor.stop" => {
            crate::monitor::stop(state);
            to_value(crate::monitor::status(state))
        }
        "monitor.status" => {
            let st = state.clone();
            to_value(blocking(move || crate::monitor::status(&st)).await?)
        }
        "monitor.series" => {
            let r: Range = params(p)?;
            let st = state.clone();
            to_value(
                blocking(move || {
                    crate::monitor::series(&st, r.from_ms, r.to_ms, r.points.unwrap_or(600))
                })
                .await?,
            )
        }
        "monitor.clear" => {
            crate::monitor::clear(state);
            Ok(Value::Null)
        }

        "advisor.diagnose" => {
            let tz: Tz = params(p)?;
            // The demo history is recorded in Warsaw time.
            let tz_offset_min = if state.use_simulator() {
                60
            } else {
                tz.tz_offset_min.unwrap_or(0)
            };
            let scan = crate::wifi::scan_for_analysis(state).await;
            let st = state.clone();
            let monitor = blocking(move || crate::monitor::status(&st)).await?;
            let perf = state.perf.lock().clone();
            let path = state.path.lock().clone();
            let discovery = state.discovery.lock().clone();
            let security = state.security.lock().clone();
            let settings = state.settings();
            let d = crate::advisor::diagnose(&crate::advisor::Inputs {
                scan: scan.as_ref(),
                perf: Some(&perf),
                path: Some(&path),
                discovery: Some(&discovery),
                security: security.as_ref(),
                monitor: Some(&monitor),
                settings: &settings,
                tz_offset_min,
            });
            to_value(d)
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
