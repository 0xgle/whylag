//! Simulated office LAN for the discovery module.

use crate::discovery::{classify, port_name, DiscoveryRun, Host, Link, OpenPort};
use crate::net::LocalNet;
use crate::state::AppState;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

struct Dev {
    ip: u8,
    mac: &'static str,
    names: &'static [(&'static str, &'static str)],
    ports: &'static [u16],
    mdns: &'static [&'static str],
    model: Option<&'static str>,
    http_title: Option<&'static str>,
    snmp: Option<(&'static str, &'static str)>,
    rtt: f64,
}

const fn dev(ip: u8, mac: &'static str, names: &'static [(&'static str, &'static str)], ports: &'static [u16]) -> Dev {
    Dev { ip, mac, names, ports, mdns: &[], model: None, http_title: None, snmp: None, rtt: 2.0 }
}

fn devices() -> Vec<Dev> {
    vec![
        Dev {
            snmp: Some(("UDM-Pro", "UniFi Dream Machine Pro, Linux 4.19")),
            http_title: Some("UniFi OS"),
            rtt: 0.9,
            ..dev(1, "24:5a:4c:10:00:01", &[("dns", "unifi.localdomain")], &[22, 53, 80, 443])
        },
        Dev {
            snmp: Some(("core-sw-01", "Aruba JL322A 2930M-48G-PoE+ Switch, revision WC.16.11")),
            rtt: 1.1,
            ..dev(2, "00:0b:86:44:12:01", &[("dns", "core-sw-01.lan")], &[22])
        },
        Dev { model: Some("U6-Pro"), ..dev(3, "24:5a:4c:a1:01:00", &[("dns", "ap-01.lan")], &[22]) },
        Dev { model: Some("U6-Pro"), ..dev(4, "24:5a:4c:a1:02:00", &[("dns", "ap-02.lan")], &[22]) },
        Dev { model: Some("U6-Pro"), ..dev(5, "24:5a:4c:a1:03:00", &[("dns", "ap-03.lan")], &[22]) },
        Dev { model: Some("U6-Pro"), ..dev(6, "24:5a:4c:a1:04:00", &[("dns", "ap-04.lan")], &[22]) },
        Dev { model: Some("U6-Pro"), ..dev(7, "24:5a:4c:a1:05:00", &[("dns", "ap-05.lan")], &[22]) },
        Dev {
            mdns: &["_smb._tcp", "_http._tcp", "_device-info._tcp"],
            model: Some("Synology DS920+"),
            http_title: Some("Synology DiskStation - nas01"),
            ..dev(10, "00:11:32:7a:21:0f", &[("mdns", "nas01"), ("dns", "nas01.lan")], &[22, 80, 443, 445, 5000, 5001])
        },
        Dev {
            mdns: &["_ipp._tcp", "_printer._tcp", "_http._tcp"],
            model: Some("HP LaserJet Pro M404dn"),
            http_title: Some("HP LaserJet Pro M404dn"),
            snmp: Some(("NPI7B2C1", "HP ETHERNET MULTI-ENVIRONMENT")),
            ..dev(11, "00:01:e6:7b:2c:11", &[("mdns", "HP LaserJet Pro M404dn"), ("dns", "printer-2p.lan")], &[80, 443, 631, 9100])
        },
        Dev {
            model: Some("Hikvision DS-2CD2143G2-I"),
            http_title: Some("Hikvision Web"),
            ..dev(12, "00:bc:99:3e:41:0c", &[("dns", "cam-entrance.lan")], &[80, 554, 8000])
        },
        // Old camera with telnet still enabled (security finding).
        Dev { http_title: Some("WEB SERVICE"), ..dev(13, "08:ed:ed:52:19:aa", &[], &[23, 80, 554]) },
        dev(20, "00:06:5b:1a:22:20", &[("netbios", "DESKTOP-ANNA"), ("dns", "desktop-anna.lan")], &[135, 139, 445]),
        dev(21, "10:c5:95:4e:01:21", &[("netbios", "LAPTOP-PIOTR")], &[135, 139, 445, 3389]),
        dev(22, "00:06:5b:1a:22:22", &[("netbios", "RECEPCJA-PC")], &[135, 139, 445]),
        dev(23, "10:c5:95:4e:01:23", &[("netbios", "KSIEGOWOSC-01")], &[135, 139, 445, 3389, 5900]),
        Dev {
            mdns: &["_companion-link._tcp", "_smb._tcp", "_airplay._tcp"],
            model: Some("MacBookPro18,3"),
            ..dev(25, "00:03:93:5c:aa:25", &[("mdns", "MacBook Pro Kasi")], &[445, 5900, 7000])
        },
        Dev { rtt: 9.0, ..dev(30, "a6:3e:91:0c:44:30", &[("mdns", "iPhone Tomka")], &[62078]) },
        Dev { rtt: 14.0, ..dev(31, "de:21:7f:30:01:31", &[], &[]) },
        Dev { rtt: 22.0, ..dev(32, "f2:9b:11:0a:5e:32", &[], &[]) },
        Dev {
            mdns: &["_airplay._tcp"],
            model: Some("OLED55C3"),
            ..dev(40, "00:1c:62:9a:7e:40", &[("mdns", "[LG] webOS TV OLED55C3")], &[3000, 7000])
        },
        Dev {
            mdns: &["_googlecast._tcp"],
            model: Some("Chromecast with Google TV"),
            ..dev(41, "00:1a:11:c0:fe:41", &[("mdns", "Sala konferencyjna")], &[8008, 8009])
        },
        Dev { mdns: &["_spotify-connect._tcp"], model: Some("Sonos One"), ..dev(42, "00:04:3c:12:9e:42", &[("upnp", "Kuchnia")], &[1400]) },
        Dev { rtt: 6.0, ..dev(50, "00:4b:12:3f:2a:01", &[("dns", "smartplug-3f2a.lan")], &[80]) },
        Dev { rtt: 5.0, ..dev(51, "84:00:ec:61:0b:51", &[("dns", "shelly1pm-0b51.lan")], &[80]) },
        Dev { http_title: Some("Pi-hole Admin Console"), ..dev(60, "b8:27:eb:5d:60:60", &[("dns", "pihole.lan")], &[22, 53, 80]) },
        Dev { http_title: Some("Yealink T54W"), ..dev(61, "24:9a:d8:6a:00:61", &[("dns", "sip-phone-01.lan")], &[80, 443, 5060]) },
        dev(70, "00:33:7a:91:7c:70", &[], &[]),
    ]
}

const SELF_IP: u8 = 100;

pub fn local() -> LocalNet {
    LocalNet {
        interface_name: "Wi-Fi".into(),
        friendly_name: Some("Wi-Fi".into()),
        description: Some("Simulated Wi-Fi 6E adapter".into()),
        kind: "Wireless80211".into(),
        mac: Some("00:12:f0:aa:bb:64".into()),
        ipv4: Some(Ipv4Addr::new(192, 168, 10, SELF_IP)),
        prefix_len: Some(24),
        ipv6: vec!["fe80::212:f0ff:feaa:bb64".into()],
        gateway: Some(Ipv4Addr::new(192, 168, 10, 1)),
        gateway_mac: Some("24:5a:4c:10:00:01".into()),
        dns_servers: vec!["192.168.10.1".parse().unwrap()],
        link_speed_mbps: Some(1201),
        mtu: Some(1500),
        dhcp: Some(true),
    }
}

pub fn hosts() -> Vec<Host> {
    let mut list: Vec<Host> = devices()
        .into_iter()
        .map(|d| {
            let mut h = Host {
                ip: Ipv4Addr::new(192, 168, 10, d.ip),
                mac: Some(d.mac.to_string()),
                vendor: crate::oui::lookup(d.mac).map(str::to_string),
                random_mac: crate::util::is_random_mac(d.mac),
                names: d.names.iter().map(|(s, n)| (s.to_string(), n.to_string())).collect(),
                ports: d.ports.iter().map(|p| OpenPort { port: *p, service: port_name(*p) }).collect(),
                mdns_services: d.mdns.iter().map(|s| s.to_string()).collect(),
                model: d.model.map(str::to_string),
                http_title: d.http_title.map(str::to_string),
                snmp_name: d.snmp.map(|s| s.0.to_string()),
                snmp_descr: d.snmp.map(|s| s.1.to_string()),
                rtt_ms: Some(d.rtt),
                is_gateway: d.ip == 1,
                ..Default::default()
            };
            if let Some((name, _)) = d.snmp {
                h.names.push(("snmp".into(), name.to_string()));
            }
            h.name = ["mdns", "upnp", "netbios", "snmp", "dns"]
                .iter()
                .find_map(|src| h.names.iter().find(|(s, _)| s == src).map(|(_, n)| n.trim_end_matches(".lan").trim_end_matches(".localdomain").to_string()));
            classify(&mut h);
            h
        })
        .collect();
    let mut me = Host {
        ip: Ipv4Addr::new(192, 168, 10, SELF_IP),
        mac: Some("00:12:f0:aa:bb:64".into()),
        vendor: crate::oui::lookup("00:12:f0:aa:bb:64").map(str::to_string),
        name: Some("WHYLAG-LAPTOP".into()),
        names: vec![("netbios".into(), "WHYLAG-LAPTOP".into())],
        is_self: true,
        rtt_ms: Some(0.0),
        ..Default::default()
    };
    classify(&mut me);
    list.push(me);
    list.sort_by_key(|h| u32::from(h.ip));
    list
}

pub fn links() -> Vec<Link> {
    let sw = Ipv4Addr::new(192, 168, 10, 2);
    let mut links = vec![Link {
        from: sw,
        local_port: Some("1/1/48".into()),
        neighbor: "UDM-Pro".into(),
        neighbor_ip: Some(Ipv4Addr::new(192, 168, 10, 1)),
        remote_port: Some("eth8".into()),
    }];
    for i in 1..=5u8 {
        links.push(Link {
            from: sw,
            local_port: Some(format!("1/1/{i}")),
            neighbor: format!("AP-0{i}"),
            neighbor_ip: Some(Ipv4Addr::new(192, 168, 10, 2 + i)),
            remote_port: Some("eth0".into()),
        });
    }
    links.push(Link { from: sw, local_port: Some("1/1/10".into()), neighbor: "nas01".into(), neighbor_ip: Some(Ipv4Addr::new(192, 168, 10, 10)), remote_port: Some("eth0".into()) });
    links
}

/// Plays a discovery run against the simulated LAN with realistic pacing.
pub async fn discover(state: Arc<AppState>) -> Result<(), String> {
    let state = &state;
    let local = local();
    let all = hosts();
    fn update(state: &AppState, f: impl FnOnce(&mut DiscoveryRun)) {
        f(&mut state.discovery.lock());
    }
    update(state, |r| {
        r.simulated = true;
        r.local = Some(local.clone());
        r.subnet = local.subnet_label();
        r.phase = "sweep".into();
    });
    for step in 1..=12 {
        tokio::time::sleep(Duration::from_millis(220)).await;
        let found: Vec<Host> = all
            .iter()
            .filter(|h| (h.ip.octets()[3] as usize) * 12 <= step * 254)
            .map(|h| Host { ip: h.ip, mac: h.mac.clone(), rtt_ms: h.rtt_ms, is_gateway: h.is_gateway, is_self: h.is_self, ..Default::default() })
            .collect();
        update(state, |r| {
            r.progress = 0.45 * step as f64 / 12.0;
            r.hosts = found.clone();
        });
    }
    update(state, |r| r.phase = "identify".into());
    for step in 1..=10 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let n = all.len() * step / 10;
        let mixed: Vec<Host> = all
            .iter()
            .enumerate()
            .map(|(i, h)| if i < n { h.clone() } else { Host { ip: h.ip, mac: h.mac.clone(), rtt_ms: h.rtt_ms, is_gateway: h.is_gateway, is_self: h.is_self, ..Default::default() } })
            .collect();
        update(state, |r| {
            r.progress = 0.5 + 0.45 * step as f64 / 10.0;
            r.hosts = mixed.clone();
        });
    }
    update(state, |r| r.phase = "topology".into());
    tokio::time::sleep(Duration::from_millis(300)).await;
    let links = links();
    update(state, |r| {
        r.hosts = all.clone();
        r.links = links.clone();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::discovery::DeviceKind;

    #[test]
    fn simulated_lan_is_classified() {
        let hosts = super::hosts();
        let kind = |last: u8| hosts.iter().find(|h| h.ip.octets()[3] == last).unwrap().kind;
        assert_eq!(kind(1), DeviceKind::Router);
        assert_eq!(kind(2), DeviceKind::Switch);
        assert_eq!(kind(3), DeviceKind::AccessPoint);
        assert_eq!(kind(10), DeviceKind::Nas);
        assert_eq!(kind(11), DeviceKind::Printer);
        assert_eq!(kind(12), DeviceKind::Camera);
        assert_eq!(kind(13), DeviceKind::Camera);
        assert_eq!(kind(20), DeviceKind::Computer);
        assert_eq!(kind(30), DeviceKind::Phone);
        assert_eq!(kind(31), DeviceKind::Phone);
        assert_eq!(kind(40), DeviceKind::Tv);
        assert_eq!(kind(41), DeviceKind::MediaPlayer);
        assert_eq!(kind(42), DeviceKind::Speaker);
        assert_eq!(kind(50), DeviceKind::Iot);
        assert_eq!(kind(51), DeviceKind::Iot);
        assert_eq!(kind(60), DeviceKind::Server);
        assert_eq!(kind(61), DeviceKind::Voip);
        assert_eq!(kind(100), DeviceKind::Computer);
    }
}
