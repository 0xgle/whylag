//! Converts raw Android `WifiManager` results (from the Kotlin plugin) into
//! WhyLag scan results. Pure parsing, so it is tested on every platform.

use super::{Bss, Notice, Pmf, ScanResult, ScanSource, Security, WifiInterface};
use crate::rf;
use crate::util::now_ms;
use serde_json::Value;

/// Security from the capabilities string, e.g. `[WPA2-PSK-CCMP][RSN-SAE-CCMP][ESS]`.
pub fn security(caps: &str) -> Security {
    let c = caps.to_ascii_uppercase();
    let sae = c.contains("SAE");
    let psk = c.contains("PSK");
    let eap = c.contains("EAP");
    let owe = c.contains("OWE");
    if c.contains("EAP_SUITE_B") || (eap && c.contains("WPA3")) {
        Security::Wpa3Enterprise
    } else if eap {
        Security::Wpa2Enterprise
    } else if sae && psk {
        Security::Wpa2Wpa3
    } else if sae {
        Security::Wpa3
    } else if owe {
        Security::Owe
    } else if psk && (c.contains("RSN") || c.contains("WPA2")) {
        Security::Wpa2
    } else if psk {
        Security::Wpa
    } else if c.contains("WEP") {
        Security::Wep
    } else {
        Security::Open
    }
}

/// `ScanResult.channelWidth` constants.
fn width(code: i64) -> u32 {
    match code {
        1 => 40,
        2 => 80,
        3 | 4 => 160,
        5 => 320,
        _ => 20,
    }
}

/// `ScanResult.wifiStandard` (API 30+) to Wi-Fi generation.
fn generation(code: i64) -> Option<u8> {
    match code {
        4 => Some(4),
        5 => Some(5),
        6 => Some(6),
        8 => Some(7),
        _ => None,
    }
}

pub fn parse(raw: &Value) -> ScanResult {
    let now = now_ms();
    let now_us = raw.get("nowUs").and_then(Value::as_i64);
    let mut networks = Vec::new();
    for n in raw
        .get("networks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let bssid = n
            .get("bssid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();
        if bssid.is_empty() || bssid == "02:00:00:00:00:00" {
            continue; // no location permission: Android hides the real BSSID
        }
        let ssid = n
            .get("ssid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let freq = n.get("frequency").and_then(Value::as_i64).unwrap_or(0) as u32;
        let (channel, band) = rf::freq_to_channel(freq)
            .map(|(c, b)| (c, Some(b)))
            .unwrap_or((0, None));
        let caps = n
            .get("capabilities")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let sec = security(caps);
        // Scan entries carry boot-relative microseconds: age them against "now".
        let age_ms = match (now_us, n.get("timestampUs").and_then(Value::as_i64)) {
            (Some(a), Some(b)) if a >= b => ((a - b) / 1000) as u64,
            _ => 0,
        };
        networks.push(Bss {
            bssid,
            hidden: ssid.is_empty(),
            ssid,
            rssi: n.get("level").and_then(Value::as_i64).unwrap_or(-100) as i32,
            channel,
            frequency_mhz: freq,
            band,
            width_mhz: width(n.get("channelWidth").and_then(Value::as_i64).unwrap_or(0)),
            wifi_gen: n
                .get("wifiStandard")
                .and_then(Value::as_i64)
                .and_then(generation),
            security: sec,
            weak_cipher: caps.contains("TKIP") || matches!(sec, Security::Wep | Security::Wpa),
            pmf: if matches!(
                sec,
                Security::Wpa3 | Security::Wpa3Enterprise | Security::Owe
            ) {
                Pmf::Required
            } else {
                Pmf::Unknown
            },
            wps: caps.contains("WPS"),
            last_seen_ms: now.saturating_sub(age_ms),
            ..Default::default()
        });
    }
    let mut interfaces = Vec::new();
    let enabled = raw.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    let connected = raw.get("connected");
    let iface = match connected {
        Some(c) => {
            let freq = c.get("frequency").and_then(Value::as_i64).unwrap_or(0) as u32;
            let ch = rf::freq_to_channel(freq);
            WifiInterface {
                id: "wlan0".into(),
                name: "Wi-Fi".into(),
                description: "Android Wi-Fi".into(),
                state: "connected".into(),
                ssid: c.get("ssid").and_then(Value::as_str).map(str::to_string),
                bssid: c
                    .get("bssid")
                    .and_then(Value::as_str)
                    .map(str::to_lowercase),
                rssi: c.get("rssi").and_then(Value::as_i64).map(|v| v as i32),
                channel: ch.map(|(c, _)| c),
                frequency_mhz: Some(freq),
                band: ch.map(|(_, b)| b),
                tx_rate_mbps: c
                    .get("txLinkSpeedMbps")
                    .or(c.get("linkSpeedMbps"))
                    .and_then(Value::as_f64),
                rx_rate_mbps: c.get("rxLinkSpeedMbps").and_then(Value::as_f64),
                ..Default::default()
            }
        }
        None => WifiInterface {
            id: "wlan0".into(),
            name: "Wi-Fi".into(),
            description: "Android Wi-Fi".into(),
            state: if enabled { "disconnected" } else { "off" }.into(),
            ..Default::default()
        },
    };
    interfaces.push(iface);
    let mut notices = Vec::new();
    if !enabled {
        notices.push(Notice {
            code: "wifiOff".into(),
            detail: String::new(),
        });
    }
    ScanResult {
        source: ScanSource::Android,
        interfaces,
        networks,
        timestamp_ms: now,
        notices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_android_scan() {
        let raw = json!({
            "nowUs": 10_000_000i64,
            "enabled": true,
            "networks": [
                { "bssid": "AA:BB:CC:00:11:22", "ssid": "Office", "level": -52, "frequency": 5180, "channelWidth": 2, "capabilities": "[WPA2-PSK-CCMP][RSN-SAE-CCMP][ESS]", "timestampUs": 9_000_000i64, "wifiStandard": 6 },
                { "bssid": "02:00:00:00:00:00", "ssid": "", "level": -70, "frequency": 2412 },
                { "bssid": "aa:bb:cc:00:11:33", "ssid": "Cafe", "level": -80, "frequency": 2437, "channelWidth": 0, "capabilities": "[ESS]" }
            ],
            "connected": { "bssid": "aa:bb:cc:00:11:22", "ssid": "Office", "rssi": -51, "frequency": 5180, "linkSpeedMbps": 866 }
        });
        let r = parse(&raw);
        assert_eq!(r.networks.len(), 2, "hidden-BSSID entries are dropped");
        let office = &r.networks[0];
        assert_eq!(
            (office.channel, office.width_mhz, office.wifi_gen),
            (36, 80, Some(6))
        );
        assert_eq!(office.security, Security::Wpa2Wpa3);
        assert_eq!(r.networks[1].security, Security::Open);
        assert_eq!(r.interfaces[0].state, "connected");
        assert_eq!(r.interfaces[0].channel, Some(36));
        assert_eq!(r.interfaces[0].tx_rate_mbps, Some(866.0));
    }

    #[test]
    fn maps_security_strings() {
        assert_eq!(security("[WPA2-EAP-CCMP][ESS]"), Security::Wpa2Enterprise);
        assert_eq!(security("[RSN-SAE-CCMP][ESS]"), Security::Wpa3);
        assert_eq!(security("[WPA-PSK-TKIP][ESS]"), Security::Wpa);
        assert_eq!(security("[WEP][ESS]"), Security::Wep);
        assert_eq!(security("[RSN-OWE-CCMP][ESS]"), Security::Owe);
    }
}
