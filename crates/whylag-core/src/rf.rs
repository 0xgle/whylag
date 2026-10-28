//! Indoor radio propagation (multi-wall model).
//!
//! Received power = EIRP − path loss(distance, frequency) − Σ wall attenuation.
//! Path loss uses free-space loss at 1 m plus a log-distance term. Walls are
//! counted explicitly, so the distance exponent is lower than the "everything
//! included" indoor exponent of ~3.
//!
//! The same model powers the simulator (with noise added) and predictive
//! planning, so a plan drawn in the app behaves like the simulated office.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Band {
    #[serde(rename = "2.4")]
    B24,
    #[serde(rename = "5")]
    B5,
    #[serde(rename = "6")]
    B6,
}

impl Band {
    pub fn label(self) -> &'static str {
        match self {
            Band::B24 => "2.4 GHz",
            Band::B5 => "5 GHz",
            Band::B6 => "6 GHz",
        }
    }

    /// Typical noise floor for an office environment, dBm.
    pub fn typical_noise_dbm(self) -> f64 {
        match self {
            Band::B24 => -92.0,
            Band::B5 => -95.0,
            Band::B6 => -96.0,
        }
    }
}

/// Converts a channel centre frequency (MHz) to (channel number, band).
pub fn freq_to_channel(mhz: u32) -> Option<(u32, Band)> {
    match mhz {
        2412..=2472 => Some(((mhz - 2407) / 5, Band::B24)),
        2484 => Some((14, Band::B24)),
        // 6 GHz must be tested before the generic 5 GHz range (5955 MHz = ch 1).
        5955..=7115 => Some(((mhz - 5950) / 5, Band::B6)),
        5150..=5925 => Some(((mhz - 5000) / 5, Band::B5)),
        _ => None,
    }
}

/// Converts a channel number in a band to its centre frequency (MHz).
pub fn channel_to_freq(channel: u32, band: Band) -> u32 {
    match band {
        Band::B24 if channel == 14 => 2484,
        Band::B24 => 2407 + channel * 5,
        Band::B5 => 5000 + channel * 5,
        Band::B6 => 5950 + channel * 5,
    }
}

/// Guesses the band from a bare channel number (used by parsers that report only channels).
pub fn band_from_channel(channel: u32) -> Band {
    if channel <= 14 {
        Band::B24
    } else {
        Band::B5
    }
}

/// Frequency range (MHz) occupied by a transmission of `width_mhz` on a primary
/// channel. For bonded channels the block is aligned the way 802.11 aligns it.
pub fn occupied_range(channel: u32, band: Band, width_mhz: u32) -> (f64, f64) {
    let primary = channel_to_freq(channel, band) as f64;
    let width = width_mhz.max(20) as f64;
    if band == Band::B24 {
        // 2.4 GHz: 20 MHz channels overlap neighbours; 40 MHz extends up or down.
        // We assume "above" for channels 1-7 and "below" otherwise.
        if width_mhz >= 40 {
            return if channel <= 7 { (primary - 10.0, primary + 30.0) } else { (primary - 30.0, primary + 10.0) };
        }
        return (primary - 11.0, primary + 11.0);
    }
    // Bonded blocks are aligned to multiples of the width from the band's raster
    // origin. UNII-3 (channel 149 and up) has its own origin at 5735 MHz.
    let origin = match band {
        Band::B5 if primary >= 5745.0 => 5735.0,
        Band::B5 => 5170.0,
        _ => 5945.0,
    };
    let low_edge = primary - 10.0 - origin;
    let start = origin + (low_edge / width).floor() * width;
    (start, start + width)
}

/// True when two transmissions overlap in frequency.
pub fn overlaps(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Material {
    Drywall,
    Glass,
    Wood,
    Brick,
    Concrete,
    Metal,
    /// Lift shaft, server room, fire door: very high loss.
    Shaft,
}

impl Material {
    /// Attenuation in dB for a single crossing.
    pub fn attenuation_db(self, band: Band) -> f64 {
        let (low, high) = match self {
            Material::Drywall => (3.0, 4.0),
            Material::Glass => (2.0, 6.0),
            Material::Wood => (3.0, 5.0),
            Material::Brick => (7.0, 11.0),
            Material::Concrete => (12.0, 20.0),
            Material::Metal => (20.0, 30.0),
            Material::Shaft => (30.0, 38.0),
        };
        match band {
            Band::B24 => low,
            Band::B5 => high,
            Band::B6 => high + 1.5,
        }
    }

    pub const ALL: [Material; 7] = [
        Material::Drywall,
        Material::Glass,
        Material::Wood,
        Material::Brick,
        Material::Concrete,
        Material::Metal,
        Material::Shaft,
    ];
}

/// A wall segment in metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wall {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub material: Material,
}

/// A transmitter position in metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Emitter {
    pub x: f64,
    pub y: f64,
    pub eirp_dbm: f64,
    pub freq_mhz: u32,
}

/// Log-distance exponent used together with explicit walls.
pub const PATH_LOSS_EXPONENT: f64 = 2.4;

/// Free-space loss at 1 m plus log-distance loss. Distances below 1 m clamp to 1 m.
pub fn path_loss_db(distance_m: f64, freq_mhz: u32) -> f64 {
    let d = distance_m.max(1.0);
    let fspl_1m = 20.0 * (freq_mhz as f64).log10() - 27.55;
    fspl_1m + 10.0 * PATH_LOSS_EXPONENT * d.log10()
}

fn orient(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> f64 {
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

/// Proper segment intersection test (touching endpoints count as a crossing).
pub fn segments_intersect(p1: (f64, f64), p2: (f64, f64), q1: (f64, f64), q2: (f64, f64)) -> bool {
    let d1 = orient(q1.0, q1.1, q2.0, q2.1, p1.0, p1.1);
    let d2 = orient(q1.0, q1.1, q2.0, q2.1, p2.0, p2.1);
    let d3 = orient(p1.0, p1.1, p2.0, p2.1, q1.0, q1.1);
    let d4 = orient(p1.0, p1.1, p2.0, p2.1, q2.0, q2.1);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// Total wall attenuation between two points.
pub fn wall_loss_db(from: (f64, f64), to: (f64, f64), walls: &[Wall], band: Band) -> f64 {
    walls
        .iter()
        .filter(|w| segments_intersect(from, to, (w.x1, w.y1), (w.x2, w.y2)))
        .map(|w| w.material.attenuation_db(band))
        .sum()
}

/// Predicted received signal strength (dBm) at (x, y).
pub fn predict_rssi(emitter: &Emitter, x: f64, y: f64, walls: &[Wall]) -> f64 {
    let band = freq_to_channel(emitter.freq_mhz).map(|(_, b)| b).unwrap_or(Band::B5);
    let d = ((emitter.x - x).powi(2) + (emitter.y - y).powi(2)).sqrt();
    emitter.eirp_dbm - path_loss_db(d, emitter.freq_mhz) - wall_loss_db((emitter.x, emitter.y), (x, y), walls, band)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_frequency_round_trip() {
        assert_eq!(freq_to_channel(2412), Some((1, Band::B24)));
        assert_eq!(freq_to_channel(2437), Some((6, Band::B24)));
        assert_eq!(freq_to_channel(2484), Some((14, Band::B24)));
        assert_eq!(freq_to_channel(5180), Some((36, Band::B5)));
        assert_eq!(freq_to_channel(5745), Some((149, Band::B5)));
        assert_eq!(freq_to_channel(5955), Some((1, Band::B6)));
        assert_eq!(freq_to_channel(6115), Some((33, Band::B6)));
        for (ch, band) in [(1, Band::B24), (11, Band::B24), (36, Band::B5), (165, Band::B5), (37, Band::B6)] {
            assert_eq!(freq_to_channel(channel_to_freq(ch, band)), Some((ch, band)));
        }
    }

    #[test]
    fn bonded_blocks_align() {
        // Channel 36 and 48 share the same 80 MHz block (36-48).
        assert_eq!(occupied_range(36, Band::B5, 80), occupied_range(48, Band::B5, 80));
        assert_ne!(occupied_range(36, Band::B5, 80), occupied_range(52, Band::B5, 80));
        assert!(overlaps(occupied_range(1, Band::B24, 20), occupied_range(3, Band::B24, 20)));
        assert!(!overlaps(occupied_range(1, Band::B24, 20), occupied_range(6, Band::B24, 20)));
        assert_eq!(occupied_range(149, Band::B5, 80), (5735.0, 5815.0));
        assert_eq!(occupied_range(161, Band::B5, 80), (5735.0, 5815.0));
        assert_eq!(occupied_range(100, Band::B5, 160), (5490.0, 5650.0));
        assert_eq!(occupied_range(1, Band::B6, 80), (5945.0, 6025.0));
    }

    #[test]
    fn walls_attenuate() {
        let e = Emitter { x: 0.0, y: 0.0, eirp_dbm: 20.0, freq_mhz: 5180 };
        let open = predict_rssi(&e, 10.0, 0.0, &[]);
        let wall = Wall { x1: 5.0, y1: -5.0, x2: 5.0, y2: 5.0, material: Material::Concrete };
        let blocked = predict_rssi(&e, 10.0, 0.0, &[wall]);
        assert!((open - blocked - 20.0).abs() < 1e-9);
        // Sanity: an AP 10 m away in open space on 5 GHz lands in a realistic range.
        assert!(open < -40.0 && open > -65.0, "{open}");
    }

    #[test]
    fn signal_decreases_with_distance() {
        let e = Emitter { x: 0.0, y: 0.0, eirp_dbm: 20.0, freq_mhz: 2437 };
        assert!(predict_rssi(&e, 2.0, 0.0, &[]) > predict_rssi(&e, 20.0, 0.0, &[]));
    }
}
