//! Roaming analysis: while the user walks around, records which access point
//! the computer is connected to, its signal, and whether the router keeps
//! answering. Every change of access point becomes a roam event with the
//! signal before and after and how long the connection stalled, so sticky
//! clients (holding on to a weak AP) and slow handovers are visible.

use crate::net;
use crate::state::AppState;
use crate::util::now_ms;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Sticky: the old AP was this weak when the client finally left it, and the
/// new one was much stronger, so it should have moved earlier.
pub const STICKY_DBM: i32 = -72;
pub const STICKY_GAIN_DB: i32 = 10;
/// A stall longer than this during a roam is noticeable in calls.
pub const SLOW_GAP_MS: f64 = 150.0;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoamSample {
    pub t: u64,
    pub bssid: Option<String>,
    pub rssi: Option<i32>,
    pub channel: Option<u32>,
    /// Router round trip; `None` when the probe got no answer.
    pub gw_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoamEvent {
    pub t: u64,
    /// "roam", "disconnect" or "reconnect".
    pub kind: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub from_rssi: Option<i32>,
    pub to_rssi: Option<i32>,
    pub from_channel: Option<u32>,
    pub to_channel: Option<u32>,
    /// Longest time the router did not answer around the change.
    pub gap_ms: Option<f64>,
    pub sticky: bool,
    pub slow: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RoamRun {
    pub running: bool,
    pub phase: String,
    pub progress: f64,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub simulated: bool,
    pub ssid: Option<String>,
    pub gateway: Option<String>,
    pub events: Vec<RoamEvent>,
    /// Recent samples (last few minutes) for the timeline chart.
    pub samples: Vec<RoamSample>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct RoamHandle {
    pub cancel: AtomicBool,
}

const KEEP_SAMPLES: usize = 3000;
const PING_EVERY: Duration = Duration::from_millis(100);
const WIFI_EVERY_MS: u64 = 500;
/// A forgotten test stops by itself.
const MAX_RUN_MS: u64 = 30 * 60_000;

/// Turns the sample stream into events. Pure, so it is testable.
#[derive(Default)]
pub struct Tracker {
    last: Option<RoamSample>,
    /// The last sample with a known AP (for the "from" side of a roam).
    last_connected: Option<RoamSample>,
}

impl Tracker {
    /// Feeds a Wi-Fi reading; returns a new event if the AP changed.
    pub fn wifi(&mut self, s: RoamSample) -> Option<RoamEvent> {
        let prev = self.last.replace(s.clone());
        let before = self.last_connected.clone();
        if s.bssid.is_some() {
            self.last_connected = Some(s.clone());
        }
        let prev = prev?;
        let ev = |kind: &str, from: Option<&RoamSample>| RoamEvent {
            t: s.t,
            kind: kind.into(),
            from: from.and_then(|f| f.bssid.clone()),
            to: s.bssid.clone(),
            from_rssi: from.and_then(|f| f.rssi),
            to_rssi: s.rssi,
            from_channel: from.and_then(|f| f.channel),
            to_channel: s.channel,
            gap_ms: None,
            sticky: match (from.and_then(|f| f.rssi), s.rssi) {
                (Some(old), Some(new)) => old <= STICKY_DBM && new - old >= STICKY_GAIN_DB,
                _ => false,
            },
            slow: false,
        };
        match (&prev.bssid, &s.bssid) {
            (Some(a), Some(b)) if !a.eq_ignore_ascii_case(b) => Some(ev("roam", Some(&prev))),
            (Some(_), None) => Some(ev("disconnect", Some(&prev))),
            (None, Some(_)) => {
                // Back after a drop: a roam if it is a different AP.
                let changed = before
                    .as_ref()
                    .and_then(|b| b.bssid.as_ref())
                    .is_some_and(|b| !s.bssid.as_ref().unwrap().eq_ignore_ascii_case(b));
                Some(ev(
                    if changed { "roam" } else { "reconnect" },
                    before.as_ref(),
                ))
            }
            _ => None,
        }
    }
}

/// Longest gap between router answers in the window around `t`.
pub fn gap_around(samples: &[RoamSample], t: u64, window_ms: u64) -> Option<f64> {
    let answered: Vec<u64> = samples
        .iter()
        .filter(|s| s.gw_ms.is_some() && s.t + window_ms >= t && s.t <= t + window_ms)
        .map(|s| s.t)
        .collect();
    if answered.len() < 2 {
        return None;
    }
    // Probes are 100 ms apart: a gap of one interval is normal, not a stall.
    let worst = answered.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0) as f64;
    Some((worst - PING_EVERY.as_millis() as f64).max(0.0))
}

pub fn start(state: Arc<AppState>) -> RoamRun {
    {
        let mut run = state.roam.lock();
        if run.running {
            return run.clone();
        }
        *run = RoamRun {
            running: true,
            phase: "running".into(),
            started_ms: now_ms(),
            simulated: state.use_simulator(),
            ..Default::default()
        };
    }
    state.roam_handle.cancel.store(false, Ordering::SeqCst);
    let st = state.clone();
    tokio::spawn(async move {
        let result = crate::util::catch_job(run(st.clone())).await;
        let mut run = st.roam.lock();
        if let Err(e) = result {
            run.error = Some(e);
        }
        run.running = false;
        run.phase = "done".into();
        run.finished_ms = Some(now_ms());
    });
    state.roam.lock().clone()
}

pub fn stop(state: &AppState) {
    state.roam_handle.cancel.store(true, Ordering::SeqCst);
}

/// A slow lap around the demo office, past the corners and the coverage hole
/// in the south-east, so the (sticky) simulated client has to roam.
fn sim_walk(elapsed_s: f64) -> (f64, f64) {
    const ROUTE: [(f64, f64); 7] = [
        (3.0, 4.0),
        (21.0, 4.0),
        (39.0, 3.0),
        (40.0, 23.0),
        (21.0, 22.0),
        (3.0, 23.0),
        (3.0, 4.0),
    ];
    const LAP_S: f64 = 70.0;
    let legs = (ROUTE.len() - 1) as f64;
    let p = (elapsed_s % LAP_S) / LAP_S * legs;
    let i = (p.floor() as usize).min(ROUTE.len() - 2);
    let f = p - i as f64;
    let (a, b) = (ROUTE[i], ROUTE[i + 1]);
    (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f)
}

async fn run(state: Arc<AppState>) -> Result<(), String> {
    let simulated = state.use_simulator();
    let local = if simulated {
        Some(crate::sim::lan::local())
    } else {
        tokio::task::spawn_blocking(net::local_net)
            .await
            .ok()
            .flatten()
    };
    let gateway: Option<Ipv4Addr> = local.as_ref().and_then(|l| l.gateway);
    state.roam.lock().gateway = gateway.map(|g| g.to_string());
    let mut tracker = Tracker::default();
    let mut samples: VecDeque<RoamSample> = VecDeque::new();
    let mut current: (Option<String>, Option<i32>, Option<u32>) = (None, None, None);
    let started = now_ms();
    // Events wait ~2 s for the router answers after them before their gap is final.
    let mut unfinished: Vec<usize> = Vec::new();
    // Simulated handover stall in progress (until this time).
    let mut sim_stall_until = 0u64;

    // Wi-Fi is read in its own task: a scan can take from milliseconds to
    // seconds (macOS), and the router pings must not wait for it, or the wait
    // itself would look like a stall.
    let readings: Arc<parking_lot::Mutex<VecDeque<(u64, Option<crate::wifi::WifiInterface>)>>> =
        Default::default();
    let poller = {
        let st = state.clone();
        let readings = readings.clone();
        tokio::spawn(async move {
            while !st.roam_handle.cancel.load(Ordering::SeqCst) {
                let tick = std::time::Instant::now();
                let reading = if simulated {
                    let mut sim = st.sim.lock();
                    sim.position = sim_walk((now_ms() - started) as f64 / 1000.0);
                    Some(sim.wifi_scan())
                } else {
                    crate::wifi::scan_for_analysis(&st).await
                };
                let iface =
                    reading.and_then(|r| r.interfaces.into_iter().find(|i| i.state == "connected"));
                readings.lock().push_back((now_ms(), iface));
                tokio::time::sleep(
                    Duration::from_millis(WIFI_EVERY_MS).saturating_sub(tick.elapsed()),
                )
                .await;
            }
        })
    };

    while !state.roam_handle.cancel.load(Ordering::SeqCst) {
        let tick = std::time::Instant::now();
        let now = now_ms();
        if now - started > MAX_RUN_MS {
            break;
        }
        let mut events = Vec::new();
        let fresh: Vec<_> = readings.lock().drain(..).collect();
        for (t, iface) in fresh {
            if let Some(i) = &iface {
                let mut run = state.roam.lock();
                if run.ssid.is_none() {
                    run.ssid = i.ssid.clone();
                }
            }
            current = (
                iface.as_ref().and_then(|i| i.bssid.clone()),
                iface.as_ref().and_then(|i| i.rssi),
                iface.as_ref().and_then(|i| i.channel),
            );
            if let Some(e) = tracker.wifi(RoamSample {
                t,
                bssid: current.0.clone(),
                rssi: current.1,
                channel: current.2,
                gw_ms: None,
            }) {
                if simulated && e.kind == "roam" {
                    // Fast roaming (802.11r) is ~30 ms, a full re-auth 200-600 ms.
                    let stall = if rand::random::<f64>() < 0.6 {
                        40
                    } else {
                        200 + rand::random::<u64>() % 400
                    };
                    sim_stall_until = now + stall;
                }
                events.push(e);
            }
        }
        let gw_ms = match gateway {
            Some(_) if simulated => {
                (now >= sim_stall_until).then(|| 1.5 + rand::random::<f64>() * 2.0)
            }
            Some(g) => net::icmp::ping_async(g, Duration::from_millis(300))
                .await
                .rtt(),
            None => None,
        };
        samples.push_back(RoamSample {
            t: now,
            bssid: current.0.clone(),
            rssi: current.1,
            channel: current.2,
            gw_ms,
        });
        while samples.len() > KEEP_SAMPLES {
            samples.pop_front();
        }
        {
            let mut run = state.roam.lock();
            for e in events {
                run.events.push(e);
                unfinished.push(run.events.len() - 1);
            }
            let contiguous = samples.make_contiguous();
            unfinished.retain(|&i| {
                let ev = &mut run.events[i];
                ev.gap_ms = gap_around(contiguous, ev.t, 2000);
                ev.slow = ev.gap_ms.is_some_and(|g| g > SLOW_GAP_MS);
                now < ev.t + 2500
            });
            // The UI only needs a coarse timeline: every Wi-Fi tick.
            run.samples = contiguous
                .iter()
                .filter(|s| s.t % 500 < 100 || s.gw_ms.is_none())
                .cloned()
                .collect();
            run.progress = 0.5;
        }
        tokio::time::sleep(PING_EVERY.saturating_sub(tick.elapsed())).await;
    }
    poller.abort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: u64, bssid: Option<&str>, rssi: i32) -> RoamSample {
        RoamSample {
            t,
            bssid: bssid.map(str::to_string),
            rssi: Some(rssi),
            channel: Some(36),
            gw_ms: Some(2.0),
        }
    }

    #[test]
    fn detects_roams_drops_and_sticky_clients() {
        let mut tr = Tracker::default();
        assert!(tr.wifi(s(0, Some("a"), -50)).is_none());
        assert!(tr.wifi(s(500, Some("a"), -78)).is_none());
        let roam = tr.wifi(s(1000, Some("b"), -55)).unwrap();
        assert_eq!(
            (roam.kind.as_str(), roam.from.as_deref(), roam.to.as_deref()),
            ("roam", Some("a"), Some("b"))
        );
        assert!(roam.sticky, "left the old AP at -78 dBm");
        assert_eq!(
            tr.wifi(RoamSample {
                t: 1500,
                ..Default::default()
            })
            .unwrap()
            .kind,
            "disconnect"
        );
        let back = tr.wifi(s(2500, Some("c"), -60)).unwrap();
        assert_eq!(
            (back.kind.as_str(), back.from.as_deref()),
            ("roam", Some("b"))
        );
        tr.wifi(RoamSample {
            t: 3000,
            ..Default::default()
        });
        assert_eq!(tr.wifi(s(3500, Some("c"), -60)).unwrap().kind, "reconnect");
    }

    #[test]
    fn measures_the_stall() {
        let mut v: Vec<RoamSample> = (0..30)
            .map(|i| RoamSample {
                t: i * 100,
                gw_ms: Some(2.0),
                ..Default::default()
            })
            .collect();
        for x in v.iter_mut().filter(|x| (1000..1400).contains(&x.t)) {
            x.gw_ms = None;
        }
        let gap = gap_around(&v, 1200, 2000).unwrap();
        assert!((gap - 400.0).abs() < 1.0, "{gap}");
    }

    #[tokio::test]
    async fn simulated_walk_roams() {
        let dir = std::env::temp_dir().join(format!("whylag-roam-{}", rand::random::<u32>()));
        let state = AppState::new(dir);
        state
            .update_settings(serde_json::json!({ "dataSource": "simulator" }))
            .unwrap();
        // One lap at the job's Wi-Fi rate makes the sticky client switch APs.
        let mut tr = Tracker::default();
        let mut roams = 0;
        for step in 0..140 {
            let mut sim = state.sim.lock();
            sim.position = sim_walk(step as f64 * 0.5);
            let i = sim.wifi_scan().interfaces[0].clone();
            drop(sim);
            if tr
                .wifi(RoamSample {
                    t: step * 500,
                    bssid: i.bssid,
                    rssi: i.rssi,
                    channel: i.channel,
                    gw_ms: None,
                })
                .is_some_and(|e| e.kind == "roam")
            {
                roams += 1;
            }
        }
        assert!(roams >= 2, "only {roams} roams in a lap");
        // The job runs, samples, and stops.
        start(state.clone());
        tokio::time::sleep(Duration::from_millis(700)).await;
        stop(&state);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let run = state.roam.lock().clone();
        assert!(!run.running);
        assert!(!run.samples.is_empty());
        assert_eq!(run.gateway.as_deref(), Some("192.168.10.1"));
    }
}
