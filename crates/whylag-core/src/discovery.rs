//! Local network discovery: who is on the network, what they are and how they
//! are connected.
//!
//! 1. Sweep the subnet with ICMP and TCP probes (unprivileged).
//! 2. Read the ARP cache: devices that ignore every probe (phones, IoT) still
//!    answered ARP while we probed them.
//! 3. Listen for mDNS and SSDP announcements.
//! 4. Enrich each host: reverse DNS, NetBIOS, open ports, HTTP banner, SNMP.
//! 5. Ask SNMP-capable switches for their LLDP neighbours (topology).
//! 6. Classify every device from all the evidence.

use crate::net::{self, snmp, LocalNet, TcpOutcome};
use crate::state::AppState;
use crate::util::now_ms;
use futures_util::future::{BoxFuture, FutureExt};
use futures_util::stream::{self, StreamExt};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    Router,
    Switch,
    AccessPoint,
    Firewall,
    Server,
    Nas,
    Printer,
    Camera,
    Phone,
    Tablet,
    Computer,
    Tv,
    MediaPlayer,
    Speaker,
    Iot,
    GameConsole,
    Voip,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPort {
    pub port: u16,
    pub service: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub ip: Ipv4Addr,
    pub mac: Option<String>,
    pub vendor: Option<String>,
    /// Randomised ("private") MAC: typical of phones and laptops.
    pub random_mac: bool,
    pub name: Option<String>,
    /// Every name we learned, with its source (mdns, netbios, dns, snmp, upnp).
    pub names: Vec<(String, String)>,
    pub kind: DeviceKind,
    /// Evidence codes behind the classification (translated by the UI).
    pub evidence: Vec<String>,
    pub model: Option<String>,
    pub os: Option<String>,
    pub ports: Vec<OpenPort>,
    pub mdns_services: Vec<String>,
    pub http_title: Option<String>,
    pub http_server: Option<String>,
    pub snmp_name: Option<String>,
    pub snmp_descr: Option<String>,
    pub upnp_type: Option<String>,
    pub rtt_ms: Option<f64>,
    pub is_gateway: bool,
    pub is_self: bool,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            ip: Ipv4Addr::UNSPECIFIED,
            mac: None,
            vendor: None,
            random_mac: false,
            name: None,
            names: Vec::new(),
            kind: DeviceKind::Unknown,
            evidence: Vec::new(),
            model: None,
            os: None,
            ports: Vec::new(),
            mdns_services: Vec::new(),
            http_title: None,
            http_server: None,
            snmp_name: None,
            snmp_descr: None,
            upnp_type: None,
            rtt_ms: None,
            is_gateway: false,
            is_self: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub from: Ipv4Addr,
    pub local_port: Option<String>,
    pub neighbor: String,
    pub neighbor_ip: Option<Ipv4Addr>,
    pub remote_port: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryRun {
    pub running: bool,
    pub phase: String,
    pub progress: f64,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub simulated: bool,
    pub local: Option<LocalNet>,
    pub subnet: Option<String>,
    pub hosts: Vec<Host>,
    pub links: Vec<Link>,
    pub error: Option<String>,
}

/// Ports checked on every live host.
pub const PORTS: &[(u16, &str)] = &[
    (21, "ftp"),
    (22, "ssh"),
    (23, "telnet"),
    (53, "dns"),
    (80, "http"),
    (135, "msrpc"),
    (139, "netbios"),
    (443, "https"),
    (445, "smb"),
    (515, "lpd"),
    (548, "afp"),
    (554, "rtsp"),
    (631, "ipp"),
    (1400, "sonos"),
    (1883, "mqtt"),
    (2049, "nfs"),
    (3306, "mysql"),
    (3389, "rdp"),
    (5000, "upnp-web"),
    (5001, "https-alt"),
    (5060, "sip"),
    (5357, "wsd"),
    (5900, "vnc"),
    (7000, "airplay"),
    (8000, "http-alt"),
    (8008, "cast"),
    (8009, "cast-tls"),
    (8080, "http-proxy"),
    (8291, "winbox"),
    (8443, "https-alt"),
    (9100, "jetdirect"),
    (32400, "plex"),
    (49152, "upnp"),
    (62078, "apple-sync"),
];

const SWEEP_PORTS: &[u16] = &[80, 443, 445, 22, 62078, 554, 8080, 9100];

pub fn port_name(port: u16) -> &'static str {
    PORTS
        .iter()
        .find(|(p, _)| *p == port)
        .map(|(_, n)| *n)
        .unwrap_or("tcp")
}

/// Shared handle used to report progress and stop a running scan.
pub struct DiscoveryHandle {
    pub cancel: AtomicBool,
}

impl Default for DiscoveryHandle {
    fn default() -> Self {
        Self {
            cancel: AtomicBool::new(false),
        }
    }
}

fn update(state: &AppState, f: impl FnOnce(&mut DiscoveryRun)) {
    f(&mut state.discovery.lock());
}

/// Starts a discovery run in the background (no-op if one is running).
pub fn start(state: Arc<AppState>) -> DiscoveryRun {
    {
        let mut run = state.discovery.lock();
        if run.running {
            return run.clone();
        }
        *run = DiscoveryRun {
            running: true,
            phase: "starting".into(),
            started_ms: now_ms(),
            ..Default::default()
        };
    }
    state.discovery_handle.cancel.store(false, Ordering::SeqCst);
    let simulate = state.use_simulator();
    let st = state.clone();
    tokio::spawn(async move {
        let result = if simulate {
            crate::util::catch_job(crate::sim::lan::discover(st.clone())).await
        } else {
            crate::util::catch_job(discover(st.clone())).await
        };
        let mut run = st.discovery.lock();
        if let Err(e) = result {
            run.error = Some(e);
        }
        run.running = false;
        run.progress = 1.0;
        run.phase = "done".into();
        run.finished_ms = Some(now_ms());
    });
    state.discovery.lock().clone()
}

pub fn stop(state: &AppState) {
    state.discovery_handle.cancel.store(true, Ordering::SeqCst);
}

fn cancelled(state: &AppState) -> bool {
    state.discovery_handle.cancel.load(Ordering::SeqCst)
}

async fn probe_alive(ip: Ipv4Addr) -> Option<f64> {
    let timeout = Duration::from_millis(700);
    let icmp = tokio::task::spawn_blocking(move || net::icmp::ping(ip, timeout, None));
    let tcp = futures_util::future::join_all(
        SWEEP_PORTS
            .iter()
            .map(|p| net::tcp_probe(SocketAddr::new(IpAddr::V4(ip), *p), timeout)),
    );
    let (icmp, tcp) = tokio::join!(icmp, tcp);
    if let Ok(r) = icmp {
        if let Some(rtt) = r.rtt() {
            return Some(rtt);
        }
    }
    tcp.into_iter()
        .filter(|(o, _)| matches!(o, TcpOutcome::Open | TcpOutcome::Closed))
        .filter_map(|(_, rtt)| rtt)
        .reduce(f64::min)
}

async fn scan_ports(ip: Ipv4Addr) -> Vec<OpenPort> {
    let results = stream::iter(PORTS.iter().map(|(p, _)| *p).collect::<Vec<u16>>())
        .map(move |port: u16| -> BoxFuture<'static, Option<OpenPort>> {
            async move {
                let (o, _) = net::tcp_probe(
                    SocketAddr::new(IpAddr::V4(ip), port),
                    Duration::from_millis(500),
                )
                .await;
                (o == TcpOutcome::Open).then_some(OpenPort {
                    port,
                    service: port_name(port),
                })
            }
            .boxed()
        })
        .buffer_unordered(16)
        .collect::<Vec<_>>()
        .await;
    let mut ports: Vec<OpenPort> = results.into_iter().flatten().collect();
    ports.sort_by_key(|p| p.port);
    ports
}

fn extract_title(html: &str) -> Option<String> {
    // ASCII lowercasing keeps byte offsets identical to `html`.
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = lower[start..].find('>')? + start + 1;
    let end = lower[open_end..].find("</title>")? + open_end;
    let title = html[open_end..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!title.is_empty()).then(|| title.chars().take(80).collect())
}

async fn http_banner(
    client: &reqwest::Client,
    ip: Ipv4Addr,
    ports: &[OpenPort],
) -> (Option<String>, Option<String>) {
    for (port, https) in [
        (80, false),
        (443, true),
        (8080, false),
        (8443, true),
        (5000, false),
        (8000, false),
    ] {
        if !ports.iter().any(|p| p.port == port) {
            continue;
        }
        let url = format!(
            "{}://{}:{}/",
            if https { "https" } else { "http" },
            ip,
            port
        );
        if let Ok(resp) = client.get(&url).send().await {
            let server = resp
                .headers()
                .get("server")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let body = crate::util::read_capped(resp, 64 * 1024).await;
            let title = extract_title(&body);
            if title.is_some() || server.is_some() {
                return (title, server);
            }
        }
    }
    (None, None)
}

/// mDNS services by IPv4 address: (service types, instance name, TXT model).
struct MdnsInfo {
    services: Vec<String>,
    names: Vec<String>,
    model: Option<String>,
}

fn browse_mdns(window: Duration) -> HashMap<Ipv4Addr, MdnsInfo> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    let mut out: HashMap<Ipv4Addr, MdnsInfo> = HashMap::new();
    let Ok(daemon) = ServiceDaemon::new() else {
        return out;
    };
    let deadline = std::time::Instant::now() + window;
    let mut types: Vec<String> = Vec::new();
    if let Ok(rx) = daemon.browse("_services._dns-sd._udp.local.") {
        let until = std::time::Instant::now() + window / 2;
        while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(ServiceEvent::ServiceFound(_, fullname)) => {
                    let ty = fullname.trim_end_matches('.').to_string() + ".";
                    if !types.contains(&ty) {
                        types.push(ty);
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let _ = daemon.stop_browse("_services._dns-sd._udp.local.");
    }
    // Always look for the common ones even if the enumeration is not answered.
    for t in [
        "_http._tcp.local.",
        "_ipp._tcp.local.",
        "_printer._tcp.local.",
        "_airplay._tcp.local.",
        "_googlecast._tcp.local.",
        "_smb._tcp.local.",
        "_device-info._tcp.local.",
        "_hap._tcp.local.",
        "_raop._tcp.local.",
        "_spotify-connect._tcp.local.",
        "_workstation._tcp.local.",
        "_companion-link._tcp.local.",
    ] {
        if !types.iter().any(|x| x == t) {
            types.push(t.to_string());
        }
    }
    let receivers: Vec<(String, mdns_sd::Receiver<ServiceEvent>)> = types
        .iter()
        .filter_map(|t| daemon.browse(t).ok().map(|r| (t.clone(), r)))
        .collect();
    while std::time::Instant::now() < deadline {
        let mut idle = true;
        for (ty, rx) in &receivers {
            while let Ok(ev) = rx.try_recv() {
                idle = false;
                if let ServiceEvent::ServiceResolved(info) = ev {
                    let instance = info
                        .fullname
                        .trim_end_matches(&format!(".{}", ty.trim_end_matches('.')))
                        .trim_end_matches('.')
                        .to_string();
                    let model = info
                        .get_property_val_str("md")
                        .or(info.get_property_val_str("model"))
                        .or(info.get_property_val_str("ty"))
                        .or(info.get_property_val_str("am"))
                        .map(str::to_string);
                    let friendly = info.get_property_val_str("fn").map(str::to_string);
                    for addr in info.get_addresses() {
                        if let IpAddr::V4(v4) = addr.to_ip_addr() {
                            let e = out.entry(v4).or_insert(MdnsInfo {
                                services: vec![],
                                names: vec![],
                                model: None,
                            });
                            let short = ty.trim_end_matches(".local.").to_string();
                            if !e.services.contains(&short) {
                                e.services.push(short);
                            }
                            for n in [friendly.clone(), Some(instance.clone())]
                                .into_iter()
                                .flatten()
                            {
                                if !n.is_empty() && !e.names.contains(&n) {
                                    e.names.push(n);
                                }
                            }
                            if e.model.is_none() {
                                e.model = model.clone();
                            }
                        }
                    }
                }
            }
        }
        if idle {
            std::thread::sleep(Duration::from_millis(60));
        }
    }
    let _ = daemon.shutdown();
    out
}

/// SSDP/UPnP: device descriptions by IP (friendly name, manufacturer, model, type).
async fn ssdp(
    client: &reqwest::Client,
    window: Duration,
) -> HashMap<Ipv4Addr, (Option<String>, Option<String>, Option<String>)> {
    let mut out = HashMap::new();
    let Ok(sock) = tokio::net::UdpSocket::bind("0.0.0.0:0").await else {
        return out;
    };
    let msg = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: ssdp:all\r\n\r\n";
    for _ in 0..2 {
        let _ = sock.send_to(msg.as_bytes(), "239.255.255.250:1900").await;
    }
    let mut locations: HashMap<Ipv4Addr, String> = HashMap::new();
    let deadline = tokio::time::Instant::now() + window;
    let mut buf = [0u8; 2048];
    while let Ok(Ok((n, from))) = tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await
    {
        let text = String::from_utf8_lossy(&buf[..n]);
        if let (IpAddr::V4(ip), Some(loc)) = (
            from.ip(),
            text.lines().find_map(|l| {
                l.split_once(':')
                    .filter(|(k, _)| k.eq_ignore_ascii_case("location"))
                    .map(|(_, v)| v.trim().to_string())
            }),
        ) {
            locations.entry(ip).or_insert(loc);
        }
    }
    let tag = |xml: &str, t: &str| -> Option<String> {
        let open = format!("<{t}>");
        let s = xml.find(&open)? + open.len();
        let e = xml[s..].find(&format!("</{t}>"))? + s;
        Some(xml[s..e].trim().to_string()).filter(|v| !v.is_empty())
    };
    for (ip, loc) in locations {
        // Only fetch descriptions from the device that announced them.
        let same_host = url_host(&loc).map(|h| h == ip.to_string()).unwrap_or(false);
        if !same_host {
            continue;
        }
        if let Ok(resp) = client.get(&loc).send().await {
            if let Ok(xml) = resp.text().await {
                let friendly = tag(&xml, "friendlyName");
                let model = match (tag(&xml, "manufacturer"), tag(&xml, "modelName")) {
                    (Some(m), Some(n)) if !n.to_lowercase().contains(&m.to_lowercase()) => {
                        Some(format!("{m} {n}"))
                    }
                    (_, Some(n)) => Some(n),
                    (Some(m), None) => Some(m),
                    _ => None,
                };
                let ty =
                    tag(&xml, "deviceType").map(|t| t.split(':').nth(3).unwrap_or(&t).to_string());
                out.insert(ip, (friendly, model, ty));
            }
        }
    }
    out
}

fn url_host(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let host = rest.split(['/', ':']).next()?;
    Some(host.to_string())
}

/// Picks the device type from every piece of evidence we have.
pub fn classify(h: &mut Host) {
    let vendor = h.vendor.clone().unwrap_or_default().to_lowercase();
    let has = |p: u16| h.ports.iter().any(|x| x.port == p);
    let svc = |s: &str| h.mdns_services.iter().any(|x| x.contains(s));
    let text = [
        h.http_title.clone(),
        h.http_server.clone(),
        h.snmp_descr.clone(),
        h.model.clone(),
        h.upnp_type.clone(),
        h.name.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_lowercase();
    let mut ev: Vec<String> = Vec::new();
    let any = |words: &[&str], t: &str| words.iter().any(|w| t.contains(w));
    // Whole-word vendor match ("LG" must not match "Logitech").
    let vendor_word = |words: &[&str]| {
        words
            .iter()
            .any(|w| vendor == *w || vendor.starts_with(&format!("{w} ")))
    };

    let kind = if h.is_self {
        ev.push("self".into());
        DeviceKind::Computer
    } else if h.is_gateway {
        ev.push("gateway".into());
        if any(
            &[
                "fortigate",
                "fortinet",
                "pfsense",
                "opnsense",
                "sophos",
                "watchguard",
                "sonicwall",
            ],
            &(vendor.clone() + " " + &text),
        ) {
            DeviceKind::Firewall
        } else {
            DeviceKind::Router
        }
    } else if has(554)
        || any(
            &[
                "hikvision",
                "dahua",
                "ezviz",
                "reolink",
                "axis comm",
                "amcrest",
                "uniview",
                "foscam",
            ],
            &vendor,
        )
        || any(&["ipcam", "nvr", "dvr", "camera", "onvif"], &text)
    {
        ev.push(if has(554) { "rtsp" } else { "cameraVendor" }.into());
        DeviceKind::Camera
    } else if has(9100)
        || has(631)
        || has(515)
        || svc("_ipp")
        || svc("_printer")
        || any(
            &[
                "printer",
                "laserjet",
                "officejet",
                "deskjet",
                "mfc-",
                "ecosys",
                "imagerunner",
            ],
            &text,
        )
    {
        ev.push("printing".into());
        DeviceKind::Printer
    } else if any(
        &[
            "synology",
            "qnap",
            "western digital",
            "asustor",
            "terramaster",
            "netgear readynas",
        ],
        &vendor,
    ) || any(&["diskstation", "nas"], &text)
    {
        ev.push("nasVendor".into());
        DeviceKind::Nas
    } else if any(
        &[
            "switch",
            "procurve",
            "catalyst",
            "aruba os",
            "edgeswitch",
            "usw-",
        ],
        &text,
    ) || (h.snmp_name.is_some()
        && any(
            &[
                "cisco",
                "hewlett packard enterprise",
                "aruba",
                "netgear",
                "d-link",
                "zyxel",
            ],
            &vendor,
        )
        && !has(80))
    {
        ev.push("switch".into());
        DeviceKind::Switch
    } else if any(
        &[
            "ubiquiti", "ruckus", "aruba", "meraki", "cambium", "engenius", "mist",
        ],
        &vendor,
    ) || any(&["unifi ap", "access point", "uap-", "u6-", "u7-"], &text)
    {
        ev.push("apVendor".into());
        DeviceKind::AccessPoint
    } else if any(
        &[
            "mikrotik",
            "routerboard",
            "tp-link",
            "netgear",
            "asus",
            "zyxel",
            "draytek",
            "avm",
            "sagemcom",
            "technicolor",
            "huawei",
        ],
        &vendor,
    ) && (has(80) || has(443) || has(8291) || has(53))
    {
        ev.push("routerVendor".into());
        DeviceKind::Router
    } else if has(5060)
        || any(
            &["yealink", "polycom", "grandstream", "snom", "fanvil"],
            &vendor,
        )
    {
        ev.push("sip".into());
        DeviceKind::Voip
    } else if any(&["sonos", "bose", "harman"], &vendor)
        || has(1400)
        || svc("_spotify-connect") && !svc("_googlecast")
    {
        ev.push("speaker".into());
        DeviceKind::Speaker
    } else if svc("_googlecast")
        || has(8008)
        || any(
            &["roku", "chromecast", "apple tv", "fire tv", "nvidia shield"],
            &(vendor.clone() + " " + &text),
        )
    {
        ev.push("cast".into());
        DeviceKind::MediaPlayer
    } else if vendor_word(&[
        "lg", "samsung", "tcl", "hisense", "vizio", "philips", "sony",
    ]) && (svc("_airplay")
        || has(7000)
        || has(3000)
        || any(&["tv", "webos", "tizen"], &text))
        || any(&["smart tv", "bravia", "webos"], &text)
    {
        ev.push("tv".into());
        DeviceKind::Tv
    } else if any(
        &[
            "nintendo",
            "sony interactive",
            "sony computer entertainment",
            "microsoft xbox",
        ],
        &vendor,
    ) {
        ev.push("console".into());
        DeviceKind::GameConsole
    } else if has(62078) || svc("_companion-link") && !has(445) {
        ev.push("appleMobile".into());
        DeviceKind::Phone
    } else if any(
        &[
            "espressif",
            "tuya",
            "shelly",
            "allterco",
            "sonoff",
            "itead",
            "xiaomi",
            "philips lighting",
            "signify",
            "ecobee",
            "tado",
        ],
        &vendor,
    ) || vendor_word(&["ring", "nest"])
        || svc("_hap")
        || has(1883)
    {
        ev.push("iot".into());
        DeviceKind::Iot
    } else if has(3389)
        || has(445)
        || has(135)
        || has(139)
        || svc("_smb")
        || svc("_workstation")
        || any(&["windows", "macbook", "imac"], &text)
        || any(
            &[
                "intel corporate",
                "dell",
                "lenovo",
                "hewlett packard",
                "micro-star",
                "asustek",
                "gigabyte",
                "apple",
            ],
            &vendor,
        ) && !h.random_mac
    {
        ev.push("computer".into());
        DeviceKind::Computer
    } else if has(22) && (has(80) || has(443) || has(3306) || has(2049))
        || any(&["raspberry"], &vendor)
    {
        ev.push("server".into());
        DeviceKind::Server
    } else if h.random_mac && h.ports.is_empty() {
        ev.push("randomMac".into());
        DeviceKind::Phone
    } else {
        DeviceKind::Unknown
    };
    h.os = h.os.clone().or_else(|| {
        if has(3389) || has(135) || h.names.iter().any(|(s, _)| s == "netbios") {
            Some("Windows".into())
        } else if has(62078) {
            Some("iOS".into())
        } else if svc("_companion-link")
            || vendor.contains("apple") && (svc("_airplay") || svc("_smb"))
        {
            Some("Apple".into())
        } else if text.contains("linux") || text.contains("ubuntu") || text.contains("debian") {
            Some("Linux".into())
        } else {
            None
        }
    });
    h.kind = kind;
    h.evidence = ev;
}

/// Chooses the friendliest name from all sources.
fn pick_name(h: &mut Host) {
    let order = ["mdns", "upnp", "netbios", "snmp", "dns"];
    for src in order {
        if let Some((_, n)) = h.names.iter().find(|(s, _)| s == src) {
            let clean = n
                .trim_end_matches(".local")
                .trim_end_matches(".lan")
                .trim_end_matches(".home")
                .trim_end_matches(".localdomain")
                .to_string();
            h.name = Some(clean);
            return;
        }
    }
}

async fn discover(state: Arc<AppState>) -> Result<(), String> {
    let state = &state;
    let local = tokio::task::spawn_blocking(net::local_net)
        .await
        .ok()
        .flatten()
        .ok_or_else(|| "noNetwork".to_string())?;
    let range = local.scan_range();
    if range.is_empty() {
        return Err("noIpv4".into());
    }
    let community = state.settings().snmp_community;
    update(state, |r| {
        r.local = Some(local.clone());
        r.subnet = local.subnet_label();
        r.phase = "sweep".into();
    });

    // mDNS and SSDP listen in the background while we sweep.
    let mdns_task = tokio::task::spawn_blocking(|| browse_mdns(Duration::from_secs(6)));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .danger_accept_invalid_certs(true)
        .redirect(reqwest::redirect::Policy::limited(2))
        .user_agent("WhyLag network discovery")
        .build()
        .map_err(|e| e.to_string())?;
    let ssdp_client = client.clone();
    let ssdp_task = tokio::spawn(async move { ssdp(&ssdp_client, Duration::from_secs(3)).await });

    // 1. Sweep.
    let total = range.len();
    let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let st = state.clone();
    let (sweep_done, sweep_st) = (done.clone(), st.clone());
    let alive: Vec<(Ipv4Addr, f64)> = stream::iter(range.clone())
        .map(move |ip| -> BoxFuture<'static, Option<(Ipv4Addr, f64)>> {
            let done = sweep_done.clone();
            let st = sweep_st.clone();
            async move {
                if cancelled(&st) {
                    return None;
                }
                let r = probe_alive(ip).await;
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                if n % 8 == 0 {
                    update(&st, |run| run.progress = 0.45 * n as f64 / total as f64);
                }
                r.map(|rtt| (ip, rtt))
            }
            .boxed()
        })
        .buffer_unordered(96)
        .collect::<Vec<Option<(Ipv4Addr, f64)>>>()
        .await
        .into_iter()
        .flatten()
        .collect();
    if cancelled(state) {
        return Ok(());
    }

    // 2. ARP cache: includes devices that ignored our probes.
    let arp = tokio::task::spawn_blocking(net::arp::table)
        .await
        .unwrap_or_default();
    let mut hosts: BTreeMap<Ipv4Addr, Host> = BTreeMap::new();
    for (ip, rtt) in &alive {
        hosts.insert(
            *ip,
            Host {
                ip: *ip,
                rtt_ms: Some(*rtt),
                ..Default::default()
            },
        );
    }
    for (ip, mac) in &arp {
        if range.contains(ip) {
            hosts
                .entry(*ip)
                .or_insert(Host {
                    ip: *ip,
                    ..Default::default()
                })
                .mac = Some(mac.clone());
        }
    }
    if let Some(me) = local.ipv4 {
        hosts.insert(
            me,
            Host {
                ip: me,
                mac: local.mac.clone(),
                is_self: true,
                ..Default::default()
            },
        );
    }
    if let Some(gw) = local.gateway {
        let h = hosts.entry(gw).or_insert(Host {
            ip: gw,
            ..Default::default()
        });
        h.is_gateway = true;
        if h.mac.is_none() {
            h.mac = local.gateway_mac.clone();
        }
    }
    update(state, |r| {
        r.phase = "identify".into();
        r.progress = 0.5;
        r.hosts = hosts.values().cloned().collect();
    });

    // 3. Wait for the listeners.
    let mdns = mdns_task.await.unwrap_or_default();
    let upnp = ssdp_task.await.unwrap_or_default();

    // 4. Enrich hosts.
    let total = hosts.len().max(1);
    let done = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (e_client, e_community, e_done, e_st) =
        (client.clone(), community.clone(), done.clone(), st.clone());
    let enriched: Vec<Host> = stream::iter(hosts.into_values())
        .map(move |mut h| -> BoxFuture<'static, Host> {
            let client = e_client.clone();
            let community = e_community.clone();
            let done = e_done.clone();
            let st = e_st.clone();
            async move {
                if !h.is_self && !cancelled(&st) {
                    let ip = IpAddr::V4(h.ip);
                    let (dns, nb, ports, snmp_r) = tokio::join!(
                        net::reverse_dns(ip, Duration::from_millis(1500)),
                        net::netbios::query(ip, Duration::from_millis(800)),
                        scan_ports(h.ip),
                        snmp::get(
                            ip,
                            &community,
                            &[snmp::SYS_NAME, snmp::SYS_DESCR],
                            Duration::from_millis(800)
                        ),
                    );
                    if let Some(n) = dns {
                        h.names.push(("dns".into(), n));
                    }
                    if let Some(nb) = nb {
                        if let Some(n) = nb.name {
                            h.names.push(("netbios".into(), n));
                        }
                    }
                    h.ports = ports;
                    if let Some(vbs) = snmp_r {
                        h.snmp_name = vbs
                            .first()
                            .and_then(|(_, v)| v.as_text())
                            .filter(|s| !s.is_empty());
                        h.snmp_descr = vbs
                            .get(1)
                            .and_then(|(_, v)| v.as_text())
                            .filter(|s| !s.is_empty());
                        if let Some(n) = &h.snmp_name {
                            h.names.push(("snmp".into(), n.clone()));
                        }
                    }
                    let (title, server) = http_banner(&client, h.ip, &h.ports).await;
                    h.http_title = title;
                    h.http_server = server;
                }
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                update(&st, |run| {
                    run.progress = 0.5 + 0.45 * n as f64 / total as f64
                });
                h
            }
            .boxed()
        })
        .buffer_unordered(24)
        .collect()
        .await;

    let mut hosts: Vec<Host> = enriched
        .into_iter()
        .map(|mut h| {
            if let Some(m) = mdns.get(&h.ip) {
                h.mdns_services = m.services.clone();
                for n in &m.names {
                    h.names.push(("mdns".into(), n.clone()));
                }
                h.model = h.model.clone().or(m.model.clone());
            }
            if let Some((friendly, model, ty)) = upnp.get(&h.ip) {
                if let Some(f) = friendly {
                    h.names.push(("upnp".into(), f.clone()));
                }
                h.model = h.model.clone().or(model.clone());
                h.upnp_type = ty.clone();
            }
            if let Some(mac) = &h.mac {
                h.random_mac = crate::util::is_random_mac(mac);
                h.vendor = crate::oui::lookup(mac).map(str::to_string);
            }
            pick_name(&mut h);
            classify(&mut h);
            h
        })
        .collect();
    hosts.sort_by_key(|h| u32::from(h.ip));

    // 5. LLDP neighbours from SNMP-speaking devices.
    update(state, |r| {
        r.phase = "topology".into();
        r.progress = 0.96;
        r.hosts = hosts.clone();
    });
    let mut links = Vec::new();
    let snmp_hosts: Vec<Ipv4Addr> = hosts
        .iter()
        .filter(|h| h.snmp_name.is_some())
        .map(|h| h.ip)
        .collect();
    for host_ip in snmp_hosts {
        let ip = IpAddr::V4(host_ip);
        let names = snmp::walk(
            ip,
            &community,
            snmp::LLDP_REM_SYS_NAME,
            128,
            Duration::from_millis(800),
        )
        .await;
        let ports = snmp::walk(
            ip,
            &community,
            snmp::LLDP_REM_PORT_ID,
            128,
            Duration::from_millis(800),
        )
        .await;
        let locals = snmp::walk(
            ip,
            &community,
            snmp::LLDP_LOC_PORT_DESC,
            128,
            Duration::from_millis(800),
        )
        .await;
        let root_len = snmp::parse_oid(snmp::LLDP_REM_SYS_NAME).len();
        for (oid, value) in names {
            // Index: timeMark.localPortNum.remIndex
            let index = &oid[root_len..];
            let local_port_num = index.get(1).copied();
            let remote_port = ports
                .iter()
                .find(|(o, _)| o.ends_with(index))
                .and_then(|(_, v)| v.as_text());
            let local_port = local_port_num
                .and_then(|n| {
                    locals
                        .iter()
                        .find(|(o, _)| o.last() == Some(&n))
                        .and_then(|(_, v)| v.as_text())
                })
                .or(local_port_num.map(|n| format!("port {n}")));
            if let Some(neighbor) = value.as_text().filter(|s| !s.is_empty()) {
                let neighbor_ip = hosts
                    .iter()
                    .find(|x| {
                        x.snmp_name.as_deref() == Some(&neighbor)
                            || x.name.as_deref() == Some(&neighbor)
                    })
                    .map(|x| x.ip);
                links.push(Link {
                    from: host_ip,
                    local_port,
                    neighbor,
                    neighbor_ip,
                    remote_port,
                });
            }
        }
    }
    update(state, |r| {
        r.hosts = hosts;
        r.links = links;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(vendor: &str, ports: &[u16]) -> Host {
        Host {
            vendor: Some(vendor.into()),
            ports: ports
                .iter()
                .map(|p| OpenPort {
                    port: *p,
                    service: port_name(*p),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn classifies_common_devices() {
        let mut h = host("Hangzhou Hikvision Digital", &[80, 554]);
        classify(&mut h);
        assert_eq!(h.kind, DeviceKind::Camera);
        let mut h = host("Hewlett Packard", &[80, 631, 9100]);
        classify(&mut h);
        assert_eq!(h.kind, DeviceKind::Printer);
        let mut h = host("Synology", &[22, 80, 445, 5000]);
        classify(&mut h);
        assert_eq!(h.kind, DeviceKind::Nas);
        let mut h = host("Apple", &[62078]);
        classify(&mut h);
        assert_eq!(h.kind, DeviceKind::Phone);
        assert_eq!(h.os.as_deref(), Some("iOS"));
        let mut h = host("Dell", &[135, 139, 445, 3389]);
        classify(&mut h);
        assert_eq!(h.kind, DeviceKind::Computer);
        assert_eq!(h.os.as_deref(), Some("Windows"));
        let mut h = host("Espressif", &[80]);
        classify(&mut h);
        assert_eq!(h.kind, DeviceKind::Iot);
        let mut h = Host {
            random_mac: true,
            ..Default::default()
        };
        classify(&mut h);
        assert_eq!(h.kind, DeviceKind::Phone);
        let mut h = host("Ubiquiti", &[22]);
        h.is_gateway = true;
        classify(&mut h);
        assert_eq!(h.kind, DeviceKind::Router);
    }

    #[test]
    fn titles_and_urls() {
        assert_eq!(
            extract_title("<html><head><TITLE>\n HP LaserJet  M404 </title>").as_deref(),
            Some("HP LaserJet M404")
        );
        assert_eq!(
            url_host("http://192.168.1.5:49152/desc.xml").as_deref(),
            Some("192.168.1.5")
        );
    }
}
