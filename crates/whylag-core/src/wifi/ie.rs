//! 802.11 information element parser.
//!
//! Windows hands us the raw IE blob of every beacon, which carries far more than
//! the OS shows: exact security suites, management-frame protection, channel
//! width, Wi-Fi generation, WPS, fast-roaming support and AP load.

use super::{Pmf, Roaming, Security};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IeInfo {
    pub ssid: Option<Vec<u8>>,
    pub ds_channel: Option<u32>,
    pub country: Option<String>,
    pub station_count: Option<u16>,
    pub utilization_pct: Option<u8>,
    pub ht: bool,
    pub vht: bool,
    pub he: bool,
    pub eht: bool,
    pub width_mhz: u32,
    pub rsn_akms: Vec<u8>,
    /// An RSN element was present, even if we could not decode its AKMs.
    pub rsn_present: bool,
    pub rsn_ciphers: Vec<u8>,
    pub rsn_caps: Option<u16>,
    pub wpa1: bool,
    pub wpa1_ciphers: Vec<u8>,
    pub wps: bool,
    pub rrm: bool,
    pub bss_transition: bool,
    pub mobility_domain: bool,
}

const OUI_IEEE: [u8; 3] = [0x00, 0x0f, 0xac];
const OUI_MS: [u8; 3] = [0x00, 0x50, 0xf2];

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

/// Parses a suite list (`count` u16 followed by 4-byte selectors) and returns
/// the suite types whose OUI matches `oui`, plus the offset after the list.
fn suite_list(b: &[u8], at: usize, oui: [u8; 3]) -> Option<(Vec<u8>, usize)> {
    let count = u16le(b, at)? as usize;
    let mut out = Vec::with_capacity(count);
    let mut pos = at + 2;
    for _ in 0..count {
        let sel = b.get(pos..pos + 4)?;
        if sel[..3] == oui {
            out.push(sel[3]);
        }
        pos += 4;
    }
    Some((out, pos))
}

fn parse_rsn(body: &[u8], info: &mut IeInfo) {
    // version(2) group(4) pairwise-list akm-list caps(2)
    if body.len() < 6 {
        return;
    }
    info.rsn_present = true;
    let mut pos = 2;
    if let Some(g) = body.get(pos..pos + 4) {
        if g[..3] == OUI_IEEE {
            info.rsn_ciphers.push(g[3]);
        }
    }
    pos += 4;
    // Lists may be omitted from the end of the element; 802.11 then defines
    // CCMP as the pairwise cipher and 802.1X as the AKM. A truncated RSN
    // element still means WPA2 or later, never WEP.
    let Some((pairwise, next)) = suite_list(body, pos, OUI_IEEE) else {
        info.rsn_ciphers.push(4);
        info.rsn_akms = vec![1];
        return;
    };
    info.rsn_ciphers.extend(pairwise);
    let Some((akms, next)) = suite_list(body, next, OUI_IEEE) else {
        info.rsn_akms = vec![1];
        return;
    };
    info.rsn_akms = akms;
    info.rsn_caps = u16le(body, next);
}

fn parse_vendor(body: &[u8], info: &mut IeInfo) {
    if body.len() < 4 || body[..3] != OUI_MS {
        return;
    }
    match body[3] {
        1 => {
            info.wpa1 = true;
            // type(1) version(2) group(4) pairwise-list akm-list
            if let Some(g) = body.get(6..10) {
                if g[..3] == OUI_MS {
                    info.wpa1_ciphers.push(g[3]);
                }
            }
            if let Some((pw, _)) = suite_list(body, 10, OUI_MS) {
                info.wpa1_ciphers.extend(pw);
            }
        }
        4 => info.wps = true,
        _ => {}
    }
}

fn width_from_vht(body: &[u8]) -> Option<u32> {
    let width = *body.first()?;
    let seg0 = *body.get(1)? as i32;
    let seg1 = *body.get(2)? as i32;
    Some(match width {
        0 => 0, // defer to HT operation (20/40)
        1 if seg1 != 0 && (seg1 - seg0).abs() == 8 => 160,
        1 if seg1 != 0 && (seg1 - seg0).abs() > 16 => 160, // 80+80
        1 => 80,
        2 | 3 => 160,
        _ => 0,
    })
}

fn parse_he_operation(body: &[u8], info: &mut IeInfo) {
    // params(3) bss-color(1) basic-mcs(2) [vht-op(3)] [cohosted(1)] [6ghz-op(5)]
    if body.len() < 6 {
        return;
    }
    let params = u32::from_le_bytes([body[0], body[1], body[2], 0]);
    let mut pos = 6;
    if params & (1 << 14) != 0 {
        if let Some(w) = body.get(pos..pos + 3).and_then(width_from_vht) {
            info.width_mhz = info.width_mhz.max(w);
        }
        pos += 3;
    }
    if params & (1 << 15) != 0 {
        pos += 1;
    }
    if params & (1 << 17) != 0 {
        if let Some(op) = body.get(pos..pos + 5) {
            info.ds_channel.get_or_insert(op[0] as u32);
            let w = match op[1] & 0x03 {
                0 => 20,
                1 => 40,
                2 => 80,
                _ => 160,
            };
            info.width_mhz = info.width_mhz.max(w);
        }
    }
}

fn parse_eht_operation(body: &[u8], info: &mut IeInfo) {
    // params(1) basic-mcs(4) [control(1) ccfs0(1) ccfs1(1)]
    let Some(params) = body.first() else { return };
    if params & 0x01 == 0 {
        return;
    }
    if let Some(control) = body.get(5) {
        let w = match control & 0x07 {
            0 => 20,
            1 => 40,
            2 => 80,
            3 => 160,
            4 => 320,
            _ => 0,
        };
        info.width_mhz = info.width_mhz.max(w);
    }
}

/// Parses a raw IE blob. Malformed trailing data is ignored.
pub fn parse(blob: &[u8]) -> IeInfo {
    let mut info = IeInfo::default();
    let mut ht_width = 20;
    let mut pos = 0;
    while pos + 2 <= blob.len() {
        let id = blob[pos];
        let len = blob[pos + 1] as usize;
        let Some(body) = blob.get(pos + 2..pos + 2 + len) else {
            break;
        };
        match id {
            0 => info.ssid = Some(body.to_vec()),
            3 => info.ds_channel = body.first().map(|c| *c as u32),
            7 if body.len() >= 2 => {
                info.country = Some(
                    body[..2]
                        .iter()
                        .map(|c| *c as char)
                        .collect::<String>()
                        .trim()
                        .to_string(),
                )
            }
            11 if body.len() >= 3 => {
                info.station_count = u16le(body, 0);
                info.utilization_pct = Some(((body[2] as u32 * 100 + 127) / 255) as u8);
            }
            45 => info.ht = true,
            48 => parse_rsn(body, &mut info),
            54 => info.mobility_domain = true,
            61 if body.len() >= 2 => {
                info.ds_channel.get_or_insert(body[0] as u32);
                // Secondary channel offset 1 (above) or 3 (below) and STA width bit.
                if body[1] & 0x03 != 0 && body[1] & 0x04 != 0 {
                    ht_width = 40;
                }
            }
            70 => info.rrm = true,
            127 if body.len() >= 3 => info.bss_transition = body[2] & 0x08 != 0,
            191 => info.vht = true,
            192 => {
                if let Some(w) = width_from_vht(body) {
                    info.width_mhz = info.width_mhz.max(w);
                }
            }
            221 => parse_vendor(body, &mut info),
            255 if !body.is_empty() => match body[0] {
                35 => info.he = true,
                36 => parse_he_operation(&body[1..], &mut info),
                106 => parse_eht_operation(&body[1..], &mut info),
                108 => info.eht = true,
                _ => {}
            },
            _ => {}
        }
        pos += 2 + len;
    }
    info.width_mhz = info.width_mhz.max(ht_width);
    info
}

impl IeInfo {
    /// Derives the security mode. `privacy` is bit 4 of the capability field.
    pub fn security(&self, privacy: bool) -> Security {
        let akm = |set: &[u8]| self.rsn_akms.iter().any(|a| set.contains(a));
        if !self.rsn_akms.is_empty() {
            let enterprise = akm(&[1, 3, 5, 11, 12, 13]);
            let suite_b = akm(&[12, 13]);
            let sae = akm(&[8, 9, 24, 25]);
            let psk = akm(&[2, 4, 6]);
            let owe = akm(&[18]);
            let mfp_required = self.rsn_caps.map(|c| c & 0x0040 != 0).unwrap_or(false);
            return if suite_b || (enterprise && mfp_required && !akm(&[1, 3])) {
                Security::Wpa3Enterprise
            } else if enterprise {
                Security::Wpa2Enterprise
            } else if sae && psk {
                Security::Wpa2Wpa3
            } else if sae {
                Security::Wpa3
            } else if psk {
                Security::Wpa2
            } else if owe {
                Security::Owe
            } else {
                Security::Unknown
            };
        }
        if self.rsn_present {
            return Security::Unknown;
        }
        if self.wpa1 {
            return Security::Wpa;
        }
        if privacy {
            Security::Wep
        } else {
            Security::Open
        }
    }

    pub fn weak_cipher(&self, security: Security) -> bool {
        // Cipher suite 2 = TKIP (both IEEE and Microsoft OUIs); WEP is weak by definition.
        security == Security::Wep
            || security == Security::Wpa
            || self.rsn_ciphers.contains(&2)
            || self.wpa1_ciphers.contains(&2)
    }

    pub fn pmf(&self) -> Pmf {
        match self.rsn_caps {
            None => Pmf::Unknown,
            Some(c) if c & 0x0040 != 0 => Pmf::Required,
            Some(c) if c & 0x0080 != 0 => Pmf::Capable,
            Some(_) => Pmf::Disabled,
        }
    }

    pub fn roaming(&self) -> Roaming {
        Roaming {
            k: self.rrm,
            v: self.bss_transition,
            r: self.mobility_domain || self.rsn_akms.iter().any(|a| [3, 4, 9, 25].contains(a)),
        }
    }

    pub fn wifi_gen(&self) -> Option<u8> {
        if self.eht {
            Some(7)
        } else if self.he {
            Some(6)
        } else if self.vht {
            Some(5)
        } else if self.ht {
            Some(4)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ie(id: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![id, body.len() as u8];
        v.extend_from_slice(body);
        v
    }

    fn rsn(akms: &[u8], ciphers: &[u8], caps: u16) -> Vec<u8> {
        let mut b = vec![1, 0, 0x00, 0x0f, 0xac, 4];
        b.extend_from_slice(&(ciphers.len() as u16).to_le_bytes());
        for c in ciphers {
            b.extend_from_slice(&[0x00, 0x0f, 0xac, *c]);
        }
        b.extend_from_slice(&(akms.len() as u16).to_le_bytes());
        for a in akms {
            b.extend_from_slice(&[0x00, 0x0f, 0xac, *a]);
        }
        b.extend_from_slice(&caps.to_le_bytes());
        ie(48, &b)
    }

    #[test]
    fn truncated_rsn_is_not_wep() {
        // Version and group cipher only: defaults apply (CCMP, 802.1X).
        let info = parse(&ie(48, &[1, 0, 0x00, 0x0f, 0xac, 4]));
        assert_eq!(info.security(true), Security::Wpa2Enterprise);
        // Only vendor AKMs we do not know.
        let mut b = vec![1, 0, 0x00, 0x0f, 0xac, 4, 1, 0, 0x00, 0x0f, 0xac, 4, 1, 0];
        b.extend_from_slice(&[0x00, 0x40, 0x96, 0x01, 0, 0]);
        assert_eq!(parse(&ie(48, &b)).security(true), Security::Unknown);
    }

    #[test]
    fn wpa2_psk_ht40() {
        let mut blob = ie(0, b"Office");
        blob.extend(ie(3, &[6]));
        blob.extend(ie(45, &[0; 26]));
        blob.extend(ie(61, &[6, 0x05, 0, 0, 0, 0]));
        blob.extend(rsn(&[2], &[4], 0x0080));
        let info = parse(&blob);
        assert_eq!(info.ssid.as_deref(), Some(&b"Office"[..]));
        assert_eq!(info.ds_channel, Some(6));
        assert_eq!(info.width_mhz, 40);
        assert_eq!(info.security(true), Security::Wpa2);
        assert_eq!(info.pmf(), Pmf::Capable);
        assert_eq!(info.wifi_gen(), Some(4));
        assert!(!info.weak_cipher(Security::Wpa2));
    }

    #[test]
    fn wpa3_transition_and_vht80() {
        let mut blob = rsn(&[2, 8], &[4], 0x0080);
        blob.extend(ie(191, &[0; 12]));
        blob.extend(ie(192, &[1, 42, 0, 0, 0]));
        blob.extend(ie(255, &[35, 0, 0]));
        let info = parse(&blob);
        assert_eq!(info.security(true), Security::Wpa2Wpa3);
        assert_eq!(info.width_mhz, 80);
        assert_eq!(info.wifi_gen(), Some(6));
    }

    #[test]
    fn enterprise_ft_and_roaming() {
        let mut blob = rsn(&[1, 3], &[4], 0);
        blob.extend(ie(54, &[0x12, 0x34, 0x01]));
        blob.extend(ie(70, &[0x73, 0, 0, 0, 0]));
        blob.extend(ie(127, &[0, 0, 0x08]));
        let info = parse(&blob);
        assert_eq!(info.security(true), Security::Wpa2Enterprise);
        assert_eq!(
            info.roaming(),
            Roaming {
                k: true,
                v: true,
                r: true
            }
        );
        assert_eq!(info.pmf(), Pmf::Disabled);
    }

    #[test]
    fn legacy_wpa_tkip_wps_and_wep() {
        let mut wpa = vec![
            0x00, 0x50, 0xf2, 1, 1, 0, 0x00, 0x50, 0xf2, 2, 1, 0, 0x00, 0x50, 0xf2, 2,
        ];
        wpa.extend_from_slice(&[1, 0, 0x00, 0x50, 0xf2, 2]);
        let mut blob = ie(221, &wpa);
        blob.extend(ie(221, &[0x00, 0x50, 0xf2, 4, 0x10, 0x4a]));
        let info = parse(&blob);
        assert_eq!(info.security(true), Security::Wpa);
        assert!(info.weak_cipher(Security::Wpa));
        assert!(info.wps);
        assert_eq!(parse(&[]).security(true), Security::Wep);
        assert_eq!(parse(&[]).security(false), Security::Open);
    }

    #[test]
    fn bss_load_and_country() {
        let mut blob = ie(11, &[12, 0, 191, 0, 0]);
        blob.extend(ie(7, b"PL \x01\x0d\x14"));
        let info = parse(&blob);
        assert_eq!(info.station_count, Some(12));
        assert_eq!(info.utilization_pct, Some(75));
        assert_eq!(info.country.as_deref(), Some("PL"));
    }

    #[test]
    fn he_6ghz_operation_and_eht_320() {
        // HE operation with the 6 GHz info present (bit 17) -> 160 MHz.
        let he_op = [36, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 37, 0x03, 47, 39, 0];
        let mut blob = ie(255, &he_op);
        blob.extend(ie(255, &[35, 0]));
        let info = parse(&blob);
        assert_eq!(info.width_mhz, 160);
        assert_eq!(info.ds_channel, Some(37));
        let eht_op = [106, 0x01, 0, 0, 0, 0, 0x04, 31, 63];
        let mut blob = ie(255, &eht_op);
        blob.extend(ie(255, &[108, 0]));
        let info = parse(&blob);
        assert_eq!(info.width_mhz, 320);
        assert_eq!(info.wifi_gen(), Some(7));
    }

    #[test]
    fn truncated_blob_does_not_panic() {
        let info = parse(&[48, 20, 1, 0]);
        assert!(info.rsn_akms.is_empty());
        let _ = parse(&[255]);
        let _ = parse(&[192, 1, 1]);
    }
}
