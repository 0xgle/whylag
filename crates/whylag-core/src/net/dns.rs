//! Minimal DNS client (A queries over UDP) for resolver benchmarks and
//! hijacking checks. We need precise timing per resolver, which the system
//! resolver API cannot give us.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct DnsAnswer {
    pub rtt_ms: f64,
    /// RCODE from the response header (0 = NOERROR, 3 = NXDOMAIN).
    pub rcode: u8,
    pub addresses: Vec<Ipv4Addr>,
}

pub fn build_query(id: u16, name: &str) -> Vec<u8> {
    let mut q = Vec::with_capacity(32 + name.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00]); // standard query, recursion desired
    q.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]); // 1 question
    for label in name.trim_end_matches('.').split('.') {
        let bytes = label.as_bytes();
        q.push(bytes.len().min(63) as u8);
        q.extend_from_slice(&bytes[..bytes.len().min(63)]);
    }
    q.push(0);
    q.extend_from_slice(&[0, 1, 0, 1]); // type A, class IN
    q
}

/// Skips a (possibly compressed) name, returning the offset after it.
fn skip_name(buf: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        let len = *buf.get(pos)? as usize;
        if len == 0 {
            return Some(pos + 1);
        }
        if len & 0xc0 == 0xc0 {
            return Some(pos + 2);
        }
        pos += 1 + len;
    }
}

pub fn parse_response(buf: &[u8], id: u16) -> Option<(u8, Vec<Ipv4Addr>)> {
    if buf.len() < 12 || u16::from_be_bytes([buf[0], buf[1]]) != id {
        return None;
    }
    let rcode = buf[3] & 0x0f;
    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let an = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    let mut pos = 12;
    for _ in 0..qd {
        pos = skip_name(buf, pos)? + 4;
    }
    let mut addrs = Vec::new();
    for _ in 0..an {
        pos = skip_name(buf, pos)?;
        let rtype = u16::from_be_bytes([*buf.get(pos)?, *buf.get(pos + 1)?]);
        let rdlen = u16::from_be_bytes([*buf.get(pos + 8)?, *buf.get(pos + 9)?]) as usize;
        let data = buf.get(pos + 10..pos + 10 + rdlen)?;
        if rtype == 1 && rdlen == 4 {
            addrs.push(Ipv4Addr::new(data[0], data[1], data[2], data[3]));
        }
        pos += 10 + rdlen;
    }
    Some((rcode, addrs))
}

/// Sends one A query to `server` and waits for the matching answer.
pub async fn query(server: IpAddr, name: &str, timeout: Duration) -> Option<DnsAnswer> {
    let bind: SocketAddr = if server.is_ipv4() { "0.0.0.0:0".parse().unwrap() } else { "[::]:0".parse().unwrap() };
    let sock = tokio::net::UdpSocket::bind(bind).await.ok()?;
    let id: u16 = rand::random();
    let packet = build_query(id, name);
    let started = Instant::now();
    sock.send_to(&packet, SocketAddr::new(server, 53)).await.ok()?;
    let mut buf = [0u8; 1500];
    let deadline = started + timeout;
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        let (n, _) = tokio::time::timeout(left, sock.recv_from(&mut buf)).await.ok()?.ok()?;
        if let Some((rcode, addresses)) = parse_response(&buf[..n], id) {
            return Some(DnsAnswer { rtt_ms: started.elapsed().as_secs_f64() * 1000.0, rcode, addresses });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_and_response_round_trip() {
        let q = build_query(0x1234, "example.com");
        assert_eq!(&q[..2], &[0x12, 0x34]);
        assert_eq!(&q[12..20], &[7, b'e', b'x', b'a', b'm', b'p', b'l', b'e']);
        // Build a response: header + question + one compressed A answer.
        let mut r = q.clone();
        r[2] = 0x81;
        r[3] = 0x80;
        r[7] = 1;
        r.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 93, 184, 216, 34]);
        let (rcode, addrs) = parse_response(&r, 0x1234).unwrap();
        assert_eq!(rcode, 0);
        assert_eq!(addrs, vec![Ipv4Addr::new(93, 184, 216, 34)]);
        assert!(parse_response(&r, 0x9999).is_none());
    }
}
