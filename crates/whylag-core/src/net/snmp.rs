//! Minimal SNMP v2c client (GET and GETNEXT walks) for identifying routers,
//! switches and printers and for reading LLDP neighbour tables.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Str(Vec<u8>),
    Oid(Vec<u32>),
    Ip([u8; 4]),
    Counter(u64),
    Ticks(u64),
    Null,
    /// noSuchObject / noSuchInstance / endOfMibView
    Missing,
}

impl Value {
    pub fn as_text(&self) -> Option<String> {
        match self {
            Value::Str(b) => Some(String::from_utf8_lossy(b).trim_matches(char::from(0)).trim().to_string()),
            Value::Int(i) => Some(i.to_string()),
            Value::Counter(c) | Value::Ticks(c) => Some(c.to_string()),
            Value::Ip(a) => Some(format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])),
            Value::Oid(o) => Some(oid_string(o)),
            _ => None,
        }
    }
}

pub fn parse_oid(s: &str) -> Vec<u32> {
    s.trim_start_matches('.').split('.').filter_map(|p| p.parse().ok()).collect()
}

pub fn oid_string(o: &[u32]) -> String {
    o.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(".")
}

fn push_len(out: &mut Vec<u8>, len: usize) {
    if len < 0x80 {
        out.push(len as u8);
    } else {
        let bytes: Vec<u8> = (len as u32).to_be_bytes().into_iter().skip_while(|b| *b == 0).collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
}

fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    push_len(&mut out, body.len());
    out.extend_from_slice(body);
    out
}

fn int(v: i64) -> Vec<u8> {
    let mut bytes = v.to_be_bytes().to_vec();
    while bytes.len() > 1 && ((bytes[0] == 0 && bytes[1] & 0x80 == 0) || (bytes[0] == 0xff && bytes[1] & 0x80 != 0)) {
        bytes.remove(0);
    }
    tlv(0x02, &bytes)
}

fn oid(o: &[u32]) -> Vec<u8> {
    let mut body = Vec::new();
    if o.len() >= 2 {
        body.push((o[0] * 40 + o[1]) as u8);
    }
    for &n in o.iter().skip(2) {
        let mut stack = vec![(n & 0x7f) as u8];
        let mut v = n >> 7;
        while v > 0 {
            stack.push(((v & 0x7f) as u8) | 0x80);
            v >>= 7;
        }
        stack.reverse();
        body.extend(stack);
    }
    tlv(0x06, &body)
}

/// Encodes a v2c request. `pdu_tag` 0xA0 = GET, 0xA1 = GETNEXT.
pub fn encode_request(community: &str, request_id: i32, pdu_tag: u8, oids: &[Vec<u32>]) -> Vec<u8> {
    let varbinds: Vec<u8> = oids.iter().flat_map(|o| tlv(0x30, &[oid(o), vec![0x05, 0x00]].concat())).collect();
    let pdu = tlv(pdu_tag, &[int(request_id as i64), int(0), int(0), tlv(0x30, &varbinds)].concat());
    tlv(0x30, &[int(1), tlv(0x04, community.as_bytes()), pdu].concat())
}

/// Reads one TLV at `pos`: (tag, body range, next position).
fn read(buf: &[u8], pos: usize) -> Option<(u8, std::ops::Range<usize>, usize)> {
    let tag = *buf.get(pos)?;
    let first = *buf.get(pos + 1)? as usize;
    let (len, hdr) = if first & 0x80 == 0 {
        (first, 2)
    } else {
        let n = first & 0x7f;
        if n > 4 {
            return None;
        }
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | *buf.get(pos + 2 + i)? as usize;
        }
        (len, 2 + n)
    };
    let start = pos + hdr;
    let end = start.checked_add(len)?;
    if end > buf.len() {
        return None;
    }
    Some((tag, start..end, end))
}

fn decode_uint(b: &[u8]) -> u64 {
    b.iter().fold(0u64, |acc, x| (acc << 8) | *x as u64)
}

fn decode_oid(b: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    if let Some(first) = b.first() {
        out.push((*first / 40) as u32);
        out.push((*first % 40) as u32);
    }
    let mut cur = 0u32;
    for &x in b.iter().skip(1) {
        cur = (cur << 7) | (x & 0x7f) as u32;
        if x & 0x80 == 0 {
            out.push(cur);
            cur = 0;
        }
    }
    out
}

/// Decodes a response into (request id, error status, varbinds).
pub fn decode_response(buf: &[u8]) -> Option<(i32, i64, Vec<(Vec<u32>, Value)>)> {
    let (_, msg, _) = read(buf, 0)?;
    let (_, _ver, p) = read(buf, msg.start)?;
    let (_, _comm, p) = read(buf, p)?;
    let (tag, pdu, _) = read(buf, p)?;
    if tag != 0xA2 {
        return None;
    }
    let (_, rid, p) = read(buf, pdu.start)?;
    let (_, err, p) = read(buf, p)?;
    let (_, _idx, p) = read(buf, p)?;
    let (_, list, _) = read(buf, p)?;
    let request_id = decode_uint(&buf[rid.clone()]) as i32;
    let error = decode_uint(&buf[err]) as i64;
    let mut out = Vec::new();
    let mut pos = list.start;
    while pos < list.end {
        let (_, vb, next) = read(buf, pos)?;
        let (_, o, vp) = read(buf, vb.start)?;
        let (vtag, vr, _) = read(buf, vp)?;
        let body = &buf[vr];
        let value = match vtag {
            0x02 => {
                let mut v = if body.first().map(|b| b & 0x80 != 0).unwrap_or(false) { -1i64 } else { 0 };
                for b in body {
                    v = (v << 8) | *b as i64;
                }
                Value::Int(v)
            }
            0x04 => Value::Str(body.to_vec()),
            0x06 => Value::Oid(decode_oid(body)),
            0x40 if body.len() == 4 => Value::Ip([body[0], body[1], body[2], body[3]]),
            0x41 | 0x42 | 0x46 => Value::Counter(decode_uint(body)),
            0x43 => Value::Ticks(decode_uint(body)),
            0x05 => Value::Null,
            _ => Value::Missing,
        };
        out.push((decode_oid(&buf[o]), value));
        pos = next;
    }
    Some((request_id, error, out))
}

async fn exchange(ip: IpAddr, packet: &[u8], request_id: i32, timeout: Duration) -> Option<Vec<(Vec<u32>, Value)>> {
    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0").await.ok()?;
    sock.send_to(packet, SocketAddr::new(ip, 161)).await.ok()?;
    let mut buf = vec![0u8; 8192];
    let (n, _) = tokio::time::timeout(timeout, sock.recv_from(&mut buf)).await.ok()?.ok()?;
    let (rid, err, vbs) = decode_response(&buf[..n])?;
    (rid == request_id && err == 0).then_some(vbs)
}

pub async fn get(ip: IpAddr, community: &str, oids: &[&str], timeout: Duration) -> Option<Vec<(Vec<u32>, Value)>> {
    let rid: i32 = rand::random::<i32>().abs();
    let parsed: Vec<Vec<u32>> = oids.iter().map(|o| parse_oid(o)).collect();
    exchange(ip, &encode_request(community, rid, 0xA0, &parsed), rid, timeout).await
}

/// Walks a subtree with GETNEXT, returning at most `max` entries.
pub async fn walk(ip: IpAddr, community: &str, root: &str, max: usize, timeout: Duration) -> Vec<(Vec<u32>, Value)> {
    let root = parse_oid(root);
    let mut cur = root.clone();
    let mut out = Vec::new();
    while out.len() < max {
        let rid: i32 = rand::random::<i32>().abs();
        let Some(vbs) = exchange(ip, &encode_request(community, rid, 0xA1, &[cur.clone()]), rid, timeout).await else { break };
        let Some((o, v)) = vbs.into_iter().next() else { break };
        if !o.starts_with(&root) || v == Value::Missing || o <= cur {
            break;
        }
        cur = o.clone();
        out.push((o, v));
    }
    out
}

pub const SYS_DESCR: &str = "1.3.6.1.2.1.1.1.0";
pub const SYS_OBJECT_ID: &str = "1.3.6.1.2.1.1.2.0";
pub const SYS_NAME: &str = "1.3.6.1.2.1.1.5.0";
/// lldpRemSysName (LLDP-MIB): neighbours' system names.
pub const LLDP_REM_SYS_NAME: &str = "1.0.8802.1.1.2.1.4.1.1.9";
/// lldpRemPortId
pub const LLDP_REM_PORT_ID: &str = "1.0.8802.1.1.2.1.4.1.1.7";
/// lldpRemManAddrIfSubtype — the index of this table carries neighbours' management IPs.
pub const LLDP_REM_MAN_ADDR: &str = "1.0.8802.1.1.2.1.4.2.1.3";
/// lldpLocPortDesc
pub const LLDP_LOC_PORT_DESC: &str = "1.0.8802.1.1.2.1.3.7.1.4";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oid_round_trip() {
        let o = parse_oid("1.3.6.1.4.1.2636.1.1.1");
        let enc = oid(&o);
        assert_eq!(decode_oid(&enc[2..]), o);
        let big = parse_oid("1.0.8802.1.1.2.1.4.1.1.9");
        assert_eq!(decode_oid(&oid(&big)[2..]), big);
    }

    #[test]
    fn decodes_a_response() {
        // Craft a response by hand: same structure as a request but tag 0xA2 and a string value.
        let vb = tlv(0x30, &[oid(&parse_oid(SYS_NAME)), tlv(0x04, b"core-sw-01")].concat());
        let pdu = tlv(0xA2, &[int(42), int(0), int(0), tlv(0x30, &vb)].concat());
        let msg = tlv(0x30, &[int(1), tlv(0x04, b"public"), pdu].concat());
        let (rid, err, vbs) = decode_response(&msg).unwrap();
        assert_eq!((rid, err), (42, 0));
        assert_eq!(vbs[0].1.as_text().as_deref(), Some("core-sw-01"));
        let req = encode_request("public", 7, 0xA0, &[parse_oid(SYS_DESCR)]);
        assert_eq!(req[0], 0x30);
        assert!(decode_response(&req).is_none(), "requests are not responses");
    }

    #[test]
    fn integer_encoding_is_minimal() {
        assert_eq!(int(0), vec![2, 1, 0]);
        assert_eq!(int(128), vec![2, 2, 0, 128]);
        assert_eq!(int(-1), vec![2, 1, 0xff]);
    }
}
