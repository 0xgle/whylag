//! Continuous monitoring: samples the gateway, the internet, DNS and the Wi-Fi
//! link every few seconds, stores everything in SQLite and turns patterns into
//! incidents that say *where* the problem was (Wi-Fi/LAN, ISP or beyond).
//!
//! Leave a laptop running it for a day and you have the evidence an ISP
//! complaint or a client report needs: "the line dropped 14 times between 19:00
//! and 22:00 while the router answered fine".

use crate::net::{self, dns};
use crate::state::AppState;
use crate::util::{median, now_ms};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub ts: u64,
    /// Round trip to the gateway (ms); None = no answer.
    pub gateway_ms: Option<f64>,
    /// Round trip to an internet anchor (ms).
    pub internet_ms: Option<f64>,
    /// DNS lookup time through the system resolver (ms).
    pub dns_ms: Option<f64>,
    pub rssi: Option<i32>,
    pub bssid: Option<String>,
    pub channel: Option<u32>,
    pub connected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IncidentKind {
    /// Gateway unreachable or Wi-Fi disconnected.
    LocalOutage,
    /// Gateway fine, internet unreachable.
    IspOutage,
    /// Packet loss to the gateway (Wi-Fi/LAN).
    LocalLoss,
    /// Packet loss beyond the gateway.
    IspLoss,
    /// Internet latency far above normal while the gateway is normal.
    LatencySpike,
    /// Latency spike that also shows on the gateway: local congestion or Wi-Fi.
    LocalLatency,
    DnsFailure,
    DnsSlow,
    WeakSignal,
    Roam,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Incident {
    pub id: i64,
    pub kind: IncidentKind,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
    /// Peak value: latency in ms, loss in %, RSSI in dBm.
    pub peak: Option<f64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MonitorStatus {
    pub running: bool,
    pub simulated: bool,
    pub interval_s: u64,
    pub started_ms: Option<u64>,
    pub sample_count: u64,
    pub first_ms: Option<u64>,
    pub last: Option<Sample>,
    pub incidents: Vec<Incident>,
    pub gateway: Option<String>,
    pub dns_server: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SampleSeries {
    pub ts: Vec<u64>,
    pub gateway_ms: Vec<Option<f64>>,
    pub internet_ms: Vec<Option<f64>>,
    pub dns_ms: Vec<Option<f64>>,
    pub rssi: Vec<Option<f64>>,
    /// Share of failed probes in each bucket (0..1) for internet and gateway.
    pub internet_loss: Vec<f64>,
    pub gateway_loss: Vec<f64>,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS samples (
                ts INTEGER NOT NULL, gw REAL, inet REAL, dns REAL, rssi INTEGER,
                bssid TEXT, channel INTEGER, connected INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS samples_ts ON samples(ts);
             CREATE TABLE IF NOT EXISTS incidents (
                id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL,
                start_ms INTEGER NOT NULL, end_ms INTEGER, peak REAL, detail TEXT);",
        )?;
        Ok(Store { conn })
    }

    pub fn insert(&self, s: &Sample) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO samples (ts, gw, inet, dns, rssi, bssid, channel, connected) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![s.ts as i64, s.gateway_ms, s.internet_ms, s.dns_ms, s.rssi, s.bssid, s.channel, s.connected as i32],
        )?;
        Ok(())
    }

    pub fn recent(&self, limit: usize) -> Vec<Sample> {
        let mut out: Vec<Sample> = self
            .conn
            .prepare("SELECT ts, gw, inet, dns, rssi, bssid, channel, connected FROM samples ORDER BY ts DESC LIMIT ?1")
            .and_then(|mut st| {
                st.query_map([limit as i64], |r| {
                    Ok(Sample {
                        ts: r.get::<_, i64>(0)? as u64,
                        gateway_ms: r.get(1)?,
                        internet_ms: r.get(2)?,
                        dns_ms: r.get(3)?,
                        rssi: r.get(4)?,
                        bssid: r.get(5)?,
                        channel: r.get(6)?,
                        connected: r.get::<_, i32>(7)? != 0,
                    })
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default();
        out.reverse();
        out
    }

    pub fn count_and_first(&self) -> (u64, Option<u64>) {
        self.conn
            .query_row("SELECT COUNT(*), MIN(ts) FROM samples", [], |r| {
                Ok((
                    r.get::<_, i64>(0)? as u64,
                    r.get::<_, Option<i64>>(1)?.map(|v| v as u64),
                ))
            })
            .unwrap_or((0, None))
    }

    pub fn open_incident(
        &self,
        kind: IncidentKind,
        start: u64,
        peak: Option<f64>,
        detail: Option<&str>,
    ) -> i64 {
        let k = serde_json::to_value(kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let _ = self.conn.execute(
            "INSERT INTO incidents (kind, start_ms, peak, detail) VALUES (?1,?2,?3,?4)",
            params![k, start as i64, peak, detail],
        );
        self.conn.last_insert_rowid()
    }

    pub fn update_incident(&self, id: i64, end: Option<u64>, peak: Option<f64>) {
        let _ = self.conn.execute(
            "UPDATE incidents SET end_ms = ?2, peak = COALESCE(?3, peak) WHERE id = ?1",
            params![id, end.map(|e| e as i64), peak],
        );
    }

    pub fn reopen_incident(&self, id: i64, peak: Option<f64>) {
        let _ = self.conn.execute(
            "UPDATE incidents SET end_ms = NULL, peak = COALESCE(?2, peak) WHERE id = ?1",
            params![id, peak],
        );
    }

    pub fn incidents(&self, limit: usize) -> Vec<Incident> {
        self.conn
            .prepare("SELECT id, kind, start_ms, end_ms, peak, detail FROM incidents ORDER BY start_ms DESC LIMIT ?1")
            .and_then(|mut st| {
                st.query_map([limit as i64], |r| {
                    let kind: String = r.get(1)?;
                    Ok(Incident {
                        id: r.get(0)?,
                        kind: serde_json::from_value(serde_json::Value::String(kind)).unwrap_or(IncidentKind::LatencySpike),
                        start_ms: r.get::<_, i64>(2)? as u64,
                        end_ms: r.get::<_, Option<i64>>(3)?.map(|v| v as u64),
                        peak: r.get(4)?,
                        detail: r.get(5)?,
                    })
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default()
    }

    /// Bucketed series between two times for charts (at most `points` buckets).
    pub fn series(&self, from: u64, to: u64, points: usize) -> SampleSeries {
        let points = points.clamp(10, 4000);
        let bucket = ((to.saturating_sub(from)) / points as u64).max(1);
        let mut out = SampleSeries::default();
        let mut stmt = match self.conn.prepare(
            "SELECT (ts - ?1) / ?3 AS b, MIN(ts), AVG(gw), AVG(inet), AVG(dns), AVG(rssi),
                    SUM(CASE WHEN inet IS NULL THEN 1 ELSE 0 END) * 1.0 / COUNT(*),
                    SUM(CASE WHEN gw IS NULL THEN 1 ELSE 0 END) * 1.0 / COUNT(*)
             FROM samples WHERE ts >= ?1 AND ts <= ?2 GROUP BY b ORDER BY b",
        ) {
            Ok(s) => s,
            Err(_) => return out,
        };
        let rows = stmt.query_map(params![from as i64, to as i64, bucket as i64], |r| {
            Ok((
                r.get::<_, i64>(1)? as u64,
                r.get::<_, Option<f64>>(2)?,
                r.get::<_, Option<f64>>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, Option<f64>>(5)?,
                r.get::<_, f64>(6)?,
                r.get::<_, f64>(7)?,
            ))
        });
        if let Ok(rows) = rows {
            for (ts, gw, inet, dns, rssi, il, gl) in rows.flatten() {
                out.ts.push(ts);
                out.gateway_ms.push(gw);
                out.internet_ms.push(inet);
                out.dns_ms.push(dns);
                out.rssi.push(rssi);
                out.internet_loss.push(il);
                out.gateway_loss.push(gl);
            }
        }
        out
    }

    pub fn clear(&self) {
        let _ = self
            .conn
            .execute_batch("DELETE FROM samples; DELETE FROM incidents;");
    }

    /// Ends incidents left open by a monitor that stopped or crashed, at the
    /// last sample recorded, so they do not read as still ongoing.
    pub fn close_dangling(&self) {
        let _ = self.conn.execute(
            "UPDATE incidents SET end_ms = MAX(start_ms, COALESCE((SELECT MAX(ts) FROM samples), start_ms)) WHERE end_ms IS NULL",
            [],
        );
    }
}

/// Turns the stream of samples into incidents. Keeps the state of incidents
/// that are still open.
#[derive(Default)]
pub struct Detector {
    window: Vec<Sample>,
    open: Vec<(IncidentKind, i64, f64)>,
    /// Recently closed incidents (kind, id, end, peak): a recurrence within
    /// two minutes continues the same incident instead of starting a new one.
    closed: Vec<(IncidentKind, i64, u64, f64)>,
    last_bssid: Option<String>,
}

const MERGE_GAP_MS: u64 = 120_000;

impl Detector {
    fn baseline(&self) -> Option<f64> {
        let vals: Vec<f64> = self.window.iter().filter_map(|s| s.internet_ms).collect();
        (vals.len() >= 10).then(|| median(&vals)).flatten()
    }

    /// Feeds one sample; returns (kind, active, peak, detail) for each condition.
    pub fn feed(&mut self, s: &Sample, store: &Store) {
        let baseline = self.baseline();
        let gw_baseline = median(
            &self
                .window
                .iter()
                .filter_map(|x| x.gateway_ms)
                .collect::<Vec<_>>(),
        );
        self.window.push(s.clone());
        if self.window.len() > 120 {
            self.window.remove(0);
        }
        let recent: Vec<&Sample> = self.window.iter().rev().take(12).collect();
        let gw_fail = recent.iter().filter(|x| x.gateway_ms.is_none()).count();
        let inet_fail = recent
            .iter()
            .filter(|x| x.internet_ms.is_none() && x.gateway_ms.is_some())
            .count();
        let last3 = self.window.iter().rev().take(3).collect::<Vec<_>>();
        let local_down =
            last3.len() == 3 && last3.iter().all(|x| x.gateway_ms.is_none() || !x.connected);
        let isp_down = last3.len() == 3
            && last3
                .iter()
                .all(|x| x.gateway_ms.is_some() && x.internet_ms.is_none());

        let mut active: Vec<(IncidentKind, f64, Option<String>)> = Vec::new();
        if local_down {
            active.push((IncidentKind::LocalOutage, 0.0, None));
        } else if isp_down {
            active.push((IncidentKind::IspOutage, 0.0, None));
        } else {
            if gw_fail >= 2 {
                active.push((
                    IncidentKind::LocalLoss,
                    gw_fail as f64 * 100.0 / recent.len() as f64,
                    None,
                ));
            }
            if inet_fail >= 2 {
                active.push((
                    IncidentKind::IspLoss,
                    inet_fail as f64 * 100.0 / recent.len() as f64,
                    None,
                ));
            }
        }
        if let (Some(b), Some(v)) = (baseline, s.internet_ms) {
            if v > b * 3.0 && v > b + 60.0 {
                let gw_spiked = match (gw_baseline, s.gateway_ms) {
                    (Some(gb), Some(g)) => g > gb * 3.0 && g > gb + 25.0,
                    _ => false,
                };
                active.push((
                    if gw_spiked {
                        IncidentKind::LocalLatency
                    } else {
                        IncidentKind::LatencySpike
                    },
                    v,
                    None,
                ));
            }
        }
        match s.dns_ms {
            None if s.internet_ms.is_some() => active.push((IncidentKind::DnsFailure, 0.0, None)),
            Some(d) if d > 300.0 => active.push((IncidentKind::DnsSlow, d, None)),
            _ => {}
        }
        if let Some(r) = s.rssi {
            if r < -75 {
                active.push((IncidentKind::WeakSignal, r as f64, None));
            }
        }
        // Roams are instantaneous events.
        if let (Some(prev), Some(cur)) = (&self.last_bssid, &s.bssid) {
            if prev != cur {
                let id = store.open_incident(
                    IncidentKind::Roam,
                    s.ts,
                    s.rssi.map(|v| v as f64),
                    Some(&format!("{prev} → {cur}")),
                );
                store.update_incident(id, Some(s.ts), None);
            }
        }
        if s.bssid.is_some() {
            self.last_bssid = s.bssid.clone();
        }

        // Close incidents that are no longer active.
        let mut still_open = Vec::new();
        for (kind, id, peak) in std::mem::take(&mut self.open) {
            if let Some((_, v, _)) = active.iter().find(|(k, _, _)| *k == kind) {
                let new_peak = if kind == IncidentKind::WeakSignal {
                    peak.min(*v)
                } else {
                    peak.max(*v)
                };
                if new_peak != peak {
                    store.update_incident(id, None, Some(new_peak));
                }
                still_open.push((kind, id, new_peak));
            } else {
                store.update_incident(id, Some(s.ts), None);
                self.closed.retain(|(k, _, _, _)| *k != kind);
                self.closed.push((kind, id, s.ts, peak));
            }
        }
        for (kind, v, detail) in active {
            if still_open.iter().any(|(k, _, _)| *k == kind) {
                continue;
            }
            let recent = self
                .closed
                .iter()
                .position(|(k, _, end, _)| *k == kind && s.ts.saturating_sub(*end) <= MERGE_GAP_MS);
            if let Some(pos) = recent {
                let (_, id, _, peak) = self.closed.remove(pos);
                let new_peak = if kind == IncidentKind::WeakSignal {
                    peak.min(v)
                } else {
                    peak.max(v)
                };
                store.reopen_incident(id, Some(new_peak));
                still_open.push((kind, id, new_peak));
            } else {
                let id = store.open_incident(kind, s.ts, Some(v), detail.as_deref());
                still_open.push((kind, id, v));
            }
        }
        self.open = still_open;
    }
}

pub struct MonitorHandle {
    pub running: AtomicBool,
    /// Bumped on every start, so a loop from an earlier start that is still
    /// sleeping exits instead of running alongside the new one.
    pub generation: std::sync::atomic::AtomicU64,
    pub store: Mutex<Option<Store>>,
    pub info: Mutex<MonitorStatus>,
}

impl Default for MonitorHandle {
    fn default() -> Self {
        Self {
            running: AtomicBool::new(false),
            generation: Default::default(),
            store: Mutex::new(None),
            info: Mutex::new(MonitorStatus::default()),
        }
    }
}

fn db_path(state: &AppState, simulated: bool) -> std::path::PathBuf {
    state.data_dir.join(if simulated {
        "monitor-demo.sqlite"
    } else {
        "monitor.sqlite"
    })
}

/// Opens the right database (demo or live) if not open yet.
pub fn ensure_store(state: &AppState) -> Result<(), String> {
    let simulated = state.use_simulator();
    let mut guard = state.monitor.store.lock();
    let mut info = state.monitor.info.lock();
    if guard.is_none() || info.simulated != simulated {
        let store = Store::open(&db_path(state, simulated)).map_err(|e| e.to_string())?;
        if !state.monitor.running.load(Ordering::SeqCst) {
            store.close_dangling();
        }
        if simulated && store.count_and_first().0 == 0 {
            crate::sim::monitor::seed_history(&store);
        }
        *guard = Some(store);
        info.simulated = simulated;
    }
    Ok(())
}

pub fn status(state: &AppState) -> MonitorStatus {
    let _ = ensure_store(state);
    let mut info = state.monitor.info.lock().clone();
    info.running = state.monitor.running.load(Ordering::SeqCst);
    if let Some(store) = state.monitor.store.lock().as_ref() {
        let (count, first) = store.count_and_first();
        info.sample_count = count;
        info.first_ms = first;
        info.last = store.recent(1).pop();
        info.incidents = store.incidents(200);
    }
    info
}

pub fn series(state: &AppState, from: u64, to: u64, points: usize) -> SampleSeries {
    let _ = ensure_store(state);
    state
        .monitor
        .store
        .lock()
        .as_ref()
        .map(|s| s.series(from, to, points))
        .unwrap_or_default()
}

pub fn clear(state: &AppState) {
    if let Some(s) = state.monitor.store.lock().as_ref() {
        s.clear();
    }
}

pub fn stop(state: &AppState) {
    state.monitor.running.store(false, Ordering::SeqCst);
}

/// Takes one sample. `wifi_expected` remembers whether Wi-Fi was connected on
/// the first sample: on a wired machine a missing Wi-Fi link is not an outage.
async fn take_sample(
    state: &Arc<AppState>,
    gateway: Option<Ipv4Addr>,
    dns_server: Option<IpAddr>,
    wifi_expected: &mut Option<bool>,
) -> Sample {
    let timeout = Duration::from_millis(1500);
    let gw = async {
        let g = gateway?;
        if let Some(rtt) = net::icmp::ping_async(g, timeout).await.rtt() {
            return Some(rtt);
        }
        // Some routers ignore ping: any TCP answer (even a reset) proves it is up.
        let probes = [80u16, 443, 53].map(|p| {
            net::tcp_probe(
                SocketAddr::new(IpAddr::V4(g), p),
                Duration::from_millis(800),
            )
        });
        futures_util::future::join_all(probes)
            .await
            .into_iter()
            .filter(|(o, _)| matches!(o, net::TcpOutcome::Open | net::TcpOutcome::Closed))
            .filter_map(|(_, rtt)| rtt)
            .reduce(f64::min)
    };
    let inet = async {
        // Two anchors: the line is "up" if either answers.
        let (a, b) = tokio::join!(
            net::icmp::ping_async(Ipv4Addr::new(1, 1, 1, 1), timeout),
            net::icmp::ping_async(Ipv4Addr::new(8, 8, 8, 8), timeout)
        );
        match (a.rtt(), b.rtt()) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (x, y) => x.or(y),
        }
    };
    let dns_t = async {
        match dns_server {
            Some(server) => {
                let name = format!(
                    "{}.wikipedia.org",
                    ["en", "pl", "de", "fr", "es", "it"][(now_ms() / 1000 % 6) as usize]
                );
                dns::query(server, &name, Duration::from_millis(2000))
                    .await
                    .filter(|a| a.rcode == 0)
                    .map(|a| a.rtt_ms)
            }
            None => None,
        }
    };
    let wifi = crate::wifi::scan_for_analysis(state);
    let (gateway_ms, internet_ms, dns_ms, scan) = tokio::join!(gw, inet, dns_t, wifi);
    let iface = scan.as_ref().and_then(|s| {
        s.interfaces
            .iter()
            .find(|i| i.state == "connected")
            .cloned()
    });
    Sample {
        ts: now_ms(),
        gateway_ms,
        internet_ms,
        dns_ms,
        rssi: iface.as_ref().and_then(|i| i.rssi),
        bssid: iface.as_ref().and_then(|i| i.bssid.clone()),
        channel: iface.as_ref().and_then(|i| i.channel),
        // A missing Wi-Fi link only means "offline" if Wi-Fi was in use when
        // monitoring began and the router is not answering by other means.
        connected: {
            let expected = *wifi_expected.get_or_insert(iface.is_some());
            iface.is_some() || !expected || gateway_ms.is_some()
        },
    }
}

pub fn start(state: Arc<AppState>, interval_s: u64) -> Result<MonitorStatus, String> {
    ensure_store(&state)?;
    if state.monitor.running.swap(true, Ordering::SeqCst) {
        return Ok(status(&state));
    }
    let generation = state.monitor.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let interval = interval_s.clamp(1, 300);
    {
        let mut info = state.monitor.info.lock();
        info.interval_s = interval;
        info.started_ms = Some(now_ms());
    }
    let st = state.clone();
    tokio::spawn(async move {
        let simulated = st.use_simulator();
        let local = if simulated {
            Some(crate::sim::lan::local())
        } else {
            tokio::task::spawn_blocking(net::local_net)
                .await
                .ok()
                .flatten()
        };
        let gateway = local.as_ref().and_then(|l| l.gateway);
        let dns_server = local
            .as_ref()
            .and_then(|l| l.dns_servers.iter().find(|d| !d.is_loopback()).copied())
            .or(Some(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        {
            let mut info = st.monitor.info.lock();
            info.gateway = gateway.map(|g| g.to_string());
            info.dns_server = dns_server.map(|d| d.to_string());
        }
        let mut detector = Detector::default();
        let mut sim = crate::sim::monitor::Live::default();
        let mut wifi_expected = None;
        let alive = |st: &AppState| {
            st.monitor.running.load(Ordering::SeqCst)
                && st.monitor.generation.load(Ordering::SeqCst) == generation
        };
        while alive(&st) {
            let started = std::time::Instant::now();
            let sample = if simulated {
                sim.next(now_ms())
            } else {
                take_sample(&st, gateway, dns_server, &mut wifi_expected).await
            };
            if !alive(&st) {
                break;
            }
            if let Some(store) = st.monitor.store.lock().as_ref() {
                let _ = store.insert(&sample);
                detector.feed(&sample, store);
            }
            let wait = Duration::from_secs(interval).saturating_sub(started.elapsed());
            // Sleep in short steps so Stop reacts quickly.
            let until = std::time::Instant::now() + wait;
            while std::time::Instant::now() < until && alive(&st) {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        // Stopped: end whatever was still open at the last sample.
        if st.monitor.generation.load(Ordering::SeqCst) == generation {
            if let Some(store) = st.monitor.store.lock().as_ref() {
                store.close_dangling();
            }
        }
    });
    Ok(status(&state))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(ts: u64, gw: Option<f64>, inet: Option<f64>) -> Sample {
        Sample {
            ts,
            gateway_ms: gw,
            internet_ms: inet,
            dns_ms: Some(12.0),
            rssi: Some(-55),
            bssid: Some("a".into()),
            channel: Some(36),
            connected: true,
        }
    }

    #[test]
    fn detects_where_the_outage_is() {
        let dir = std::env::temp_dir().join(format!("whylag-mon-{}", rand::random::<u32>()));
        let store = Store::open(&dir.join("m.sqlite")).unwrap();
        let mut d = Detector::default();
        let mut t = 0;
        for _ in 0..20 {
            t += 5000;
            let s = sample(t, Some(2.0), Some(15.0));
            store.insert(&s).unwrap();
            d.feed(&s, &store);
        }
        // ISP outage: gateway fine, internet gone for 5 samples.
        for _ in 0..5 {
            t += 5000;
            let s = sample(t, Some(2.0), None);
            store.insert(&s).unwrap();
            d.feed(&s, &store);
        }
        for _ in 0..3 {
            t += 5000;
            let s = sample(t, Some(2.0), Some(15.0));
            d.feed(&s, &store);
        }
        // Latency spike beyond the gateway.
        t += 5000;
        d.feed(&sample(t, Some(2.1), Some(180.0)), &store);
        t += 5000;
        d.feed(&sample(t, Some(2.0), Some(15.0)), &store);
        let incidents = store.incidents(50);
        let kinds: Vec<IncidentKind> = incidents.iter().map(|i| i.kind).collect();
        assert!(kinds.contains(&IncidentKind::IspOutage), "{kinds:?}");
        assert!(kinds.contains(&IncidentKind::LatencySpike), "{kinds:?}");
        assert!(!kinds.contains(&IncidentKind::LocalOutage));
        assert!(incidents
            .iter()
            .filter(|i| i.kind == IncidentKind::IspOutage)
            .all(|i| i.end_ms.is_some()));
        let series = store.series(0, t, 10);
        assert!(!series.ts.is_empty());
        assert!(series.internet_loss.iter().any(|l| *l > 0.0));
        let _ = std::fs::remove_dir_all(dir);
    }
}
