//! Simulated performance and path results for the demo office: a fast but
//! bufferbloated fibre line, a slow ISP resolver and CGNAT.

use crate::path::{assign_segments, diagnose, Hop, PathRequest};
use crate::perf::{
    bufferbloat_grade, DnsResult, LatencyStats, PerfRequest, ProbeKind, SpeedResult,
};
use crate::state::AppState;
use rand::Rng;
use std::net::Ipv4Addr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

fn jittered(base: f64, spread: f64, loss: f64) -> Vec<Option<f64>> {
    let mut rng = rand::thread_rng();
    (0..20)
        .map(|_| {
            if rng.gen::<f64>() < loss {
                None
            } else {
                // Mostly tight with an occasional Wi-Fi retry spike.
                let spike = if rng.gen::<f64>() < 0.08 {
                    rng.gen_range(5.0..25.0)
                } else {
                    0.0
                };
                Some((base + rng.gen_range(0.0..spread) + spike).max(0.3))
            }
        })
        .collect()
}

pub async fn perf(state: Arc<AppState>, req: PerfRequest) -> Result<(), String> {
    let set = |f: &mut dyn FnMut(&mut crate::perf::PerfRun)| f(&mut state.perf.lock());
    set(&mut |r| r.simulated = true);
    if req.latency {
        set(&mut |r| r.phase = "latency".into());
        tokio::time::sleep(Duration::from_millis(1800)).await;
        let targets: [(&str, &str, &str, &str, f64, f64, f64, ProbeKind); 9] = [
            (
                "gateway",
                "gateway",
                "192.168.10.1",
                "lan",
                1.6,
                3.0,
                0.0,
                ProbeKind::Icmp,
            ),
            (
                "cloudflare",
                "Cloudflare DNS",
                "1.1.1.1",
                "internet",
                11.0,
                3.0,
                0.0,
                ProbeKind::Icmp,
            ),
            (
                "google-dns",
                "Google DNS",
                "8.8.8.8",
                "internet",
                13.0,
                3.5,
                0.0,
                ProbeKind::Icmp,
            ),
            (
                "google",
                "Google",
                "www.google.com",
                "internet",
                14.0,
                4.0,
                0.0,
                ProbeKind::Tcp,
            ),
            (
                "youtube",
                "YouTube",
                "www.youtube.com",
                "internet",
                14.5,
                4.0,
                0.0,
                ProbeKind::Tcp,
            ),
            (
                "teams",
                "Microsoft Teams",
                "teams.microsoft.com",
                "internet",
                27.0,
                6.0,
                0.0,
                ProbeKind::Tcp,
            ),
            (
                "zoom",
                "Zoom",
                "zoom.us",
                "internet",
                31.0,
                6.0,
                0.05,
                ProbeKind::Tcp,
            ),
            (
                "netflix",
                "Netflix",
                "www.netflix.com",
                "internet",
                16.0,
                4.0,
                0.0,
                ProbeKind::Tcp,
            ),
            (
                "steam",
                "Steam",
                "store.steampowered.com",
                "internet",
                38.0,
                7.0,
                0.0,
                ProbeKind::Tcp,
            ),
        ];
        let latency: Vec<LatencyStats> = targets
            .iter()
            .map(|(id, label, host, group, base, spread, loss, kind)| {
                LatencyStats::from_samples(
                    id,
                    label,
                    host,
                    group,
                    Some(*kind),
                    jittered(*base, *spread, *loss),
                )
            })
            .collect();
        set(&mut |r| {
            r.latency = latency.clone();
            r.progress = 0.3;
        });
    }
    if req.dns {
        set(&mut |r| r.phase = "dns".into());
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let dns = vec![
            DnsResult {
                server: "192.168.10.1".into(),
                label: "192.168.10.1".into(),
                system: true,
                median_ms: Some(41.0),
                succeeded: 8,
                total: 8,
            },
            DnsResult {
                server: "1.1.1.1".into(),
                label: "Cloudflare".into(),
                system: false,
                median_ms: Some(10.8),
                succeeded: 8,
                total: 8,
            },
            DnsResult {
                server: "8.8.8.8".into(),
                label: "Google".into(),
                system: false,
                median_ms: Some(14.2),
                succeeded: 8,
                total: 8,
            },
            DnsResult {
                server: "9.9.9.9".into(),
                label: "Quad9".into(),
                system: false,
                median_ms: Some(17.5),
                succeeded: 8,
                total: 8,
            },
            DnsResult {
                server: "208.67.222.222".into(),
                label: "OpenDNS".into(),
                system: false,
                median_ms: Some(22.9),
                succeeded: 8,
                total: 8,
            },
        ];
        set(&mut |r| {
            r.dns = dns.clone();
            r.dns_hijack = Some(false);
            r.progress = 0.45;
        });
    }
    if req.speed {
        let mut speed = SpeedResult {
            server: "Warsaw, PL (mlab1-waw03, simulated)".into(),
            idle_latency_ms: Some(12.0),
            ..Default::default()
        };
        let mut rng = rand::thread_rng().gen::<u64>();
        let mut noise = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng % 1000) as f64 / 1000.0
        };
        for (phase, target, series_is_down) in [("download", 286.0, true), ("upload", 37.5, false)]
        {
            for i in 1..=40 {
                if state.perf_handle.cancel.load(Ordering::SeqCst) {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
                let t = i as f64 * 0.25;
                let ramp = (1.0 - (-t * 1.6).exp()).min(1.0);
                let v = target * ramp * (0.93 + 0.12 * noise());
                if series_is_down {
                    speed.down_series.push((t, v));
                    speed.download_mbps = Some(
                        speed.down_series.iter().map(|p| p.1).sum::<f64>()
                            / speed.down_series.len() as f64,
                    );
                } else {
                    speed.up_series.push((t, v));
                    speed.upload_mbps = Some(
                        speed.up_series.iter().map(|p| p.1).sum::<f64>()
                            / speed.up_series.len() as f64,
                    );
                }
                let progress = if series_is_down {
                    0.45 + 0.27 * i as f64 / 40.0
                } else {
                    0.72 + 0.28 * i as f64 / 40.0
                };
                let s = speed.clone();
                set(&mut |r| {
                    r.phase = phase.into();
                    r.progress = progress;
                    r.speed = Some(s.clone());
                });
            }
        }
        speed.loaded_down_latency_ms = Some(96.0);
        speed.loaded_up_latency_ms = Some(212.0);
        speed.grade = Some(bufferbloat_grade(12.0, 212.0).into());
        set(&mut |r| r.speed = Some(speed.clone()));
    }
    Ok(())
}

pub async fn path(state: Arc<AppState>, req: PathRequest) -> Result<(), String> {
    let dest: Ipv4Addr = match req.target.parse() {
        Ok(ip) => ip,
        Err(_) => match req.target.as_str() {
            t if t.contains("google") || t.contains("youtube") => {
                "142.250.186.206".parse().unwrap()
            }
            t if t.contains("microsoft") || t.contains("teams") => {
                "52.112.120.204".parse().unwrap()
            }
            _ => "104.18.32.7".parse().unwrap(),
        },
    };
    let template: Vec<(&str, Option<&str>, f64, f64)> = vec![
        ("192.168.10.1", Some("unifi.localdomain"), 1.4, 0.0),
        ("100.72.0.1", None, 7.8, 0.0),
        ("83.1.4.10", Some("war-r1.core.orange.pl"), 9.1, 0.0),
        // Rate-limits ICMP replies: loss here only, not real loss.
        ("83.1.5.29", Some("war-b2.core.orange.pl"), 9.6, 0.35),
        ("195.66.224.12", Some("plix.waw.equinix.net"), 10.4, 0.0),
        ("72.14.214.2", Some("72-14-214-2.google.com"), 11.8, 0.0),
    ];
    let mut hops: Vec<Hop> = template
        .iter()
        .enumerate()
        .map(|(i, (ip, name, _, _))| {
            let mut h = Hop::new(i as u8 + 1, ip.parse().ok());
            h.name = name.map(str::to_string);
            h
        })
        .collect();
    let dest_base = 12.6;
    let mut last = Hop::new(hops.len() as u8 + 1, Some(dest));
    last.name = Some(req.target.clone());
    hops.push(last);
    assign_segments(&mut hops, dest);
    {
        let mut r = state.path.lock();
        r.simulated = true;
        r.target_ip = Some(dest);
        r.phase = "trace".into();
    }
    tokio::time::sleep(Duration::from_millis(900)).await;
    state.path.lock().phase = "measure".into();
    let rounds = req.rounds.clamp(5, 120);
    for round in 0..rounds {
        if state.path_handle.cancel.load(Ordering::SeqCst) {
            break;
        }
        {
            let mut rng = rand::thread_rng();
            for (i, h) in hops.iter_mut().enumerate() {
                let (base, loss) = template
                    .get(i)
                    .map(|t| (t.2, t.3))
                    .unwrap_or((dest_base, 0.0));
                let wifi_spike = if rng.gen::<f64>() < 0.07 {
                    rng.gen_range(4.0..20.0)
                } else {
                    0.0
                };
                let v = if rng.gen::<f64>() < loss {
                    None
                } else {
                    Some(base + rng.gen_range(0.0..2.2) + wifi_spike)
                };
                h.samples.push(v);
                h.recompute();
            }
        }
        {
            let mut r = state.path.lock();
            r.hops = hops.clone();
            r.rounds = round + 1;
            r.progress = (round + 1) as f64 / rounds as f64;
            r.findings = diagnose(&hops);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(())
}
