//! LAN throughput test between two computers running WhyLag.
//!
//! One side enables "accept tests" (a TCP listener); the other connects and
//! measures download (server streams to us) and upload (we stream to it). This
//! separates Wi-Fi/LAN capacity from the internet connection, without iperf.
//!
//! The listener is open to the LAN, so it is guarded: clients must send the
//! six-digit pairing code shown on the serving computer, at most two tests run
//! at once, each connection is time-limited, and the listener closes itself
//! after `AUTO_OFF`.

use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const PORT: u16 = 47321;
const MAGIC: &[u8; 6] = b"WHYLAG";
const PHASE: Duration = Duration::from_secs(5);
/// Longest phase a client may ask for.
const MAX_PHASE: Duration = Duration::from_secs(10);
const CHUNK: usize = 128 * 1024;
/// The listener closes on its own after this long.
pub const AUTO_OFF: Duration = Duration::from_secs(30 * 60);
const MAX_SESSIONS: usize = 2;
const HELLO_LEN: usize = 12;

/// A running listener: abort `handle` to stop it early.
pub struct PeerServer {
    pub handle: tokio::task::JoinHandle<()>,
    pub code: String,
    pub expires_ms: u64,
}

impl PeerServer {
    pub fn is_running(&self) -> bool {
        !self.handle.is_finished()
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PeerServerStatus {
    pub listening: bool,
    pub port: u16,
    pub addresses: Vec<String>,
    pub code: Option<String>,
    pub expires_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PeerResult {
    pub host: String,
    pub download_mbps: f64,
    pub upload_mbps: f64,
    pub connect_ms: f64,
}

async fn serve_one(mut sock: TcpStream, code: u32) -> std::io::Result<()> {
    let mut hello = [0u8; HELLO_LEN];
    tokio::time::timeout(Duration::from_secs(5), sock.read_exact(&mut hello)).await??;
    if &hello[..6] != MAGIC {
        return Ok(());
    }
    if u32::from_be_bytes(hello[8..12].try_into().unwrap()) != code {
        let _ = sock.write_all(b"NO").await;
        return Ok(());
    }
    let phase = Duration::from_secs(hello[7].clamp(1, MAX_PHASE.as_secs() as u8) as u64);
    let buf = vec![0x5au8; CHUNK];
    match hello[6] {
        b'D' => {
            let start = Instant::now();
            while start.elapsed() < phase {
                if sock.write_all(&buf).await.is_err() {
                    break;
                }
            }
            let _ = sock.shutdown().await;
        }
        b'U' => {
            let mut total = 0u64;
            let mut rbuf = vec![0u8; CHUNK];
            let start = Instant::now();
            // Stop reading once the phase (plus slack) is over, even if the
            // client keeps sending.
            let limit = phase + Duration::from_secs(2);
            while start.elapsed() < limit {
                match tokio::time::timeout(
                    limit.saturating_sub(start.elapsed()),
                    sock.read(&mut rbuf),
                )
                .await
                {
                    Ok(Ok(0)) | Err(_) | Ok(Err(_)) => break,
                    Ok(Ok(n)) => total += n as u64,
                }
            }
            // Report what actually arrived and how long it took (µs).
            let micros = start.elapsed().as_micros() as u64;
            let mut reply = total.to_be_bytes().to_vec();
            reply.extend_from_slice(&micros.to_be_bytes());
            let _ = sock.write_all(&reply).await;
        }
        _ => {}
    }
    Ok(())
}

/// Runs the listener until it is aborted or `AUTO_OFF` passes.
pub async fn start_server() -> std::io::Result<PeerServer> {
    let listener = TcpListener::bind(("0.0.0.0", PORT)).await?;
    let code: u32 = rand::random::<u32>() % 1_000_000;
    let slots = Arc::new(tokio::sync::Semaphore::new(MAX_SESSIONS));
    let handle = tokio::spawn(async move {
        let accept_loop = async {
            while let Ok((sock, _)) = listener.accept().await {
                // Busy: drop the connection rather than queue it.
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    continue;
                };
                tokio::spawn(async move {
                    let _ = tokio::time::timeout(
                        MAX_PHASE + Duration::from_secs(10),
                        serve_one(sock, code),
                    )
                    .await;
                    drop(permit);
                });
            }
        };
        let _ = tokio::time::timeout(AUTO_OFF, accept_loop).await;
    });
    Ok(PeerServer {
        handle,
        code: format!("{code:06}"),
        expires_ms: crate::util::now_ms() + AUTO_OFF.as_millis() as u64,
    })
}

/// Normalises a pairing code typed by the user ("123 456" → 123456).
fn parse_code(code: &str) -> Option<u32> {
    let digits: String = code.chars().filter(char::is_ascii_digit).collect();
    (digits.len() == 6).then(|| digits.parse().ok()).flatten()
}

pub fn local_addresses() -> Vec<String> {
    netdev::get_interfaces()
        .into_iter()
        .filter(|i| i.is_up() && !i.is_loopback())
        .flat_map(|i| i.ipv4.into_iter().map(|n| n.addr().to_string()))
        .collect()
}

pub async fn run_test(host: &str, code: &str) -> Result<PeerResult, String> {
    run_test_for(host, code, PHASE).await
}

/// Runs the test with a custom phase length (survey points use short phases).
pub async fn run_test_for(host: &str, code: &str, phase: Duration) -> Result<PeerResult, String> {
    let code = parse_code(code).ok_or("peerCode")?;
    let phase = phase.clamp(Duration::from_secs(1), MAX_PHASE);
    let ip: IpAddr = crate::net::resolve(host).await.ok_or("unresolved")?;
    let addr = SocketAddr::new(ip, PORT);
    let connect = |mode: u8| async move {
        let started = Instant::now();
        let mut s = tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(addr))
            .await
            .map_err(|_| "peerUnreachable".to_string())?
            .map_err(|_| "peerUnreachable".to_string())?;
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut hello = MAGIC.to_vec();
        hello.push(mode);
        hello.push(phase.as_secs() as u8);
        hello.extend_from_slice(&code.to_be_bytes());
        s.write_all(&hello).await.map_err(|e| e.to_string())?;
        Ok::<_, String>((s, ms))
    };

    // Download: count what the peer streams to us.
    let (mut s, connect_ms) = connect(b'D').await?;
    let mut buf = vec![0u8; CHUNK];
    let mut down = 0u64;
    let start = Instant::now();
    loop {
        match tokio::time::timeout(phase + Duration::from_secs(2), s.read(&mut buf)).await {
            Ok(Ok(0)) | Err(_) | Ok(Err(_)) => break,
            Ok(Ok(n)) => down += n as u64,
        }
    }
    let down_secs = start.elapsed().as_secs_f64();
    // A wrong code gets a two-byte refusal instead of a data stream.
    if down <= 2 {
        return Err("peerRefused".into());
    }

    // Upload: stream to the peer, then read its byte count.
    let (mut s, _) = connect(b'U').await?;
    let data = Arc::new(vec![0xa5u8; CHUNK]);
    let start = Instant::now();
    while start.elapsed() < phase {
        if s.write_all(&data).await.is_err() {
            break;
        }
    }
    let _ = s.shutdown().await;
    let mut reply = [0u8; 16];
    let (up, up_secs) =
        match tokio::time::timeout(Duration::from_secs(4), s.read_exact(&mut reply)).await {
            Ok(Ok(_)) => {
                let bytes = u64::from_be_bytes(reply[..8].try_into().unwrap());
                let micros = u64::from_be_bytes(reply[8..].try_into().unwrap()).max(1);
                (bytes, micros as f64 / 1e6)
            }
            _ => (0, start.elapsed().as_secs_f64()),
        };
    Ok(PeerResult {
        host: host.to_string(),
        download_mbps: down as f64 * 8.0 / down_secs.max(0.001) / 1e6,
        upload_mbps: up as f64 * 8.0 / up_secs.max(0.001) / 1e6,
        connect_ms,
    })
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn loopback_peer_test() {
        let server = super::start_server().await.expect("bind");
        assert_eq!(
            super::run_test("127.0.0.1", "12345").await.unwrap_err(),
            "peerCode"
        );
        let wrong = format!(
            "{:06}",
            (server.code.parse::<u32>().unwrap() + 1) % 1_000_000
        );
        assert_eq!(
            super::run_test("127.0.0.1", &wrong).await.unwrap_err(),
            "peerRefused"
        );
        let r = super::run_test("127.0.0.1", &server.code)
            .await
            .expect("test");
        server.handle.abort();
        assert!(r.download_mbps > 50.0, "{r:?}");
        assert!(r.upload_mbps > 50.0, "{r:?}");
    }
}
