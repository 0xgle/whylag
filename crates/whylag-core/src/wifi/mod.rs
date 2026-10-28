//! Wi-Fi scanning: data model, OS scanners and the simulator bridge.

pub mod ie;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(target_os = "macos", test))]
mod macos;
#[cfg(windows)]
mod windows;

use crate::error::{ApiError, ApiResult};
use crate::rf::Band;
use crate::settings::DataSource;
use crate::state::AppState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Security {
    Open,
    /// Opportunistic Wireless Encryption ("Enhanced Open").
    Owe,
    Wep,
    /// WPA (version 1), deprecated.
    Wpa,
    Wpa2,
    /// Transition mode advertising both WPA2-PSK and WPA3-SAE.
    Wpa2Wpa3,
    Wpa3,
    WpaEnterprise,
    Wpa2Enterprise,
    Wpa3Enterprise,
    #[default]
    Unknown,
}

impl Security {
    /// Lower is worse; used for sorting and advisor thresholds.
    pub fn strength(self) -> u8 {
        match self {
            Security::Open => 0,
            Security::Wep => 1,
            Security::Wpa | Security::WpaEnterprise => 2,
            Security::Owe => 3,
            Security::Wpa2 => 4,
            Security::Wpa2Wpa3 => 5,
            Security::Wpa2Enterprise => 5,
            Security::Wpa3 => 6,
            Security::Wpa3Enterprise => 7,
            Security::Unknown => 4,
        }
    }

    pub fn is_open(self) -> bool {
        matches!(self, Security::Open | Security::Owe)
    }

    pub fn is_enterprise(self) -> bool {
        matches!(self, Security::WpaEnterprise | Security::Wpa2Enterprise | Security::Wpa3Enterprise)
    }
}

/// Protected Management Frames (802.11w).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Pmf {
    #[default]
    Unknown,
    Disabled,
    Capable,
    Required,
}

/// Fast-roaming features advertised by the AP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Roaming {
    /// 802.11k radio resource management (neighbour reports).
    pub k: bool,
    /// 802.11v BSS transition management.
    pub v: bool,
    /// 802.11r fast BSS transition.
    pub r: bool,
}

/// One basic service set (a single radio advertising a single SSID).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Bss {
    pub bssid: String,
    pub ssid: String,
    pub hidden: bool,
    pub rssi: i32,
    pub noise: Option<i32>,
    pub channel: u32,
    pub frequency_mhz: u32,
    pub band: Option<Band>,
    pub width_mhz: u32,
    /// Wi-Fi generation: 4 (n), 5 (ac), 6 (ax), 7 (be). `None` for legacy a/b/g.
    pub wifi_gen: Option<u8>,
    pub security: Security,
    /// TKIP or WEP ciphers in use.
    pub weak_cipher: bool,
    pub pmf: Pmf,
    pub wps: bool,
    pub roaming: Roaming,
    pub station_count: Option<u16>,
    /// Channel busy time reported by the AP (BSS Load element), percent.
    pub utilization_pct: Option<u8>,
    pub country: Option<String>,
    pub vendor: Option<String>,
    pub connected: bool,
    pub beacon_interval: Option<u16>,
    pub last_seen_ms: u64,
}

impl Bss {
    pub fn snr(&self) -> Option<i32> {
        self.noise.map(|n| self.rssi - n)
    }
}

/// The local Wi-Fi adapter and its current association.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WifiInterface {
    pub id: String,
    pub name: String,
    pub description: String,
    pub state: String,
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub rssi: Option<i32>,
    pub signal_quality: Option<u32>,
    pub channel: Option<u32>,
    pub frequency_mhz: Option<u32>,
    pub band: Option<Band>,
    pub tx_rate_mbps: Option<f64>,
    pub rx_rate_mbps: Option<f64>,
    pub phy: Option<String>,
    pub wifi_gen: Option<u8>,
    pub security: Option<Security>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanSource {
    WindowsNative,
    LinuxIw,
    LinuxNmcli,
    Macos,
    Simulator,
}

/// A notice the UI should show next to the scan results (translated by code).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub source: ScanSource,
    pub interfaces: Vec<WifiInterface>,
    pub networks: Vec<Bss>,
    pub timestamp_ms: u64,
    pub notices: Vec<Notice>,
}

/// Runs a scan on the real adapter. Blocking; call from `spawn_blocking`.
#[allow(unreachable_code)]
fn native_scan() -> ApiResult<ScanResult> {
    #[cfg(windows)]
    return windows::scan();
    #[cfg(target_os = "linux")]
    return linux::scan();
    #[cfg(target_os = "macos")]
    return macos::scan();
    Err(ApiError::new("unsupportedPlatform", "Wi-Fi scanning is not supported on this platform"))
}

/// Scans using the configured data source.
pub async fn scan(state: &AppState) -> ApiResult<ScanResult> {
    let source = state.settings().data_source;
    if source == DataSource::Simulator {
        return Ok(state.sim.lock().wifi_scan());
    }
    let native = tokio::task::spawn_blocking(native_scan)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let mut result = match (source, native) {
        (DataSource::Live, r) => r?,
        (_, Ok(r)) if !r.interfaces.is_empty() => r,
        (_, Ok(r)) => {
            let mut sim = state.sim.lock().wifi_scan();
            sim.notices.extend(r.notices);
            sim.notices.push(Notice { code: "noWifiAdapter".into(), detail: String::new() });
            sim
        }
        (_, Err(e)) => {
            let mut sim = state.sim.lock().wifi_scan();
            sim.notices.push(Notice { code: e.code.clone(), detail: e.message.clone() });
            sim.notices.push(Notice { code: "fellBackToSimulator".into(), detail: String::new() });
            sim
        }
    };
    enrich(&mut result);
    Ok(result)
}

/// Triggers a scan and waits for the adapter to finish before reading results,
/// so a survey sample reflects the spot where it was taken. Windows and
/// NetworkManager refresh their caches 2-4 s after a scan request.
pub async fn scan_fresh(state: &AppState, wait_ms: u64) -> ApiResult<ScanResult> {
    let first = scan(state).await?;
    if first.source == ScanSource::Simulator {
        return Ok(first);
    }
    tokio::time::sleep(std::time::Duration::from_millis(wait_ms.min(8000))).await;
    scan(state).await
}

/// Fills derived fields that scanners leave empty.
pub fn enrich(result: &mut ScanResult) {
    for bss in &mut result.networks {
        if bss.vendor.is_none() {
            bss.vendor = crate::oui::lookup(&bss.bssid).map(str::to_string);
        }
        if bss.band.is_none() {
            bss.band = crate::rf::freq_to_channel(bss.frequency_mhz).map(|(_, b)| b);
        }
        if bss.width_mhz == 0 {
            bss.width_mhz = 20;
        }
    }
    // Mark the associated BSS when the scanner did not.
    let connected: Vec<String> = result.interfaces.iter().filter_map(|i| i.bssid.clone()).collect();
    for bss in &mut result.networks {
        if connected.iter().any(|b| b.eq_ignore_ascii_case(&bss.bssid)) {
            bss.connected = true;
        }
    }
    result.networks.sort_by(|a, b| b.rssi.cmp(&a.rssi));
}

/// Approximate dBm from a 0-100 % quality value (Windows/NetworkManager convention).
pub fn quality_to_dbm(quality: u32) -> i32 {
    (quality.min(100) as i32) / 2 - 100
}
