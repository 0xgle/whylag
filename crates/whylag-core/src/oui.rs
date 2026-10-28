//! MAC vendor lookup from the embedded IEEE MA-L registry (see scripts/build-oui.mjs).

use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

static DATA: &[u8] = include_bytes!("../data/oui.tsv.gz");

fn table() -> &'static HashMap<u32, String> {
    static TABLE: OnceLock<HashMap<u32, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut text = String::new();
        let mut map = HashMap::new();
        if flate2::read::GzDecoder::new(DATA)
            .read_to_string(&mut text)
            .is_ok()
        {
            for line in text.lines() {
                if let Some((prefix, vendor)) = line.split_once('\t') {
                    if let Ok(key) = u32::from_str_radix(prefix, 16) {
                        map.insert(key, vendor.to_string());
                    }
                }
            }
        }
        map
    })
}

/// Vendor for a MAC address in any common notation. Randomised (locally
/// administered) addresses have no registered vendor and return `None`.
pub fn lookup(mac: &str) -> Option<&'static str> {
    let normalized = crate::util::normalize_mac(mac)?;
    if crate::util::is_random_mac(&normalized) {
        return None;
    }
    let hex: String = normalized.chars().filter(|c| *c != ':').take(6).collect();
    let key = u32::from_str_radix(&hex, 16).ok()?;
    table().get(&key).map(String::as_str)
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_vendors() {
        assert_eq!(
            super::lookup("b8:27:eb:12:34:56"),
            Some("Raspberry Pi Foundation")
        );
        assert_eq!(super::lookup("F4-F2-6D-00-00-01"), Some("TP-Link"));
        assert_eq!(super::lookup("da:a1:19:00:00:01"), None);
    }
}
