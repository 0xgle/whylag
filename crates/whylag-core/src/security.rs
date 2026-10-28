//! Security assessment: Wi-Fi configuration, impostor access points, risky
//! services on the local network and DNS tampering, combined into a score.

use crate::discovery::{DeviceKind, DiscoveryRun};
use crate::perf::PerfRun;
use crate::util::now_ms;
use crate::wifi::{Bss, Pmf, ScanResult, Security};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl Severity {
    fn penalty(self) -> u32 {
        match self {
            Severity::Critical => 25,
            Severity::High => 12,
            Severity::Medium => 6,
            Severity::Low => 2,
            Severity::Info => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub code: String,
    pub severity: Severity,
    /// "wifi", "lan" or "internet"
    pub area: String,
    /// What the finding is about (SSID, BSSID, IP, device name).
    pub subject: Option<String>,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityReport {
    pub score: u32,
    pub findings: Vec<Finding>,
    pub checked_wifi: bool,
    pub checked_lan: bool,
    pub checked_dns: bool,
    pub generated_ms: u64,
}

fn finding(
    code: &str,
    severity: Severity,
    area: &str,
    subject: Option<String>,
    params: Value,
) -> Finding {
    Finding {
        code: code.into(),
        severity,
        area: area.into(),
        subject,
        params,
    }
}

/// Same physical AP family: BSSIDs share the first five octets (virtual SSIDs).
fn same_radio(a: &str, b: &str) -> bool {
    a.len() >= 14 && b.len() >= 14 && a[..14].eq_ignore_ascii_case(&b[..14])
}

pub fn wifi_findings(scan: &ScanResult) -> Vec<Finding> {
    let mut out = Vec::new();
    let Some(conn) = scan.networks.iter().find(|n| n.connected) else {
        return out;
    };
    let ssid = conn.ssid.clone();
    let mine: Vec<&Bss> = scan
        .networks
        .iter()
        .filter(|n| n.ssid == ssid && !n.hidden)
        .collect();
    let sub = Some(ssid.clone());
    match conn.security {
        Security::Open => out.push(finding(
            "wifiOpen",
            Severity::Critical,
            "wifi",
            sub.clone(),
            json!({}),
        )),
        Security::Wep => out.push(finding(
            "wifiWep",
            Severity::Critical,
            "wifi",
            sub.clone(),
            json!({}),
        )),
        Security::Wpa | Security::WpaEnterprise => out.push(finding(
            "wifiWpa1",
            Severity::High,
            "wifi",
            sub.clone(),
            json!({}),
        )),
        Security::Owe => out.push(finding(
            "wifiOwe",
            Severity::Info,
            "wifi",
            sub.clone(),
            json!({}),
        )),
        _ => {}
    }
    if conn.weak_cipher && !matches!(conn.security, Security::Wep | Security::Wpa) {
        out.push(finding(
            "wifiTkip",
            Severity::High,
            "wifi",
            sub.clone(),
            json!({}),
        ));
    }
    if conn.pmf == Pmf::Disabled
        && matches!(conn.security, Security::Wpa2 | Security::Wpa2Enterprise)
    {
        out.push(finding(
            "wifiNoPmf",
            Severity::Medium,
            "wifi",
            sub.clone(),
            json!({}),
        ));
    }
    if matches!(conn.security, Security::Wpa2)
        && !mine
            .iter()
            .any(|n| matches!(n.security, Security::Wpa2Wpa3 | Security::Wpa3))
    {
        out.push(finding(
            "wifiNoWpa3",
            Severity::Low,
            "wifi",
            sub.clone(),
            json!({}),
        ));
    }
    if mine.iter().any(|n| n.wps) {
        out.push(finding(
            "wifiWps",
            Severity::Medium,
            "wifi",
            sub.clone(),
            json!({}),
        ));
    }
    // Impostors: your SSID advertised by hardware that is not yours.
    let my_vendors: Vec<Option<String>> = mine
        .iter()
        .filter(|n| n.security == conn.security || n.connected)
        .map(|n| n.vendor.clone())
        .collect();
    for n in scan
        .networks
        .iter()
        .filter(|n| n.ssid == ssid && !n.connected)
    {
        let weaker = n.security.strength() < conn.security.strength();
        let foreign = !my_vendors.contains(&n.vendor)
            && !mine
                .iter()
                .any(|m| m.connected && same_radio(&m.bssid, &n.bssid));
        if weaker && n.security.is_open() {
            out.push(finding(
                "evilTwin",
                Severity::High,
                "wifi",
                Some(n.bssid.clone()),
                json!({ "ssid": ssid, "vendor": n.vendor, "channel": n.channel, "rssi": n.rssi }),
            ));
        } else if foreign && n.vendor.is_some() {
            out.push(finding(
                "rogueAp",
                Severity::Medium,
                "wifi",
                Some(n.bssid.clone()),
                json!({ "ssid": ssid, "vendor": n.vendor, "channel": n.channel }),
            ));
        }
    }
    // An unencrypted SSID served by your own APs: usually a guest network.
    let guests: Vec<String> = scan
        .networks
        .iter()
        .filter(|n| {
            n.security == Security::Open
                && n.ssid != ssid
                && !n.hidden
                && mine.iter().any(|m| same_radio(&m.bssid, &n.bssid))
        })
        .map(|n| n.ssid.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    for g in guests {
        out.push(finding(
            "guestOpen",
            Severity::Low,
            "wifi",
            Some(g),
            json!({}),
        ));
    }
    let wep_nearby = scan
        .networks
        .iter()
        .filter(|n| n.security == Security::Wep && n.ssid != ssid)
        .count();
    if wep_nearby > 0 {
        out.push(finding(
            "wepNearby",
            Severity::Info,
            "wifi",
            None,
            json!({ "count": wep_nearby }),
        ));
    }
    out
}

pub fn lan_findings(run: &DiscoveryRun) -> Vec<Finding> {
    let mut out = Vec::new();
    let label = |h: &crate::discovery::Host| h.name.clone().unwrap_or_else(|| h.ip.to_string());
    let mut cameras = 0;
    for h in run.hosts.iter().filter(|h| !h.is_self) {
        let has = |p: u16| h.ports.iter().any(|x| x.port == p);
        let params = json!({ "ip": h.ip.to_string(), "name": h.name, "kind": h.kind });
        if has(23) {
            out.push(finding(
                "telnet",
                Severity::High,
                "lan",
                Some(label(h)),
                params.clone(),
            ));
        }
        if has(21) {
            out.push(finding(
                "ftp",
                Severity::Medium,
                "lan",
                Some(label(h)),
                params.clone(),
            ));
        }
        if has(5900) {
            out.push(finding(
                "vnc",
                Severity::Medium,
                "lan",
                Some(label(h)),
                params.clone(),
            ));
        }
        if has(3389) {
            out.push(finding(
                "rdp",
                Severity::Low,
                "lan",
                Some(label(h)),
                params.clone(),
            ));
        }
        if has(1883) {
            out.push(finding(
                "mqtt",
                Severity::Medium,
                "lan",
                Some(label(h)),
                params.clone(),
            ));
        }
        if has(3306) {
            out.push(finding(
                "database",
                Severity::Medium,
                "lan",
                Some(label(h)),
                params.clone(),
            ));
        }
        if h.kind == DeviceKind::Camera {
            cameras += 1;
            if has(80) && !has(443) {
                out.push(finding(
                    "cameraHttp",
                    Severity::Low,
                    "lan",
                    Some(label(h)),
                    params.clone(),
                ));
            }
        }
        if h.is_gateway {
            if has(80) && !has(443) {
                out.push(finding(
                    "routerHttp",
                    Severity::Low,
                    "lan",
                    Some(label(h)),
                    params.clone(),
                ));
            }
            if h.upnp_type
                .as_deref()
                .map(|t| t.contains("InternetGatewayDevice"))
                .unwrap_or(false)
            {
                out.push(finding(
                    "upnp",
                    Severity::Medium,
                    "lan",
                    Some(label(h)),
                    params.clone(),
                ));
            }
        }
    }
    if cameras > 0 {
        out.push(finding(
            "camerasPresent",
            Severity::Info,
            "lan",
            None,
            json!({ "count": cameras }),
        ));
    }
    let unknown = run
        .hosts
        .iter()
        .filter(|h| h.kind == DeviceKind::Unknown && h.name.is_none())
        .count();
    if unknown > 0 {
        out.push(finding(
            "unknownDevices",
            Severity::Info,
            "lan",
            None,
            json!({ "count": unknown }),
        ));
    }
    out
}

pub fn report(
    scan: Option<&ScanResult>,
    discovery: Option<&DiscoveryRun>,
    perf: Option<&PerfRun>,
) -> SecurityReport {
    let mut findings = Vec::new();
    if let Some(s) = scan {
        findings.extend(wifi_findings(s));
    }
    let lan_checked = discovery
        .map(|d| !d.hosts.is_empty() && !d.running)
        .unwrap_or(false);
    if lan_checked {
        findings.extend(lan_findings(discovery.unwrap()));
    }
    let dns_checked = perf.and_then(|p| p.dns_hijack).is_some();
    if perf.and_then(|p| p.dns_hijack) == Some(true) {
        findings.push(finding(
            "dnsHijack",
            Severity::Medium,
            "internet",
            None,
            json!({}),
        ));
    }
    findings.sort_by_key(|f| f.severity);
    let penalty: u32 = findings.iter().map(|f| f.severity.penalty()).sum();
    SecurityReport {
        score: 100u32.saturating_sub(penalty),
        findings,
        checked_wifi: scan
            .map(|s| s.networks.iter().any(|n| n.connected))
            .unwrap_or(false),
        checked_lan: lan_checked,
        checked_dns: dns_checked,
        generated_ms: now_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::SimWorld;

    #[test]
    fn finds_the_demo_office_issues() {
        let mut world = SimWorld::new();
        world.set_position_px(20.0 * 17.0, 20.0 * 23.0); // near the evil twin in open space B
        let scan = world.wifi_scan();
        let f = wifi_findings(&scan);
        let codes: Vec<&str> = f.iter().map(|x| x.code.as_str()).collect();
        assert!(codes.contains(&"evilTwin"), "{codes:?}");
        assert!(codes.contains(&"guestOpen"), "{codes:?}");
        assert!(
            codes.contains(&"wepNearby")
                || !scan.networks.iter().any(|n| n.security == Security::Wep),
            "{codes:?}"
        );
        let lan = DiscoveryRun {
            hosts: crate::sim::lan::hosts(),
            ..Default::default()
        };
        let codes: Vec<String> = lan_findings(&lan).into_iter().map(|f| f.code).collect();
        assert!(codes.contains(&"telnet".to_string()));
        assert!(codes.contains(&"camerasPresent".to_string()));
        assert!(codes.contains(&"vnc".to_string()));
        let r = report(Some(&scan), Some(&lan), None);
        assert!(r.score < 80 && r.score > 0, "{}", r.score);
        assert!(r
            .findings
            .windows(2)
            .all(|w| w[0].severity <= w[1].severity));
    }
}
