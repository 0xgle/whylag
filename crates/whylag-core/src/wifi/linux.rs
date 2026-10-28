//! Linux scanner: `iw` (full detail, real dBm) with a NetworkManager fallback.

use super::ie::IeInfo;
use super::{quality_to_dbm, Bss, Notice, ScanResult, ScanSource, Security, WifiInterface};
use crate::error::{ApiError, ApiResult};
use crate::rf;
use crate::util::{now_ms, run_tool};
use std::time::Duration;

const T: Duration = Duration::from_secs(8);

pub fn scan(trigger: bool) -> ApiResult<ScanResult> {
    let ifaces = wifi_interfaces();
    if ifaces.is_empty() {
        // No `iw` or no wireless device: try NetworkManager directly.
        return nmcli_scan(trigger);
    }
    // Ask NetworkManager for a fresh scan (unprivileged); `iw scan trigger` needs root.
    if trigger {
        let _ = run_tool(
            "nmcli",
            &["device", "wifi", "rescan"],
            Duration::from_secs(3),
        );
    }
    let mut networks = Vec::new();
    let mut interfaces = Vec::new();
    for name in &ifaces {
        if let Ok(out) = run_tool("iw", &["dev", name, "scan", "dump"], T) {
            networks.extend(parse_iw_scan(&out));
        }
        let link = run_tool("iw", &["dev", name, "link"], T).unwrap_or_default();
        interfaces.push(parse_iw_link(name, &link));
    }
    if networks.is_empty() {
        if let Ok(mut nm) = nmcli_scan(false) {
            nm.interfaces = interfaces;
            return Ok(nm);
        }
    }
    Ok(ScanResult {
        source: ScanSource::LinuxIw,
        interfaces,
        networks,
        timestamp_ms: now_ms(),
        notices: vec![],
    })
}

fn wifi_interfaces() -> Vec<String> {
    run_tool("iw", &["dev"], T)
        .map(|out| {
            out.lines()
                .filter_map(|l| l.trim().strip_prefix("Interface "))
                .map(|s| s.trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn akm_from_name(name: &str) -> Option<u8> {
    Some(match name {
        "IEEE 802.1X" => 1,
        "PSK" => 2,
        "FT/IEEE 802.1X" => 3,
        "FT/PSK" => 4,
        "IEEE 802.1X/SHA-256" => 5,
        "PSK/SHA-256" => 6,
        "SAE" => 8,
        "FT/SAE" => 9,
        "IEEE 802.1X/SUITE-B" => 11,
        "IEEE 802.1X/SUITE-B-192" => 12,
        "OWE" => 18,
        "SAE-EXT-KEY" => 24,
        _ => return None,
    })
}

/// Parses `iw dev <if> scan dump` output.
pub(crate) fn parse_iw_scan(out: &str) -> Vec<Bss> {
    let mut result = Vec::new();
    let mut cur: Option<(Bss, IeInfo, bool)> = None;
    let mut section = "";
    let finish = |item: Option<(Bss, IeInfo, bool)>, result: &mut Vec<Bss>| {
        if let Some((mut b, info, privacy)) = item {
            b.security = info.security(privacy);
            b.weak_cipher = info.weak_cipher(b.security);
            b.pmf = info.pmf();
            b.roaming = info.roaming();
            b.wifi_gen = info.wifi_gen();
            b.width_mhz = info.width_mhz.max(20);
            b.wps = info.wps;
            if b.channel == 0 {
                if let Some((ch, band)) = rf::freq_to_channel(b.frequency_mhz) {
                    b.channel = ch;
                    b.band = Some(band);
                }
            }
            result.push(b);
        }
    };
    for raw in out.lines() {
        let line = raw.trim();
        if let Some(rest) = raw.strip_prefix("BSS ") {
            finish(cur.take(), &mut result);
            let mac = rest.split(['(', ' ']).next().unwrap_or("").to_lowercase();
            let bss = Bss {
                bssid: mac,
                connected: rest.contains("associated"),
                last_seen_ms: now_ms(),
                ..Default::default()
            };
            cur = Some((bss, IeInfo::default(), false));
            section = "";
            continue;
        }
        let Some((bss, info, privacy)) = cur.as_mut() else {
            continue;
        };
        if !raw.starts_with("\t\t") && !raw.starts_with("\t *") {
            section = "";
        }
        if let Some(v) = line.strip_prefix("freq:") {
            bss.frequency_mhz = v.trim().parse::<f64>().map(|f| f as u32).unwrap_or(0);
            if let Some((ch, band)) = rf::freq_to_channel(bss.frequency_mhz) {
                bss.channel = ch;
                bss.band = Some(band);
            }
        } else if let Some(v) = line.strip_prefix("last seen:") {
            let ago: u64 = v
                .trim()
                .trim_end_matches("ms ago")
                .trim()
                .parse()
                .unwrap_or(0);
            bss.last_seen_ms = now_ms().saturating_sub(ago);
        } else if let Some(v) = line.strip_prefix("signal:") {
            bss.rssi = v
                .trim()
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<f64>().ok())
                .map(|f| f.round() as i32)
                .unwrap_or(-100);
        } else if let Some(v) = line.strip_prefix("SSID:") {
            let ssid = v.trim();
            bss.hidden =
                ssid.is_empty() || ssid.chars().all(|c| c == '\0') || ssid.starts_with("\\x00");
            bss.ssid = if bss.hidden {
                String::new()
            } else {
                ssid.to_string()
            };
        } else if let Some(v) = line.strip_prefix("capability:") {
            *privacy = v.contains("Privacy");
        } else if let Some(v) = line.strip_prefix("beacon interval:") {
            bss.beacon_interval = v
                .trim()
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok());
        } else if let Some(v) = line.strip_prefix("Country:") {
            bss.country = v.split_whitespace().next().map(str::to_string);
        } else if line.starts_with("RSN:") {
            section = "rsn";
            info.rsn_caps.get_or_insert(0);
        } else if line.starts_with("WPA:") {
            section = "wpa";
            info.wpa1 = true;
        } else if line.starts_with("WPS:") {
            info.wps = true;
        } else if line.starts_with("BSS Load:") {
            section = "load";
        } else if line.starts_with("HT operation:") {
            section = "ht";
        } else if line.starts_with("VHT operation:") {
            section = "vht";
        } else if line.starts_with("HT capabilities:") {
            info.ht = true;
        } else if line.starts_with("VHT capabilities:") {
            info.vht = true;
        } else if line.starts_with("HE capabilities:") {
            info.he = true;
        } else if line.starts_with("EHT capabilities") {
            info.eht = true;
        } else if line.starts_with("RM enabled capabilities") {
            info.rrm = true;
        } else if line.starts_with("Extended capabilities:") {
            section = "ext";
        } else if line.starts_with("Mobility Domain") || line.starts_with("MD:") {
            info.mobility_domain = true;
        } else if let Some(item) = line.strip_prefix("* ") {
            match section {
                "rsn" | "wpa" => {
                    if let Some(v) = item.strip_prefix("Authentication suites:") {
                        let akms: Vec<u8> =
                            v.split_whitespace().filter_map(akm_from_name).collect();
                        if section == "rsn" {
                            info.rsn_akms = akms;
                        }
                        // iw prints multi-word suite names separated by spaces;
                        // catch the 802.1X family explicitly.
                        if v.contains("802.1X") && section == "rsn" && !info.rsn_akms.contains(&1) {
                            info.rsn_akms
                                .push(if v.contains("FT/IEEE") { 3 } else { 1 });
                        }
                    } else if let Some(v) = item
                        .strip_prefix("Pairwise ciphers:")
                        .or(item.strip_prefix("Group cipher:"))
                    {
                        let tkip = v.contains("TKIP");
                        if section == "rsn" && tkip {
                            info.rsn_ciphers.push(2);
                        }
                        if section == "wpa" && tkip {
                            info.wpa1_ciphers.push(2);
                        }
                    } else if let Some(v) = item.strip_prefix("Capabilities:") {
                        if section == "rsn" {
                            let hex = v.rsplit("(0x").next().and_then(|h| h.strip_suffix(')'));
                            info.rsn_caps = hex
                                .and_then(|h| u16::from_str_radix(h, 16).ok())
                                .or(info.rsn_caps);
                        }
                    }
                }
                "load" => {
                    if let Some(v) = item.strip_prefix("station count:") {
                        bss.station_count = v.trim().parse().ok();
                    } else if let Some(v) = item.strip_prefix("channel utilisation:") {
                        let n: Option<u32> = v
                            .trim()
                            .split('/')
                            .next()
                            .and_then(|s| s.trim().parse().ok());
                        bss.utilization_pct = n.map(|n| ((n * 100 + 127) / 255) as u8);
                    }
                }
                "ht" => {
                    if let Some(v) = item.strip_prefix("secondary channel offset:") {
                        if v.contains("above") || v.contains("below") {
                            info.width_mhz = info.width_mhz.max(40);
                        }
                    }
                }
                "vht" => {
                    if let Some(v) = item.strip_prefix("channel width:") {
                        if v.contains("160") || v.contains("80+80") {
                            info.width_mhz = info.width_mhz.max(160);
                        } else if v.contains("80") {
                            info.width_mhz = info.width_mhz.max(80);
                        }
                    }
                }
                "ext" => {
                    if item.contains("BSS Transition") {
                        info.bss_transition = true;
                    }
                }
                _ => {}
            }
        }
    }
    finish(cur.take(), &mut result);
    result
}

fn parse_iw_link(name: &str, out: &str) -> WifiInterface {
    let mut iface = WifiInterface {
        id: name.to_string(),
        name: name.to_string(),
        description: "Wireless adapter".into(),
        state: "disconnected".into(),
        ..Default::default()
    };
    for line in out.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Connected to ") {
            iface.state = "connected".into();
            iface.bssid = rest.split_whitespace().next().map(|s| s.to_lowercase());
        } else if let Some(v) = line.strip_prefix("SSID:") {
            iface.ssid = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("freq:") {
            let f = v.trim().parse::<f64>().map(|f| f as u32).ok();
            iface.frequency_mhz = f;
            if let Some((ch, band)) = f.and_then(rf::freq_to_channel) {
                iface.channel = Some(ch);
                iface.band = Some(band);
            }
        } else if let Some(v) = line.strip_prefix("signal:") {
            iface.rssi = v.split_whitespace().next().and_then(|s| s.parse().ok());
        } else if let Some(v) = line.strip_prefix("tx bitrate:") {
            iface.tx_rate_mbps = v.split_whitespace().next().and_then(|s| s.parse().ok());
            iface.phy = phy_from_bitrate(v);
        } else if let Some(v) = line.strip_prefix("rx bitrate:") {
            iface.rx_rate_mbps = v.split_whitespace().next().and_then(|s| s.parse().ok());
        }
    }
    iface.wifi_gen = iface.phy.as_deref().and_then(|p| match p {
        "802.11be" => Some(7),
        "802.11ax" => Some(6),
        "802.11ac" => Some(5),
        "802.11n" => Some(4),
        _ => None,
    });
    iface
}

fn phy_from_bitrate(v: &str) -> Option<String> {
    let p = if v.contains("EHT-MCS") {
        "802.11be"
    } else if v.contains("HE-MCS") {
        "802.11ax"
    } else if v.contains("VHT-MCS") {
        "802.11ac"
    } else if v.contains("MCS") {
        "802.11n"
    } else {
        return None;
    };
    Some(p.to_string())
}

/// Splits an `nmcli -t` line on unescaped colons.
fn split_terse(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut escape = false;
    for c in line.chars() {
        if escape {
            fields.last_mut().unwrap().push(c);
            escape = false;
        } else if c == '\\' {
            escape = true;
        } else if c == ':' {
            fields.push(String::new());
        } else {
            fields.last_mut().unwrap().push(c);
        }
    }
    fields
}

fn nmcli_security(s: &str) -> Security {
    let s = s.to_uppercase();
    if s.contains("802.1X") {
        if s.contains("WPA3") {
            Security::Wpa3Enterprise
        } else {
            Security::Wpa2Enterprise
        }
    } else if s.contains("WPA3") && s.contains("WPA2") {
        Security::Wpa2Wpa3
    } else if s.contains("WPA3") {
        Security::Wpa3
    } else if s.contains("WPA2") {
        Security::Wpa2
    } else if s.contains("WPA1") || s.contains("WPA") {
        Security::Wpa
    } else if s.contains("WEP") {
        Security::Wep
    } else if s.contains("OWE") {
        Security::Owe
    } else if s.trim().is_empty() || s.trim() == "--" {
        Security::Open
    } else {
        Security::Unknown
    }
}

pub(crate) fn parse_nmcli(out: &str) -> (Vec<Bss>, Option<WifiInterface>) {
    let mut networks = Vec::new();
    let mut iface = None;
    for line in out.lines().filter(|l| !l.trim().is_empty()) {
        let f = split_terse(line);
        if f.len() < 8 {
            continue;
        }
        let in_use = f[0].trim() == "*";
        let freq: u32 = f[4]
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let (channel, band) = rf::freq_to_channel(freq)
            .map(|(c, b)| (c, Some(b)))
            .unwrap_or_else(|| (f[3].trim().parse().unwrap_or(0), None));
        let quality: u32 = f[6].trim().parse().unwrap_or(0);
        let ssid = f[2].clone();
        let bss = Bss {
            bssid: f[1].to_lowercase(),
            hidden: ssid.is_empty(),
            ssid,
            rssi: quality_to_dbm(quality),
            channel,
            frequency_mhz: freq,
            band,
            width_mhz: 20,
            security: nmcli_security(&f[7]),
            connected: in_use,
            last_seen_ms: now_ms(),
            ..Default::default()
        };
        if in_use {
            iface = Some(WifiInterface {
                id: "nm".into(),
                name: "Wi-Fi".into(),
                description: "NetworkManager".into(),
                state: "connected".into(),
                ssid: Some(bss.ssid.clone()),
                bssid: Some(bss.bssid.clone()),
                rssi: Some(bss.rssi),
                signal_quality: Some(quality),
                channel: Some(bss.channel),
                frequency_mhz: Some(freq),
                band: bss.band,
                tx_rate_mbps: f[5].split_whitespace().next().and_then(|s| s.parse().ok()),
                security: Some(bss.security),
                ..Default::default()
            });
        }
        networks.push(bss);
    }
    (networks, iface)
}

fn nmcli_scan(trigger: bool) -> ApiResult<ScanResult> {
    let out = run_tool(
        "nmcli",
        &[
            "-t",
            "-f",
            "IN-USE,BSSID,SSID,CHAN,FREQ,RATE,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "--rescan",
            if trigger { "auto" } else { "no" },
        ],
        T,
    )
    .map_err(|e| ApiError::new("noWifiTool", e.to_string()))?;
    let (networks, iface) = parse_nmcli(&out);
    Ok(ScanResult {
        source: ScanSource::LinuxNmcli,
        interfaces: iface.into_iter().collect(),
        networks,
        timestamp_ms: now_ms(),
        notices: vec![Notice {
            code: "approximateSignal".into(),
            detail: String::new(),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const IW: &str = "BSS 24:5a:4c:a1:01:18(on wlan0) -- associated
\tlast seen: 120 ms ago
\tfreq: 5180.0
\tbeacon interval: 100 TUs
\tcapability: ESS Privacy SpectrumMgmt (0x0111)
\tsignal: -48.00 dBm
\tSSID: Acme-Office
\tBSS Load:
\t\t * station count: 7
\t\t * channel utilisation: 64/255
\tHT capabilities:
\tHT operation:
\t\t * primary channel: 36
\t\t * secondary channel offset: above
\tVHT capabilities:
\tVHT operation:
\t\t * channel width: 1 (80 MHz)
\tHE capabilities:
\tRSN:\t * Version: 1
\t\t * Group cipher: CCMP
\t\t * Pairwise ciphers: CCMP
\t\t * Authentication suites: PSK SAE FT/PSK
\t\t * Capabilities: 1-PTKSA-RC 1-GTKSA-RC MFP-capable (0x0080)
\tRM enabled capabilities:
\tExtended capabilities:
\t\t * BSS Transition
\tCountry: PL\tEnvironment: Indoor/Outdoor
BSS 00:0c:41:44:55:66(on wlan0)
\tfreq: 2452
\tcapability: ESS Privacy (0x0011)
\tsignal: -81.00 dBm
\tSSID: Magazyn
\tWPS:\t * Version: 1.0
";

    #[test]
    fn parses_iw_dump() {
        let list = parse_iw_scan(IW);
        assert_eq!(list.len(), 2);
        let a = &list[0];
        assert_eq!(a.ssid, "Acme-Office");
        assert_eq!(a.channel, 36);
        assert_eq!(a.rssi, -48);
        assert_eq!(a.width_mhz, 80);
        assert_eq!(a.security, Security::Wpa2Wpa3);
        assert_eq!(a.wifi_gen, Some(6));
        assert_eq!(a.station_count, Some(7));
        assert_eq!(a.utilization_pct, Some(25));
        assert!(a.connected && a.roaming.k && a.roaming.v && a.roaming.r);
        assert_eq!(a.pmf, crate::wifi::Pmf::Capable);
        let b = &list[1];
        assert_eq!(b.channel, 9);
        assert_eq!(b.security, Security::Wep);
        assert!(b.wps);
    }

    #[test]
    fn parses_nmcli_terse() {
        let out = "*:24\\:5A\\:4C\\:A1\\:01\\:18:Acme-Office:36:5180 MHz:540 Mbit/s:82:WPA2 WPA3\n :F4\\:F2\\:6D\\:3A\\:11\\:C2:UPC2471139:6:2437 MHz:130 Mbit/s:40:WPA2\n";
        let (list, iface) = parse_nmcli(out);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].bssid, "24:5a:4c:a1:01:18");
        assert_eq!(list[0].security, Security::Wpa2Wpa3);
        assert_eq!(list[1].rssi, -80);
        assert_eq!(iface.unwrap().tx_rate_mbps, Some(540.0));
    }

    #[test]
    fn parses_iw_link() {
        let out = "Connected to 24:5a:4c:a1:01:18 (on wlan0)\n\tSSID: Acme-Office\n\tfreq: 5180.0\n\tsignal: -50 dBm\n\trx bitrate: 1200.9 MBit/s 80MHz HE-MCS 11 HE-NSS 2\n\ttx bitrate: 960.7 MBit/s 80MHz HE-MCS 9 HE-NSS 2\n";
        let i = parse_iw_link("wlan0", out);
        assert_eq!(i.state, "connected");
        assert_eq!(i.channel, Some(36));
        assert_eq!(i.rssi, Some(-50));
        assert_eq!(i.wifi_gen, Some(6));
        assert_eq!(i.tx_rate_mbps, Some(960.7));
    }
}
