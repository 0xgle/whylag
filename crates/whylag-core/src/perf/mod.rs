//! Performance tests: latency/jitter/loss to the gateway and popular services,
//! DNS resolver benchmark with hijacking detection, internet speed with
//! bufferbloat (M-Lab ndt7) and a LAN throughput test between two WhyLags.

pub mod ndt7;
pub mod peer;

use crate::net::{self, dns, TcpOutcome};
use crate::state::AppState;
use crate::util::{jitter, median, now_ms, percentile};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProbeKind {
    Icmp,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LatencyStats {
    pub id: String,
    pub label: String,
    pub host: String,
    pub kind: Option<ProbeKind>,
    /// "lan", "isp", "dns" or "internet" — where the target sits.
    pub group: String,
    pub sent: u32,
    pub received: u32,
    pub loss_pct: f64,
    pub min: Option<f64>,
    pub avg: Option<f64>,
    pub median: Option<f64>,
    pub p95: Option<f64>,
    pub max: Option<f64>,
    pub jitter: Option<f64>,
    pub samples: Vec<Option<f64>>,
}

impl LatencyStats {
    pub fn from_samples(id: &str, label: &str, host: &str, group: &str, kind: Option<ProbeKind>, samples: Vec<Option<f64>>) -> Self {
        let ok: Vec<f64> = samples.iter().flatten().copied().collect();
        let sent = samples.len() as u32;
        let received = ok.len() as u32;
        LatencyStats {
            id: id.into(),
            label: label.into(),
            host: host.into(),
            kind,
            group: group.into(),
            sent,
            received,
            loss_pct: if sent == 0 { 0.0 } else { 100.0 * (sent - received) as f64 / sent as f64 },
            min: ok.iter().copied().reduce(f64::min),
            avg: (!ok.is_empty()).then(|| ok.iter().sum::<f64>() / ok.len() as f64),
            median: median(&ok),
            p95: percentile(&ok, 95.0),
            max: ok.iter().copied().reduce(f64::max),
            jitter: jitter(&ok),
            samples,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DnsResult {
    pub server: String,
    pub label: String,
    /// The resolver this computer actually uses.
    pub system: bool,
    pub median_ms: Option<f64>,
    pub succeeded: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpeedResult {
    pub server: String,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub idle_latency_ms: Option<f64>,
    pub loaded_down_latency_ms: Option<f64>,
    pub loaded_up_latency_ms: Option<f64>,
    pub grade: Option<String>,
    /// (seconds since phase start, Mb/s)
    pub down_series: Vec<(f64, f64)>,
    pub up_series: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PerfRun {
    pub running: bool,
    pub phase: String,
    pub progress: f64,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub simulated: bool,
    pub latency: Vec<LatencyStats>,
    pub dns: Vec<DnsResult>,
    /// The system resolver answered a name that does not exist (ISP ad redirect).
    pub dns_hijack: Option<bool>,
    pub speed: Option<SpeedResult>,
    pub speed_error: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfRequest {
    #[serde(default = "yes")]
    pub latency: bool,
    #[serde(default = "yes")]
    pub dns: bool,
    #[serde(default)]
    pub speed: bool,
}

fn yes() -> bool {
    true
}

#[derive(Default)]
pub struct PerfHandle {
    pub cancel: AtomicBool,
}

/// Bufferbloat grade from the extra latency under load (Waveform-style scale).
pub fn bufferbloat_grade(idle: f64, loaded: f64) -> &'static str {
    let extra = (loaded - idle).max(0.0);
    match extra {
        x if x < 5.0 => "A+",
        x if x < 30.0 => "A",
        x if x < 60.0 => "B",
        x if x < 200.0 => "C",
        x if x < 400.0 => "D",
        _ => "F",
    }
}

struct Target {
    id: &'static str,
    label: &'static str,
    host: &'static str,
    group: &'static str,
    /// TCP port for services that may block ICMP.
    tcp_port: Option<u16>,
}

const INTERNET_TARGETS: &[Target] = &[
    Target { id: "cloudflare", label: "Cloudflare DNS", host: "1.1.1.1", group: "internet", tcp_port: None },
    Target { id: "google-dns", label: "Google DNS", host: "8.8.8.8", group: "internet", tcp_port: None },
    Target { id: "google", label: "Google", host: "www.google.com", group: "internet", tcp_port: Some(443) },
    Target { id: "youtube", label: "YouTube", host: "www.youtube.com", group: "internet", tcp_port: Some(443) },
    Target { id: "teams", label: "Microsoft Teams", host: "teams.microsoft.com", group: "internet", tcp_port: Some(443) },
    Target { id: "zoom", label: "Zoom", host: "zoom.us", group: "internet", tcp_port: Some(443) },
    Target { id: "netflix", label: "Netflix", host: "www.netflix.com", group: "internet", tcp_port: Some(443) },
    Target { id: "steam", label: "Steam", host: "store.steampowered.com", group: "internet", tcp_port: Some(443) },
];

const PUBLIC_RESOLVERS: &[(&str, &str)] = &[
    ("1.1.1.1", "Cloudflare"),
    ("8.8.8.8", "Google"),
    ("9.9.9.9", "Quad9"),
    ("208.67.222.222", "OpenDNS"),
];

const DNS_NAMES: &[&str] = &["google.com", "wikipedia.org", "microsoft.com", "netflix.com", "github.com", "amazon.com", "allegro.pl", "onet.pl"];

/// Runs `count` probes at `interval`. Services with a TCP port are timed by
/// TCP handshake; everything else by ICMP echo, switching to a TCP handshake
/// on port 443 if the first few echoes all go unanswered (ICMP filtered).
pub async fn probe_series(ip: IpAddr, tcp_port: Option<u16>, count: usize, interval: Duration) -> (Option<ProbeKind>, Vec<Option<f64>>) {
    let timeout = Duration::from_millis(1000);
    let mut kind = match (ip, tcp_port) {
        (_, Some(_)) | (IpAddr::V6(_), None) => ProbeKind::Tcp,
        (IpAddr::V4(_), None) => ProbeKind::Icmp,
    };
    let port = tcp_port.unwrap_or(443);
    let mut samples: Vec<Option<f64>> = Vec::with_capacity(count);
    for i in 0..count {
        if i > 0 {
            tokio::time::sleep(interval).await;
        }
        let rtt = match (kind, ip) {
            (ProbeKind::Icmp, IpAddr::V4(v4)) => net::icmp::ping_async(v4, timeout).await.rtt(),
            _ => {
                let (o, rtt) = net::tcp_probe(SocketAddr::new(ip, port), timeout).await;
                rtt.filter(|_| matches!(o, TcpOutcome::Open | TcpOutcome::Closed))
            }
        };
        samples.push(rtt);
        if kind == ProbeKind::Icmp && i == 2 && samples.iter().all(Option::is_none) {
            kind = ProbeKind::Tcp;
            samples.clear();
        }
    }
    (Some(kind), samples)
}

pub fn start(state: Arc<AppState>, req: PerfRequest) -> PerfRun {
    {
        let mut run = state.perf.lock();
        if run.running {
            return run.clone();
        }
        *run = PerfRun { running: true, phase: "starting".into(), started_ms: now_ms(), ..Default::default() };
    }
    state.perf_handle.cancel.store(false, Ordering::SeqCst);
    let simulate = state.use_simulator();
    let st = state.clone();
    tokio::spawn(async move {
        let result = if simulate { crate::sim::net::perf(st.clone(), req).await } else { run(st.clone(), req).await };
        let mut run = st.perf.lock();
        if let Err(e) = result {
            run.error = Some(e);
        }
        run.running = false;
        run.progress = 1.0;
        run.phase = "done".into();
        run.finished_ms = Some(now_ms());
    });
    state.perf.lock().clone()
}

pub fn stop(state: &AppState) {
    state.perf_handle.cancel.store(true, Ordering::SeqCst);
}

fn set(state: &AppState, f: impl FnOnce(&mut PerfRun)) {
    f(&mut state.perf.lock());
}

async fn run(state: Arc<AppState>, req: PerfRequest) -> Result<(), String> {
    let local = tokio::task::spawn_blocking(net::local_net).await.ok().flatten();
    if req.latency {
        set(&state, |r| r.phase = "latency".into());
        let mut jobs: Vec<(String, String, String, String, Option<u16>)> = Vec::new();
        if let Some(gw) = local.as_ref().and_then(|l| l.gateway) {
            jobs.push(("gateway".into(), "gateway".into(), gw.to_string(), "lan".into(), None));
        }
        for t in INTERNET_TARGETS {
            jobs.push((t.id.into(), t.label.into(), t.host.into(), t.group.into(), t.tcp_port));
        }
        let futures = jobs.into_iter().map(|(id, label, host, group, port)| async move {
            let Some(ip) = net::resolve(&host).await else {
                return LatencyStats { id, label, host, group, sent: 20, received: 0, loss_pct: 100.0, ..Default::default() };
            };
            let (kind, samples) = probe_series(ip, port, 20, Duration::from_millis(150)).await;
            LatencyStats::from_samples(&id, &label, &host, &group, kind, samples)
        });
        let results = join_all(futures).await;
        set(&state, |r| {
            r.latency = results;
            r.progress = 0.3;
        });
    }
    if state.perf_handle.cancel.load(Ordering::SeqCst) {
        return Ok(());
    }

    if req.dns {
        set(&state, |r| r.phase = "dns".into());
        let system: Vec<IpAddr> = local.as_ref().map(|l| l.dns_servers.clone()).unwrap_or_default();
        let mut servers: Vec<(IpAddr, String, bool)> = system.iter().filter(|ip| !ip.is_loopback()).map(|ip| (*ip, ip.to_string(), true)).collect();
        for (ip, name) in PUBLIC_RESOLVERS {
            let ip: IpAddr = ip.parse().unwrap();
            if !servers.iter().any(|(s, _, _)| *s == ip) {
                servers.push((ip, name.to_string(), false));
            }
        }
        let results = join_all(servers.iter().map(|(ip, label, sys)| async move {
            let mut times = Vec::new();
            for name in DNS_NAMES {
                if let Some(a) = dns::query(*ip, name, Duration::from_millis(1500)).await {
                    if a.rcode == 0 {
                        times.push(a.rtt_ms);
                    }
                }
            }
            DnsResult { server: ip.to_string(), label: label.clone(), system: *sys, median_ms: median(&times), succeeded: times.len() as u32, total: DNS_NAMES.len() as u32 }
        }))
        .await;
        // Hijack check: a random name must come back NXDOMAIN.
        let probe = format!("whylag-{:x}.invalid-domain-check.com", rand::random::<u64>());
        let mut hijack = None;
        if let Some(sys) = system.iter().find(|ip| !ip.is_loopback()) {
            if let Some(a) = dns::query(*sys, &probe, Duration::from_millis(2000)).await {
                hijack = Some(a.rcode == 0 && !a.addresses.is_empty());
            }
        }
        set(&state, |r| {
            r.dns = results;
            r.dns_hijack = hijack;
            r.progress = 0.45;
        });
    }
    if state.perf_handle.cancel.load(Ordering::SeqCst) {
        return Ok(());
    }

    if req.speed {
        if !state.settings().mlab_consent {
            set(&state, |r| r.speed_error = Some("consentRequired".into()));
            return Ok(());
        }
        set(&state, |r| r.phase = "speed".into());
        let st = state.clone();
        let progress = move |phase: &str, p: f64, partial: &SpeedResult| {
            set(&st, |r| {
                r.phase = phase.into();
                r.progress = 0.45 + 0.55 * p;
                r.speed = Some(partial.clone());
            });
        };
        match ndt7::run(&state.perf_handle.cancel, progress).await {
            Ok(result) => set(&state, |r| r.speed = Some(result)),
            Err(e) => set(&state, |r| r.speed_error = Some(e)),
        }
    }
    Ok(())
}

/// Default target for latency checks during the speed test.
pub const LATENCY_ANCHOR: Ipv4Addr = Ipv4Addr::new(1, 1, 1, 1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grades() {
        assert_eq!(bufferbloat_grade(12.0, 14.0), "A+");
        assert_eq!(bufferbloat_grade(12.0, 30.0), "A");
        assert_eq!(bufferbloat_grade(12.0, 110.0), "C");
        assert_eq!(bufferbloat_grade(12.0, 900.0), "F");
    }

    #[test]
    fn latency_stats() {
        let s = LatencyStats::from_samples("x", "X", "h", "internet", Some(ProbeKind::Icmp), vec![Some(10.0), None, Some(14.0), Some(12.0)]);
        assert_eq!(s.sent, 4);
        assert_eq!(s.received, 3);
        assert!((s.loss_pct - 25.0).abs() < 1e-9);
        assert_eq!(s.min, Some(10.0));
        assert_eq!(s.median, Some(12.0));
        assert_eq!(s.jitter, Some(3.0));
    }
}
