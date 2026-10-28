//! Shared networking primitives: local interface facts, TCP and ICMP probes,
//! a tiny DNS client, ARP table, SNMP and NetBIOS lookups.

pub mod arp;
pub mod dns;
pub mod icmp;
pub mod netbios;
pub mod snmp;

use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

/// Facts about the active network connection.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalNet {
    pub interface_name: String,
    pub friendly_name: Option<String>,
    pub description: Option<String>,
    pub kind: String,
    pub mac: Option<String>,
    pub ipv4: Option<Ipv4Addr>,
    pub prefix_len: Option<u8>,
    pub ipv6: Vec<String>,
    pub gateway: Option<Ipv4Addr>,
    pub gateway_mac: Option<String>,
    pub dns_servers: Vec<IpAddr>,
    pub link_speed_mbps: Option<u64>,
    pub mtu: Option<u32>,
    pub dhcp: Option<bool>,
}

impl LocalNet {
    /// The IPv4 network to scan, capped at /22 (1022 hosts) around our address.
    pub fn scan_range(&self) -> Vec<Ipv4Addr> {
        let (Some(ip), Some(prefix)) = (self.ipv4, self.prefix_len) else {
            return vec![];
        };
        let prefix = prefix.max(22);
        let mask = if prefix >= 32 {
            u32::MAX
        } else {
            !((1u32 << (32 - prefix)) - 1)
        };
        let base = u32::from(ip) & mask;
        let size = 1u32 << (32 - prefix as u32);
        // Point-to-point (/31) and host (/32) links have no network or
        // broadcast address: every address in the block counts, plus the
        // gateway, which on a /32 lies outside it.
        if prefix >= 31 {
            let mut v: Vec<Ipv4Addr> = (0..size)
                .map(|i| Ipv4Addr::from(base + i))
                .filter(|a| *a != ip)
                .collect();
            if let Some(g) = self.gateway.filter(|g| *g != ip && !v.contains(g)) {
                v.push(g);
            }
            return v;
        }
        (1..size.saturating_sub(1))
            .map(|i| Ipv4Addr::from(base + i))
            .filter(|a| *a != ip)
            .collect()
    }

    pub fn subnet_label(&self) -> Option<String> {
        let (ip, prefix) = (self.ipv4?, self.prefix_len?);
        let mask = if prefix >= 32 {
            u32::MAX
        } else {
            !((1u32 << (32 - prefix)) - 1)
        };
        Some(format!(
            "{}/{}",
            Ipv4Addr::from(u32::from(ip) & mask),
            prefix
        ))
    }
}

/// Reads the default interface. Returns `None` when there is no usable IPv4 route.
pub fn local_net() -> Option<LocalNet> {
    let iface = netdev::get_default_interface().ok()?;
    let v4 = iface.ipv4.first();
    let gateway = iface.gateway.as_ref();
    let gw_mac = gateway
        .map(|g| g.mac_addr.octets())
        .filter(|o| o.iter().any(|b| *b != 0))
        .map(|o| crate::util::fmt_mac(&o));
    Some(LocalNet {
        interface_name: iface.name.clone(),
        friendly_name: iface.friendly_name.clone(),
        description: iface.description.clone(),
        kind: format!("{:?}", iface.if_type),
        mac: iface.mac_addr.map(|m| crate::util::fmt_mac(&m.octets())),
        ipv4: v4.map(|n| n.addr()),
        prefix_len: v4.map(|n| n.prefix_len()),
        ipv6: iface.ipv6.iter().map(|n| n.addr().to_string()).collect(),
        gateway: gateway.and_then(|g| g.ipv4.first().copied()),
        gateway_mac: gw_mac,
        dns_servers: iface.dns_servers.clone(),
        link_speed_mbps: iface
            .transmit_speed
            .or(iface.receive_speed)
            .map(|b| b / 1_000_000),
        mtu: iface.mtu,
        dhcp: iface.dhcp_v4_enabled,
    })
}

/// RFC 1918, link-local, loopback and carrier-grade NAT (100.64.0.0/10) ranges.
pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_private() || v4.is_loopback() || v4.is_link_local() || is_cgnat(v4) || o[0] == 0
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

pub fn is_cgnat(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && (64..=127).contains(&o[1])
}

/// Outcome of a TCP connection attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpOutcome {
    Open,
    /// The host answered with a reset: alive, port closed.
    Closed,
    /// No answer in time (filtered or host down).
    Timeout,
    Error,
}

/// Attempts a TCP connection and reports how long the answer took (ms).
pub async fn tcp_probe(addr: SocketAddr, timeout: Duration) -> (TcpOutcome, Option<f64>) {
    let start = Instant::now();
    match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await {
        Ok(Ok(_stream)) => (
            TcpOutcome::Open,
            Some(start.elapsed().as_secs_f64() * 1000.0),
        ),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => (
            TcpOutcome::Closed,
            Some(start.elapsed().as_secs_f64() * 1000.0),
        ),
        Ok(Err(_)) => (TcpOutcome::Error, None),
        Err(_) => (TcpOutcome::Timeout, None),
    }
}

/// Resolves a host name or literal to the first IPv4 address (falls back to IPv6).
pub async fn resolve(host: &str) -> Option<IpAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(ip);
    }
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, 0)).await.ok()?.collect();
    addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or(addrs.first())
        .map(|a| a.ip())
}

/// Reverse DNS with a time limit (system resolver).
pub async fn reverse_dns(ip: IpAddr, timeout: Duration) -> Option<String> {
    let task = tokio::task::spawn_blocking(move || dns_lookup::lookup_addr(&ip).ok());
    let name = tokio::time::timeout(timeout, task).await.ok()?.ok()??;
    // getnameinfo returns the literal address when there is no PTR record.
    if name.parse::<IpAddr>().is_ok() {
        None
    } else {
        Some(name.trim_end_matches('.').to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_range_is_capped() {
        let net = LocalNet {
            ipv4: Some(Ipv4Addr::new(10, 1, 2, 3)),
            prefix_len: Some(16),
            ..Default::default()
        };
        let range = net.scan_range();
        assert_eq!(range.len(), 1021); // /22 minus network, broadcast and ourselves
        assert!(!range.contains(&Ipv4Addr::new(10, 1, 2, 3)));
        let net = LocalNet {
            ipv4: Some(Ipv4Addr::new(192, 168, 1, 10)),
            prefix_len: Some(24),
            ..Default::default()
        };
        assert_eq!(net.scan_range().len(), 253);
        assert_eq!(net.subnet_label().as_deref(), Some("192.168.1.0/24"));
        // Point-to-point and host routes still find the other end / gateway.
        let p2p = LocalNet {
            ipv4: Some(Ipv4Addr::new(10, 0, 0, 1)),
            prefix_len: Some(31),
            ..Default::default()
        };
        assert_eq!(p2p.scan_range(), vec![Ipv4Addr::new(10, 0, 0, 0)]);
        let host = LocalNet {
            ipv4: Some(Ipv4Addr::new(100, 64, 3, 7)),
            prefix_len: Some(32),
            gateway: Some(Ipv4Addr::new(100, 64, 0, 1)),
            ..Default::default()
        };
        assert_eq!(host.scan_range(), vec![Ipv4Addr::new(100, 64, 0, 1)]);
    }

    #[test]
    fn private_ranges() {
        assert!(is_private("192.168.0.1".parse().unwrap()));
        assert!(is_private("100.72.1.1".parse().unwrap()));
        assert!(!is_private("8.8.8.8".parse().unwrap()));
        assert!(is_private("fe80::1".parse().unwrap()));
    }
}
