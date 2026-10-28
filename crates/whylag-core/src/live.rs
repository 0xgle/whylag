//! Live probe for the gamer overlay: one quick sample per call of the router,
//! a chosen game or internet host, and the Wi-Fi signal. The overlay polls it
//! every second and works out from the last seconds whether lag comes from
//! Wi-Fi, the home network or the provider.

use crate::net::{self, TcpOutcome};
use crate::state::AppState;
use crate::util::now_ms;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LiveSample {
    pub t: u64,
    pub gateway_ms: Option<f64>,
    pub target_ms: Option<f64>,
    pub target: String,
    pub rssi: Option<i32>,
    pub ssid: Option<String>,
    /// False when the computer has no Wi-Fi link (wired, or Wi-Fi off).
    pub wifi: bool,
    pub simulated: bool,
}

#[derive(Default)]
pub struct LiveState {
    sim: Option<crate::sim::monitor::Live>,
    gateway: Option<(u64, Option<Ipv4Addr>)>,
    wifi: Option<(u64, Option<i32>, Option<String>)>,
    target: Option<(String, Option<IpAddr>)>,
}

pub type LiveHandle = Mutex<LiveState>;

const GATEWAY_TTL_MS: u64 = 30_000;
const WIFI_TTL_MS: u64 = 3_000;

async fn rtt(ip: IpAddr, timeout: Duration) -> Option<f64> {
    if let IpAddr::V4(v4) = ip {
        if let Some(r) = net::icmp::ping_async(v4, timeout).await.rtt() {
            return Some(r);
        }
    }
    // Hosts and networks that drop ping still complete a TCP handshake.
    match net::tcp_probe(SocketAddr::new(ip, 443), timeout).await {
        (TcpOutcome::Open | TcpOutcome::Closed, Some(t)) => Some(t),
        _ => None,
    }
}

pub async fn sample(state: &AppState, target: Option<String>) -> LiveSample {
    let target = target
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "1.1.1.1".into());
    let now = now_ms();
    if state.use_simulator() {
        let s = state
            .live
            .lock()
            .sim
            .get_or_insert_with(Default::default)
            .next(now);
        let scan = state.sim.lock().wifi_scan();
        let iface = scan.interfaces.first();
        return LiveSample {
            t: now,
            gateway_ms: s.gateway_ms,
            target_ms: s.internet_ms,
            target,
            rssi: iface.and_then(|i| i.rssi),
            ssid: iface.and_then(|i| i.ssid.clone()),
            wifi: true,
            simulated: true,
        };
    }

    // Cached lookups: the default route, the target's address, the Wi-Fi link.
    let gateway = match state.live.lock().gateway {
        Some((at, gw)) if now - at < GATEWAY_TTL_MS => Some(gw),
        _ => None,
    };
    let gateway = match gateway {
        Some(g) => g,
        None => {
            let g = tokio::task::spawn_blocking(net::local_net)
                .await
                .ok()
                .flatten()
                .and_then(|l| l.gateway);
            state.live.lock().gateway = Some((now, g));
            g
        }
    };
    let cached_target = state
        .live
        .lock()
        .target
        .clone()
        .filter(|(name, _)| *name == target)
        .map(|(_, ip)| ip);
    let target_ip = match cached_target {
        Some(ip) => ip,
        None => {
            let ip = net::resolve(&target).await;
            state.live.lock().target = Some((target.clone(), ip));
            ip
        }
    };
    let need_wifi = !matches!(state.live.lock().wifi, Some((at, _, _)) if now - at < WIFI_TTL_MS);

    let timeout = Duration::from_millis(800);
    let gw = async {
        match gateway {
            Some(g) => rtt(IpAddr::V4(g), timeout).await,
            None => None,
        }
    };
    let tg = async {
        match target_ip {
            Some(ip) => rtt(ip, timeout).await,
            None => None,
        }
    };
    let wifi = async {
        if !need_wifi {
            return None;
        }
        let scan = crate::wifi::scan_for_analysis(state).await;
        let iface = scan.as_ref().and_then(|s| {
            s.interfaces
                .iter()
                .find(|i| i.state == "connected")
                .cloned()
        });
        Some((
            iface.as_ref().and_then(|i| i.rssi),
            iface.and_then(|i| i.ssid),
        ))
    };
    let (gateway_ms, target_ms, fresh) = tokio::join!(gw, tg, wifi);
    let mut live = state.live.lock();
    if let Some((rssi, ssid)) = fresh {
        live.wifi = Some((now, rssi, ssid));
    }
    let (rssi, ssid) = live
        .wifi
        .as_ref()
        .map(|(_, r, s)| (*r, s.clone()))
        .unwrap_or((None, None));
    LiveSample {
        t: now,
        gateway_ms,
        target_ms,
        target,
        wifi: rssi.is_some(),
        rssi,
        ssid,
        simulated: false,
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn simulated_samples_have_values() {
        let dir = std::env::temp_dir().join(format!("whylag-live-{}", rand::random::<u32>()));
        let state = crate::state::AppState::new(dir);
        state
            .update_settings(serde_json::json!({ "dataSource": "simulator" }))
            .unwrap();
        let s = super::sample(&state, None).await;
        assert!(s.simulated && s.wifi);
        assert_eq!(s.target, "1.1.1.1");
        assert!(s.rssi.is_some());
    }
}
