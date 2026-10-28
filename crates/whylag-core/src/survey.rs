//! Site survey engine: heatmaps from walked measurements and from predicted
//! coverage, automatic channel planning and AP placement suggestions.
//!
//! Coordinates are floor-plan pixels. `meters_per_px` converts to metres for the
//! propagation model and for distance limits.
//!
//! Measured heatmaps interpolate each BSSID separately (inverse distance
//! weighting over the nearest samples) and derive the layers per cell. That is
//! more faithful than interpolating a derived value: "second-best AP" or
//! "co-channel count" depend on which radios are where.

use crate::rf::{self, Band, Material, Wall};
use crate::wifi::Security;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Signal assumed for a radio a sample did not hear (below adapter sensitivity).
const NOT_HEARD_DBM: f32 = -95.0;
/// Signals above this count as interference for co-channel analysis.
const INTERFERENCE_FLOOR_DBM: f32 = -82.0;
/// Maximum number of grid cells per heatmap.
const MAX_CELLS: f64 = 50_000.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub bssid: String,
    pub ssid: String,
    pub rssi: i32,
    pub channel: u32,
    pub band: Option<Band>,
    #[serde(default = "default_width")]
    pub width_mhz: u32,
    #[serde(default)]
    pub noise: Option<i32>,
    #[serde(default)]
    pub security: Security,
}

fn default_width() -> u32 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub t_ms: u64,
    pub readings: Vec<Reading>,
    #[serde(default)]
    pub throughput_mbps: Option<f64>,
    #[serde(default)]
    pub latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualRadio {
    pub band: Band,
    pub channel: u32,
    pub width_mhz: u32,
    pub eirp_dbm: f64,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualAp {
    pub id: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub radios: Vec<VirtualRadio>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Layer {
    /// Strongest signal (primary coverage).
    Signal,
    /// Second strongest AP: roaming redundancy and failover coverage.
    SecondSignal,
    /// Signal-to-noise ratio of the strongest AP.
    Snr,
    /// Number of APs above the coverage threshold.
    ApCount,
    /// Other radios sharing the strongest AP's frequencies above -82 dBm.
    CoChannel,
    /// Which AP is strongest (categorical).
    BestAp,
    /// Measured throughput (walked samples with a speed test).
    Throughput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Measured,
    Predicted,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    /// SSIDs that count as "your" network. Empty = every SSID.
    #[serde(default)]
    pub ssids: Vec<String>,
    /// Bands to include. Empty = all bands.
    #[serde(default)]
    pub bands: Vec<Band>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatmapRequest {
    pub mode: Mode,
    pub layer: Layer,
    pub width_px: f64,
    pub height_px: f64,
    pub meters_per_px: Option<f64>,
    #[serde(default)]
    pub samples: Vec<Sample>,
    #[serde(default)]
    pub walls: Vec<Wall>,
    #[serde(default)]
    pub aps: Vec<VirtualAp>,
    #[serde(default)]
    pub filter: Filter,
    #[serde(default = "default_threshold")]
    pub threshold_dbm: i32,
    /// Measured mode: how far (metres) from the nearest sample a cell may be.
    #[serde(default = "default_reach")]
    pub reach_m: f64,
}

fn default_threshold() -> i32 {
    -67
}

fn default_reach() -> f64 {
    5.0
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegendEntry {
    pub index: u32,
    pub label: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HeatmapStats {
    pub cells: usize,
    pub covered_cells: usize,
    /// Share of the mapped area meeting the layer's goal, 0..1.
    pub coverage_ratio: f64,
    pub area_m2: Option<f64>,
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub mean: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Heatmap {
    pub cols: usize,
    pub rows: usize,
    pub cell_px: f64,
    pub layer: Layer,
    pub mode: Mode,
    /// Row-major values; `None` = no data (outside the surveyed area).
    pub values: Vec<Option<f32>>,
    /// Labels for categorical layers (best AP).
    pub legend: Vec<LegendEntry>,
    pub stats: HeatmapStats,
}

/// One radio's signal field over the grid.
struct Field {
    key: String,
    /// Physical radio identity: one radio advertises several SSIDs with
    /// BSSIDs that differ only in the last octet.
    radio: String,
    label: String,
    band: Option<Band>,
    range: Option<(f64, f64)>,
    noise: f32,
    /// Belongs to the filtered ("your") network set.
    mine: bool,
    values: Vec<f32>,
}

struct Grid {
    cols: usize,
    rows: usize,
    cell: f64,
}

impl Grid {
    fn new(width: f64, height: f64) -> Self {
        let cell = ((width * height / MAX_CELLS).sqrt()).ceil().max(2.0);
        Grid { cols: (width / cell).ceil().max(1.0) as usize, rows: (height / cell).ceil().max(1.0) as usize, cell }
    }

    fn center(&self, i: usize) -> (f64, f64) {
        let (c, r) = (i % self.cols, i / self.cols);
        ((c as f64 + 0.5) * self.cell, (r as f64 + 0.5) * self.cell)
    }

    fn len(&self) -> usize {
        self.cols * self.rows
    }
}

fn matches_filter(filter: &Filter, ssid: &str, band: Option<Band>) -> bool {
    (filter.ssids.is_empty() || filter.ssids.iter().any(|s| s == ssid))
        && (filter.bands.is_empty() || band.map(|b| filter.bands.contains(&b)).unwrap_or(false))
}

/// For each cell, the nearest samples (index, weight) within reach.
fn neighbours(grid: &Grid, samples: &[Sample], reach_px: f64, k: usize) -> Vec<Vec<(usize, f64)>> {
    // Bucket samples so each cell only checks nearby buckets.
    let bucket = reach_px.max(grid.cell);
    let mut buckets: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in samples.iter().enumerate() {
        buckets.entry(((s.x / bucket).floor() as i64, (s.y / bucket).floor() as i64)).or_default().push(i);
    }
    (0..grid.len())
        .map(|i| {
            let (x, y) = grid.center(i);
            let (bx, by) = ((x / bucket).floor() as i64, (y / bucket).floor() as i64);
            let mut near: Vec<(usize, f64)> = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(list) = buckets.get(&(bx + dx, by + dy)) {
                        for &j in list {
                            let d = ((samples[j].x - x).powi(2) + (samples[j].y - y).powi(2)).sqrt();
                            if d <= reach_px {
                                near.push((j, d));
                            }
                        }
                    }
                }
            }
            near.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            near.truncate(k);
            // Inverse distance weights (power 2); a sample right on the cell dominates.
            let floor = grid.cell * 0.35;
            near.into_iter().map(|(j, d)| (j, 1.0 / d.max(floor).powi(2))).collect()
        })
        .collect()
}

fn measured_fields(req: &HeatmapRequest, grid: &Grid, near: &[Vec<(usize, f64)>]) -> Vec<Field> {
    // Collect every radio heard anywhere.
    let mut radios: BTreeMap<String, &Reading> = BTreeMap::new();
    for s in &req.samples {
        for r in &s.readings {
            radios.entry(r.bssid.clone()).or_insert(r);
        }
    }
    // Per-sample lookup: bssid -> rssi.
    let lookup: Vec<HashMap<&str, i32>> =
        req.samples.iter().map(|s| s.readings.iter().map(|r| (r.bssid.as_str(), r.rssi)).collect()).collect();
    radios
        .into_iter()
        .map(|(bssid, r)| {
            let values = (0..grid.len())
                .map(|i| {
                    let n = &near[i];
                    if n.is_empty() {
                        return f32::NAN;
                    }
                    let (mut sum, mut wsum) = (0.0f64, 0.0f64);
                    for &(j, w) in n {
                        let v = lookup[j].get(bssid.as_str()).map(|v| *v as f32).unwrap_or(NOT_HEARD_DBM);
                        sum += v as f64 * w;
                        wsum += w;
                    }
                    (sum / wsum) as f32
                })
                .collect();
            let band = r.band.or_else(|| Some(rf::band_from_channel(r.channel)));
            Field {
                label: if r.ssid.is_empty() { bssid.clone() } else { format!("{} ({})", r.ssid, r.channel) },
                range: band.map(|b| rf::occupied_range(r.channel, b, r.width_mhz)),
                noise: r.noise.map(|n| n as f32).unwrap_or_else(|| band.map(|b| b.typical_noise_dbm() as f32).unwrap_or(-95.0)),
                mine: matches_filter(&req.filter, &r.ssid, band),
                radio: format!("{}/{}", &bssid[..bssid.len().min(14)], r.channel),
                key: bssid,
                band,
                values,
            }
        })
        .collect()
}

fn predicted_fields(req: &HeatmapRequest, grid: &Grid) -> Vec<Field> {
    let mpp = req.meters_per_px.unwrap_or(0.05);
    let walls_m: Vec<Wall> =
        req.walls.iter().map(|w| Wall { x1: w.x1 * mpp, y1: w.y1 * mpp, x2: w.x2 * mpp, y2: w.y2 * mpp, material: w.material }).collect();
    let mut fields = Vec::new();
    for ap in &req.aps {
        for (ri, radio) in ap.radios.iter().enumerate().filter(|(_, r)| r.enabled) {
            if !req.filter.bands.is_empty() && !req.filter.bands.contains(&radio.band) {
                continue;
            }
            let emitter = rf::Emitter {
                x: ap.x * mpp,
                y: ap.y * mpp,
                eirp_dbm: radio.eirp_dbm,
                freq_mhz: rf::channel_to_freq(radio.channel, radio.band),
            };
            let values = (0..grid.len())
                .map(|i| {
                    let (x, y) = grid.center(i);
                    rf::predict_rssi(&emitter, x * mpp, y * mpp, &walls_m) as f32
                })
                .collect();
            fields.push(Field {
                key: format!("{}#{}", ap.id, ri),
                radio: format!("{}#{}", ap.id, ri),
                label: format!("{} ({} GHz ch {})", ap.name, radio.band.label().trim_end_matches(" GHz"), radio.channel),
                band: Some(radio.band),
                range: Some(rf::occupied_range(radio.channel, radio.band, radio.width_mhz)),
                noise: radio.band.typical_noise_dbm() as f32,
                mine: true,
                values,
            });
        }
    }
    fields
}

/// Computes a heatmap layer.
pub fn heatmap(req: &HeatmapRequest) -> Heatmap {
    let grid = Grid::new(req.width_px.max(1.0), req.height_px.max(1.0));
    let threshold = req.threshold_dbm as f32;
    let mpp = req.meters_per_px;

    let (fields, mask): (Vec<Field>, Vec<bool>) = match req.mode {
        Mode::Measured => {
            let reach_px = match mpp {
                Some(m) if m > 0.0 => req.reach_m / m,
                _ => (req.width_px.powi(2) + req.height_px.powi(2)).sqrt() * 0.08,
            };
            let near = neighbours(&grid, &req.samples, reach_px, 8);
            let mask = near.iter().map(|n| !n.is_empty()).collect();
            (measured_fields(req, &grid, &near), mask)
        }
        Mode::Predicted => (predicted_fields(req, &grid), vec![true; grid.len()]),
    };

    let mine: Vec<&Field> = fields.iter().filter(|f| f.mine).collect();
    let mut legend: Vec<LegendEntry> = Vec::new();
    let mut legend_index: HashMap<String, u32> = HashMap::new();

    // Throughput is a scalar layer interpolated directly from samples.
    let throughput: Option<Vec<f32>> = (req.layer == Layer::Throughput).then(|| {
        let with: Vec<Sample> = req.samples.iter().filter(|s| s.throughput_mbps.is_some()).cloned().collect();
        let reach_px = mpp.map(|m| req.reach_m / m).unwrap_or(req.width_px * 0.1);
        let near = neighbours(&grid, &with, reach_px, 6);
        near.iter()
            .map(|n| {
                if n.is_empty() {
                    return f32::NAN;
                }
                let (s, w) = n.iter().fold((0.0, 0.0), |(s, w), &(j, wt)| (s + with[j].throughput_mbps.unwrap() * wt, w + wt));
                (s / w) as f32
            })
            .collect()
    });

    let values: Vec<Option<f32>> = (0..grid.len())
        .map(|i| {
            if !mask[i] {
                return None;
            }
            if let Some(tp) = &throughput {
                return tp[i].is_finite().then_some(tp[i]);
            }
            // Strongest "your" radio here, and the strongest *other* radio.
            let mut best: Option<&Field> = None;
            for f in &mine {
                if f.values[i].is_finite() && best.map(|b| f.values[i] > b.values[i]).unwrap_or(true) {
                    best = Some(f);
                }
            }
            let best = best?;
            let best_v = best.values[i];
            if best_v <= NOT_HEARD_DBM + 0.5 {
                return Some(match req.layer {
                    Layer::Signal | Layer::SecondSignal | Layer::Snr => NOT_HEARD_DBM,
                    _ => 0.0,
                });
            }
            Some(match req.layer {
                Layer::Signal => best_v,
                Layer::SecondSignal => mine
                    .iter()
                    .filter(|f| f.radio != best.radio && f.values[i].is_finite())
                    .map(|f| f.values[i])
                    .fold(NOT_HEARD_DBM, f32::max),
                Layer::Snr => best_v - best.noise,
                Layer::ApCount => {
                    let mut radios: Vec<&str> = mine.iter().filter(|f| f.values[i] >= threshold).map(|f| f.radio.as_str()).collect();
                    radios.sort_unstable();
                    radios.dedup();
                    radios.len() as f32
                }
                Layer::CoChannel => {
                    let Some(range) = best.range else { return Some(0.0) };
                    // Any radio (yours or a neighbour's) loud enough to defer to.
                    let mut radios: Vec<&str> = fields
                        .iter()
                        .filter(|f| {
                            f.radio != best.radio
                                && f.band == best.band
                                && f.values[i] >= INTERFERENCE_FLOOR_DBM
                                && f.range.map(|r| rf::overlaps(r, range)).unwrap_or(false)
                        })
                        .map(|f| f.radio.as_str())
                        .collect();
                    radios.sort_unstable();
                    radios.dedup();
                    radios.len() as f32
                }
                Layer::BestAp => {
                    let next = legend_index.len() as u32;
                    let idx = *legend_index.entry(best.radio.clone()).or_insert_with(|| {
                        legend.push(LegendEntry { index: next, label: best.label.clone(), detail: best.key.clone() });
                        next
                    });
                    idx as f32
                }
                Layer::Throughput => unreachable!(),
            })
        })
        .collect();

    let stats = stats(&values, req.layer, threshold, grid.cell, mpp);
    Heatmap { cols: grid.cols, rows: grid.rows, cell_px: grid.cell, layer: req.layer, mode: req.mode, values, legend, stats }
}

fn stats(values: &[Option<f32>], layer: Layer, threshold: f32, cell_px: f64, mpp: Option<f64>) -> HeatmapStats {
    let present: Vec<f32> = values.iter().flatten().copied().collect();
    if present.is_empty() {
        return HeatmapStats::default();
    }
    let covered = present
        .iter()
        .filter(|v| match layer {
            Layer::Signal | Layer::SecondSignal => **v >= threshold,
            Layer::Snr => **v >= 25.0,
            Layer::ApCount => **v >= 2.0,
            Layer::CoChannel => **v <= 1.0,
            Layer::Throughput => **v >= 50.0,
            Layer::BestAp => true,
        })
        .count();
    let sum: f64 = present.iter().map(|v| *v as f64).sum();
    HeatmapStats {
        cells: present.len(),
        covered_cells: covered,
        coverage_ratio: covered as f64 / present.len() as f64,
        area_m2: mpp.map(|m| present.len() as f64 * (cell_px * m).powi(2)),
        min: present.iter().copied().reduce(f32::min),
        max: present.iter().copied().reduce(f32::max),
        mean: Some((sum / present.len() as f64) as f32),
    }
}

// ---------------------------------------------------------------------------
// Channel planning
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRadio {
    pub channel: u32,
    pub band: Band,
    pub width_mhz: u32,
    pub rssi: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPlanRequest {
    pub width_px: f64,
    pub height_px: f64,
    pub meters_per_px: Option<f64>,
    #[serde(default)]
    pub walls: Vec<Wall>,
    pub aps: Vec<VirtualAp>,
    pub band: Band,
    pub width_mhz: u32,
    /// Allow DFS channels (52-144) on 5 GHz.
    #[serde(default = "yes")]
    pub allow_dfs: bool,
    /// Neighbouring networks measured on site, added as interference cost.
    #[serde(default)]
    pub external: Vec<ExternalRadio>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAssignment {
    pub ap_id: String,
    pub ap_name: String,
    pub from: Option<u32>,
    pub to: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPlan {
    pub band: Band,
    pub width_mhz: u32,
    pub assignments: Vec<ChannelAssignment>,
    /// Relative interference score before and after (lower is better).
    pub cost_before: f64,
    pub cost_after: f64,
}

/// Candidate primary channels for a band and width (one per non-overlapping block).
pub fn candidate_channels(band: Band, width_mhz: u32, allow_dfs: bool) -> Vec<u32> {
    let is_dfs = |c: u32| (52..=144).contains(&c);
    let list: Vec<u32> = match (band, width_mhz) {
        (Band::B24, _) => vec![1, 6, 11],
        (Band::B5, w) if w >= 160 => vec![36, 100, 149],
        (Band::B5, 80) => vec![36, 52, 100, 116, 132, 149],
        (Band::B5, 40) => vec![36, 44, 52, 60, 100, 108, 116, 124, 132, 140, 149, 157],
        (Band::B5, _) => vec![36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144, 149, 153, 157, 161, 165],
        (Band::B6, w) if w >= 320 => vec![1, 65, 129],
        (Band::B6, w) if w >= 160 => vec![1, 33, 65, 97, 129, 161, 193],
        (Band::B6, 80) => (0..14).map(|i| 1 + i * 16).collect(),
        (Band::B6, 40) => (0..29).map(|i| 1 + i * 8).collect(),
        (Band::B6, _) => (0..59).map(|i| 1 + i * 4).collect(),
    };
    let filtered: Vec<u32> = list.iter().copied().filter(|c| allow_dfs || band != Band::B5 || !is_dfs(*c) || *c == 36).collect();
    if filtered.len() >= 2 {
        filtered
    } else {
        list
    }
}

/// Plans channels so APs that hear each other share as little airtime as possible.
pub fn channel_plan(req: &ChannelPlanRequest) -> ChannelPlan {
    let mpp = req.meters_per_px.unwrap_or(0.05);
    let walls_m: Vec<Wall> =
        req.walls.iter().map(|w| Wall { x1: w.x1 * mpp, y1: w.y1 * mpp, x2: w.x2 * mpp, y2: w.y2 * mpp, material: w.material }).collect();
    let aps: Vec<(&VirtualAp, Option<&VirtualRadio>)> =
        req.aps.iter().map(|ap| (ap, ap.radios.iter().find(|r| r.band == req.band && r.enabled))).filter(|(_, r)| r.is_some()).collect();
    let n = aps.len();
    let freq = rf::channel_to_freq(match req.band {
        Band::B24 => 6,
        Band::B5 => 36,
        Band::B6 => 37,
    }, req.band);
    // Pairwise coupling: how loud AP j is at AP i (and vice versa), mapped to 0..1.
    let mut coupling = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            let (a, ra) = aps[i];
            let (b, _) = aps[j];
            let e = rf::Emitter { x: b.x * mpp, y: b.y * mpp, eirp_dbm: ra.unwrap().eirp_dbm, freq_mhz: freq };
            let rssi = rf::predict_rssi(&e, a.x * mpp, a.y * mpp, &walls_m);
            coupling[i][j] = ((rssi + 90.0) / 40.0).clamp(0.0, 1.0);
        }
    }
    let candidates = candidate_channels(req.band, req.width_mhz, req.allow_dfs);
    let range = |c: u32| rf::occupied_range(c, req.band, req.width_mhz);
    let external_cost = |c: u32| -> f64 {
        req.external
            .iter()
            .filter(|e| e.band == req.band && rf::overlaps(rf::occupied_range(e.channel, e.band, e.width_mhz), range(c)))
            .map(|e| ((e.rssi as f64 + 90.0) / 40.0).clamp(0.0, 1.0) * 0.6)
            .sum()
    };
    let cost = |assign: &[u32]| -> f64 {
        let mut total = 0.0;
        for i in 0..n {
            for j in (i + 1)..n {
                if rf::overlaps(range(assign[i]), range(assign[j])) {
                    total += coupling[i][j] + coupling[j][i];
                }
            }
            total += external_cost(assign[i]);
        }
        total
    };
    let current: Vec<u32> = aps.iter().map(|(_, r)| r.unwrap().channel).collect();
    let cost_before = cost(&current);
    // Greedy colouring (most coupled AP first), then local search improvements.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        let sa: f64 = coupling[a].iter().sum();
        let sb: f64 = coupling[b].iter().sum();
        sb.partial_cmp(&sa).unwrap()
    });
    let mut assign = vec![candidates[0]; n];
    let mut placed = vec![false; n];
    for &i in &order {
        let best = candidates
            .iter()
            .copied()
            .min_by(|&c1, &c2| {
                let score = |c: u32| -> f64 {
                    let mut s = external_cost(c);
                    for j in 0..n {
                        if placed[j] && rf::overlaps(range(c), range(assign[j])) {
                            s += coupling[i][j] + coupling[j][i];
                        }
                    }
                    s
                };
                score(c1).partial_cmp(&score(c2)).unwrap()
            })
            .unwrap();
        assign[i] = best;
        placed[i] = true;
    }
    for _ in 0..6 {
        let mut improved = false;
        for i in 0..n {
            for &c in &candidates {
                let mut trial = assign.clone();
                trial[i] = c;
                if cost(&trial) + 1e-9 < cost(&assign) {
                    assign = trial;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
    let cost_after = cost(&assign).min(cost_before);
    let final_assign = if cost(&assign) <= cost_before { assign } else { current.clone() };
    ChannelPlan {
        band: req.band,
        width_mhz: req.width_mhz,
        assignments: aps
            .iter()
            .zip(final_assign)
            .map(|((ap, r), to)| ChannelAssignment { ap_id: ap.id.clone(), ap_name: ap.name.clone(), from: r.map(|r| r.channel), to })
            .collect(),
        cost_before,
        cost_after,
    }
}

// ---------------------------------------------------------------------------
// AP placement
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementRequest {
    #[serde(flatten)]
    pub heatmap: HeatmapRequest,
    /// Radio of the AP to add (defaults to a 5 GHz enterprise AP).
    #[serde(default)]
    pub radio: Option<VirtualRadio>,
    #[serde(default = "default_suggestions")]
    pub count: usize,
}

fn default_suggestions() -> usize {
    3
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementSuggestion {
    pub x: f64,
    pub y: f64,
    pub coverage_before: f64,
    pub coverage_after: f64,
}

/// Suggests where one additional AP would close the most coverage gaps.
pub fn suggest_placement(req: &PlacementRequest) -> Vec<PlacementSuggestion> {
    let mut base_req = req.heatmap.clone();
    base_req.layer = Layer::Signal;
    let base = heatmap(&base_req);
    let radio = req.radio.clone().unwrap_or(VirtualRadio { band: Band::B5, channel: 36, width_mhz: 80, eirp_dbm: 17.0, enabled: true });
    let mpp = req.heatmap.meters_per_px.unwrap_or(0.05);
    let threshold = req.heatmap.threshold_dbm as f32;
    let walls_m: Vec<Wall> = req
        .heatmap
        .walls
        .iter()
        .map(|w| Wall { x1: w.x1 * mpp, y1: w.y1 * mpp, x2: w.x2 * mpp, y2: w.y2 * mpp, material: w.material })
        .collect();
    // Evaluate on a coarser grid for speed.
    let stride = ((base.cols * base.rows) as f64 / 4000.0).sqrt().ceil().max(1.0) as usize;
    let cells: Vec<(f64, f64, f32)> = (0..base.rows)
        .step_by(stride)
        .flat_map(|r| (0..base.cols).step_by(stride).map(move |c| (r, c)))
        .filter_map(|(r, c)| {
            base.values[r * base.cols + c].map(|v| ((c as f64 + 0.5) * base.cell_px, (r as f64 + 0.5) * base.cell_px, v))
        })
        .collect();
    if cells.is_empty() {
        return vec![];
    }
    let covered_before = cells.iter().filter(|c| c.2 >= threshold).count() as f64 / cells.len() as f64;
    // Candidate positions: every ~2 m across the mapped area.
    let step_px = (2.0 / mpp).max(base.cell_px);
    let mut candidates = Vec::new();
    let mut y = step_px / 2.0;
    while y < req.heatmap.height_px {
        let mut x = step_px / 2.0;
        while x < req.heatmap.width_px {
            candidates.push((x, y));
            x += step_px;
        }
        y += step_px;
    }
    let freq = rf::channel_to_freq(radio.channel, radio.band);
    // Keep candidates inside the surveyed space (bounding box of the samples, or
    // of the walls in a predictive plan) and slightly away from walls.
    let points: Vec<(f64, f64)> = if !req.heatmap.samples.is_empty() {
        req.heatmap.samples.iter().map(|s| (s.x, s.y)).collect()
    } else {
        req.heatmap.walls.iter().flat_map(|w| [(w.x1, w.y1), (w.x2, w.y2)]).collect()
    };
    let inset = 0.5 / mpp;
    let bounds = if points.is_empty() {
        (0.0, 0.0, req.heatmap.width_px, req.heatmap.height_px)
    } else {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (x, y) in &points {
            x0 = x0.min(*x);
            y0 = y0.min(*y);
            x1 = x1.max(*x);
            y1 = y1.max(*y);
        }
        (x0 + inset, y0 + inset, x1 - inset, y1 - inset)
    };
    let clearance = 0.4 / mpp;
    let near_wall = |x: f64, y: f64| {
        req.heatmap.walls.iter().any(|w| point_segment_distance(x, y, w.x1, w.y1, w.x2, w.y2) < clearance)
    };
    let mut scored: Vec<PlacementSuggestion> = candidates
        .into_iter()
        // Only places inside the mapped area.
        .filter(|(x, y)| {
            let (c, r) = ((x / base.cell_px) as usize, (y / base.cell_px) as usize);
            c < base.cols && r < base.rows && base.values[r * base.cols + c].is_some()
        })
        .filter(|(x, y)| *x >= bounds.0 && *y >= bounds.1 && *x <= bounds.2 && *y <= bounds.3 && !near_wall(*x, *y))
        .map(|(x, y)| {
            let e = rf::Emitter { x: x * mpp, y: y * mpp, eirp_dbm: radio.eirp_dbm, freq_mhz: freq };
            let covered = cells
                .iter()
                .filter(|(cx, cy, v)| *v >= threshold || rf::predict_rssi(&e, cx * mpp, cy * mpp, &walls_m) as f32 >= threshold)
                .count();
            PlacementSuggestion { x, y, coverage_before: covered_before, coverage_after: covered as f64 / cells.len() as f64 }
        })
        .collect();
    scored.sort_by(|a, b| b.coverage_after.partial_cmp(&a.coverage_after).unwrap());
    // Keep suggestions at least ~6 m apart so they are real alternatives.
    let min_sep = 6.0 / mpp;
    let mut out: Vec<PlacementSuggestion> = Vec::new();
    for s in scored {
        if out.iter().all(|o| ((o.x - s.x).powi(2) + (o.y - s.y).powi(2)).sqrt() >= min_sep) {
            out.push(s);
        }
        if out.len() >= req.count {
            break;
        }
    }
    out
}

fn point_segment_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((px - x1) * dx + (py - y1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    ((px - (x1 + t * dx)).powi(2) + (py - (y1 + t * dy)).powi(2)).sqrt()
}

/// Built-in material list for the wall drawing tool (with dB loss per band).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialInfo {
    pub material: Material,
    pub loss_24: f64,
    pub loss_5: f64,
    pub loss_6: f64,
}

pub fn materials() -> Vec<MaterialInfo> {
    Material::ALL
        .iter()
        .map(|m| MaterialInfo {
            material: *m,
            loss_24: m.attenuation_db(Band::B24),
            loss_5: m.attenuation_db(Band::B5),
            loss_6: m.attenuation_db(Band::B6),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{SimWorld, MARGIN_M, PX_PER_M};

    fn walk_sim() -> (Vec<Sample>, crate::sim::FloorPlan) {
        let mut world = SimWorld::new();
        let plan = world.info().floor_plan;
        let mut samples = Vec::new();
        // Walk a 2 m grid over the office.
        let mut y = 1.0;
        while y < 26.0 {
            let mut x = 1.0;
            while x < 42.0 {
                let (px, py) = ((x + MARGIN_M) * PX_PER_M, (y + MARGIN_M) * PX_PER_M);
                world.set_position_px(px, py);
                let scan = world.wifi_scan();
                samples.push(Sample {
                    x: px,
                    y: py,
                    t_ms: 0,
                    readings: scan
                        .networks
                        .iter()
                        .map(|n| Reading {
                            bssid: n.bssid.clone(),
                            ssid: n.ssid.clone(),
                            rssi: n.rssi,
                            channel: n.channel,
                            band: n.band,
                            width_mhz: n.width_mhz,
                            noise: n.noise,
                            security: n.security,
                        })
                        .collect(),
                    throughput_mbps: None,
                    latency_ms: None,
                });
                x += 2.0;
            }
            y += 2.0;
        }
        (samples, plan)
    }

    fn request(samples: Vec<Sample>, plan: &crate::sim::FloorPlan, layer: Layer) -> HeatmapRequest {
        HeatmapRequest {
            mode: Mode::Measured,
            layer,
            width_px: plan.width_px as f64,
            height_px: plan.height_px as f64,
            meters_per_px: Some(plan.meters_per_px),
            samples,
            walls: plan.walls_px.clone(),
            aps: vec![],
            filter: Filter { ssids: vec!["Acme-Office".into()], bands: vec![Band::B5] },
            threshold_dbm: -67,
            reach_m: 5.0,
        }
    }

    #[test]
    fn measured_heatmap_finds_the_coverage_hole() {
        let (samples, plan) = walk_sim();
        let map = heatmap(&request(samples, &plan, Layer::Signal));
        assert!(map.cols * map.rows <= 50_000);
        let at = |xm: f64, ym: f64| {
            let (px, py) = ((xm + MARGIN_M) * PX_PER_M, (ym + MARGIN_M) * PX_PER_M);
            map.values[(py / map.cell_px) as usize * map.cols + (px / map.cell_px) as usize].unwrap()
        };
        assert!(at(13.0, 6.0) > -55.0, "near AP-02: {}", at(13.0, 6.0));
        assert!(at(39.0, 23.0) < -67.0, "SE office: {}", at(39.0, 23.0));
        let ratio = map.stats.coverage_ratio;
        assert!(ratio > 0.6 && ratio < 0.97, "coverage {ratio}");
    }

    #[test]
    fn layers_compute() {
        let (samples, plan) = walk_sim();
        for layer in [Layer::SecondSignal, Layer::Snr, Layer::ApCount, Layer::CoChannel, Layer::BestAp] {
            let map = heatmap(&request(samples.clone(), &plan, layer));
            assert!(map.values.iter().flatten().count() > 1000, "{layer:?}");
        }
        let best = heatmap(&request(samples, &plan, Layer::BestAp));
        assert!(best.legend.len() >= 4, "legend {:?}", best.legend.len());
    }

    fn demo_aps() -> Vec<VirtualAp> {
        let ap = |id: &str, x: f64, y: f64, ch: u32| VirtualAp {
            id: id.into(),
            name: id.into(),
            x: (x + MARGIN_M) * PX_PER_M,
            y: (y + MARGIN_M) * PX_PER_M,
            radios: vec![VirtualRadio { band: Band::B5, channel: ch, width_mhz: 80, eirp_dbm: 17.0, enabled: true }],
        };
        vec![ap("A", 5.0, 5.0, 36), ap("B", 13.0, 6.0, 36), ap("C", 36.0, 4.0, 36), ap("D", 9.0, 21.0, 36), ap("E", 25.0, 13.0, 36)]
    }

    #[test]
    fn channel_plan_separates_neighbours() {
        let plan = SimWorld::new().info().floor_plan;
        let req = ChannelPlanRequest {
            width_px: plan.width_px as f64,
            height_px: plan.height_px as f64,
            meters_per_px: Some(plan.meters_per_px),
            walls: plan.walls_px.clone(),
            aps: demo_aps(),
            band: Band::B5,
            width_mhz: 80,
            allow_dfs: true,
            external: vec![],
        };
        let result = channel_plan(&req);
        assert!(result.cost_after < result.cost_before);
        let chans: std::collections::HashSet<u32> = result.assignments.iter().map(|a| a.to).collect();
        assert!(chans.len() >= 4, "{chans:?}");
    }

    #[test]
    fn placement_targets_the_south_east() {
        let (samples, plan) = walk_sim();
        let req = PlacementRequest { heatmap: request(samples, &plan, Layer::Signal), radio: None, count: 3 };
        let out = suggest_placement(&req);
        assert!(!out.is_empty());
        let best = &out[0];
        let xm = best.x / PX_PER_M - MARGIN_M;
        let ym = best.y / PX_PER_M - MARGIN_M;
        assert!(xm > 30.0 && ym > 14.0, "suggested ({xm:.1}, {ym:.1})");
        assert!(best.coverage_after > best.coverage_before);
    }

    #[test]
    fn predicted_mode_needs_no_samples() {
        let plan = SimWorld::new().info().floor_plan;
        let req = HeatmapRequest {
            mode: Mode::Predicted,
            layer: Layer::Signal,
            width_px: plan.width_px as f64,
            height_px: plan.height_px as f64,
            meters_per_px: Some(plan.meters_per_px),
            samples: vec![],
            walls: plan.walls_px.clone(),
            aps: demo_aps(),
            filter: Filter::default(),
            threshold_dbm: -67,
            reach_m: 5.0,
        };
        let map = heatmap(&req);
        assert_eq!(map.values.iter().flatten().count(), map.cols * map.rows);
    }
}
