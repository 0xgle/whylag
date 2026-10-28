//! Internet speed test using M-Lab's open ndt7 protocol.
//!
//! M-Lab (Measurement Lab) runs free test servers worldwide for exactly this
//! purpose. Results are published by M-Lab as open data, so the UI asks for
//! consent before the first test. While data flows we keep pinging to measure
//! latency under load: the difference to idle latency is the bufferbloat that
//! makes calls stutter when someone else is downloading.

use super::{bufferbloat_grade, SpeedResult, LATENCY_ANCHOR};
use crate::net::icmp::{self, PingResult};
use crate::util::median;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

const LOCATE: &str = "https://locate.measurementlab.net/v2/nearest/ndt/ndt7?client_name=whylag&client_version=0.1";
const PROTOCOL: &str = "net.measurementlab.ndt.v7";
const PHASE: Duration = Duration::from_secs(10);

#[derive(Debug, Deserialize)]
struct LocateResponse {
    results: Vec<LocateResult>,
}

#[derive(Debug, Deserialize)]
struct LocateResult {
    machine: String,
    #[serde(default)]
    location: Option<Location>,
    urls: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Location {
    city: Option<String>,
    country: Option<String>,
}

/// Server-side measurement messages (subset).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Measurement {
    #[serde(rename = "TCPInfo")]
    tcp_info: Option<TcpInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TcpInfo {
    bytes_received: Option<u64>,
    elapsed_time: Option<u64>,
}

fn mbps(bytes: u64, secs: f64) -> f64 {
    if secs <= 0.0 {
        0.0
    } else {
        bytes as f64 * 8.0 / secs / 1e6
    }
}

/// Pings the latency anchor every 250 ms until `stop` is set; returns RTTs.
fn spawn_pinger(stop: Arc<AtomicBool>) -> tokio::task::JoinHandle<Vec<f64>> {
    tokio::spawn(async move {
        let mut out = Vec::new();
        while !stop.load(Ordering::Relaxed) {
            if let PingResult::Reply { rtt_ms, .. } = icmp::ping_async(LATENCY_ANCHOR, Duration::from_millis(900)).await {
                out.push(rtt_ms);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        out
    })
}

async fn connect(url: &str) -> Result<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, String> {
    let mut request = url.into_client_request().map_err(|e| format!("connectFailed: {e}"))?;
    request.headers_mut().insert("Sec-WebSocket-Protocol", HeaderValue::from_static(PROTOCOL));
    let (ws, _) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(request))
        .await
        .map_err(|_| "connectFailed: timeout".to_string())?
        .map_err(|e| format!("connectFailed: {e}"))?;
    Ok(ws)
}

pub async fn run(cancel: &AtomicBool, progress: impl Fn(&str, f64, &SpeedResult)) -> Result<SpeedResult, String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().map_err(|e| e.to_string())?;
    let located: LocateResponse = client
        .get(LOCATE)
        .send()
        .await
        .map_err(|e| format!("locateFailed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("locateFailed: {e}"))?;
    let server = located.results.into_iter().next().ok_or("locateFailed: no servers")?;
    let down_url = server.urls.get("wss:///ndt/v7/download").cloned().ok_or("locateFailed: no download url")?;
    let up_url = server.urls.get("wss:///ndt/v7/upload").cloned().ok_or("locateFailed: no upload url")?;
    let mut result = SpeedResult {
        server: match &server.location {
            Some(Location { city: Some(c), country: Some(k) }) => format!("{c}, {k} ({})", server.machine),
            _ => server.machine.clone(),
        },
        ..Default::default()
    };

    // Idle latency.
    let mut idle = Vec::new();
    for _ in 0..10 {
        if let PingResult::Reply { rtt_ms, .. } = icmp::ping_async(LATENCY_ANCHOR, Duration::from_millis(900)).await {
            idle.push(rtt_ms);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    result.idle_latency_ms = median(&idle);
    progress("download", 0.05, &result);

    // Download.
    let mut ws = connect(&down_url).await?;
    let stop = Arc::new(AtomicBool::new(false));
    let pinger = spawn_pinger(stop.clone());
    let started = Instant::now();
    let mut bytes = 0u64;
    let mut last_mark = Instant::now();
    let mut last_bytes = 0u64;
    while started.elapsed() < PHASE + Duration::from_secs(3) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match tokio::time::timeout(Duration::from_secs(3), ws.next()).await {
            Ok(Some(Ok(msg))) => {
                if matches!(msg, Message::Close(_)) {
                    break;
                }
                bytes += msg.len() as u64;
            }
            _ => break,
        }
        if last_mark.elapsed() >= Duration::from_millis(250) {
            let secs = last_mark.elapsed().as_secs_f64();
            result.down_series.push((started.elapsed().as_secs_f64(), mbps(bytes - last_bytes, secs)));
            result.download_mbps = Some(mbps(bytes, started.elapsed().as_secs_f64()));
            last_mark = Instant::now();
            last_bytes = bytes;
            progress("download", 0.05 + 0.45 * (started.elapsed().as_secs_f64() / PHASE.as_secs_f64()).min(1.0), &result);
        }
    }
    let _ = ws.close(None).await;
    stop.store(true, Ordering::Relaxed);
    let loaded_down = pinger.await.unwrap_or_default();
    result.download_mbps = Some(mbps(bytes, started.elapsed().as_secs_f64()));
    result.loaded_down_latency_ms = median(&loaded_down);
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }

    // Upload.
    let ws = connect(&up_url).await?;
    let (mut sink, mut stream) = ws.split();
    let stop = Arc::new(AtomicBool::new(false));
    let pinger = spawn_pinger(stop.clone());
    let server_bytes = Arc::new(std::sync::Mutex::new(None::<(u64, u64)>));
    let sb = server_bytes.clone();
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            if let Message::Text(t) = msg {
                if let Ok(m) = serde_json::from_str::<Measurement>(t.as_str()) {
                    if let Some(TcpInfo { bytes_received: Some(b), elapsed_time: Some(e) }) = m.tcp_info {
                        *sb.lock().unwrap() = Some((b, e));
                    }
                }
            }
        }
    });
    let started = Instant::now();
    let mut size = 1usize << 13;
    let mut sent = 0u64;
    let mut last_mark = Instant::now();
    let mut last_sent = 0u64;
    while started.elapsed() < PHASE && !cancel.load(Ordering::Relaxed) {
        let payload = vec![0u8; size];
        if sink.send(Message::binary(payload)).await.is_err() {
            break;
        }
        sent += size as u64;
        // Grow messages as throughput rises (ndt7 recommendation), capped at 1 MiB.
        if size < (1 << 20) && sent >= (size as u64) * 16 {
            size *= 2;
        }
        if last_mark.elapsed() >= Duration::from_millis(250) {
            let secs = last_mark.elapsed().as_secs_f64();
            result.up_series.push((started.elapsed().as_secs_f64(), mbps(sent - last_sent, secs)));
            result.upload_mbps = Some(mbps(sent, started.elapsed().as_secs_f64()));
            last_mark = Instant::now();
            last_sent = sent;
            progress("upload", 0.5 + 0.48 * (started.elapsed().as_secs_f64() / PHASE.as_secs_f64()).min(1.0), &result);
        }
    }
    let _ = sink.send(Message::Close(None)).await;
    let _ = tokio::time::timeout(Duration::from_secs(2), reader).await;
    stop.store(true, Ordering::Relaxed);
    let loaded_up = pinger.await.unwrap_or_default();
    // Prefer the server's count: the client also counts bytes still in its buffers.
    let server_side = *server_bytes.lock().unwrap();
    result.upload_mbps = Some(match server_side {
        Some((b, us)) if us > 0 => mbps(b, us as f64 / 1e6),
        _ => mbps(sent, started.elapsed().as_secs_f64()),
    });
    result.loaded_up_latency_ms = median(&loaded_up);
    let worst_loaded = [result.loaded_down_latency_ms, result.loaded_up_latency_ms].into_iter().flatten().reduce(f64::max);
    if let (Some(idle), Some(loaded)) = (result.idle_latency_ms, worst_loaded) {
        result.grade = Some(bufferbloat_grade(idle, loaded).to_string());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_locate_and_measurements() {
        let json = r#"{"results":[{"machine":"mlab1-waw03.mlab-oti.measurement-lab.org","location":{"city":"Warsaw","country":"PL"},
            "urls":{"wss:///ndt/v7/download":"wss://ndt-mlab1-waw03.example/ndt/v7/download?access_token=x",
                    "wss:///ndt/v7/upload":"wss://ndt-mlab1-waw03.example/ndt/v7/upload?access_token=y"}}]}"#;
        let r: LocateResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.results[0].urls.len(), 2);
        let m: Measurement = serde_json::from_str(r#"{"TCPInfo":{"BytesReceived":12500000,"ElapsedTime":1000000},"AppInfo":{}}"#).unwrap();
        let t = m.tcp_info.unwrap();
        assert_eq!(mbps(t.bytes_received.unwrap(), t.elapsed_time.unwrap() as f64 / 1e6), 100.0);
    }
}
