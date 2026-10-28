//! macOS scanner based on `system_profiler SPAirPortDataType -json`.
//!
//! Apple no longer exposes BSSIDs to unsigned tools and redacts SSIDs unless the
//! app has Location Services permission, so the results are coarser than on
//! Windows or Linux. Each network gets a stable synthetic identifier instead.

use super::{Bss, Notice, ScanResult, ScanSource, Security, WifiInterface};
use crate::error::{ApiError, ApiResult};
use crate::rf::{self, Band};
use crate::util::{now_ms, run_tool};
use serde_json::Value;
use std::time::Duration;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn scan() -> ApiResult<ScanResult> {
    let out = run_tool("system_profiler", &["SPAirPortDataType", "-json"], Duration::from_secs(20))
        .map_err(|e| ApiError::new("noWifiTool", e.to_string()))?;
    parse(&out)
}

fn security(mode: &str) -> Security {
    let m = mode.to_lowercase();
    if m.contains("none") || m.contains("open") {
        Security::Open
    } else if m.contains("owe") {
        Security::Owe
    } else if m.contains("wep") {
        Security::Wep
    } else if m.contains("enterprise") && m.contains("wpa3") {
        Security::Wpa3Enterprise
    } else if m.contains("enterprise") {
        Security::Wpa2Enterprise
    } else if m.contains("wpa2") && m.contains("wpa3") {
        Security::Wpa2Wpa3
    } else if m.contains("wpa3") {
        Security::Wpa3
    } else if m.contains("wpa2") {
        Security::Wpa2
    } else if m.contains("wpa") {
        Security::Wpa
    } else {
        Security::Unknown
    }
}

/// Parses "36 (5GHz, 80MHz)" into (channel, band, width).
fn channel(s: &str) -> (u32, Option<Band>, u32) {
    let ch = s.split_whitespace().next().and_then(|c| c.parse().ok()).unwrap_or(0);
    let band = if s.contains("6GHz") {
        Some(Band::B6)
    } else if s.contains("5GHz") {
        Some(Band::B5)
    } else if s.contains("2GHz") || ch <= 14 {
        Some(Band::B24)
    } else {
        Some(Band::B5)
    };
    let width = s
        .split([',', '(', ')'])
        .filter_map(|p| p.trim().strip_suffix("MHz"))
        .filter_map(|p| p.trim().parse().ok())
        .next()
        .unwrap_or(20);
    (ch, band, width)
}

fn signal_noise(s: &str) -> (Option<i32>, Option<i32>) {
    let nums: Vec<i32> = s
        .split('/')
        .filter_map(|p| p.trim().split_whitespace().next().and_then(|n| n.parse().ok()))
        .collect();
    (nums.first().copied(), nums.get(1).copied())
}

fn phy_gen(phy: &str) -> Option<u8> {
    if phy.contains("be") {
        Some(7)
    } else if phy.contains("ax") {
        Some(6)
    } else if phy.contains("ac") {
        Some(5)
    } else if phy.contains('n') {
        Some(4)
    } else {
        None
    }
}

fn to_bss(net: &Value, connected: bool) -> Option<Bss> {
    let name = net.get("_name")?.as_str().unwrap_or_default().to_string();
    let (ch, band, width) = channel(net.get("spairport_network_channel").and_then(Value::as_str).unwrap_or(""));
    let (rssi, noise) = signal_noise(net.get("spairport_signal_noise").and_then(Value::as_str).unwrap_or(""));
    let phy = net.get("spairport_network_phymode").and_then(Value::as_str).unwrap_or("");
    let freq = band.map(|b| rf::channel_to_freq(ch, b)).unwrap_or(0);
    // Stable pseudo-identifier: there is no BSSID on modern macOS.
    let bssid = net
        .get("spairport_network_bssid")
        .and_then(Value::as_str)
        .map(str::to_lowercase)
        .unwrap_or_else(|| format!("mac:{}:{}", name, ch));
    Some(Bss {
        bssid,
        hidden: name.is_empty(),
        ssid: name,
        rssi: rssi.unwrap_or(-100),
        noise,
        channel: ch,
        frequency_mhz: freq,
        band,
        width_mhz: width,
        wifi_gen: phy_gen(phy),
        security: security(net.get("spairport_security_mode").and_then(Value::as_str).unwrap_or("")),
        connected,
        last_seen_ms: now_ms(),
        ..Default::default()
    })
}

pub(crate) fn parse(json: &str) -> ApiResult<ScanResult> {
    let root: Value = serde_json::from_str(json).map_err(|e| ApiError::new("parse", e.to_string()))?;
    let mut networks = Vec::new();
    let mut interfaces = Vec::new();
    let entries = root.get("SPAirPortDataType").and_then(Value::as_array).cloned().unwrap_or_default();
    for entry in entries {
        for iface in entry.get("spairport_airport_interfaces").and_then(Value::as_array).cloned().unwrap_or_default() {
            let name = iface.get("_name").and_then(Value::as_str).unwrap_or("en0").to_string();
            if !name.starts_with("en") {
                continue;
            }
            let mut info = WifiInterface {
                id: name.clone(),
                name,
                description: iface.get("spairport_wireless_card_type").and_then(Value::as_str).unwrap_or("Wi-Fi").to_string(),
                state: "disconnected".into(),
                ..Default::default()
            };
            if let Some(cur) = iface.get("spairport_current_network_information") {
                if let Some(bss) = to_bss(cur, true) {
                    info.state = "connected".into();
                    info.ssid = Some(bss.ssid.clone());
                    info.bssid = Some(bss.bssid.clone());
                    info.rssi = Some(bss.rssi);
                    info.channel = Some(bss.channel);
                    info.frequency_mhz = Some(bss.frequency_mhz);
                    info.band = bss.band;
                    info.security = Some(bss.security);
                    info.wifi_gen = bss.wifi_gen;
                    info.phy = cur.get("spairport_network_phymode").and_then(Value::as_str).map(str::to_string);
                    info.tx_rate_mbps = cur.get("spairport_network_rate").and_then(Value::as_f64);
                    networks.push(bss);
                }
            }
            for net in iface.get("spairport_airport_other_local_wireless_networks").and_then(Value::as_array).cloned().unwrap_or_default() {
                if let Some(bss) = to_bss(&net, false) {
                    networks.push(bss);
                }
            }
            interfaces.push(info);
        }
    }
    let mut notices = vec![Notice { code: "macosLimited".into(), detail: String::new() }];
    if networks.iter().any(|n| n.ssid.is_empty() || n.ssid == "<redacted>") {
        notices.push(Notice { code: "locationPermission".into(), detail: String::new() });
    }
    Ok(ScanResult { source: ScanSource::Macos, interfaces, networks, timestamp_ms: now_ms(), notices })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_system_profiler() {
        let json = r#"{"SPAirPortDataType":[{"spairport_airport_interfaces":[{"_name":"en0",
          "spairport_current_network_information":{"_name":"Acme-Office","spairport_network_channel":"36 (5GHz, 80MHz)",
            "spairport_network_phymode":"802.11ax","spairport_network_rate":864,
            "spairport_security_mode":"spairport_security_mode_wpa2_wpa3_personal","spairport_signal_noise":"-52 dBm / -93 dBm"},
          "spairport_airport_other_local_wireless_networks":[{"_name":"UPC2471139","spairport_network_channel":"6 (2GHz, 20MHz)",
            "spairport_network_phymode":"802.11b/g/n","spairport_security_mode":"spairport_security_mode_wpa2_personal",
            "spairport_signal_noise":"-70 dBm / -94 dBm"}]}]}]}"#;
        let r = parse(json).unwrap();
        assert_eq!(r.networks.len(), 2);
        assert_eq!(r.networks[0].security, Security::Wpa2Wpa3);
        assert_eq!(r.networks[0].width_mhz, 80);
        assert_eq!(r.networks[0].noise, Some(-93));
        assert_eq!(r.networks[1].band, Some(Band::B24));
        assert_eq!(r.interfaces[0].tx_rate_mbps, Some(864.0));
    }
}
