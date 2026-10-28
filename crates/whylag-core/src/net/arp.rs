//! Reads the operating system's ARP (neighbour) cache: IP to MAC mappings of
//! devices this computer has talked to on the local network.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::Duration;

/// Extracts IPv4 → MAC pairs from any ARP listing (Windows `arp -a`,
/// macOS `arp -an`, Linux `/proc/net/arp` or `ip neigh`). Works regardless of
/// the system language because it looks only at address-shaped tokens.
pub fn parse_table(text: &str) -> HashMap<Ipv4Addr, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let mut ip: Option<Ipv4Addr> = None;
        let mut mac: Option<String> = None;
        for token in line.split(|c: char| c.is_whitespace() || c == '(' || c == ')') {
            if ip.is_none() {
                if let Ok(a) = token.parse::<Ipv4Addr>() {
                    ip = Some(a);
                    continue;
                }
            }
            if mac.is_none() && (token.contains(':') || token.contains('-')) && token.len() >= 11 {
                // macOS prints single-digit octets ("0:1a:2b:..."); pad them.
                let parts: Vec<&str> = token.split([':', '-']).collect();
                if parts.len() == 6 && parts.iter().all(|p| !p.is_empty() && p.len() <= 2 && p.chars().all(|c| c.is_ascii_hexdigit())) {
                    let padded: Vec<String> = parts.iter().map(|p| format!("{:0>2}", p.to_lowercase())).collect();
                    mac = Some(padded.join(":"));
                }
            }
        }
        if let (Some(ip), Some(mac)) = (ip, mac) {
            // Skip incomplete, broadcast and multicast entries.
            if mac != "00:00:00:00:00:00" && mac != "ff:ff:ff:ff:ff:ff" && !mac.starts_with("01:00:5e") && !ip.is_multicast() && !ip.is_broadcast() {
                map.insert(ip, mac);
            }
        }
    }
    map
}

/// Current ARP cache. Blocking.
pub fn table() -> HashMap<Ipv4Addr, String> {
    #[cfg(target_os = "linux")]
    if let Ok(text) = std::fs::read_to_string("/proc/net/arp") {
        return parse_table(&text);
    }
    let args: &[&str] = if cfg!(windows) { &["-a"] } else { &["-an"] };
    crate::util::run_tool_lenient("arp", args, Duration::from_secs(5)).map(|t| parse_table(&t)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_formats() {
        let windows_pl = "Interfejs: 192.168.1.10 --- 0x7\n  Adres internetowy      Adres fizyczny        Typ\n  192.168.1.1           24-5a-4c-11-22-33     dynamiczne\n  192.168.1.255         ff-ff-ff-ff-ff-ff     statyczne\n";
        let m = parse_table(windows_pl);
        assert_eq!(m.get(&"192.168.1.1".parse().unwrap()).map(String::as_str), Some("24:5a:4c:11:22:33"));
        assert_eq!(m.len(), 1);
        let mac = "? (192.168.1.20) at 0:1a:2b:c:d:e on en0 ifscope [ethernet]\n? (192.168.1.21) at (incomplete) on en0\n";
        let m = parse_table(mac);
        assert_eq!(m.get(&"192.168.1.20".parse().unwrap()).map(String::as_str), Some("00:1a:2b:0c:0d:0e"));
        assert_eq!(m.len(), 1);
        let linux = "IP address       HW type     Flags       HW address            Mask     Device\n192.168.1.30     0x1         0x2         b8:27:eb:00:11:22     *        wlan0\n192.168.1.31     0x1         0x0         00:00:00:00:00:00     *        wlan0\n";
        let m = parse_table(linux);
        assert_eq!(m.len(), 1);
    }
}
