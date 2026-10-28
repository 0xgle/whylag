//! Windows scanner using the Native Wi-Fi API (wlanapi.dll).
//!
//! `WlanGetNetworkBssList` returns every BSS with its raw information elements,
//! which we decode ourselves (see `ie.rs`) for detail Windows does not surface.
//! Since Windows 11 24H2 these calls need location access for desktop apps; an
//! access-denied error is reported with the `locationPermission` code so the UI
//! can explain how to enable it.

use super::ie;
use super::{Bss, Notice, ScanResult, ScanSource, Security, WifiInterface};
use crate::error::{ApiError, ApiResult};
use crate::rf;
use crate::util::{fmt_mac, now_ms};
use std::ffi::c_void;
use std::ptr;
use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::NetworkManagement::WiFi::*;

const ERROR_SUCCESS: u32 = 0;
const ERROR_ACCESS_DENIED: u32 = 5;
/// Cached BSS entries older than this are dropped.
const STALE_MS: u64 = 60_000;
const ERROR_SERVICE_NOT_ACTIVE: u32 = 1062;

/// Owned WLAN client handle, closed on drop.
struct Client(HANDLE);

impl Client {
    fn open() -> ApiResult<Self> {
        let mut negotiated = 0u32;
        let mut handle: HANDLE = ptr::null_mut();
        let rc = unsafe { WlanOpenHandle(2, ptr::null(), &mut negotiated, &mut handle) };
        match rc {
            ERROR_SUCCESS => Ok(Client(handle)),
            ERROR_SERVICE_NOT_ACTIVE => Err(ApiError::new(
                "noWifiAdapter",
                "WLAN AutoConfig service is not running",
            )),
            other => Err(ApiError::new(
                "wlanApi",
                format!("WlanOpenHandle failed: {other}"),
            )),
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            WlanCloseHandle(self.0, ptr::null());
        }
    }
}

/// Memory returned by wlanapi, freed with WlanFreeMemory.
struct WlanMem<T>(*mut T);

impl<T> Drop for WlanMem<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WlanFreeMemory(self.0 as *const c_void) };
        }
    }
}

fn guid_string(g: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        g.data1,
        g.data2,
        g.data3,
        g.data4[0],
        g.data4[1],
        g.data4[2],
        g.data4[3],
        g.data4[4],
        g.data4[5],
        g.data4[6],
        g.data4[7]
    )
}

fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|c| *c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

fn ssid_to_string(ssid: &DOT11_SSID) -> (String, bool) {
    let len = (ssid.uSSIDLength as usize).min(32);
    let bytes = &ssid.ucSSID[..len];
    let hidden = len == 0 || bytes.iter().all(|b| *b == 0);
    (String::from_utf8_lossy(bytes).into_owned(), hidden)
}

fn phy_name(phy: DOT11_PHY_TYPE) -> Option<(&'static str, Option<u8>)> {
    Some(match phy {
        dot11_phy_type_eht => ("802.11be", Some(7)),
        dot11_phy_type_he => ("802.11ax", Some(6)),
        dot11_phy_type_vht => ("802.11ac", Some(5)),
        dot11_phy_type_ht => ("802.11n", Some(4)),
        dot11_phy_type_ofdm => ("802.11a", None),
        dot11_phy_type_erp => ("802.11g", None),
        dot11_phy_type_hrdsss => ("802.11b", None),
        _ => return None,
    })
}

fn security_from_auth(auth: DOT11_AUTH_ALGORITHM, enabled: bool, onex: bool) -> Security {
    match auth {
        DOT11_AUTH_ALGO_WPA3_SAE => Security::Wpa3,
        DOT11_AUTH_ALGO_WPA3 | DOT11_AUTH_ALGO_WPA3_ENT => Security::Wpa3Enterprise,
        DOT11_AUTH_ALGO_RSNA_PSK => Security::Wpa2,
        DOT11_AUTH_ALGO_RSNA => Security::Wpa2Enterprise,
        DOT11_AUTH_ALGO_WPA_PSK => Security::Wpa,
        DOT11_AUTH_ALGO_WPA => Security::WpaEnterprise,
        DOT11_AUTH_ALGO_OWE => Security::Owe,
        DOT11_AUTH_ALGO_80211_SHARED_KEY => Security::Wep,
        DOT11_AUTH_ALGO_80211_OPEN if enabled && !onex => Security::Wep,
        DOT11_AUTH_ALGO_80211_OPEN => Security::Open,
        _ => Security::Unknown,
    }
}

unsafe fn query<T: Copy>(client: &Client, guid: &GUID, opcode: WLAN_INTF_OPCODE) -> Option<T> {
    let mut size = 0u32;
    let mut data: *mut c_void = ptr::null_mut();
    let mut value_type: WLAN_OPCODE_VALUE_TYPE = 0;
    let rc = WlanQueryInterface(
        client.0,
        guid,
        opcode,
        ptr::null(),
        &mut size,
        &mut data,
        &mut value_type,
    );
    if rc != ERROR_SUCCESS || data.is_null() {
        return None;
    }
    let mem = WlanMem(data as *mut T);
    if (size as usize) < std::mem::size_of::<T>() {
        return None;
    }
    Some(ptr::read_unaligned(mem.0))
}

unsafe fn interface_info(client: &Client, info: &WLAN_INTERFACE_INFO) -> WifiInterface {
    let guid = info.InterfaceGuid;
    let connected = info.isState == wlan_interface_state_connected;
    let mut iface = WifiInterface {
        id: guid_string(&guid),
        name: "Wi-Fi".into(),
        description: wide_to_string(&info.strInterfaceDescription),
        state: if connected {
            "connected"
        } else {
            "disconnected"
        }
        .into(),
        ..Default::default()
    };
    if !connected {
        return iface;
    }
    if let Some(conn) =
        query::<WLAN_CONNECTION_ATTRIBUTES>(client, &guid, wlan_intf_opcode_current_connection)
    {
        let assoc = conn.wlanAssociationAttributes;
        let sec = conn.wlanSecurityAttributes;
        iface.ssid = Some(ssid_to_string(&assoc.dot11Ssid).0);
        iface.bssid = Some(fmt_mac(&assoc.dot11Bssid));
        iface.signal_quality = Some(assoc.wlanSignalQuality);
        // Rates are reported in kbps.
        iface.rx_rate_mbps = Some(assoc.ulRxRate as f64 / 1000.0);
        iface.tx_rate_mbps = Some(assoc.ulTxRate as f64 / 1000.0);
        if let Some((name, gen)) = phy_name(assoc.dot11PhyType) {
            iface.phy = Some(name.into());
            iface.wifi_gen = gen;
        }
        iface.security = Some(security_from_auth(
            sec.dot11AuthAlgorithm,
            sec.bSecurityEnabled != 0,
            sec.bOneXEnabled != 0,
        ));
    }
    iface.rssi = query::<i32>(client, &guid, wlan_intf_opcode_rssi);
    iface.channel = query::<u32>(client, &guid, wlan_intf_opcode_channel_number);
    iface
}

unsafe fn bss_list(client: &Client, guid: &GUID) -> Result<Vec<Bss>, u32> {
    let mut list: *mut WLAN_BSS_LIST = ptr::null_mut();
    let rc = WlanGetNetworkBssList(
        client.0,
        guid,
        ptr::null(),
        dot11_BSS_type_any,
        0,
        ptr::null(),
        &mut list,
    );
    if rc != ERROR_SUCCESS {
        return Err(rc);
    }
    let mem = WlanMem(list);
    let count = (*mem.0).dwNumberOfItems as usize;
    let first = ptr::addr_of!((*mem.0).wlanBssEntries) as *const WLAN_BSS_ENTRY;
    let now = now_ms();
    // FILETIME (100 ns since 1601) of "now", to age cached entries.
    let now_ft = now * 10_000 + 116_444_736_000_000_000;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let entry_ptr = first.add(i);
        let entry = ptr::read_unaligned(entry_ptr);
        // When the last beacon or probe response was received. Entries the
        // adapter has not heard for a while are stale cache, not live APs.
        let age_ms = if entry.ullHostTimestamp > 0 && entry.ullHostTimestamp <= now_ft {
            (now_ft - entry.ullHostTimestamp) / 10_000
        } else {
            0
        };
        if age_ms > STALE_MS {
            continue;
        }
        let ies = std::slice::from_raw_parts(
            (entry_ptr as *const u8).add(entry.ulIeOffset as usize),
            entry.ulIeSize as usize,
        );
        let info = ie::parse(ies);
        let (ssid, hidden) = ssid_to_string(&entry.dot11Ssid);
        let freq = entry.ulChCenterFrequency / 1000;
        let (channel, band) = rf::freq_to_channel(freq)
            .map(|(c, b)| (c, Some(b)))
            .unwrap_or((info.ds_channel.unwrap_or(0), None));
        let privacy = entry.usCapabilityInformation & 0x0010 != 0;
        let security = info.security(privacy);
        let phy_gen = phy_name(entry.dot11BssPhyType).and_then(|(_, g)| g);
        out.push(Bss {
            bssid: fmt_mac(&entry.dot11Bssid),
            ssid,
            hidden,
            rssi: entry.lRssi,
            noise: None,
            channel,
            frequency_mhz: freq,
            band,
            width_mhz: info.width_mhz,
            wifi_gen: info.wifi_gen().or(phy_gen),
            security,
            weak_cipher: info.weak_cipher(security),
            pmf: info.pmf(),
            wps: info.wps,
            roaming: info.roaming(),
            station_count: info.station_count,
            utilization_pct: info.utilization_pct,
            country: info.country.clone(),
            vendor: None,
            connected: false,
            beacon_interval: Some(entry.usBeaconPeriod),
            last_seen_ms: now.saturating_sub(age_ms),
        });
    }
    Ok(out)
}

pub fn scan(trigger: bool) -> ApiResult<ScanResult> {
    let client = Client::open()?;
    let mut list: *mut WLAN_INTERFACE_INFO_LIST = ptr::null_mut();
    let rc = unsafe { WlanEnumInterfaces(client.0, ptr::null(), &mut list) };
    if rc != ERROR_SUCCESS {
        return Err(ApiError::new(
            "wlanApi",
            format!("WlanEnumInterfaces failed: {rc}"),
        ));
    }
    let mem = WlanMem(list);
    let mut interfaces = Vec::new();
    let mut networks = Vec::new();
    let mut notices = Vec::new();
    unsafe {
        let count = (*mem.0).dwNumberOfItems as usize;
        let first = ptr::addr_of!((*mem.0).InterfaceInfo) as *const WLAN_INTERFACE_INFO;
        for i in 0..count {
            let info = ptr::read_unaligned(first.add(i));
            let guid = info.InterfaceGuid;
            // Request a fresh scan only when asked: results land in the cache
            // within ~4 s, and the UI polls, so we read the cache straight away.
            if trigger {
                WlanScan(client.0, &guid, ptr::null(), ptr::null(), ptr::null());
            }
            interfaces.push(interface_info(&client, &info));
            match bss_list(&client, &guid) {
                Ok(list) => networks.extend(list),
                Err(ERROR_ACCESS_DENIED) => {
                    notices.push(Notice {
                        code: "locationPermission".into(),
                        detail: String::new(),
                    });
                }
                Err(rc) => notices.push(Notice {
                    code: "wlanApi".into(),
                    detail: format!("WlanGetNetworkBssList: {rc}"),
                }),
            }
        }
    }
    if interfaces.is_empty() {
        return Err(ApiError::new(
            "noWifiAdapter",
            "no wireless interface found",
        ));
    }
    if networks.is_empty() && notices.iter().any(|n| n.code == "locationPermission") {
        return Err(ApiError::new(
            "locationPermission",
            "Windows location access is off for desktop apps",
        ));
    }
    Ok(ScanResult {
        source: ScanSource::WindowsNative,
        interfaces,
        networks,
        timestamp_ms: now_ms(),
        notices,
    })
}
