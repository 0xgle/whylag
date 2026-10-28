//! NetBIOS node status query (UDP 137): Windows computer and workgroup names.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NetbiosInfo {
    pub name: Option<String>,
    pub workgroup: Option<String>,
}

pub fn build_query(id: u16) -> Vec<u8> {
    let mut q = Vec::with_capacity(50);
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    // Name "*" padded to 16 bytes with NULs, first-level encoded (each nibble + 'A').
    q.push(32);
    let mut raw = [0u8; 16];
    raw[0] = b'*';
    for b in raw {
        q.push(b'A' + (b >> 4));
        q.push(b'A' + (b & 0x0f));
    }
    q.push(0);
    q.extend_from_slice(&[0x00, 0x21, 0x00, 0x01]); // NBSTAT, IN
    q
}

pub fn parse_response(buf: &[u8], id: u16) -> Option<NetbiosInfo> {
    if buf.len() < 57 || u16::from_be_bytes([buf[0], buf[1]]) != id {
        return None;
    }
    // Header (12) + encoded name (34) + type/class/ttl/rdlength (10) = 56.
    let count = buf[56] as usize;
    let mut info = NetbiosInfo::default();
    for i in 0..count {
        let start = 57 + i * 18;
        let entry = buf.get(start..start + 18)?;
        let name = String::from_utf8_lossy(&entry[..15]).trim().to_string();
        let suffix = entry[15];
        let group = entry[16] & 0x80 != 0;
        if suffix == 0x00 && !group && info.name.is_none() {
            info.name = Some(name);
        } else if suffix == 0x00 && group && info.workgroup.is_none() {
            info.workgroup = Some(name);
        }
    }
    (info.name.is_some() || info.workgroup.is_some()).then_some(info)
}

pub async fn query(ip: IpAddr, timeout: Duration) -> Option<NetbiosInfo> {
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await.ok()?;
    let id: u16 = rand::random();
    sock.send_to(&build_query(id), SocketAddr::new(ip, 137)).await.ok()?;
    let mut buf = [0u8; 1024];
    let (n, _) = tokio::time::timeout(timeout, sock.recv_from(&mut buf)).await.ok()?.ok()?;
    parse_response(&buf[..n], id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_status() {
        let mut r = build_query(7);
        r[2] = 0x84;
        r.truncate(12 + 34);
        r.extend_from_slice(&[0, 0x21, 0, 1, 0, 0, 0, 0, 0, 0]);
        r.push(2);
        let mut e1 = *b"DESKTOP-ANNA    ";
        e1[15] = 0;
        r.extend_from_slice(&e1);
        r.extend_from_slice(&[0x04, 0x00]);
        let mut e2 = *b"WORKGROUP       ";
        e2[15] = 0;
        r.extend_from_slice(&e2);
        r.extend_from_slice(&[0x84, 0x00]);
        let info = parse_response(&r, 7).unwrap();
        assert_eq!(info.name.as_deref(), Some("DESKTOP-ANNA"));
        assert_eq!(info.workgroup.as_deref(), Some("WORKGROUP"));
    }
}
