//! Path analysis: every router between this computer and a destination, with
//! MTR-style loss and latency per hop, split into your network, your ISP and
//! the wider internet — plus a diagnosis of where trouble actually starts.

use crate::net::{self, icmp::PingResult};
use crate::state::AppState;
use crate::util::{jitter, now_ms};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Segment {
    Lan,
    Isp,
    Internet,
    Destination,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hop {
    pub ttl: u8,
    pub ip: Option<Ipv4Addr>,
    pub name: Option<String>,
    pub segment: Segment,
    pub sent: u32,
    pub received: u32,
    pub loss_pct: f64,
    pub best: Option<f64>,
    pub avg: Option<f64>,
    pub worst: Option<f64>,
    pub last: Option<f64>,
    pub jitter: Option<f64>,
    #[serde(skip)]
    pub samples: Vec<Option<f64>>,
}

impl Hop {
    pub fn new(ttl: u8, ip: Option<Ipv4Addr>) -> Self {
        Hop {
            ttl,
            ip,
            name: None,
            segment: Segment::Unknown,
            sent: 0,
            received: 0,
            loss_pct: 0.0,
            best: None,
            avg: None,
            worst: None,
            last: None,
            jitter: None,
            samples: vec![],
        }
    }

    pub fn recompute(&mut self) {
        let ok: Vec<f64> = self.samples.iter().flatten().copied().collect();
        self.sent = self.samples.len() as u32;
        self.received = ok.len() as u32;
        self.loss_pct = if self.sent == 0 {
            0.0
        } else {
            100.0 * (self.sent - self.received) as f64 / self.sent as f64
        };
        self.best = ok.iter().copied().reduce(f64::min);
        self.worst = ok.iter().copied().reduce(f64::max);
        self.avg = (!ok.is_empty()).then(|| ok.iter().sum::<f64>() / ok.len() as f64);
        self.last = self.samples.last().copied().flatten();
        self.jitter = jitter(&ok);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathFinding {
    /// Stable code translated by the UI.
    pub code: String,
    pub ttl: Option<u8>,
    pub segment: Option<Segment>,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PathRun {
    pub running: bool,
    pub phase: String,
    pub progress: f64,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub simulated: bool,
    pub target: String,
    pub target_ip: Option<Ipv4Addr>,
    pub hops: Vec<Hop>,
    pub findings: Vec<PathFinding>,
    pub rounds: u32,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct PathHandle {
    pub cancel: AtomicBool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathRequest {
    pub target: String,
    #[serde(default = "default_rounds")]
    pub rounds: u32,
}

fn default_rounds() -> u32 {
    20
}

/// Parses `traceroute -n` / `tracepath -n` / Windows `tracert -d` output into
/// (ttl, address) pairs. Lines without an address are timeouts.
pub fn parse_trace(text: &str) -> Vec<(u8, Option<Ipv4Addr>)> {
    let mut out: Vec<(u8, Option<Ipv4Addr>)> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let ttl_str: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(ttl) = ttl_str.parse::<u8>() else {
            continue;
        };
        if ttl == 0 || trimmed.contains("[LOCALHOST]") {
            continue;
        }
        let ip = trimmed[ttl_str.len()..]
            .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '[' || c == ']')
            .find_map(|t| t.trim_end_matches(':').parse::<Ipv4Addr>().ok());
        match out.iter_mut().find(|(t, _)| *t == ttl) {
            Some(existing) if existing.1.is_none() => existing.1 = ip,
            Some(_) => {}
            None => out.push((ttl, ip)),
        }
    }
    out
}

/// Finds the hops. Windows sends TTL-limited echoes in parallel (fast);
/// other systems use the traceroute tool.
async fn trace(dest: Ipv4Addr, max: u8) -> Vec<(u8, Option<Ipv4Addr>)> {
    if net::icmp::native_ttl() {
        // Stagger the probes ~50 ms apart: a burst of 30 at once trips ICMP
        // rate limits on routers and shows up as fake loss.
        let probes = (1..=max).map(|ttl| async move {
            tokio::time::sleep(Duration::from_millis(50 * (ttl as u64 - 1))).await;
            tokio::task::spawn_blocking(move || {
                (
                    ttl,
                    net::icmp::ping(dest, Duration::from_millis(1500), Some(ttl)),
                )
            })
            .await
        });
        let mut hops: Vec<(u8, Option<Ipv4Addr>)> = Vec::new();
        for r in join_all(probes).await.into_iter().flatten() {
            let (ttl, res) = r;
            match res {
                PingResult::TtlExceeded { from, .. } => hops.push((ttl, Some(from))),
                PingResult::Reply { from, .. } => {
                    hops.push((ttl, Some(from)));
                }
                PingResult::Unreachable { from } => hops.push((ttl, Some(from))),
                PingResult::Timeout => hops.push((ttl, None)),
            }
        }
        hops.sort_by_key(|h| h.0);
        // Cut after the first time the destination itself answers.
        if let Some(pos) = hops.iter().position(|(_, ip)| *ip == Some(dest)) {
            hops.truncate(pos + 1);
        } else {
            // Trim trailing timeouts.
            while hops.last().map(|h| h.1.is_none()).unwrap_or(false) && hops.len() > 1 {
                hops.pop();
            }
        }
        return hops;
    }
    let ip = dest.to_string();
    let max_s = max.to_string();
    let attempts: Vec<(&str, Vec<&str>)> = vec![
        (
            "traceroute",
            vec!["-n", "-q", "1", "-w", "1", "-m", &max_s, &ip],
        ),
        ("tracepath", vec!["-n", "-m", &max_s, &ip]),
    ];
    for (tool, args) in attempts {
        let tool = tool.to_string();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let out = tokio::task::spawn_blocking(move || {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            crate::util::run_tool_lenient(&tool, &refs, Duration::from_secs(60))
        })
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
        let hops = parse_trace(&out);
        if !hops.is_empty() {
            return hops;
        }
    }
    vec![]
}

/// Registrable-ish domain of a reverse DNS name ("war-r1.core.orange.pl" -> "orange.pl").
fn domain_of(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.trim_end_matches('.').split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let tld2 = ["co.uk", "com.pl", "net.pl", "org.pl", "com.au", "co.jp"];
    let last2 = parts[parts.len() - 2..].join(".");
    if tld2.contains(&last2.as_str()) && parts.len() >= 3 {
        return Some(parts[parts.len() - 3..].join("."));
    }
    Some(last2)
}

/// Labels hops as LAN, ISP or internet.
pub fn assign_segments(hops: &mut [Hop], dest: Ipv4Addr) {
    let mut isp_domain: Option<String> = None;
    let mut left_isp = false;
    let mut seen_public = false;
    for hop in hops.iter_mut() {
        let Some(ip) = hop.ip else { continue };
        if ip == dest {
            hop.segment = Segment::Destination;
            continue;
        }
        if net::is_cgnat(ip) {
            hop.segment = Segment::Isp;
            continue;
        }
        if net::is_private(IpAddr::V4(ip)) {
            // Private addresses after the ISP edge are still inside the ISP.
            hop.segment = if seen_public {
                Segment::Isp
            } else {
                Segment::Lan
            };
            continue;
        }
        let domain = hop.name.as_deref().and_then(domain_of);
        if !seen_public {
            seen_public = true;
            isp_domain = domain;
            hop.segment = Segment::Isp;
            continue;
        }
        let same = match (&isp_domain, &domain) {
            (Some(a), Some(b)) => a == b,
            // Without reverse DNS we cannot tell where the ISP ends.
            _ => false,
        };
        if !left_isp && same {
            hop.segment = Segment::Isp;
        } else {
            left_isp = true;
            hop.segment = Segment::Internet;
        }
    }
}

/// Turns hop statistics into findings. The key expert rule: loss at one hop
/// that does not continue to later hops is the router de-prioritising replies
/// to probes (ICMP rate limiting), not real packet loss.
pub fn diagnose(hops: &[Hop]) -> Vec<PathFinding> {
    let mut f = Vec::new();
    let answered: Vec<&Hop> = hops
        .iter()
        .filter(|h| h.ip.is_some() && h.sent > 0)
        .collect();
    let Some(last) = answered.last() else {
        f.push(PathFinding {
            code: "noRoute".into(),
            ttl: None,
            segment: None,
            value: None,
        });
        return f;
    };
    let dest_loss = last.loss_pct;
    if hops
        .iter()
        .any(|h| h.ip.map(net::is_cgnat).unwrap_or(false))
    {
        f.push(PathFinding {
            code: "cgnat".into(),
            ttl: None,
            segment: Some(Segment::Isp),
            value: None,
        });
    }
    // Where does loss that persists to the destination begin?
    if dest_loss >= 2.0 {
        let start = answered
            .iter()
            .position(|h| {
                answered
                    .iter()
                    .skip_while(|x| x.ttl < h.ttl)
                    .all(|x| x.loss_pct >= dest_loss * 0.5)
            })
            .unwrap_or(answered.len() - 1);
        let hop = answered[start];
        f.push(PathFinding {
            code: "lossFrom".into(),
            ttl: Some(hop.ttl),
            segment: Some(hop.segment),
            value: Some(dest_loss),
        });
    }
    for (i, h) in answered.iter().enumerate() {
        if i + 1 < answered.len() && h.loss_pct >= 10.0 && dest_loss < h.loss_pct * 0.3 {
            f.push(PathFinding {
                code: "rateLimited".into(),
                ttl: Some(h.ttl),
                segment: Some(h.segment),
                value: Some(h.loss_pct),
            });
        }
    }
    // Latency jumps that persist (not just one slow-to-answer router).
    for w in answered.windows(2) {
        let (a, b) = (w[0], w[1]);
        if let (Some(la), Some(lb)) = (a.avg, b.avg) {
            let persists = answered
                .iter()
                .filter(|x| x.ttl > b.ttl)
                .all(|x| x.avg.map(|v| v >= lb * 0.8).unwrap_or(true));
            if lb - la > 30.0 && persists {
                f.push(PathFinding {
                    code: "latencyJump".into(),
                    ttl: Some(b.ttl),
                    segment: Some(b.segment),
                    value: Some(lb - la),
                });
            }
        }
    }
    if let Some(first) = answered.first().filter(|h| h.segment == Segment::Lan) {
        if first.avg.unwrap_or(0.0) > 10.0 || first.jitter.unwrap_or(0.0) > 5.0 {
            f.push(PathFinding {
                code: "slowGateway".into(),
                ttl: Some(first.ttl),
                segment: Some(Segment::Lan),
                value: first.avg,
            });
        }
    }
    if f.iter()
        .all(|x| x.code == "cgnat" || x.code == "rateLimited")
    {
        f.push(PathFinding {
            code: "healthy".into(),
            ttl: None,
            segment: None,
            value: last.avg,
        });
    }
    f
}

pub fn start(state: Arc<AppState>, req: PathRequest) -> PathRun {
    {
        let mut run = state.path.lock();
        if run.running {
            return run.clone();
        }
        *run = PathRun {
            running: true,
            phase: "resolve".into(),
            started_ms: now_ms(),
            target: req.target.clone(),
            ..Default::default()
        };
    }
    state.path_handle.cancel.store(false, Ordering::SeqCst);
    let simulate = state.use_simulator();
    let st = state.clone();
    tokio::spawn(async move {
        let result = if simulate {
            crate::util::catch_job(crate::sim::net::path(st.clone(), req)).await
        } else {
            crate::util::catch_job(run(st.clone(), req)).await
        };
        let mut run = st.path.lock();
        if let Err(e) = result {
            run.error = Some(e);
        }
        run.running = false;
        run.progress = 1.0;
        run.phase = "done".into();
        run.finished_ms = Some(now_ms());
    });
    state.path.lock().clone()
}

pub fn stop(state: &AppState) {
    state.path_handle.cancel.store(true, Ordering::SeqCst);
}

async fn run(state: Arc<AppState>, req: PathRequest) -> Result<(), String> {
    let dest = match net::resolve(&req.target).await {
        Some(IpAddr::V4(v4)) => v4,
        Some(IpAddr::V6(_)) => return Err("ipv6Unsupported".into()),
        None => return Err("unresolved".into()),
    };
    {
        let mut r = state.path.lock();
        r.target_ip = Some(dest);
        r.phase = "trace".into();
    }
    let found = trace(dest, 30).await;
    if found.is_empty() {
        return Err("traceFailed".into());
    }
    let mut hops: Vec<Hop> = found.iter().map(|(ttl, ip)| Hop::new(*ttl, *ip)).collect();
    // Reverse DNS for every hop (in parallel).
    let names = join_all(hops.iter().map(|h| async move {
        match h.ip {
            Some(ip) => net::reverse_dns(IpAddr::V4(ip), Duration::from_millis(1500)).await,
            None => None,
        }
    }))
    .await;
    for (h, n) in hops.iter_mut().zip(names) {
        h.name = n;
    }
    assign_segments(&mut hops, dest);
    {
        let mut r = state.path.lock();
        r.hops = hops.clone();
        r.phase = "measure".into();
    }
    let rounds = req.rounds.clamp(5, 120);
    for round in 0..rounds {
        if state.path_handle.cancel.load(Ordering::SeqCst) {
            break;
        }
        let results = join_all(hops.iter().map(|h| async move {
            match h.ip {
                Some(ip) => net::icmp::ping_async(ip, Duration::from_millis(1200))
                    .await
                    .rtt(),
                None => None,
            }
        }))
        .await;
        for (h, r) in hops.iter_mut().zip(results) {
            h.samples.push(r);
            h.recompute();
        }
        {
            let mut r = state.path.lock();
            r.hops = hops.clone();
            r.rounds = round + 1;
            r.progress = (round + 1) as f64 / rounds as f64;
            r.findings = diagnose(&hops);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_traceroute_formats() {
        let tr = "traceroute to 8.8.8.8 (8.8.8.8), 30 hops max\n 1  192.168.1.1  1.123 ms\n 2  *\n 3  83.1.4.10  9.88 ms\n";
        assert_eq!(
            parse_trace(tr),
            vec![
                (1, Some("192.168.1.1".parse().unwrap())),
                (2, None),
                (3, Some("83.1.4.10".parse().unwrap()))
            ]
        );
        let tp = " 1?: [LOCALHOST]                      pmtu 1500\n 1:  192.168.1.1                                           0.512ms\n 1:  192.168.1.1                                           0.401ms\n 2:  no reply\n 3:  100.72.0.1                                            8.1ms\n";
        let p = parse_trace(tp);
        assert_eq!(p[0], (1, Some("192.168.1.1".parse().unwrap())));
        assert_eq!(p[1], (2, None));
        assert_eq!(p.len(), 3);
        let win = "  1    <1 ms    <1 ms    <1 ms  192.168.1.1\n  2     *        *        *     Upłynął limit czasu żądania.\n  3     9 ms     8 ms     9 ms  83.1.4.10\n";
        let w = parse_trace(win);
        assert_eq!(w[2], (3, Some("83.1.4.10".parse().unwrap())));
    }

    fn hop(ttl: u8, ip: &str, seg: Segment, loss: f64, avg: f64) -> Hop {
        let mut h = Hop::new(ttl, Some(ip.parse().unwrap()));
        h.segment = seg;
        h.sent = 20;
        h.loss_pct = loss;
        h.received = (20.0 * (1.0 - loss / 100.0)) as u32;
        h.avg = Some(avg);
        h
    }

    #[test]
    fn diagnosis_tells_rate_limiting_from_real_loss() {
        let hops = vec![
            hop(1, "192.168.1.1", Segment::Lan, 0.0, 2.0),
            hop(2, "100.72.0.1", Segment::Isp, 0.0, 8.0),
            hop(3, "83.1.4.10", Segment::Isp, 40.0, 9.0),
            hop(4, "8.8.8.8", Segment::Destination, 0.0, 13.0),
        ];
        let f = diagnose(&hops);
        assert!(f
            .iter()
            .any(|x| x.code == "rateLimited" && x.ttl == Some(3)));
        assert!(f.iter().any(|x| x.code == "cgnat"));
        assert!(f.iter().any(|x| x.code == "healthy"));
        let hops = vec![
            hop(1, "192.168.1.1", Segment::Lan, 0.0, 2.0),
            hop(2, "83.1.4.10", Segment::Isp, 12.0, 9.0),
            hop(3, "83.1.4.11", Segment::Isp, 15.0, 10.0),
            hop(4, "8.8.8.8", Segment::Destination, 13.0, 13.0),
        ];
        let f = diagnose(&hops);
        let loss = f.iter().find(|x| x.code == "lossFrom").unwrap();
        assert_eq!(loss.ttl, Some(2));
        assert_eq!(loss.segment, Some(Segment::Isp));
    }

    #[test]
    fn segments_follow_domains() {
        let mut hops = vec![
            Hop::new(1, Some("192.168.1.1".parse().unwrap())),
            Hop::new(2, Some("100.72.0.1".parse().unwrap())),
            Hop::new(3, Some("83.1.4.10".parse().unwrap())),
            Hop::new(4, Some("83.1.5.1".parse().unwrap())),
            Hop::new(5, Some("72.14.214.2".parse().unwrap())),
            Hop::new(6, Some("8.8.8.8".parse().unwrap())),
        ];
        hops[2].name = Some("war-r1.core.orange.pl".into());
        hops[3].name = Some("war-b1.core.orange.pl".into());
        hops[4].name = Some("72-14-214-2.google.com".into());
        assert_eq!(domain_of("x.y.orange.pl").as_deref(), Some("orange.pl"));
        assign_segments(&mut hops, "8.8.8.8".parse().unwrap());
        let segs: Vec<Segment> = hops.iter().map(|h| h.segment).collect();
        assert_eq!(
            segs,
            vec![
                Segment::Lan,
                Segment::Isp,
                Segment::Isp,
                Segment::Isp,
                Segment::Internet,
                Segment::Destination
            ]
        );
    }
}
