//! WLAN profile XML for `WlanSetProfile`, following Microsoft's documented `WLANProfile`
//! schema (namespace `http://www.microsoft.com/networking/WLAN/profile/v1`; see "WLAN_profile
//! Schema" and "WLAN Profile Samples" in the Native Wifi documentation). Pure string
//! building, so it is testable on any OS; actually applying a profile is Windows-only.

use crate::error::WlanError;

const NAMESPACE: &str = "http://www.microsoft.com/networking/WLAN/profile/v1";
/// 802.11 SSIDs are at most 32 octets (IEEE 802.11-2020 9.4.2.2).
const MAX_SSID_BYTES: usize = 32;
/// WPA/WPA2/WPA3 passphrases are 8..=63 printable ASCII characters (IEEE 802.11i H.4.1); a
/// 64-digit hex string is a raw 256-bit PSK instead.
const MIN_PASSPHRASE: usize = 8;
const MAX_PASSPHRASE: usize = 63;
const RAW_PSK_HEX_DIGITS: usize = 64;
/// Prefix of profiles OpenSpot creates, so they never collide with (or overwrite) a profile
/// the user saved for the same SSID.
pub const PROFILE_PREFIX: &str = "OpenSpot-";

/// Credentials for a network OpenSpot connects to with a temporary profile.
#[derive(Clone, PartialEq, Eq)]
pub enum ProfileSecurity {
    Open,
    Wpa2Personal { passphrase: String },
    Wpa3Personal { passphrase: String },
}

// Hand-written so a passphrase can never leak into logs through `{:?}`.
impl std::fmt::Debug for ProfileSecurity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => f.write_str("Open"),
            Self::Wpa2Personal { .. } => f.write_str("Wpa2Personal { .. }"),
            Self::Wpa3Personal { .. } => f.write_str("Wpa3Personal { .. }"),
        }
    }
}

/// Name of the temporary profile for `ssid`.
pub fn profile_name(ssid: &str) -> String {
    format!("{PROFILE_PREFIX}{ssid}")
}

fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
}

fn element(out: &mut String, tag: &str, text: &str) {
    out.push('<');
    out.push_str(tag);
    out.push('>');
    escape(text, out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

/// XML 1.0 forbids most control characters even when escaped, so refuse them up front.
fn reject_control(what: &str, s: &str) -> Result<(), WlanError> {
    if s.chars().any(|c| c.is_control()) {
        return Err(WlanError::InvalidProfile(format!("{what} contains a control character")));
    }
    Ok(())
}

/// Returns the `keyType` and validated key text for a personal-security passphrase.
fn key_material(passphrase: &str) -> Result<(&'static str, &str), WlanError> {
    let n = passphrase.chars().count();
    if n == RAW_PSK_HEX_DIGITS && passphrase.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(("networkKey", passphrase));
    }
    if !(MIN_PASSPHRASE..=MAX_PASSPHRASE).contains(&n) || !passphrase.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
        return Err(WlanError::InvalidProfile(format!(
            "passphrase must be {MIN_PASSPHRASE}-{MAX_PASSPHRASE} printable ASCII characters or {RAW_PSK_HEX_DIGITS} hex digits"
        )));
    }
    Ok(("passPhrase", passphrase))
}

/// Builds the profile for `ssid` named [`profile_name`]. `connectionMode` is `manual` so
/// Windows never auto-connects to it later.
pub fn build_profile(ssid: &str, security: &ProfileSecurity) -> Result<String, WlanError> {
    if ssid.is_empty() || ssid.len() > MAX_SSID_BYTES {
        return Err(WlanError::InvalidProfile(format!("SSID must be 1-{MAX_SSID_BYTES} bytes")));
    }
    reject_control("SSID", ssid)?;
    let (auth, encryption, key) = match security {
        ProfileSecurity::Open => ("open", "none", None),
        ProfileSecurity::Wpa2Personal { passphrase } => ("WPA2PSK", "AES", Some(key_material(passphrase)?)),
        ProfileSecurity::Wpa3Personal { passphrase } => ("WPA3SAE", "AES", Some(key_material(passphrase)?)),
    };

    let mut x = String::new();
    x.push_str("<?xml version=\"1.0\"?>\n<WLANProfile xmlns=\"");
    x.push_str(NAMESPACE);
    x.push_str("\">");
    element(&mut x, "name", &profile_name(ssid));
    x.push_str("<SSIDConfig><SSID>");
    element(&mut x, "name", ssid);
    x.push_str("</SSID></SSIDConfig>");
    x.push_str("<connectionType>ESS</connectionType><connectionMode>manual</connectionMode>");
    x.push_str("<MSM><security><authEncryption>");
    element(&mut x, "authentication", auth);
    element(&mut x, "encryption", encryption);
    x.push_str("<useOneX>false</useOneX></authEncryption>");
    if let Some((key_type, material)) = key {
        x.push_str("<sharedKey>");
        element(&mut x, "keyType", key_type);
        x.push_str("<protected>false</protected>");
        element(&mut x, "keyMaterial", material);
        x.push_str("</sharedKey>");
    }
    x.push_str("</security></MSM></WLANProfile>");
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wpa2(p: &str) -> ProfileSecurity {
        ProfileSecurity::Wpa2Personal { passphrase: p.into() }
    }

    /// Minimal well-formedness check: tags nest and close properly, no stray `&` or `<` in text.
    fn assert_well_formed(xml: &str) {
        let body = xml.strip_prefix("<?xml version=\"1.0\"?>\n").expect("xml declaration");
        let mut stack: Vec<&str> = Vec::new();
        let mut rest = body;
        while let Some(lt) = rest.find('<') {
            let text = &rest[..lt];
            assert!(!text.contains('>'), "bad text {text:?}");
            // Every '&' in text must start one of the five predefined entities.
            for (i, _) in text.match_indices('&') {
                assert!(
                    ["&amp;", "&lt;", "&gt;", "&quot;", "&apos;"].iter().any(|e| text[i..].starts_with(e)),
                    "bare ampersand in {text:?}"
                );
            }
            let gt = rest[lt..].find('>').expect("unterminated tag") + lt;
            let tag = &rest[lt + 1..gt];
            if let Some(name) = tag.strip_prefix('/') {
                assert_eq!(stack.pop(), Some(name), "mismatched close {name}");
            } else {
                stack.push(tag.split_whitespace().next().unwrap());
            }
            rest = &rest[gt + 1..];
        }
        assert!(stack.is_empty(), "unclosed {stack:?}");
    }

    #[test]
    fn wpa2_profile_has_psk_aes_and_key() {
        let x = build_profile("Home", &wpa2("correct horse")).unwrap();
        assert_well_formed(&x);
        assert!(x.contains(&format!("xmlns=\"{NAMESPACE}\"")));
        assert!(x.contains("<name>OpenSpot-Home</name>"));
        assert!(x.contains("<SSID><name>Home</name></SSID>"));
        assert!(x.contains("<authentication>WPA2PSK</authentication><encryption>AES</encryption>"));
        assert!(x.contains("<keyType>passPhrase</keyType><protected>false</protected><keyMaterial>correct horse</keyMaterial>"));
        assert!(x.contains("<connectionMode>manual</connectionMode>"));
    }

    #[test]
    fn wpa3_profile_uses_sae() {
        let x = build_profile("Home", &ProfileSecurity::Wpa3Personal { passphrase: "12345678".into() }).unwrap();
        assert_well_formed(&x);
        assert!(x.contains("<authentication>WPA3SAE</authentication><encryption>AES</encryption>"));
        assert!(!x.contains("WPA2PSK"));
    }

    #[test]
    fn open_profile_has_no_key_and_no_encryption() {
        let x = build_profile("Cafe", &ProfileSecurity::Open).unwrap();
        assert_well_formed(&x);
        assert!(x.contains("<authentication>open</authentication><encryption>none</encryption>"));
        assert!(!x.contains("sharedKey") && !x.contains("keyMaterial"));
    }

    #[test]
    fn special_characters_are_escaped_in_ssid_name_and_key() {
        let x = build_profile("A&B <\"C'>", &wpa2("p&ss<w\"rd'>1")).unwrap();
        assert_well_formed(&x);
        assert!(x.contains("<name>OpenSpot-A&amp;B &lt;&quot;C&apos;&gt;</name>"));
        assert!(x.contains("<SSID><name>A&amp;B &lt;&quot;C&apos;&gt;</name></SSID>"));
        assert!(x.contains("<keyMaterial>p&amp;ss&lt;w&quot;rd&apos;&gt;1</keyMaterial>"));
        assert!(!x.contains("A&B"), "raw ampersand leaked");
    }

    #[test]
    fn non_ascii_ssid_is_kept_as_utf8_text() {
        let x = build_profile("Café 无线", &ProfileSecurity::Open).unwrap();
        assert!(x.contains("<SSID><name>Café 无线</name></SSID>"));
    }

    #[test]
    fn hex_psk_uses_network_key_type() {
        let hex = "0123456789abcdef".repeat(4);
        let x = build_profile("n", &wpa2(&hex)).unwrap();
        assert!(x.contains("<keyType>networkKey</keyType>"));
        assert!(build_profile("n", &wpa2(&"g".repeat(64))).is_err(), "64 non-hex chars is not a valid key");
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        let e = |r: Result<String, WlanError>| matches!(r, Err(WlanError::InvalidProfile(_)));
        assert!(e(build_profile("", &ProfileSecurity::Open)));
        assert!(e(build_profile(&"x".repeat(33), &ProfileSecurity::Open)));
        assert!(build_profile(&"x".repeat(32), &ProfileSecurity::Open).is_ok());
        assert!(e(build_profile("a\u{1}b", &ProfileSecurity::Open)));
        assert!(e(build_profile("n", &wpa2("short"))));
        assert!(e(build_profile("n", &wpa2(&"x".repeat(64)))));
        assert!(e(build_profile("n", &wpa2("pässwörd1"))));
        assert!(e(build_profile("n", &wpa2("pass\nword1"))));
        assert!(build_profile("n", &wpa2(&"x".repeat(63))).is_ok());
    }

    #[test]
    fn debug_never_prints_the_passphrase() {
        assert!(!format!("{:?}", wpa2("supersecret1")).contains("supersecret1"));
    }
}
