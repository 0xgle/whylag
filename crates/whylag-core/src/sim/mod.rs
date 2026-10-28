//! The simulated office.
//!
//! A 42 × 26 m floor with five enterprise APs, a crowded neighbourhood, an evil
//! twin, a coverage hole behind the server room and the usual misconfigurations.
//! Every engine can run against it, which makes the app demoable without hardware
//! and lets the whole UI be tested in CI.

pub mod lan;
pub mod net;
mod plan;
mod radios;

use crate::rf::{self, Band, Wall};
use crate::util::now_ms;
use crate::wifi::{Bss, Notice, ScanResult, ScanSource, Security, WifiInterface};
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::Serialize;

pub use plan::FloorPlan;
pub use radios::SimRadio;

/// Office size in metres.
pub const WIDTH_M: f64 = 42.0;
pub const HEIGHT_M: f64 = 26.0;
/// Floor plan image scale and margin (the plan image has a 1 m border).
pub const PX_PER_M: f64 = 20.0;
pub const MARGIN_M: f64 = 1.0;

/// Weakest signal a typical adapter still reports.
const SENSITIVITY_DBM: f64 = -92.0;

pub struct SimWorld {
    pub walls: Vec<Wall>,
    pub radios: Vec<SimRadio>,
    /// Current position of the simulated laptop, metres.
    pub position: (f64, f64),
    /// BSSID the simulated client is associated with (sticky-client behaviour).
    associated: Option<String>,
    rng: StdRng,
    started_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimInfo {
    pub position: (f64, f64),
    pub width_m: f64,
    pub height_m: f64,
    pub floor_plan: FloorPlan,
}

impl Default for SimWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl SimWorld {
    pub fn new() -> Self {
        Self {
            walls: plan::walls(),
            radios: radios::all(),
            position: (12.0, 13.0),
            associated: None,
            rng: StdRng::from_entropy(),
            started_ms: now_ms(),
        }
    }

    /// The office APs as plannable virtual APs (floor-plan pixels), so the
    /// predictive planner can be compared against the walked survey.
    pub fn virtual_aps(&self) -> Vec<crate::survey::VirtualAp> {
        let mut aps: Vec<crate::survey::VirtualAp> = Vec::new();
        for r in self.radios.iter().filter(|r| !r.ap_name.is_empty() && r.ssid == "Acme-Office") {
            let radio = crate::survey::VirtualRadio { band: r.band, channel: r.channel, width_mhz: r.width_mhz, eirp_dbm: r.eirp_dbm, enabled: true };
            match aps.iter_mut().find(|a| a.name == r.ap_name) {
                Some(ap) => ap.radios.push(radio),
                None => aps.push(crate::survey::VirtualAp {
                    id: r.ap_name.split_whitespace().next().unwrap_or("AP").to_string(),
                    name: r.ap_name.to_string(),
                    x: (r.x + MARGIN_M) * PX_PER_M,
                    y: (r.y + MARGIN_M) * PX_PER_M,
                    radios: vec![radio],
                }),
            }
        }
        aps
    }

    pub fn info(&self) -> SimInfo {
        SimInfo { position: self.position, width_m: WIDTH_M, height_m: HEIGHT_M, floor_plan: plan::floor_plan(&self.walls) }
    }

    /// Moves the simulated laptop. Accepts floor-plan pixel coordinates.
    pub fn set_position_px(&mut self, x_px: f64, y_px: f64) {
        let x = (x_px / PX_PER_M - MARGIN_M).clamp(-MARGIN_M, WIDTH_M + MARGIN_M);
        let y = (y_px / PX_PER_M - MARGIN_M).clamp(-MARGIN_M, HEIGHT_M + MARGIN_M);
        self.position = (x, y);
    }

    /// Smooth, position-dependent shadow fading (±3 dB) so heatmaps look like real
    /// measurements instead of perfect circles. Deterministic per radio and place.
    fn shadow_fading(index: usize, x: f64, y: f64) -> f64 {
        fn hash(i: usize, gx: i64, gy: i64) -> f64 {
            let mut h = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (gx as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
                ^ (gy as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
            h ^= h >> 31;
            h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
            h ^= h >> 27;
            (h % 10_000) as f64 / 10_000.0 * 2.0 - 1.0
        }
        let cell = 2.5;
        let (fx, fy) = (x / cell, y / cell);
        let (gx, gy) = (fx.floor() as i64, fy.floor() as i64);
        let (tx, ty) = (fx - gx as f64, fy - gy as f64);
        let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
        let a = hash(index, gx, gy);
        let b = hash(index, gx + 1, gy);
        let c = hash(index, gx, gy + 1);
        let d = hash(index, gx + 1, gy + 1);
        let top = a + (b - a) * sx;
        let bottom = c + (d - c) * sx;
        3.0 * (top + (bottom - top) * sy)
    }

    /// Noise-free expected RSSI of a radio at a point (used by tests and the walk demo).
    pub fn expected_rssi(&self, index: usize, x: f64, y: f64) -> f64 {
        let r = &self.radios[index];
        rf::predict_rssi(&r.emitter(), x, y, &self.walls) + Self::shadow_fading(index, x, y)
    }

    fn gaussian(&mut self, sigma: f64) -> f64 {
        // Irwin-Hall approximation, plenty for measurement jitter.
        let sum: f64 = (0..6).map(|_| self.rng.gen::<f64>()).sum();
        (sum - 3.0) * sigma * std::f64::consts::SQRT_2
    }

    pub fn wifi_scan(&mut self) -> ScanResult {
        let (x, y) = self.position;
        let now = now_ms();
        let t = (now - self.started_ms) as f64 / 1000.0;
        let mut networks = Vec::new();
        for i in 0..self.radios.len() {
            let measured = self.expected_rssi(i, x, y) + self.gaussian(1.4) + (t / 7.0 + i as f64).sin() * 0.8;
            if measured < SENSITIVITY_DBM {
                continue;
            }
            let r = &self.radios[i];
            let load_wave = ((t / 23.0) + i as f64 * 1.7).sin();
            let utilization = (r.utilization_base as f64 + load_wave * 8.0).clamp(1.0, 99.0) as u8;
            let stations = (r.station_base as f64 + load_wave * 2.0).max(0.0).round() as u16;
            networks.push(Bss {
                bssid: r.bssid.to_string(),
                ssid: if r.hidden { String::new() } else { r.ssid.to_string() },
                hidden: r.hidden,
                rssi: measured.round() as i32,
                noise: Some(r.band.typical_noise_dbm() as i32 + if r.band == Band::B24 { 2 } else { 0 }),
                channel: r.channel,
                frequency_mhz: rf::channel_to_freq(r.channel, r.band),
                band: Some(r.band),
                width_mhz: r.width_mhz,
                wifi_gen: r.wifi_gen,
                security: r.security,
                weak_cipher: matches!(r.security, Security::Wep | Security::Wpa),
                pmf: r.pmf,
                wps: r.wps,
                roaming: r.roaming,
                station_count: r.reports_load.then_some(stations),
                utilization_pct: r.reports_load.then_some(utilization),
                country: Some("PL".into()),
                vendor: None,
                connected: false,
                beacon_interval: Some(100),
                last_seen_ms: now,
            });
        }
        let iface = self.associate(&mut networks);
        let mut result = ScanResult {
            source: ScanSource::Simulator,
            interfaces: vec![iface],
            networks,
            timestamp_ms: now,
            notices: vec![Notice { code: "simulator".into(), detail: String::new() }],
        };
        crate::wifi::enrich(&mut result);
        result
    }

    /// Picks the BSS the simulated client is associated with. Like many real
    /// clients it is "sticky": it only roams once the signal drops below -75 dBm.
    fn associate(&mut self, networks: &mut [Bss]) -> WifiInterface {
        const SSID: &str = "Acme-Office";
        let current = self
            .associated
            .as_ref()
            .and_then(|b| networks.iter().find(|n| &n.bssid == b))
            .filter(|n| n.rssi > -75)
            .map(|n| n.bssid.clone());
        let chosen = current.or_else(|| {
            networks
                .iter()
                .filter(|n| n.ssid == SSID && n.security != Security::Open)
                // Prefer 5 GHz unless it is much weaker, like real band steering.
                .max_by_key(|n| n.rssi + if n.band == Some(Band::B24) { -8 } else { 0 })
                .map(|n| n.bssid.clone())
        });
        self.associated = chosen.clone();
        let Some(bss) = chosen.and_then(|b| networks.iter_mut().find(|n| n.bssid == b)) else {
            return WifiInterface {
                id: "sim0".into(),
                name: "Wi-Fi".into(),
                description: "Simulated Wi-Fi 6E adapter".into(),
                state: "disconnected".into(),
                ..Default::default()
            };
        };
        bss.connected = true;
        let snr = bss.snr().unwrap_or(25).max(0) as f64;
        // Rough MCS-from-SNR rate estimate for the simulated link.
        let per_20 = (snr * 7.5).min(286.0);
        let rate = (per_20 * bss.width_mhz as f64 / 20.0 * 0.95).round();
        WifiInterface {
            id: "sim0".into(),
            name: "Wi-Fi".into(),
            description: "Simulated Wi-Fi 6E adapter".into(),
            state: "connected".into(),
            ssid: Some(bss.ssid.clone()),
            bssid: Some(bss.bssid.clone()),
            rssi: Some(bss.rssi),
            signal_quality: Some(((bss.rssi + 100) * 2).clamp(0, 100) as u32),
            channel: Some(bss.channel),
            frequency_mhz: Some(bss.frequency_mhz),
            band: bss.band,
            tx_rate_mbps: Some(rate),
            rx_rate_mbps: Some((rate * 1.1).round()),
            phy: Some("802.11ax".into()),
            wifi_gen: Some(6),
            security: Some(bss.security),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_sees_office_and_neighbours() {
        let mut world = SimWorld::new();
        let scan = world.wifi_scan();
        assert!(scan.networks.len() > 15, "only {} networks", scan.networks.len());
        assert!(scan.networks.iter().any(|n| n.ssid == "Acme-Office" && n.connected));
        assert!(scan.networks.iter().any(|n| n.hidden));
        assert_eq!(scan.interfaces[0].state, "connected");
    }

    #[test]
    fn server_room_shadows_south_east_office() {
        let world = SimWorld::new();
        // The south-east office is a deliberate coverage hole on 5 GHz.
        let best_5ghz = |x: f64, y: f64| {
            world
                .radios
                .iter()
                .enumerate()
                .filter(|(_, r)| r.ssid == "Acme-Office" && r.band == Band::B5)
                .map(|(i, _)| world.expected_rssi(i, x, y))
                .fold(f64::MIN, f64::max)
        };
        assert!(best_5ghz(39.0, 23.0) < -67.0, "{}", best_5ghz(39.0, 23.0));
        assert!(best_5ghz(14.0, 6.0) > -55.0, "{}", best_5ghz(14.0, 6.0));
    }

    #[test]
    fn position_from_pixels() {
        let mut world = SimWorld::new();
        world.set_position_px(20.0 + 10.0 * PX_PER_M, 20.0 + 5.0 * PX_PER_M);
        assert!((world.position.0 - 10.0).abs() < 1e-9 && (world.position.1 - 5.0).abs() < 1e-9);
    }
}
