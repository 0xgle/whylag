//! Radios in and around the simulated office.

use crate::rf::{self, Band, Emitter};
use crate::wifi::{Pmf, Roaming, Security};

#[derive(Debug, Clone)]
pub struct SimRadio {
    pub ssid: &'static str,
    pub bssid: String,
    pub x: f64,
    pub y: f64,
    pub band: Band,
    pub channel: u32,
    pub width_mhz: u32,
    pub eirp_dbm: f64,
    pub security: Security,
    pub pmf: Pmf,
    pub roaming: Roaming,
    pub wps: bool,
    pub wifi_gen: Option<u8>,
    pub hidden: bool,
    pub station_base: u16,
    pub utilization_base: u8,
    pub reports_load: bool,
    /// Name of the physical AP this radio belongs to (for the UI and reports).
    pub ap_name: &'static str,
}

impl SimRadio {
    pub fn emitter(&self) -> Emitter {
        Emitter {
            x: self.x,
            y: self.y,
            eirp_dbm: self.eirp_dbm,
            freq_mhz: rf::channel_to_freq(self.channel, self.band),
        }
    }
}

struct Ap {
    name: &'static str,
    id: u8,
    x: f64,
    y: f64,
    ch24: u32,
    width24: u32,
    ch5: u32,
    ch6: Option<u32>,
}

const ACME_APS: &[Ap] = &[
    Ap {
        name: "AP-01 Open Space A",
        id: 1,
        x: 5.0,
        y: 5.0,
        ch24: 1,
        width24: 20,
        ch5: 36,
        ch6: None,
    },
    Ap {
        name: "AP-02 Open Space A",
        id: 2,
        x: 13.0,
        y: 6.0,
        ch24: 6,
        width24: 20,
        ch5: 52,
        ch6: Some(37),
    },
    // Misconfigured: 40 MHz on 2.4 GHz in a crowded band.
    Ap {
        name: "AP-03 Office North",
        id: 3,
        x: 36.0,
        y: 4.0,
        ch24: 11,
        width24: 40,
        ch5: 100,
        ch6: None,
    },
    Ap {
        name: "AP-04 Open Space B",
        id: 4,
        x: 9.0,
        y: 21.0,
        ch24: 6,
        width24: 20,
        ch5: 116,
        ch6: None,
    },
    Ap {
        name: "AP-05 Corridor",
        id: 5,
        x: 25.0,
        y: 13.0,
        ch24: 1,
        width24: 20,
        ch5: 149,
        ch6: None,
    },
];

fn office(
    ap: &Ap,
    ssid: &'static str,
    band: Band,
    channel: u32,
    width: u32,
    suffix: u8,
) -> SimRadio {
    let guest = ssid.ends_with("Guest");
    let (security, pmf, roaming) = match (guest, band) {
        (true, _) => (Security::Open, Pmf::Disabled, Roaming::default()),
        (false, Band::B6) => (
            Security::Wpa3,
            Pmf::Required,
            Roaming {
                k: true,
                v: true,
                r: true,
            },
        ),
        (false, _) => (
            Security::Wpa2Wpa3,
            Pmf::Capable,
            Roaming {
                k: true,
                v: true,
                r: true,
            },
        ),
    };
    SimRadio {
        ssid,
        bssid: format!("24:5a:4c:a1:0{}:{:02x}", ap.id, suffix),
        x: ap.x,
        y: ap.y,
        band,
        channel,
        width_mhz: width,
        eirp_dbm: match band {
            Band::B24 => 14.0,
            _ => 17.0,
        },
        security,
        pmf,
        roaming,
        wps: false,
        wifi_gen: Some(6),
        hidden: false,
        station_base: match (guest, band) {
            (true, _) => 2,
            (false, Band::B24) => 6,
            (false, _) => 14,
        },
        utilization_base: if band == Band::B24 { 58 } else { 22 },
        reports_load: true,
        ap_name: ap.name,
    }
}

#[allow(clippy::too_many_arguments)]
fn neighbour(
    ssid: &'static str,
    bssid: &str,
    x: f64,
    y: f64,
    band: Band,
    channel: u32,
    width: u32,
    eirp: f64,
    security: Security,
    wps: bool,
    wifi_gen: Option<u8>,
) -> SimRadio {
    SimRadio {
        ssid,
        bssid: bssid.to_string(),
        x,
        y,
        band,
        channel,
        width_mhz: width,
        eirp_dbm: eirp,
        security,
        pmf: if security == Security::Wpa3 || security == Security::Wpa2Wpa3 {
            Pmf::Capable
        } else {
            Pmf::Disabled
        },
        roaming: Roaming::default(),
        wps,
        wifi_gen,
        hidden: false,
        station_base: 3,
        utilization_base: 30,
        reports_load: false,
        ap_name: "",
    }
}

pub fn all() -> Vec<SimRadio> {
    use Band::*;
    use Security::*;
    let mut radios = Vec::new();
    for ap in ACME_APS {
        radios.push(office(ap, "Acme-Office", B24, ap.ch24, ap.width24, 0x10));
        radios.push(office(ap, "Acme-Guest", B24, ap.ch24, ap.width24, 0x11));
        radios.push(office(ap, "Acme-Office", B5, ap.ch5, 80, 0x18));
        radios.push(office(ap, "Acme-Guest", B5, ap.ch5, 80, 0x19));
        if let Some(ch6) = ap.ch6 {
            radios.push(office(ap, "Acme-Office", B6, ch6, 160, 0x1c));
        }
    }
    radios.extend([
        neighbour(
            "UPC2471139",
            "f4:f2:6d:3a:11:c2",
            -5.0,
            8.0,
            B24,
            6,
            20,
            20.0,
            Wpa2,
            true,
            Some(4),
        ),
        neighbour(
            "UPC2471139",
            "f4:f2:6d:3a:11:c3",
            -5.0,
            8.0,
            B5,
            40,
            80,
            20.0,
            Wpa2,
            true,
            Some(5),
        ),
        neighbour(
            "Orange_Swiatlowod_8A2C",
            "00:0e:59:8a:2c:01",
            21.0,
            31.0,
            B24,
            11,
            20,
            20.0,
            Wpa2,
            false,
            Some(6),
        ),
        neighbour(
            "Orange_Swiatlowod_8A2C",
            "00:0e:59:8a:2c:02",
            21.0,
            31.0,
            B5,
            100,
            80,
            20.0,
            Wpa2,
            false,
            Some(6),
        ),
        // Bad neighbour: 40 MHz on channel 3 overlaps everything on 2.4 GHz.
        neighbour(
            "PLAY_INTERNET_5G_4F1A",
            "00:18:82:4f:1a:01",
            47.0,
            18.0,
            B24,
            3,
            40,
            20.0,
            Wpa2Wpa3,
            false,
            Some(6),
        ),
        neighbour(
            "Netia-2C3D9F",
            "00:15:eb:2c:3d:9f",
            30.0,
            -5.0,
            B24,
            9,
            20,
            20.0,
            Wpa2,
            true,
            Some(4),
        ),
        neighbour(
            "FRITZ!Box 7590 KQ",
            "00:04:0e:7a:55:10",
            -7.0,
            22.0,
            B24,
            1,
            20,
            20.0,
            Wpa2,
            false,
            Some(5),
        ),
        neighbour(
            "FRITZ!Box 7590 KQ",
            "00:04:0e:7a:55:11",
            -7.0,
            22.0,
            B5,
            36,
            160,
            20.0,
            Wpa2,
            false,
            Some(5),
        ),
        neighbour(
            "DIRECT-7B-HP Laser 107w",
            "00:01:e6:7b:00:01",
            4.0,
            24.0,
            B24,
            6,
            20,
            12.0,
            Wpa2,
            true,
            Some(4),
        ),
        neighbour(
            "Magazyn",
            "00:0c:41:44:55:66",
            46.0,
            4.0,
            B24,
            9,
            20,
            16.0,
            Wep,
            false,
            None,
        ),
        // Evil twin: an open network impersonating the corporate SSID.
        neighbour(
            "Acme-Office",
            "00:0a:eb:66:77:88",
            16.0,
            22.0,
            B24,
            6,
            20,
            14.0,
            Open,
            false,
            Some(4),
        ),
        neighbour(
            "SmartPlug-3F2A",
            "00:4b:12:3f:2a:01",
            25.0,
            22.0,
            B24,
            1,
            20,
            8.0,
            Wpa2,
            false,
            Some(4),
        ),
    ]);
    let mut hidden = neighbour(
        "",
        "00:00:0c:5e:a0:01",
        36.0,
        8.0,
        B5,
        44,
        40,
        17.0,
        Wpa2Enterprise,
        false,
        Some(5),
    );
    hidden.hidden = true;
    hidden.pmf = Pmf::Capable;
    radios.push(hidden);
    radios
}
