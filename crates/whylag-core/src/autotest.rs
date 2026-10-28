//! AutoTest: one button that checks every layer a user depends on (link,
//! DHCP, router, DNS, Wi-Fi, internet, captive portal, custom targets) and
//! grades each against a threshold profile, like a handheld tester's AutoTest.
//!
//! The DHCP check broadcasts a DISCOVER and collects every OFFER for a few
//! seconds without ever requesting a lease, so it changes nothing on the
//! network. More than one answering server means a rogue DHCP server.

use crate::net::{self, dns, TcpOutcome};
use crate::state::AppState;
use crate::util::{median, now_ms};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Limits a check is graded against. Integrators keep one profile per client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    pub name: String,
    /// Slowest acceptable DHCP offer.
    pub dhcp_ms: f64,
    pub gateway_ms: f64,
    pub gateway_loss_pct: f64,
    pub dns_ms: f64,
    pub internet_ms: f64,
    pub wifi_rssi_dbm: i32,
    pub wifi_snr_db: i32,
    pub wifi_util_pct: f64,
    /// Minimum negotiated link speed.
    pub link_mbps: f64,
    /// Extra hosts to check, as `host` (ping) or `host:port` (TCP connect).
    pub targets: Vec<String>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Standard".into(),
            dhcp_ms: 1000.0,
            gateway_ms: 20.0,
            gateway_loss_pct: 1.0,
            dns_ms: 100.0,
            internet_ms: 80.0,
            wifi_rssi_dbm: -67,
            wifi_snr_db: 25,
            wifi_util_pct: 60.0,
            link_mbps: 100.0,
            targets: vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Pass,
    Warn,
    Fail,
    /// Could not run here (no Wi-Fi, no permission...).
    Skip,
}

/// One graded check. `code` and `params` drive the translated explanation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    /// link, dhcp, gateway, dns, wifi, internet, portal, ipv6 or target.
    pub id: String,
    pub status: Status,
    pub code: String,
    pub value: Option<f64>,
    pub unit: Option<String>,
    pub limit: Option<f64>,
    pub params: Value,
    /// Order in the UI.
    pub order: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoTestRun {
    pub running: bool,
    pub phase: String,
    pub progress: f64,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub simulated: bool,
    pub profile: Option<Profile>,
    pub checks: Vec<Check>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct AutoTestHandle {
    pub cancel: AtomicBool,
}

/// Expected number of checks before targets, for the progress bar.
const BASE_CHECKS: usize = 8;

fn check(id: &str, order: u32, status: Status, code: &str) -> Check {
    Check {
        id: id.into(),
        status,
        code: code.into(),
        value: None,
        unit: None,
        limit: None,
        params: json!({}),
        order,
    }
}

impl Check {
    fn value(mut self, v: f64, unit: &str, limit: Option<f64>) -> Self {
        self.value = Some(v);
        self.unit = Some(unit.into());
        self.limit = limit;
        self
    }
    fn params(mut self, p: Value) -> Self {
        self.params = p;
        self
    }
}

/// Pass under the limit, warn up to 1.5× the limit, fail above.
fn grade_max(v: f64, limit: f64) -> Status {
    if v <= limit {
        Status::Pass
    } else if v <= limit * 1.5 {
        Status::Warn
    } else {
        Status::Fail
    }
}

fn push(state: &AppState, c: Check) {
    let mut run = state.autotest.lock();
    run.checks.push(c);
    run.checks.sort_by_key(|c| c.order);
    let total = BASE_CHECKS + run.profile.as_ref().map(|p| p.targets.len()).unwrap_or(0);
    run.progress = (run.checks.len() as f64 / total as f64).min(0.98);
}

pub fn start(state: Arc<AppState>, profile: Profile) -> AutoTestRun {
    {
        let mut run = state.autotest.lock();
        if run.running {
            return run.clone();
        }
        *run = AutoTestRun {
            running: true,
            phase: "running".into(),
            started_ms: now_ms(),
            profile: Some(profile.clone()),
            simulated: state.use_simulator(),
            ..Default::default()
        };
    }
    state.autotest_handle.cancel.store(false, Ordering::SeqCst);
    let st = state.clone();
    tokio::spawn(async move {
        let result = if st.use_simulator() {
            crate::util::catch_job(simulate(st.clone(), profile)).await
        } else {
            crate::util::catch_job(run(st.clone(), profile)).await
        };
        let mut run = st.autotest.lock();
        if let Err(e) = result {
            run.error = Some(e);
        }
        run.running = false;
        run.progress = 1.0;
        run.phase = "done".into();
        run.finished_ms = Some(now_ms());
    });
    state.autotest.lock().clone()
}

pub fn stop(state: &AppState) {
    state.autotest_handle.cancel.store(true, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// Individual checks

fn link_check(local: &net::LocalNet, p: &Profile) -> Check {
    let name = local
        .friendly_name
        .clone()
        .unwrap_or_else(|| local.interface_name.clone());
    let Some(ip) = local.ipv4 else {
        return check("link", 0, Status::Fail, "linkNoIpv4").params(json!({ "iface": name }));
    };
    if ip.is_link_local() {
        // 169.254.x.x: the computer gave itself an address because DHCP failed.
        return check("link", 0, Status::Fail, "linkApipa")
            .params(json!({ "iface": name, "ip": ip.to_string() }));
    }
    let params = json!({ "iface": name, "ip": ip.to_string(), "prefix": local.prefix_len, "kind": local.kind, "mtu": local.mtu });
    match local.link_speed_mbps {
        Some(speed) => {
            let s = speed as f64;
            let status = if s >= p.link_mbps {
                Status::Pass
            } else if s >= p.link_mbps / 2.0 {
                Status::Warn
            } else {
                Status::Fail
            };
            check(
                "link",
                0,
                status,
                if status == Status::Pass {
                    "linkOk"
                } else {
                    "linkSlow"
                },
            )
            .value(s, "Mb/s", Some(p.link_mbps))
            .params(params)
        }
        None => check("link", 0, Status::Pass, "linkOk").params(params),
    }
}

/// A DHCP OFFER as seen on the wire.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub server: Ipv4Addr,
    pub offered: Ipv4Addr,
    pub router: Option<Ipv4Addr>,
    pub dns: Vec<Ipv4Addr>,
    pub lease_s: Option<u32>,
    pub ms: f64,
}

pub fn build_discover(xid: u32, mac: [u8; 6]) -> Vec<u8> {
    let mut p = vec![0u8; 240];
    p[0] = 1; // BOOTREQUEST
    p[1] = 1; // Ethernet
    p[2] = 6;
    p[4..8].copy_from_slice(&xid.to_be_bytes());
    p[10] = 0x80; // ask for a broadcast reply: we have no address to unicast to
    p[28..34].copy_from_slice(&mac);
    p[236..240].copy_from_slice(&[99, 130, 83, 99]);
    // DISCOVER, parameter request list (mask, router, DNS, lease, server id), end.
    p.extend_from_slice(&[53, 1, 1, 55, 5, 1, 3, 6, 51, 54, 255]);
    p
}

pub fn parse_offer(buf: &[u8], xid: u32, ms: f64) -> Option<Offer> {
    if buf.len() < 240
        || buf[0] != 2
        || buf[4..8] != xid.to_be_bytes()
        || buf[236..240] != [99, 130, 83, 99]
    {
        return None;
    }
    let ip = |b: &[u8]| Ipv4Addr::new(b[0], b[1], b[2], b[3]);
    let offered = ip(&buf[16..20]);
    let mut kind = 0;
    let (mut server, mut router, mut dns, mut lease) = (None, None, vec![], None);
    let mut i = 240;
    while i < buf.len() {
        let code = buf[i];
        if code == 255 {
            break;
        }
        if code == 0 {
            i += 1;
            continue;
        }
        let len = *buf.get(i + 1)? as usize;
        let v = buf.get(i + 2..i + 2 + len)?;
        match code {
            53 if len == 1 => kind = v[0],
            54 if len == 4 => server = Some(ip(v)),
            3 if len >= 4 => router = Some(ip(v)),
            6 => dns = v.chunks_exact(4).map(ip).collect(),
            51 if len == 4 => lease = Some(u32::from_be_bytes(v.try_into().ok()?)),
            _ => {}
        }
        i += 2 + len;
    }
    // Fall back to the reply's source when option 54 is missing (caller fills it).
    (kind == 2).then_some(Offer {
        server: server.unwrap_or(Ipv4Addr::UNSPECIFIED),
        offered,
        router,
        dns,
        lease_s: lease,
        ms,
    })
}

/// Broadcasts one DISCOVER and listens for offers. `Err` means the check could
/// not run (port 68 is taken or needs administrator rights).
async fn dhcp_offers(mac: [u8; 6], window: Duration) -> Result<Vec<Offer>, String> {
    use socket2::{Domain, Protocol, Socket, Type};
    let sock =
        Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).map_err(|e| e.to_string())?;
    sock.set_reuse_address(true).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    let _ = sock.set_reuse_port(true);
    sock.set_broadcast(true).map_err(|e| e.to_string())?;
    sock.set_nonblocking(true).map_err(|e| e.to_string())?;
    sock.bind(&SocketAddr::from(([0, 0, 0, 0], 68)).into())
        .map_err(|_| "dhcpPort".to_string())?;
    let sock = tokio::net::UdpSocket::from_std(sock.into()).map_err(|e| e.to_string())?;
    let xid: u32 = rand::random();
    let started = Instant::now();
    sock.send_to(&build_discover(xid, mac), "255.255.255.255:67")
        .await
        .map_err(|e| e.to_string())?;
    let mut offers: Vec<Offer> = Vec::new();
    let mut buf = [0u8; 1500];
    while let Some(left) = window.checked_sub(started.elapsed()) {
        match tokio::time::timeout(left, sock.recv_from(&mut buf)).await {
            Ok(Ok((n, from))) => {
                if let Some(mut o) =
                    parse_offer(&buf[..n], xid, started.elapsed().as_secs_f64() * 1000.0)
                {
                    if o.server.is_unspecified() {
                        if let IpAddr::V4(v4) = from.ip() {
                            o.server = v4;
                        }
                    }
                    if !offers.iter().any(|x| x.server == o.server) {
                        offers.push(o);
                    }
                }
            }
            _ => break,
        }
    }
    Ok(offers)
}

fn mac_bytes(mac: Option<&str>) -> [u8; 6] {
    let mut out = [0u8; 6];
    if let Some(m) = mac.and_then(crate::util::normalize_mac) {
        for (i, part) in m.split(':').enumerate().take(6) {
            out[i] = u8::from_str_radix(part, 16).unwrap_or(0);
        }
    }
    out
}

pub fn dhcp_check(
    result: Result<Vec<Offer>, String>,
    uses_dhcp: Option<bool>,
    p: &Profile,
) -> Check {
    let offers = match result {
        Err(_) => return check("dhcp", 1, Status::Skip, "dhcpSkipped"),
        Ok(o) => o,
    };
    let servers: Vec<String> = offers.iter().map(|o| o.server.to_string()).collect();
    let list = serde_json::to_value(&offers).unwrap_or(Value::Null);
    match offers.len() {
        0 if uses_dhcp == Some(false) => check("dhcp", 1, Status::Pass, "dhcpStatic"),
        0 if uses_dhcp == Some(true) => check("dhcp", 1, Status::Fail, "dhcpNone"),
        // Unknown whether this address came from DHCP: it may be fixed.
        0 => check("dhcp", 1, Status::Warn, "dhcpNoAnswer"),
        1 => {
            let o = &offers[0];
            let status = grade_max(o.ms, p.dhcp_ms);
            check(
                "dhcp",
                1,
                status,
                if status == Status::Pass {
                    "dhcpOk"
                } else {
                    "dhcpSlow"
                },
            )
            .value(o.ms, "ms", Some(p.dhcp_ms))
            .params(json!({ "server": servers[0], "offers": list }))
        }
        n => check("dhcp", 1, Status::Fail, "dhcpRogue")
            .value(offers[0].ms, "ms", Some(p.dhcp_ms))
            .params(json!({ "count": n, "servers": servers.join(", "), "offers": list })),
    }
}

fn gateway_check(gateway: Option<Ipv4Addr>, samples: &[Option<f64>], p: &Profile) -> Check {
    let Some(gw) = gateway else {
        return check("gateway", 2, Status::Fail, "gatewayNone");
    };
    let ok: Vec<f64> = samples.iter().flatten().copied().collect();
    let loss = 100.0 * (samples.len() - ok.len()) as f64 / samples.len().max(1) as f64;
    let Some(med) = median(&ok) else {
        return check("gateway", 2, Status::Fail, "gatewayDown")
            .params(json!({ "gateway": gw.to_string() }));
    };
    let mut status = grade_max(med, p.gateway_ms);
    if loss > p.gateway_loss_pct {
        status = if loss > p.gateway_loss_pct * 5.0 {
            Status::Fail
        } else {
            Status::Warn
        };
    }
    let code = if loss > p.gateway_loss_pct {
        "gatewayLoss"
    } else if status == Status::Pass {
        "gatewayOk"
    } else {
        "gatewaySlow"
    };
    check("gateway", 2, status, code)
        .value(med, "ms", Some(p.gateway_ms))
        .params(json!({ "gateway": gw.to_string(), "loss": loss, "sent": samples.len() }))
}

fn dns_check(server: Option<IpAddr>, times: &[Option<f64>], p: &Profile) -> Check {
    let Some(server) = server else {
        return check("dns", 3, Status::Fail, "dnsNone");
    };
    let ok: Vec<f64> = times.iter().flatten().copied().collect();
    let Some(med) = median(&ok) else {
        return check("dns", 3, Status::Fail, "dnsFailed")
            .params(json!({ "server": server.to_string() }));
    };
    let status = if ok.len() < times.len() {
        Status::Warn.max_with(grade_max(med, p.dns_ms))
    } else {
        grade_max(med, p.dns_ms)
    };
    check(
        "dns",
        3,
        status,
        if status == Status::Pass {
            "dnsOk"
        } else {
            "dnsSlow"
        },
    )
    .value(med, "ms", Some(p.dns_ms))
    .params(json!({ "server": server.to_string(), "ok": ok.len(), "total": times.len() }))
}

impl Status {
    fn rank(self) -> u8 {
        match self {
            Status::Pass | Status::Skip => 0,
            Status::Warn => 1,
            Status::Fail => 2,
        }
    }
    /// The worse of two statuses.
    fn max_with(self, other: Status) -> Status {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

pub fn wifi_check(scan: Option<&crate::wifi::ScanResult>, p: &Profile) -> Check {
    let Some(scan) = scan else {
        return check("wifi", 4, Status::Skip, "wifiNone");
    };
    let Some(iface) = scan.interfaces.iter().find(|i| i.state == "connected") else {
        return check("wifi", 4, Status::Skip, "wifiNotConnected");
    };
    let bss = scan.networks.iter().find(|b| b.connected);
    let rssi = iface.rssi.or(bss.map(|b| b.rssi));
    let Some(rssi) = rssi else {
        return check("wifi", 4, Status::Skip, "wifiNone");
    };
    let noise = bss.and_then(|b| b.noise).unwrap_or(-95);
    let snr = rssi - noise;
    let util = bss.and_then(|b| b.utilization_pct);
    let mut status = if rssi >= p.wifi_rssi_dbm {
        Status::Pass
    } else if rssi >= p.wifi_rssi_dbm - 8 {
        Status::Warn
    } else {
        Status::Fail
    };
    let mut code = if status == Status::Pass {
        "wifiOk"
    } else {
        "wifiWeak"
    };
    if snr < p.wifi_snr_db && status == Status::Pass {
        status = Status::Warn;
        code = "wifiNoisy";
    }
    if let Some(u) = util {
        if u as f64 > p.wifi_util_pct && status == Status::Pass {
            status = Status::Warn;
            code = "wifiBusy";
        }
    }
    check("wifi", 4, status, code).value(rssi as f64, "dBm", Some(p.wifi_rssi_dbm as f64)).params(json!({
        "ssid": iface.ssid, "bssid": iface.bssid, "channel": iface.channel, "snr": snr, "snrLimit": p.wifi_snr_db,
        "util": util, "utilLimit": p.wifi_util_pct, "rate": iface.tx_rate_mbps,
    }))
}

fn internet_check(results: &[(String, Option<f64>)], p: &Profile) -> Check {
    let ok: Vec<f64> = results.iter().filter_map(|(_, t)| *t).collect();
    let Some(med) = median(&ok) else {
        return check("internet", 5, Status::Fail, "internetDown");
    };
    let status = if ok.len() < results.len() {
        Status::Warn.max_with(grade_max(med, p.internet_ms))
    } else {
        grade_max(med, p.internet_ms)
    };
    check("internet", 5, status, if status == Status::Pass { "internetOk" } else { "internetSlow" })
        .value(med, "ms", Some(p.internet_ms))
        .params(json!({ "ok": ok.len(), "total": results.len(), "hosts": results.iter().map(|(h, t)| json!({ "host": h, "ms": t })).collect::<Vec<_>>() }))
}

/// Captive portals answer the generate_204 probe with a login page instead.
async fn portal_check() -> Check {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(_) => return check("portal", 6, Status::Skip, "portalSkipped"),
    };
    match client
        .get("http://connectivitycheck.gstatic.com/generate_204")
        .send()
        .await
    {
        Ok(r) if r.status().as_u16() == 204 => check("portal", 6, Status::Pass, "portalNone"),
        Ok(r) => check("portal", 6, Status::Warn, "portalFound")
            .params(json!({ "status": r.status().as_u16() })),
        Err(_) => check("portal", 6, Status::Skip, "portalSkipped"),
    }
}

fn ipv6_check(local: &net::LocalNet) -> Check {
    let global: Vec<&String> = local
        .ipv6
        .iter()
        .filter(|a| !a.starts_with("fe80") && !a.starts_with("::1"))
        .collect();
    if global.is_empty() {
        check("ipv6", 7, Status::Skip, "ipv6None")
    } else {
        check("ipv6", 7, Status::Pass, "ipv6Ok").params(json!({ "address": global[0] }))
    }
}

async fn target_check(target: &str, order: u32, p: &Profile) -> Check {
    let (host, port) = match target.rsplit_once(':') {
        Some((h, port)) if port.parse::<u16>().is_ok() && !h.contains(':') => {
            (h.to_string(), port.parse::<u16>().ok())
        }
        _ => (target.to_string(), None),
    };
    let base = json!({ "target": target });
    let Some(ip) = net::resolve(&host).await else {
        return check("target", order, Status::Fail, "targetUnresolved").params(base);
    };
    let ms = match (port, ip) {
        (Some(port), _) => {
            match net::tcp_probe(SocketAddr::new(ip, port), Duration::from_secs(2)).await {
                (TcpOutcome::Open, rtt) => rtt,
                _ => None,
            }
        }
        (None, IpAddr::V4(v4)) => {
            let mut times = Vec::new();
            for _ in 0..4 {
                times.push(
                    net::icmp::ping_async(v4, Duration::from_millis(1000))
                        .await
                        .rtt(),
                );
            }
            median(&times.into_iter().flatten().collect::<Vec<_>>())
        }
        (None, IpAddr::V6(_)) => None,
    };
    match ms {
        Some(v) => {
            let status = grade_max(v, p.internet_ms);
            check(
                "target",
                order,
                status,
                if status == Status::Pass {
                    "targetOk"
                } else {
                    "targetSlow"
                },
            )
            .value(v, "ms", Some(p.internet_ms))
            .params(base)
        }
        None => check("target", order, Status::Fail, "targetDown").params(base),
    }
}

async fn run(state: Arc<AppState>, p: Profile) -> Result<(), String> {
    let local = tokio::task::spawn_blocking(net::local_net)
        .await
        .ok()
        .flatten()
        .ok_or("noNetwork")?;
    push(&state, link_check(&local, &p));
    push(&state, ipv6_check(&local));
    let mac = mac_bytes(local.mac.as_deref());
    let gateway = local.gateway;
    let dns_server = local.dns_servers.iter().find(|d| !d.is_loopback()).copied();

    let st = &state;
    let p = &p;
    let dhcp = async {
        let r = dhcp_offers(mac, Duration::from_secs(3)).await;
        push(st, dhcp_check(r, local.dhcp, p));
    };
    let gw = async {
        let mut samples = Vec::new();
        if let Some(g) = gateway {
            for _ in 0..10 {
                let mut rtt = net::icmp::ping_async(g, Duration::from_millis(800))
                    .await
                    .rtt();
                if rtt.is_none() {
                    // Routers that ignore ping still answer TCP.
                    let (o, t) = net::tcp_probe(
                        SocketAddr::new(IpAddr::V4(g), 80),
                        Duration::from_millis(800),
                    )
                    .await;
                    rtt = matches!(o, TcpOutcome::Open | TcpOutcome::Closed)
                        .then_some(t)
                        .flatten();
                }
                samples.push(rtt);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        push(st, gateway_check(gateway, &samples, p));
    };
    let dns_t = async {
        let mut times = Vec::new();
        if let Some(server) = dns_server {
            for name in [
                "www.google.com",
                "www.wikipedia.org",
                "www.microsoft.com",
                "www.cloudflare.com",
            ] {
                times.push(
                    dns::query(server, name, Duration::from_millis(2000))
                        .await
                        .filter(|a| a.rcode == 0)
                        .map(|a| a.rtt_ms),
                );
            }
        }
        push(st, dns_check(dns_server, &times, p));
    };
    let wifi = async {
        let scan = crate::wifi::scan_for_analysis(st).await;
        push(st, wifi_check(scan.as_ref(), p));
    };
    let inet = async {
        let hosts = [
            ("1.1.1.1", 443u16),
            ("8.8.8.8", 443),
            ("www.google.com", 443),
        ];
        let mut results = Vec::new();
        for (h, port) in hosts {
            let t = match net::resolve(h).await {
                Some(ip) => {
                    match net::tcp_probe(SocketAddr::new(ip, port), Duration::from_secs(2)).await {
                        (TcpOutcome::Open, rtt) => rtt,
                        _ => None,
                    }
                }
                None => None,
            };
            results.push((h.to_string(), t));
        }
        push(st, internet_check(&results, p));
    };
    let portal = async { push(st, portal_check().await) };
    let targets = async {
        for (i, t) in p
            .targets
            .iter()
            .filter(|t| !t.trim().is_empty())
            .enumerate()
        {
            if st.autotest_handle.cancel.load(Ordering::SeqCst) {
                break;
            }
            push(st, target_check(t.trim(), 10 + i as u32, p).await);
        }
    };
    tokio::join!(dhcp, gw, dns_t, wifi, inet, portal, targets);
    Ok(())
}

/// Demo office: healthy except a consumer router someone plugged in, which
/// answers DHCP too — the classic "half the office has no internet" case.
async fn simulate(state: Arc<AppState>, p: Profile) -> Result<(), String> {
    let local = crate::sim::lan::local();
    push(&state, link_check(&local, &p));
    push(&state, ipv6_check(&local));
    tokio::time::sleep(Duration::from_millis(500)).await;
    let jitter = || rand::random::<f64>();
    let offers = vec![
        Offer {
            server: Ipv4Addr::new(192, 168, 10, 1),
            offered: Ipv4Addr::new(192, 168, 10, 64),
            router: Some(Ipv4Addr::new(192, 168, 10, 1)),
            dns: vec![Ipv4Addr::new(192, 168, 10, 1)],
            lease_s: Some(86_400),
            ms: 42.0 + jitter() * 20.0,
        },
        Offer {
            server: Ipv4Addr::new(192, 168, 0, 1),
            offered: Ipv4Addr::new(192, 168, 0, 101),
            router: Some(Ipv4Addr::new(192, 168, 0, 1)),
            dns: vec![Ipv4Addr::new(192, 168, 0, 1)],
            lease_s: Some(7_200),
            ms: 118.0 + jitter() * 40.0,
        },
    ];
    push(&state, dhcp_check(Ok(offers), Some(true), &p));
    tokio::time::sleep(Duration::from_millis(400)).await;
    let samples: Vec<Option<f64>> = (0..10).map(|_| Some(1.6 + jitter() * 2.5)).collect();
    push(&state, gateway_check(local.gateway, &samples, &p));
    let times: Vec<Option<f64>> = (0..4).map(|_| Some(9.0 + jitter() * 30.0)).collect();
    push(
        &state,
        dns_check(local.dns_servers.first().copied(), &times, &p),
    );
    let scan = state.sim.lock().wifi_scan();
    push(&state, wifi_check(Some(&scan), &p));
    tokio::time::sleep(Duration::from_millis(400)).await;
    let inet: Vec<(String, Option<f64>)> = ["1.1.1.1", "8.8.8.8", "www.google.com"]
        .iter()
        .map(|h| (h.to_string(), Some(12.0 + jitter() * 14.0)))
        .collect();
    push(&state, internet_check(&inet, &p));
    push(&state, check("portal", 6, Status::Pass, "portalNone"));
    for (i, t) in p
        .targets
        .iter()
        .filter(|t| !t.trim().is_empty())
        .enumerate()
    {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let v = 3.0 + jitter() * 25.0;
        push(
            &state,
            check(
                "target",
                10 + i as u32,
                grade_max(v, p.internet_ms),
                "targetOk",
            )
            .value(v, "ms", Some(p.internet_ms))
            .params(json!({ "target": t.trim() })),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer_packet(xid: u32, server: [u8; 4]) -> Vec<u8> {
        let mut p = vec![0u8; 240];
        p[0] = 2;
        p[4..8].copy_from_slice(&xid.to_be_bytes());
        p[16..20].copy_from_slice(&[192, 168, 1, 50]);
        p[236..240].copy_from_slice(&[99, 130, 83, 99]);
        p.extend_from_slice(&[53, 1, 2, 54, 4]);
        p.extend_from_slice(&server);
        p.extend_from_slice(&[
            3, 4, 192, 168, 1, 1, 6, 8, 1, 1, 1, 1, 8, 8, 8, 8, 51, 4, 0, 1, 81, 128, 255,
        ]);
        p
    }

    #[test]
    fn discover_is_well_formed() {
        let d = build_discover(0xdeadbeef, [1, 2, 3, 4, 5, 6]);
        assert_eq!(d[0], 1);
        assert_eq!(&d[4..8], &0xdeadbeefu32.to_be_bytes());
        assert_eq!(&d[28..34], &[1, 2, 3, 4, 5, 6]);
        assert_eq!(&d[240..243], &[53, 1, 1]);
        assert_eq!(*d.last().unwrap(), 255);
    }

    #[test]
    fn parses_offer_and_flags_rogue_servers() {
        let o = parse_offer(&offer_packet(7, [192, 168, 1, 1]), 7, 30.0).unwrap();
        assert_eq!(o.server, Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(o.offered, Ipv4Addr::new(192, 168, 1, 50));
        assert_eq!(o.dns.len(), 2);
        assert_eq!(o.lease_s, Some(86_400));
        assert!(
            parse_offer(&offer_packet(7, [192, 168, 1, 1]), 8, 30.0).is_none(),
            "wrong xid"
        );
        let p = Profile::default();
        assert_eq!(
            dhcp_check(Ok(vec![o.clone()]), Some(true), &p).status,
            Status::Pass
        );
        let rogue = Offer {
            server: Ipv4Addr::new(10, 0, 0, 1),
            ..o.clone()
        };
        let c = dhcp_check(Ok(vec![o, rogue]), Some(true), &p);
        assert_eq!((c.status, c.code.as_str()), (Status::Fail, "dhcpRogue"));
        assert_eq!(dhcp_check(Ok(vec![]), Some(false), &p).code, "dhcpStatic");
        assert_eq!(
            dhcp_check(Err("dhcpPort".into()), Some(true), &p).status,
            Status::Skip
        );
    }

    #[test]
    fn grades_against_profile() {
        let p = Profile::default();
        assert_eq!(
            gateway_check(Some(Ipv4Addr::new(192, 168, 1, 1)), &[Some(2.0); 10], &p).status,
            Status::Pass
        );
        let mut lossy = vec![Some(2.0); 8];
        lossy.extend([None, None]);
        assert_eq!(
            gateway_check(Some(Ipv4Addr::new(192, 168, 1, 1)), &lossy, &p).code,
            "gatewayLoss"
        );
        assert_eq!(
            gateway_check(Some(Ipv4Addr::new(192, 168, 1, 1)), &[None; 10], &p).status,
            Status::Fail
        );
        assert_eq!(grade_max(25.0, 20.0), Status::Warn);
        assert_eq!(grade_max(40.0, 20.0), Status::Fail);
    }

    #[test]
    fn apipa_address_fails_link() {
        let l = net::LocalNet {
            ipv4: Some(Ipv4Addr::new(169, 254, 3, 4)),
            ..Default::default()
        };
        assert_eq!(link_check(&l, &Profile::default()).code, "linkApipa");
    }

    #[tokio::test]
    async fn simulated_run_finds_the_rogue_router() {
        let dir = std::env::temp_dir().join(format!("whylag-at-{}", rand::random::<u32>()));
        let state = AppState::new(dir);
        state
            .update_settings(json!({ "dataSource": "simulator" }))
            .unwrap();
        start(
            state.clone(),
            Profile {
                targets: vec!["intranet.local:443".into()],
                ..Default::default()
            },
        );
        for _ in 0..50 {
            if !state.autotest.lock().running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let run = state.autotest.lock().clone();
        assert!(!run.running);
        assert_eq!(
            run.checks.len(),
            BASE_CHECKS + 1,
            "{:?}",
            run.checks.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
        assert!(run.checks.iter().any(|c| c.code == "dhcpRogue"));
    }
}
