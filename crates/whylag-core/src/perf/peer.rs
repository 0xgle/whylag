//! LAN throughput test between two computers running WhyLag.
//!
//! One side enables "accept tests" (a TCP listener); the other connects and
//! measures download (server streams to us) and upload (we stream to it). This
//! separates Wi-Fi/LAN capacity from the internet connection, without iperf.

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

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PeerServerStatus {
    pub listening: bool,
    pub port: u16,
    pub addresses: Vec<String>,
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

async fn serve_one(mut sock: TcpStream) -> std::io::Result<()> {
    let mut hello = [0u8; 8];
    tokio::time::timeout(Duration::from_secs(5), sock.read_exact(&mut hello)).await??;
    if &hello[..6] != MAGIC {
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
            loop {
                match tokio::time::timeout(phase + Duration::from_secs(2), sock.read(&mut rbuf)).await {
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

/// Runs the listener until the returned handle is aborted.
pub async fn start_server() -> std::io::Result<tokio::task::JoinHandle<()>> {
    let listener = TcpListener::bind(("0.0.0.0", PORT)).await?;
    Ok(tokio::spawn(async move {
        while let Ok((sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let _ = serve_one(sock).await;
            });
        }
    }))
}

pub fn local_addresses() -> Vec<String> {
    netdev::get_interfaces()
        .into_iter()
        .filter(|i| i.is_up() && !i.is_loopback())
        .flat_map(|i| i.ipv4.into_iter().map(|n| n.addr().to_string()))
        .collect()
}

pub async fn run_test(host: &str) -> Result<PeerResult, String> {
    run_test_for(host, PHASE).await
}

/// Runs the test with a custom phase length (survey points use short phases).
pub async fn run_test_for(host: &str, phase: Duration) -> Result<PeerResult, String> {
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
    let (up, up_secs) = match tokio::time::timeout(Duration::from_secs(4), s.read_exact(&mut reply)).await {
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
        let r = super::run_test("127.0.0.1").await.expect("test");
        server.abort();
        assert!(r.download_mbps > 50.0, "{r:?}");
        assert!(r.upload_mbps > 50.0, "{r:?}");
    }
}
