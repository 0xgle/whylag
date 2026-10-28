//! ICMP echo (ping) without administrator rights.
//!
//! * Windows: `IcmpSendEcho` from iphlpapi, which also supports a TTL so it
//!   doubles as a native traceroute.
//! * Linux/macOS: unprivileged ICMP datagram sockets, falling back to the
//!   system `ping` tool where those are disabled.

use std::net::Ipv4Addr;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PingResult {
    Reply { rtt_ms: f64, from: Ipv4Addr },
    /// A router on the way answered "time to live exceeded" (traceroute hop).
    TtlExceeded { rtt_ms: f64, from: Ipv4Addr },
    Unreachable { from: Ipv4Addr },
    Timeout,
}

impl PingResult {
    pub fn rtt(&self) -> Option<f64> {
        match self {
            PingResult::Reply { rtt_ms, .. } | PingResult::TtlExceeded { rtt_ms, .. } => Some(*rtt_ms),
            _ => None,
        }
    }
}

/// Sends one echo request. Blocking: run on a blocking thread.
pub fn ping(dest: Ipv4Addr, timeout: Duration, ttl: Option<u8>) -> PingResult {
    imp::ping(dest, timeout, ttl)
}

/// Async wrapper around [`ping`].
pub async fn ping_async(dest: Ipv4Addr, timeout: Duration) -> PingResult {
    tokio::task::spawn_blocking(move || ping(dest, timeout, None)).await.unwrap_or(PingResult::Timeout)
}

/// True when this platform can trace routes natively (TTL-limited echo).
pub fn native_ttl() -> bool {
    cfg!(windows)
}

#[cfg(windows)]
mod imp {
    use super::PingResult;
    use std::ffi::c_void;
    use std::net::Ipv4Addr;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::NetworkManagement::IpHelper::*;

    pub fn ping(dest: Ipv4Addr, timeout: Duration, ttl: Option<u8>) -> PingResult {
        unsafe {
            let handle = IcmpCreateFile();
            if handle == INVALID_HANDLE_VALUE || handle.is_null() {
                return PingResult::Timeout;
            }
            let payload = *b"WhyLag-probe-0123456789abcdefgh";
            let options = IP_OPTION_INFORMATION { Ttl: ttl.unwrap_or(128), Tos: 0, Flags: 0, OptionsSize: 0, OptionsData: std::ptr::null_mut() };
            // Reply buffer: one ICMP_ECHO_REPLY + payload + 8 bytes for an ICMP error.
            let mut buf = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 64];
            let addr = u32::from_ne_bytes(dest.octets());
            let started = Instant::now();
            let count = IcmpSendEcho(
                handle,
                addr,
                payload.as_ptr() as *const c_void,
                payload.len() as u16,
                &options,
                buf.as_mut_ptr() as *mut c_void,
                buf.len() as u32,
                timeout.as_millis().max(1) as u32,
            );
            let last_error = if count == 0 { GetLastError() } else { 0 };
            IcmpCloseHandle(handle);
            let reply = std::ptr::read_unaligned(buf.as_ptr() as *const ICMP_ECHO_REPLY);
            let from = Ipv4Addr::from(reply.Address.to_ne_bytes());
            // RoundTripTime has 1 ms resolution; measure ourselves for sub-ms LAN hops.
            let measured = started.elapsed().as_secs_f64() * 1000.0;
            let rtt = if reply.RoundTripTime == 0 { measured.min(1.0) } else { (reply.RoundTripTime as f64).min(measured.max(reply.RoundTripTime as f64)) };
            let status = if count > 0 { reply.Status } else { last_error };
            match status {
                IP_SUCCESS if count > 0 => PingResult::Reply { rtt_ms: rtt, from },
                IP_TTL_EXPIRED_TRANSIT if reply.Address != 0 => PingResult::TtlExceeded { rtt_ms: rtt, from },
                IP_DEST_HOST_UNREACHABLE | IP_DEST_NET_UNREACHABLE if reply.Address != 0 => PingResult::Unreachable { from },
                _ => PingResult::Timeout,
            }
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::PingResult;
    use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    use std::mem::MaybeUninit;
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::time::{Duration, Instant};

    static SEQ: AtomicU16 = AtomicU16::new(1);

    fn checksum(data: &[u8]) -> u16 {
        let mut sum = 0u32;
        for chunk in data.chunks(2) {
            let word = if chunk.len() == 2 { u16::from_be_bytes([chunk[0], chunk[1]]) } else { u16::from_be_bytes([chunk[0], 0]) };
            sum += word as u32;
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        !(sum as u16)
    }

    fn socket_ping(dest: Ipv4Addr, timeout: Duration, ttl: Option<u8>) -> Option<PingResult> {
        let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4)).ok()?;
        if let Some(t) = ttl {
            sock.set_ttl_v4(t as u32).ok()?;
        }
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let mut packet = vec![8u8, 0, 0, 0, 0x57, 0x4c, (seq >> 8) as u8, seq as u8];
        packet.extend_from_slice(b"WhyLag-probe-0123456789abcdefgh");
        let c = checksum(&packet);
        packet[2] = (c >> 8) as u8;
        packet[3] = c as u8;
        let target = SockAddr::from(SocketAddrV4::new(dest, 0));
        let started = Instant::now();
        sock.send_to(&packet, &target).ok()?;
        let mut buf = [MaybeUninit::<u8>::uninit(); 1500];
        loop {
            let left = timeout.checked_sub(started.elapsed())?;
            if left.is_zero() {
                return Some(PingResult::Timeout);
            }
            sock.set_read_timeout(Some(left)).ok()?;
            let (n, from) = match sock.recv_from(&mut buf) {
                Ok(r) => r,
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    return Some(PingResult::Timeout)
                }
                Err(_) => return Some(PingResult::Timeout),
            };
            let data: Vec<u8> = buf[..n].iter().map(|b| unsafe { b.assume_init() }).collect();
            // macOS includes the IP header on datagram ICMP sockets; Linux does not.
            let icmp = if !data.is_empty() && data[0] >> 4 == 4 && data.len() >= 20 {
                let ihl = ((data[0] & 0x0f) as usize) * 4;
                &data[ihl.min(data.len())..]
            } else {
                &data[..]
            };
            if icmp.len() < 8 {
                continue;
            }
            let reply_seq = u16::from_be_bytes([icmp[6], icmp[7]]);
            let from_ip = from.as_socket_ipv4().map(|a| *a.ip()).unwrap_or(dest);
            if icmp[0] == 0 && reply_seq == seq {
                return Some(PingResult::Reply { rtt_ms: started.elapsed().as_secs_f64() * 1000.0, from: from_ip });
            }
        }
    }

    fn tool_ping(dest: Ipv4Addr, timeout: Duration) -> PingResult {
        let secs = timeout.as_secs().max(1).to_string();
        let ip = dest.to_string();
        let args: Vec<&str> = if cfg!(target_os = "macos") { vec!["-c", "1", "-t", &secs, &ip] } else { vec!["-c", "1", "-W", &secs, &ip] };
        let out = crate::util::run_tool_lenient("ping", &args, timeout + Duration::from_millis(800)).unwrap_or_default();
        parse_ping_time(&out).map(|rtt_ms| PingResult::Reply { rtt_ms, from: dest }).unwrap_or(PingResult::Timeout)
    }

    pub(crate) fn parse_ping_time(out: &str) -> Option<f64> {
        let idx = out.find("time=").or_else(|| out.find("time<"))?;
        let rest = &out[idx + 5..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        num.parse().ok()
    }

    pub fn ping(dest: Ipv4Addr, timeout: Duration, ttl: Option<u8>) -> PingResult {
        socket_ping(dest, timeout, ttl).unwrap_or_else(|| if ttl.is_some() { PingResult::Timeout } else { tool_ping(dest, timeout) })
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn parses_ping_output() {
            assert_eq!(super::parse_ping_time("64 bytes from 1.1.1.1: icmp_seq=1 ttl=57 time=12.4 ms"), Some(12.4));
            assert_eq!(super::parse_ping_time("Request timeout"), None);
        }
    }
}
