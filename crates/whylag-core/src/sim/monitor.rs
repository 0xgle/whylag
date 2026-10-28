//! Simulated monitoring data: a day of history with evening congestion and two
//! short ISP outages, plus a live stream with occasional trouble.

use crate::monitor::{Detector, Sample, Store};
use crate::util::now_ms;
use rand::{rngs::StdRng, Rng, SeedableRng};

const AP_A: &str = "24:5a:4c:a1:02:18";
const AP_B: &str = "24:5a:4c:a1:05:18";

/// Local hour for the demo (the demo office is in Warsaw, UTC+1).
fn hour_of(ts: u64) -> f64 {
    ((ts / 1000 + 3600) % 86_400) as f64 / 3600.0
}

fn synth(rng: &mut StdRng, ts: u64, outage: bool, wifi_trouble: bool) -> Sample {
    let h = hour_of(ts);
    // Evening peak: everyone streams between 19:00 and 22:30.
    let evening = if (19.0..22.5).contains(&h) {
        1.0 - ((h - 20.7).abs() / 1.8).min(1.0)
    } else {
        0.0
    };
    let congestion =
        evening * rng.gen_range(20.0..140.0) * if rng.gen::<f64>() < 0.6 { 1.0 } else { 0.25 };
    let gw = if wifi_trouble && rng.gen::<f64>() < 0.35 {
        None
    } else {
        Some(
            1.2 + rng.gen_range(0.0..2.5)
                + if wifi_trouble {
                    rng.gen_range(5.0..40.0)
                } else {
                    0.0
                },
        )
    };
    let inet = if outage || gw.is_none() {
        None
    } else {
        Some(12.0 + rng.gen_range(0.0..4.0) + congestion)
    };
    let dns = inet.map(|i| {
        i * 0.9 + rng.gen_range(18.0..30.0) + if rng.gen::<f64>() < 0.002 { 400.0 } else { 0.0 }
    });
    let rssi = if wifi_trouble {
        rng.gen_range(-80..-72)
    } else {
        rng.gen_range(-58..-50)
    };
    Sample {
        ts,
        gateway_ms: gw,
        internet_ms: inet,
        dns_ms: dns,
        rssi: Some(rssi),
        bssid: Some(if wifi_trouble { AP_B } else { AP_A }.into()),
        channel: Some(if wifi_trouble { 149 } else { 52 }),
        connected: true,
    }
}

/// Fills the demo database with the last 24 hours at 30 s resolution.
pub fn seed_history(store: &Store) {
    let mut rng = StdRng::seed_from_u64(42);
    let now = now_ms();
    let step = 30_000;
    let start = now - 24 * 3_600_000;
    let mut detector = Detector::default();
    let mut ts = start;
    while ts < now - step {
        let h = hour_of(ts);
        // Two ISP outages (20:41 for ~3 min, 06:12 for ~1 min) and a Wi-Fi
        // rough patch in the afternoon when a meeting room filled up.
        let outage = (20.68..20.73).contains(&h) || (6.2..6.22).contains(&h);
        let wifi_trouble = (15.1..15.45).contains(&h);
        let s = synth(&mut rng, ts, outage, wifi_trouble);
        let _ = store.insert(&s);
        detector.feed(&s, store);
        ts += step;
    }
}

/// Live demo stream: mostly healthy, with a latency spike every ~90 s and a
/// short ISP outage every ~5 minutes so incidents appear while you watch.
pub struct Live {
    rng: StdRng,
}

impl Default for Live {
    fn default() -> Self {
        Self {
            rng: StdRng::from_entropy(),
        }
    }
}

impl Live {
    pub fn next(&mut self, ts: u64) -> Sample {
        let cycle = (ts / 1000) % 300;
        let outage = (200..215).contains(&cycle);
        let mut s = synth(&mut self.rng, ts, outage, false);
        if (cycle % 90) < 6 {
            if let Some(v) = s.internet_ms.as_mut() {
                *v += self.rng.gen_range(120.0..260.0);
            }
        }
        s
    }
}
