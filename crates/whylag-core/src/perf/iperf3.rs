//! iperf3 client: measures throughput against any standard iperf3 server
//! (`iperf3 -s`), so WhyLag works with the servers integrators already run on
//! NAS boxes, routers and switches, not only with a second WhyLag.
//!
//! Protocol (TCP, one stream): the client sends a 37-byte cookie on a control
//! connection, the server drives a state machine with one-byte states, and
//! parameters and results are exchanged as length-prefixed JSON.

use super::peer::PeerResult;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const DEFAULT_PORT: u16 = 5201;
const BLOCK: usize = 128 * 1024;

const TEST_START: i8 = 1;
const TEST_RUNNING: i8 = 2;
const TEST_END: i8 = 4;
const PARAM_EXCHANGE: i8 = 9;
const CREATE_STREAMS: i8 = 10;
const SERVER_TERMINATE: i8 = 11;
const EXCHANGE_RESULTS: i8 = 13;
const DISPLAY_RESULTS: i8 = 14;
const IPERF_DONE: i8 = 16;
const ACCESS_DENIED: i8 = -1;
const SERVER_ERROR: i8 = -2;

fn cookie() -> [u8; 37] {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut c = [0u8; 37];
    for b in c.iter_mut().take(36) {
        *b = CHARS[rand::random::<usize>() % CHARS.len()];
    }
    c
}

async fn read_state(ctrl: &mut TcpStream, timeout: Duration) -> Result<i8, String> {
    let mut b = [0u8; 1];
    tokio::time::timeout(timeout, ctrl.read_exact(&mut b))
        .await
        .map_err(|_| "iperfTimeout")?
        .map_err(|_| "iperfClosed")?;
    Ok(b[0] as i8)
}

async fn write_state(ctrl: &mut TcpStream, s: i8) -> Result<(), String> {
    ctrl.write_all(&[s as u8])
        .await
        .map_err(|_| "iperfClosed".to_string())
}

async fn write_json(ctrl: &mut TcpStream, v: &Value) -> Result<(), String> {
    let body = serde_json::to_vec(v).map_err(|e| e.to_string())?;
    ctrl.write_all(&(body.len() as u32).to_be_bytes())
        .await
        .map_err(|_| "iperfClosed")?;
    ctrl.write_all(&body)
        .await
        .map_err(|_| "iperfClosed".to_string())
}

async fn read_json(ctrl: &mut TcpStream) -> Result<Value, String> {
    let mut len = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(10), ctrl.read_exact(&mut len))
        .await
        .map_err(|_| "iperfTimeout")?
        .map_err(|_| "iperfClosed")?;
    let n = u32::from_be_bytes(len) as usize;
    if n > 1 << 20 {
        return Err("iperfProtocol".into());
    }
    let mut buf = vec![0u8; n];
    tokio::time::timeout(Duration::from_secs(10), ctrl.read_exact(&mut buf))
        .await
        .map_err(|_| "iperfTimeout")?
        .map_err(|_| "iperfClosed")?;
    serde_json::from_slice(&buf).map_err(|_| "iperfProtocol".to_string())
}

/// Waits for `want`, failing on the server's refusal states.
async fn expect(ctrl: &mut TcpStream, want: i8) -> Result<(), String> {
    loop {
        match read_state(ctrl, Duration::from_secs(10)).await? {
            s if s == want => return Ok(()),
            ACCESS_DENIED => return Err("iperfBusy".into()),
            SERVER_ERROR | SERVER_TERMINATE => return Err("iperfServerError".into()),
            // TEST_START is followed by TEST_RUNNING; tolerate either order.
            TEST_START | TEST_RUNNING => continue,
            _ => return Err("iperfProtocol".into()),
        }
    }
}

/// Bytes the server reports for its single stream.
fn server_bytes(results: &Value) -> Option<u64> {
    results.get("streams")?.get(0)?.get("bytes")?.as_u64()
}

/// One direction. `reverse` means the server sends (download). Returns Mb/s.
async fn one_direction(addr: SocketAddr, secs: u64, reverse: bool) -> Result<(f64, f64), String> {
    let started = Instant::now();
    let mut ctrl = tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(addr))
        .await
        .map_err(|_| "peerUnreachable")?
        .map_err(|_| "peerUnreachable")?;
    let connect_ms = started.elapsed().as_secs_f64() * 1000.0;
    let _ = ctrl.set_nodelay(true);
    let ck = cookie();
    ctrl.write_all(&ck).await.map_err(|_| "iperfClosed")?;
    expect(&mut ctrl, PARAM_EXCHANGE).await?;
    let mut params = json!({ "tcp": true, "omit": 0, "time": secs, "num": 0, "blockcount": 0, "parallel": 1, "len": BLOCK, "pacing_timer": 1000, "client_version": "3.16" });
    // iperf3 switches to reverse mode when the key is present at all, so it is
    // only sent for downloads.
    if reverse {
        params["reverse"] = json!(true);
    }
    write_json(&mut ctrl, &params).await?;
    expect(&mut ctrl, CREATE_STREAMS).await?;
    let mut data = tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(addr))
        .await
        .map_err(|_| "peerUnreachable")?
        .map_err(|_| "peerUnreachable")?;
    data.write_all(&ck).await.map_err(|_| "iperfClosed")?;
    expect(&mut ctrl, TEST_RUNNING).await?;

    let phase = Duration::from_secs(secs);
    let t0 = Instant::now();
    let mut moved: u64 = 0;
    if reverse {
        let mut buf = vec![0u8; BLOCK];
        while t0.elapsed() < phase {
            match tokio::time::timeout(
                phase.saturating_sub(t0.elapsed()) + Duration::from_millis(200),
                data.read(&mut buf),
            )
            .await
            {
                Ok(Ok(0)) | Ok(Err(_)) => break,
                Ok(Ok(n)) => moved += n as u64,
                Err(_) => break,
            }
        }
    } else {
        let buf = vec![0x5au8; BLOCK];
        while t0.elapsed() < phase {
            match tokio::time::timeout(Duration::from_secs(3), data.write_all(&buf)).await {
                Ok(Ok(())) => moved += BLOCK as u64,
                _ => break,
            }
        }
    }
    let elapsed = t0.elapsed().as_secs_f64();
    write_state(&mut ctrl, TEST_END).await?;
    // In download mode keep draining so the server's last writes do not stall it.
    let drain = async {
        if reverse {
            let mut buf = vec![0u8; BLOCK];
            while let Ok(Ok(n)) =
                tokio::time::timeout(Duration::from_millis(500), data.read(&mut buf)).await
            {
                if n == 0 {
                    break;
                }
            }
        }
    };
    let (state, _) = tokio::join!(expect(&mut ctrl, EXCHANGE_RESULTS), drain);
    state?;
    write_json(
        &mut ctrl,
        &json!({ "cpu_util_total": 0, "cpu_util_user": 0, "cpu_util_system": 0, "sender_has_retransmits": if reverse { 0 } else { -1 },
                 "streams": [{ "id": 1, "bytes": moved, "retransmits": -1, "jitter": 0, "errors": 0, "packets": 0, "start_time": 0, "end_time": elapsed }] }),
    )
    .await?;
    let theirs = read_json(&mut ctrl).await?;
    if let Ok(DISPLAY_RESULTS) = read_state(&mut ctrl, Duration::from_secs(5)).await {
        let _ = write_state(&mut ctrl, IPERF_DONE).await;
    }
    // Upload: what the server received is the truth (our count includes buffered bytes).
    let bytes = if reverse {
        moved
    } else {
        server_bytes(&theirs).unwrap_or(moved)
    };
    Ok((bytes as f64 * 8.0 / elapsed.max(0.001) / 1e6, connect_ms))
}

/// Download then upload against `host[:port]`.
/// Download only (survey points need one number, quickly).
pub async fn run_download(host: &str, port: Option<u16>, secs: u64) -> Result<PeerResult, String> {
    run(host, port, secs, false).await
}

pub async fn run_test(host: &str, port: Option<u16>, secs: u64) -> Result<PeerResult, String> {
    run(host, port, secs, true).await
}

async fn run(
    host: &str,
    port: Option<u16>,
    secs: u64,
    with_upload: bool,
) -> Result<PeerResult, String> {
    let (name, port) = match (host.rsplit_once(':'), port) {
        (Some((h, p)), None) if p.parse::<u16>().is_ok() && !h.contains(':') => {
            (h.to_string(), p.parse().unwrap())
        }
        (_, p) => (host.to_string(), p.unwrap_or(DEFAULT_PORT)),
    };
    let ip = crate::net::resolve(&name).await.ok_or("unresolved")?;
    let addr = SocketAddr::new(ip, port);
    let secs = secs.clamp(1, 30);
    let (down, connect_ms) = one_direction(addr, secs, true).await?;
    let up = if with_upload {
        // Servers need a moment to accept the next test.
        tokio::time::sleep(Duration::from_millis(300)).await;
        one_direction(addr, secs, false).await?.0
    } else {
        0.0
    };
    Ok(PeerResult {
        host: format!("{name}:{port}"),
        download_mbps: down,
        upload_mbps: up,
        connect_ms,
    })
}

#[cfg(test)]
mod tests {
    /// Runs against a real `iperf3 -s` when one is installed; skipped otherwise.
    #[tokio::test]
    async fn talks_to_a_real_iperf3_server() {
        let port = 15201 + (rand::random::<u16>() % 1000);
        let Ok(mut server) = std::process::Command::new("iperf3")
            .args(["-s", "-1", "-p", &port.to_string()])
            .stdout(std::process::Stdio::null())
            .spawn()
        else {
            eprintln!("iperf3 not installed, skipping");
            return;
        };
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        // `-1` serves one test; restart for the upload half.
        let down =
            super::one_direction(format!("127.0.0.1:{port}").parse().unwrap(), 1, true).await;
        let _ = server.wait();
        let mut server = std::process::Command::new("iperf3")
            .args(["-s", "-1", "-p", &port.to_string()])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let up = super::one_direction(format!("127.0.0.1:{port}").parse().unwrap(), 1, false).await;
        let _ = server.kill();
        let (down, _) = down.expect("download");
        let (up, _) = up.expect("upload");
        assert!(down > 100.0, "download {down}");
        assert!(up > 100.0, "upload {up}");
    }
}
