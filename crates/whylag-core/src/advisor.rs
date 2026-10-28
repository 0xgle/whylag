//! The advisor: reads every measurement WhyLag has and explains, in plain
//! language, what is wrong, where, and what to do about it.
//!
//! It is a rule engine on purpose: each conclusion is deterministic, works
//! offline and can be traced back to the measurement that triggered it. The UI
//! translates codes and parameters into sentences (EN/PL).

use crate::discovery::DiscoveryRun;
use crate::monitor::{IncidentKind, MonitorStatus};
use crate::path::{PathRun, Segment};
use crate::perf::PerfRun;
use crate::rf::{self, Band};
use crate::security::SecurityReport;
use crate::settings::Settings;
use crate::util::now_ms;
use crate::wifi::{Bss, ScanResult};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Critical,
    Warning,
    Info,
    Good,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Advice {
    pub code: String,
    pub level: Level,
    /// "wifi", "internet", "router", "dns", "lan", "security"
    pub area: String,
    /// UI module that shows the evidence.
    pub module: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub ssid: Option<String>,
    pub rssi: Option<i32>,
    pub band: Option<Band>,
    pub channel: Option<u32>,
    pub link_rate_mbps: Option<f64>,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub bufferbloat: Option<String>,
    pub internet_latency_ms: Option<f64>,
    pub gateway_latency_ms: Option<f64>,
    pub devices: Option<usize>,
    pub security_score: Option<u32>,
    pub incidents_24h: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub grade: String,
    /// Headline code: what limits this network most.
    pub verdict: String,
    pub advice: Vec<Advice>,
    pub summary: Summary,
    pub checked: Vec<String>,
    pub generated_ms: u64,
}

pub struct Inputs<'a> {
    pub scan: Option<&'a ScanResult>,
    pub perf: Option<&'a PerfRun>,
    pub path: Option<&'a PathRun>,
    pub discovery: Option<&'a DiscoveryRun>,
    pub security: Option<&'a SecurityReport>,
    pub monitor: Option<&'a MonitorStatus>,
    pub settings: &'a Settings,
    /// The user's UTC offset in minutes, to tell evenings apart.
    pub tz_offset_min: i32,
}

fn add(out: &mut Vec<Advice>, code: &str, level: Level, area: &str, module: &str, params: Value) {
    out.push(Advice {
        code: code.into(),
        level,
        area: area.into(),
        module: module.into(),
        params,
    });
}

fn wifi_rules(scan: &ScanResult, out: &mut Vec<Advice>, s: &mut Summary) {
    let Some(conn) = scan.networks.iter().find(|n| n.connected) else {
        return;
    };
    let iface = scan.interfaces.iter().find(|i| i.state == "connected");
    let rssi = iface.and_then(|i| i.rssi).unwrap_or(conn.rssi);
    s.ssid = Some(conn.ssid.clone());
    s.rssi = Some(rssi);
    s.band = conn.band;
    s.channel = Some(conn.channel);
    s.link_rate_mbps = iface.and_then(|i| i.tx_rate_mbps);
    // Your network across bands (6 GHz is WPA3-only, 2.4/5 GHz often transition
    // mode), excluding open impostors that copy the name.
    let mine: Vec<&Bss> = scan
        .networks
        .iter()
        .filter(|n| {
            n.ssid == conn.ssid
                && !conn.ssid.is_empty()
                && (n.security == conn.security
                    || (!n.security.is_open() && !conn.security.is_open()))
        })
        .collect();

    if rssi < -75 {
        add(
            out,
            "weakSignal",
            Level::Critical,
            "wifi",
            "wifi",
            json!({ "rssi": rssi }),
        );
    } else if rssi < -67 {
        add(
            out,
            "fairSignal",
            Level::Warning,
            "wifi",
            "wifi",
            json!({ "rssi": rssi }),
        );
    } else {
        add(
            out,
            "goodSignal",
            Level::Good,
            "wifi",
            "wifi",
            json!({ "rssi": rssi }),
        );
    }
    if let Some(snr) = conn.snr() {
        if snr < 20 && rssi >= -75 {
            add(
                out,
                "lowSnr",
                Level::Warning,
                "wifi",
                "wifi",
                json!({ "snr": snr }),
            );
        }
    }
    // Sticky client: a much better AP for the same network is right there.
    if let Some(better) = mine
        .iter()
        .filter(|n| !n.connected && n.band != Some(Band::B24))
        .max_by_key(|n| n.rssi)
    {
        if better.rssi - rssi >= 12 && rssi < -62 {
            add(
                out,
                "stickyClient",
                Level::Warning,
                "wifi",
                "wifi",
                json!({ "rssi": rssi, "better": better.rssi, "bssid": better.bssid, "channel": better.channel }),
            );
        }
    }
    if conn.band == Some(Band::B24) {
        if let Some(five) = mine
            .iter()
            .filter(|n| n.band != Some(Band::B24))
            .max_by_key(|n| n.rssi)
        {
            if five.rssi >= -67 {
                add(
                    out,
                    "use5GHz",
                    Level::Warning,
                    "wifi",
                    "wifi",
                    json!({ "rssi5": five.rssi }),
                );
            }
        }
        let range = rf::occupied_range(conn.channel, Band::B24, conn.width_mhz);
        let crowd = scan
            .networks
            .iter()
            .filter(|n| {
                n.band == Some(Band::B24)
                    && !n.connected
                    && n.rssi >= -80
                    && !mine.iter().any(|m| m.bssid == n.bssid)
            })
            .filter(|n| rf::overlaps(rf::occupied_range(n.channel, Band::B24, n.width_mhz), range))
            .count();
        if crowd >= 4 {
            add(
                out,
                "crowded24",
                Level::Warning,
                "wifi",
                "wifi",
                json!({ "count": crowd, "channel": conn.channel }),
            );
        }
    } else if let Some(band) = conn.band {
        let range = rf::occupied_range(conn.channel, band, conn.width_mhz);
        let overlapping: Vec<&Bss> = scan
            .networks
            .iter()
            .filter(|n| {
                n.band == Some(band) && !n.connected && n.rssi >= -75 && n.ssid != conn.ssid
            })
            .filter(|n| rf::overlaps(rf::occupied_range(n.channel, band, n.width_mhz), range))
            .collect();
        if !overlapping.is_empty() {
            add(
                out,
                "coChannel5",
                Level::Info,
                "wifi",
                "wifi",
                json!({ "count": overlapping.len(), "channel": conn.channel }),
            );
        }
    }
    let bad_channels: Vec<u32> = mine
        .iter()
        .filter(|n| n.band == Some(Band::B24) && ![1, 6, 11].contains(&n.channel))
        .map(|n| n.channel)
        .collect();
    if !bad_channels.is_empty() {
        add(
            out,
            "nonStandard24",
            Level::Warning,
            "wifi",
            "survey",
            json!({ "channels": bad_channels }),
        );
    }
    if mine
        .iter()
        .any(|n| n.band == Some(Band::B24) && n.width_mhz >= 40)
    {
        add(out, "wide24", Level::Warning, "wifi", "survey", json!({}));
    }
    if let Some(u) = conn.utilization_pct.filter(|u| *u >= 60) {
        add(
            out,
            "busyChannel",
            Level::Warning,
            "wifi",
            "wifi",
            json!({ "pct": u, "channel": conn.channel }),
        );
    }
    let aps = mine
        .iter()
        .map(|n| &n.bssid[..n.bssid.len().min(14)])
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    if aps >= 2
        && mine
            .iter()
            .all(|n| !n.roaming.k && !n.roaming.v && !n.roaming.r)
    {
        add(
            out,
            "noFastRoaming",
            Level::Info,
            "wifi",
            "wifi",
            json!({ "aps": aps }),
        );
    }
    if let Some(rate) = s.link_rate_mbps.filter(|r| *r < 50.0) {
        add(
            out,
            "slowLink",
            Level::Warning,
            "wifi",
            "wifi",
            json!({ "rate": rate }),
        );
    }
}

fn perf_rules(perf: &PerfRun, settings: &Settings, out: &mut Vec<Advice>, s: &mut Summary) {
    let gw = perf.latency.iter().find(|l| l.id == "gateway");
    let inet: Vec<f64> = perf
        .latency
        .iter()
        .filter(|l| l.group == "internet")
        .filter_map(|l| l.median)
        .collect();
    s.gateway_latency_ms = gw.and_then(|g| g.avg);
    s.internet_latency_ms = crate::util::median(&inet);
    if let Some(g) = gw {
        if g.loss_pct >= 5.0 {
            add(
                out,
                "gatewayLoss",
                Level::Critical,
                "wifi",
                "performance",
                json!({ "loss": g.loss_pct }),
            );
        } else if g.avg.unwrap_or(0.0) > 10.0 || g.jitter.unwrap_or(0.0) > 8.0 {
            add(
                out,
                "gatewaySlow",
                Level::Warning,
                "wifi",
                "performance",
                json!({ "avg": g.avg, "jitter": g.jitter }),
            );
        }
    }
    let gw_ok = gw.map(|g| g.loss_pct < 2.0).unwrap_or(true);
    let lossy: Vec<&str> = perf
        .latency
        .iter()
        .filter(|l| l.group == "internet" && l.loss_pct >= 5.0)
        .map(|l| l.label.as_str())
        .collect();
    if gw_ok && lossy.len() >= 2 {
        add(
            out,
            "internetLoss",
            Level::Critical,
            "internet",
            "performance",
            json!({ "targets": lossy }),
        );
    }
    if let Some(med) = s.internet_latency_ms {
        if med > 80.0 {
            add(
                out,
                "highLatency",
                Level::Warning,
                "internet",
                "performance",
                json!({ "ms": med }),
            );
        }
    }
    // DNS: is the resolver in use much slower than the best public one?
    let system = perf
        .dns
        .iter()
        .find(|d| d.system)
        .and_then(|d| d.median_ms.map(|m| (d, m)));
    let best = perf
        .dns
        .iter()
        .filter(|d| !d.system)
        .filter_map(|d| d.median_ms.map(|m| (d, m)))
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    if let (Some((sys, sm)), Some((b, bm))) = (system, best) {
        if sm > 25.0 && sm > bm * 2.0 {
            add(
                out,
                "slowDns",
                Level::Warning,
                "dns",
                "performance",
                json!({ "server": sys.server, "ms": sm, "best": b.label, "bestIp": b.server, "bestMs": bm }),
            );
        } else {
            add(
                out,
                "dnsOk",
                Level::Good,
                "dns",
                "performance",
                json!({ "ms": sm }),
            );
        }
    }
    if perf.dns_hijack == Some(true) {
        add(
            out,
            "dnsHijack",
            Level::Warning,
            "dns",
            "performance",
            json!({}),
        );
    }
    if let Some(sp) = &perf.speed {
        s.download_mbps = sp.download_mbps;
        s.upload_mbps = sp.upload_mbps;
        s.bufferbloat = sp.grade.clone();
        let loaded = [sp.loaded_up_latency_ms, sp.loaded_down_latency_ms]
            .into_iter()
            .flatten()
            .reduce(f64::max);
        match sp.grade.as_deref() {
            Some("C") | Some("D") | Some("F") => add(
                out,
                "bufferbloat",
                if sp.grade.as_deref() == Some("C") {
                    Level::Warning
                } else {
                    Level::Critical
                },
                "router",
                "performance",
                json!({ "grade": sp.grade, "idle": sp.idle_latency_ms, "loaded": loaded }),
            ),
            Some(g) => add(
                out,
                "bufferbloatOk",
                Level::Good,
                "router",
                "performance",
                json!({ "grade": g }),
            ),
            None => {}
        }
        if let (Some(d), Some(plan)) = (sp.download_mbps, settings.contracted_down_mbps) {
            if d < plan * 0.5 {
                // Over Wi-Fi the radio link may be the cap, not the ISP.
                let link_cap = s.link_rate_mbps.map(|r| r * 0.6 < plan).unwrap_or(false);
                add(
                    out,
                    if link_cap {
                        "speedWifiCap"
                    } else {
                        "speedBelowPlan"
                    },
                    Level::Warning,
                    if link_cap { "wifi" } else { "internet" },
                    "performance",
                    json!({ "measured": d, "plan": plan, "pct": d / plan * 100.0, "rate": s.link_rate_mbps }),
                );
            }
        }
    }
}

fn path_rules(path: &PathRun, out: &mut Vec<Advice>) {
    for f in &path.findings {
        match f.code.as_str() {
            "lossFrom" => {
                let area = match f.segment {
                    Some(Segment::Lan) => "wifi",
                    Some(Segment::Isp) => "internet",
                    _ => "internet",
                };
                add(
                    out,
                    if f.segment == Some(Segment::Isp) {
                        "pathLossIsp"
                    } else if f.segment == Some(Segment::Lan) {
                        "pathLossLan"
                    } else {
                        "pathLossFar"
                    },
                    Level::Critical,
                    area,
                    "path",
                    json!({ "ttl": f.ttl, "loss": f.value, "target": path.target }),
                );
            }
            "cgnat" => add(out, "cgnat", Level::Info, "internet", "path", json!({})),
            "latencyJump" if f.segment == Some(Segment::Isp) => add(
                out,
                "ispLatency",
                Level::Warning,
                "internet",
                "path",
                json!({ "ttl": f.ttl, "ms": f.value }),
            ),
            _ => {}
        }
    }
}

fn monitor_rules(m: &MonitorStatus, tz_offset_min: i32, out: &mut Vec<Advice>, s: &mut Summary) {
    let since = now_ms().saturating_sub(24 * 3_600_000);
    let recent: Vec<_> = m
        .incidents
        .iter()
        .filter(|i| {
            i.start_ms >= since
                && !matches!(
                    i.kind,
                    IncidentKind::Roam | IncidentKind::WeakSignal | IncidentKind::DnsSlow
                )
        })
        .collect();
    s.incidents_24h = Some(recent.len());
    let count = |k: IncidentKind| recent.iter().filter(|i| i.kind == k).count();
    let isp_out = count(IncidentKind::IspOutage);
    let spikes = count(IncidentKind::LatencySpike);
    let local = count(IncidentKind::LocalOutage)
        + count(IncidentKind::LocalLoss)
        + count(IncidentKind::LocalLatency);
    if isp_out > 0 {
        add(
            out,
            "ispOutages",
            Level::Critical,
            "internet",
            "monitor",
            json!({ "count": isp_out }),
        );
    }
    if spikes >= 5 {
        // When do they happen? Evening congestion is a classic ISP capacity issue.
        let evening = recent
            .iter()
            .filter(|i| i.kind == IncidentKind::LatencySpike)
            .filter(|i| {
                let local_s = i.start_ms as i64 / 1000 + tz_offset_min as i64 * 60;
                local_s.rem_euclid(86_400) / 3600 >= 18
            })
            .count();
        add(
            out,
            if evening * 2 >= spikes {
                "eveningCongestion"
            } else {
                "latencySpikes"
            },
            Level::Warning,
            "internet",
            "monitor",
            json!({ "count": spikes }),
        );
    }
    if local >= 2 {
        add(
            out,
            "localDrops",
            Level::Warning,
            "wifi",
            "monitor",
            json!({ "count": local }),
        );
    }
}

/// Builds the diagnosis from whatever has been measured so far.
pub fn diagnose(inp: &Inputs) -> Diagnosis {
    let mut advice = Vec::new();
    let mut summary = Summary::default();
    let mut checked = Vec::new();
    if let Some(scan) = inp.scan {
        checked.push("wifi".to_string());
        wifi_rules(scan, &mut advice, &mut summary);
    }
    if let Some(p) = inp
        .perf
        .filter(|p| !p.latency.is_empty() || p.speed.is_some())
    {
        checked.push("performance".into());
        perf_rules(p, inp.settings, &mut advice, &mut summary);
    }
    if let Some(p) = inp.path.filter(|p| !p.hops.is_empty()) {
        checked.push("path".into());
        path_rules(p, &mut advice);
    }
    if let Some(d) = inp.discovery.filter(|d| !d.hosts.is_empty()) {
        checked.push("discovery".into());
        summary.devices = Some(d.hosts.len());
    }
    if let Some(sec) = inp.security {
        checked.push("security".into());
        summary.security_score = Some(sec.score);
        let serious = sec
            .findings
            .iter()
            .filter(|f| f.severity <= crate::security::Severity::High)
            .count();
        if serious > 0 {
            add(
                &mut advice,
                "securityIssues",
                Level::Critical,
                "security",
                "security",
                json!({ "count": serious, "score": sec.score }),
            );
        } else {
            add(
                &mut advice,
                "securityOk",
                Level::Good,
                "security",
                "security",
                json!({ "score": sec.score }),
            );
        }
    }
    if let Some(m) = inp.monitor.filter(|m| m.sample_count > 0) {
        checked.push("monitor".into());
        monitor_rules(m, inp.tz_offset_min, &mut advice, &mut summary);
    }
    advice.sort_by_key(|a| a.level);

    // Grade: start from A and step down per problem.
    let crit = advice.iter().filter(|a| a.level == Level::Critical).count();
    let warn = advice.iter().filter(|a| a.level == Level::Warning).count();
    let points = crit * 2 + warn;
    let grade = match points {
        0 => "A",
        1..=2 => "B",
        3..=5 => "C",
        6..=9 => "D",
        _ => "F",
    }
    .to_string();
    // Verdict: the area with the heaviest problems.
    let mut areas: Vec<(&str, usize)> = Vec::new();
    for a in advice.iter().filter(|a| a.level <= Level::Warning) {
        let w = if a.level == Level::Critical { 3 } else { 1 };
        match areas.iter_mut().find(|(k, _)| *k == a.area) {
            Some(e) => e.1 += w,
            None => areas.push((a.area.as_str(), w)),
        }
    }
    areas.sort_by(|a, b| b.1.cmp(&a.1));
    let verdict = match areas.first().map(|a| a.0) {
        None if checked.is_empty() => "notChecked",
        None => "allGood",
        Some("wifi") => "wifiLimits",
        Some("internet") => "ispLimits",
        Some("router") => "routerLimits",
        Some("dns") => "dnsLimits",
        Some("security") => "securityRisk",
        Some(_) => "mixed",
    }
    .to_string();
    Diagnosis {
        grade,
        verdict,
        advice,
        summary,
        checked,
        generated_ms: now_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::SimWorld;

    #[test]
    fn demo_office_diagnosis() {
        let mut world = SimWorld::new();
        let scan = world.wifi_scan();
        let settings = Settings {
            contracted_down_mbps: Some(600.0),
            ..Default::default()
        };
        let perf = PerfRun {
            latency: vec![crate::perf::LatencyStats::from_samples(
                "gateway",
                "gw",
                "192.168.10.1",
                "lan",
                None,
                vec![Some(2.0); 20],
            )],
            dns: vec![
                crate::perf::DnsResult {
                    server: "192.168.10.1".into(),
                    label: "192.168.10.1".into(),
                    system: true,
                    median_ms: Some(41.0),
                    succeeded: 8,
                    total: 8,
                },
                crate::perf::DnsResult {
                    server: "1.1.1.1".into(),
                    label: "Cloudflare".into(),
                    system: false,
                    median_ms: Some(10.0),
                    succeeded: 8,
                    total: 8,
                },
            ],
            speed: Some(crate::perf::SpeedResult {
                download_mbps: Some(280.0),
                upload_mbps: Some(37.0),
                idle_latency_ms: Some(12.0),
                loaded_up_latency_ms: Some(212.0),
                grade: Some("C".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let d = diagnose(&Inputs {
            scan: Some(&scan),
            perf: Some(&perf),
            path: None,
            discovery: None,
            security: None,
            monitor: None,
            settings: &settings,
            tz_offset_min: 60,
        });
        let codes: Vec<&str> = d.advice.iter().map(|a| a.code.as_str()).collect();
        assert!(codes.contains(&"slowDns"), "{codes:?}");
        assert!(codes.contains(&"bufferbloat"), "{codes:?}");
        assert!(
            codes.contains(&"speedBelowPlan") || codes.contains(&"speedWifiCap"),
            "{codes:?}"
        );
        assert!(codes.contains(&"wide24"), "{codes:?}");
        assert!(d.advice.windows(2).all(|w| w[0].level <= w[1].level));
        assert_ne!(d.grade, "A");
        let empty = diagnose(&Inputs {
            scan: None,
            perf: None,
            path: None,
            discovery: None,
            security: None,
            monitor: None,
            settings: &settings,
            tz_offset_min: 60,
        });
        assert_eq!(empty.verdict, "notChecked");
    }
}
