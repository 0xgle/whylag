use std::time::{SystemTime, UNIX_EPOCH};

/// Runs a background job future and turns a panic into an error string, so
/// the job's `running` flag is always cleared by the caller.
pub async fn catch_job<F, T>(fut: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    use futures_util::FutureExt;
    match std::panic::AssertUnwindSafe(fut).catch_unwind().await {
        Ok(r) => r,
        Err(_) => Err("internal".to_string()),
    }
}

/// Reads at most `limit` bytes of a response body and decodes it lossily.
pub async fn read_capped(mut resp: reqwest::Response, limit: usize) -> String {
    let mut buf: Vec<u8> = Vec::new();
    while buf.len() < limit {
        match resp.chunk().await {
            Ok(Some(chunk)) => buf.extend_from_slice(&chunk),
            _ => break,
        }
    }
    buf.truncate(limit);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Formats six MAC bytes as lowercase `aa:bb:cc:dd:ee:ff`.
pub fn fmt_mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Normalises any MAC notation (`AA-BB-CC-DD-EE-FF`, `aabb.ccdd.eeff`, ...) to
/// lowercase colon form. Returns `None` if it does not contain exactly 12 hex digits.
pub fn normalize_mac(raw: &str) -> Option<String> {
    let hex: String = raw.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 12 {
        return None;
    }
    let bytes: Vec<u8> = (0..6)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0))
        .collect();
    Some(fmt_mac(&bytes))
}

/// True for locally administered (usually randomised, "private") MAC addresses.
pub fn is_random_mac(mac: &str) -> bool {
    normalize_mac(mac)
        .and_then(|m| u8::from_str_radix(&m[0..2], 16).ok())
        .map(|b| b & 0x02 != 0)
        .unwrap_or(false)
}

/// Median of a slice (returns `None` for empty input). Does not require sorted input.
pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = v.len() / 2;
    Some(if v.len() % 2 == 0 {
        (v[mid - 1] + v[mid]) / 2.0
    } else {
        v[mid]
    })
}

/// Percentile (0..=100) using linear interpolation between closest ranks.
pub fn percentile(values: &[f64], p: f64) -> Option<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = (p.clamp(0.0, 100.0) / 100.0) * (v.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    Some(v[lo] + (v[hi] - v[lo]) * (rank - lo as f64))
}

/// Mean absolute difference between consecutive samples (RFC 3550 style jitter estimate).
pub fn jitter(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let diffs: Vec<f64> = values.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    Some(diffs.iter().sum::<f64>() / diffs.len() as f64)
}

/// Runs a system tool and returns its stdout. Never opens a console window on
/// Windows. Fails if the tool is missing, exits non-zero or exceeds `timeout`.
pub fn run_tool(
    program: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> std::io::Result<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Force English, dot-decimal output from tools whose text we parse.
    cmd.env("LC_ALL", "C").env("LANG", "C");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let out = reader.join().unwrap_or_default();
            if !status.success() {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut err);
                }
                return Err(std::io::Error::other(format!(
                    "{program} exited with {status}: {}",
                    err.trim()
                )));
            }
            return Ok(String::from_utf8_lossy(&out).into_owned());
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("{program} timed out"),
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// Like [`run_tool`] but returns stdout even when the tool exits non-zero
/// (traceroute and ping report partial results that way).
pub fn run_tool_lenient(
    program: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> std::io::Result<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd.env("LC_ALL", "C").env("LANG", "C");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let started = std::time::Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    let out = reader.join().unwrap_or_default();
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Standard base64 (RFC 4648) with padding.
pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decodes standard base64; whitespace is ignored. Returns `None` on bad input.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        for s in [
            &b""[..],
            b"f",
            b"fo",
            b"foo",
            b"foobar",
            &[0u8, 255, 128, 7],
        ] {
            assert_eq!(base64_decode(&base64(s)).unwrap(), s);
        }
        assert!(base64_decode("@@").is_none());
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn mac_normalisation() {
        assert_eq!(
            normalize_mac("AA-BB-CC-00-11-22").as_deref(),
            Some("aa:bb:cc:00:11:22")
        );
        assert_eq!(
            normalize_mac("aabb.cc00.1122").as_deref(),
            Some("aa:bb:cc:00:11:22")
        );
        assert_eq!(normalize_mac("nope"), None);
    }

    #[test]
    fn random_mac_bit() {
        assert!(is_random_mac("da:a1:19:00:00:01"));
        assert!(!is_random_mac("00:1a:2b:00:00:01"));
    }

    #[test]
    fn stats() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0, 5.0], 50.0), Some(3.0));
        assert_eq!(jitter(&[10.0, 12.0, 10.0]), Some(2.0));
        assert_eq!(jitter(&[10.0]), None);
    }
}
