//! Parser for `netsh wlan show networks mode=bssid`.
//!
//! netsh prints a localized, human-oriented report: field names are translated and the text
//! has no stable machine format. The parser therefore only trusts what it can positively
//! recognise (English field names) and says so when it cannot, instead of reading a French
//! "Signal" line as something else or silently dropping data.

use std::collections::BTreeSet;

use openspot_wlan::MacAddress;
use thiserror::Error;

/// Field names as netsh prints them in English, compared case-insensitively.
const KEY_SIGNAL: &str = "signal";
const KEY_RADIO_TYPE: &str = "radio type";
const KEY_BAND: &str = "band";
const KEY_CHANNEL: &str = "channel";

/// How many unrecognised field names an error lists: enough to identify the language, few
/// enough to stay readable.
const MAX_KEYS_REPORTED: usize = 8;

/// One BSSID entry. Fields the output did not contain (older Windows builds print no band)
/// are `None`; they are never guessed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetshBss {
    pub ssid: String,
    pub bssid: MacAddress,
    /// Signal quality in percent, 0..=100.
    pub signal_percent: Option<u8>,
    /// As printed, e.g. `802.11ax`.
    pub radio_type: Option<String>,
    /// As printed, e.g. `5 GHz`.
    pub band: Option<String>,
    pub channel: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NetshError {
    #[error("netsh printed nothing")]
    Empty,
    #[error("no network entries could be read from the netsh output; either no network is visible or netsh is not printing English field names")]
    NothingRecognised,
    #[error(
        "netsh listed {entries} BSSID entries but none had the English fields \"Signal\" and \"Channel\", so it is probably not running in English (field names seen: {keys}); \
         nothing was compared. Run it with an English display language, or compare by hand"
    )]
    UnrecognisedLanguage { entries: usize, keys: String },
}

/// Result of a successful parse.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NetshReport {
    pub entries: Vec<NetshBss>,
    /// Entries missing a field the comparison uses; they are compared on what they have.
    pub incomplete: usize,
}

/// Parses `text` (CRLF or LF). `Ok` with no entries means netsh said zero networks are visible.
pub fn parse(text: &str) -> Result<NetshReport, NetshError> {
    if text.trim().is_empty() {
        return Err(NetshError::Empty);
    }
    let mut entries: Vec<NetshBss> = Vec::new();
    let mut ssid = String::new();
    let mut other_keys: BTreeSet<String> = BTreeSet::new();

    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else { continue };
        let (key, value) = (key.trim(), value.trim());
        let lower = key.to_ascii_lowercase();
        if is_numbered(&lower, "ssid") {
            value.clone_into(&mut ssid);
        } else if is_numbered(&lower, "bssid") {
            if let Some(bssid) = parse_mac(value) {
                entries.push(NetshBss { ssid: ssid.clone(), bssid, signal_percent: None, radio_type: None, band: None, channel: None });
            }
        } else if let Some(entry) = entries.last_mut() {
            match lower.as_str() {
                KEY_SIGNAL => entry.signal_percent = parse_percent(value),
                KEY_RADIO_TYPE => entry.radio_type = Some(value.to_owned()),
                KEY_BAND => entry.band = Some(value.to_owned()),
                KEY_CHANNEL => entry.channel = value.split_whitespace().next().and_then(|c| c.parse().ok()),
                _ => {
                    other_keys.insert(key.to_owned());
                }
            }
        }
    }

    if entries.is_empty() {
        // "There are 0 networks currently visible." is a valid, empty answer.
        let zero = text.lines().any(|l| l.contains("0 networks") || l.contains("0 network "));
        return if zero { Ok(NetshReport::default()) } else { Err(NetshError::NothingRecognised) };
    }
    if entries.iter().all(|e| e.signal_percent.is_none() && e.channel.is_none()) {
        let keys = other_keys.into_iter().take(MAX_KEYS_REPORTED).collect::<Vec<_>>().join(", ");
        return Err(NetshError::UnrecognisedLanguage { entries: entries.len(), keys });
    }
    let incomplete = entries.iter().filter(|e| e.signal_percent.is_none() || e.channel.is_none() || e.radio_type.is_none() || e.band.is_none()).count();
    Ok(NetshReport { entries, incomplete })
}

/// Whether `key` is `<word> <number>` such as `SSID 1` or `BSSID 12`.
fn is_numbered(key: &str, word: &str) -> bool {
    key.strip_prefix(word).and_then(|rest| rest.trim().parse::<u32>().ok()).is_some()
}

fn parse_percent(value: &str) -> Option<u8> {
    let n: u8 = value.trim().strip_suffix('%')?.trim().parse().ok()?;
    (n <= 100).then_some(n)
}

/// `aa:bb:cc:dd:ee:ff` as netsh prints it (also accepts `-` separators and upper case).
fn parse_mac(text: &str) -> Option<MacAddress> {
    let mut bytes = [0u8; 6];
    let mut parts = text.trim().split([':', '-']);
    for byte in &mut bytes {
        let part = parts.next()?;
        if part.len() != 2 {
            return None;
        }
        *byte = u8::from_str_radix(part, 16).ok()?;
    }
    parts.next().is_none().then_some(MacAddress(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic output in the shape Windows 11 prints, indentation included (addresses and
    /// names are made up).
    const ENGLISH_LINES: [&str; 29] = [
        "",
        "Interface name : Wi-Fi ",
        "There are 2 networks currently visible. ",
        "",
        "SSID 1 : Example-Net",
        "    Network type            : Infrastructure",
        "    Authentication          : WPA2-Personal",
        "    Encryption              : CCMP ",
        "    BSSID 1                 : 02:00:00:00:00:01",
        "         Signal             : 95%  ",
        "         Radio type         : 802.11ax",
        "         Band               : 5 GHz",
        "         Channel            : 36 ",
        "         Basic rates (Mbps) : 6 12 24",
        "    BSSID 2                 : 02:00:00:00:00:02",
        "         Signal             : 40%  ",
        "         Radio type         : 802.11n",
        "         Band               : 2.4 GHz",
        "         Channel            : 6 ",
        "",
        "SSID 2 : ",
        "    Network type            : Infrastructure",
        "    BSSID 1                 : 02:00:00:00:00:03",
        "         Signal             : 10%  ",
        "         Radio type         : 802.11ac",
        "         Band               : 5 GHz",
        "         Channel            : 149 ",
        "",
        "",
    ];

    fn english() -> String {
        ENGLISH_LINES.join("\r\n")
    }

    #[test]
    fn english_output_parses_every_field_across_ssids() {
        let r = parse(&english()).unwrap();
        assert_eq!(r.entries.len(), 3);
        assert_eq!(r.incomplete, 0);
        let a = &r.entries[0];
        assert_eq!(a.ssid, "Example-Net");
        assert_eq!(a.bssid, MacAddress([2, 0, 0, 0, 0, 1]));
        assert_eq!((a.signal_percent, a.channel), (Some(95), Some(36)));
        assert_eq!((a.radio_type.as_deref(), a.band.as_deref()), (Some("802.11ax"), Some("5 GHz")));
        assert_eq!(r.entries[1].channel, Some(6));
        assert_eq!(r.entries[2].ssid, "", "hidden networks keep an empty name");
        assert_eq!(r.entries[2].channel, Some(149), "second SSID block resets the name, not the fields");
    }

    #[test]
    fn lf_only_output_and_odd_spacing_parse_the_same() {
        let lf = english().replace("\r\n", "\n").replace("Signal             :", "SIGNAL:");
        assert_eq!(parse(&lf).unwrap().entries.len(), 3);
        assert_eq!(parse(&lf).unwrap().entries[0].signal_percent, Some(95));
    }

    #[test]
    fn missing_band_line_is_recorded_as_incomplete_not_guessed() {
        let old = english().replace("         Band               : 5 GHz\r\n", "").replace("         Band               : 2.4 GHz\r\n", "");
        let r = parse(&old).unwrap();
        assert_eq!(r.entries.len(), 3);
        assert!(r.entries.iter().all(|e| e.band.is_none()));
        assert_eq!(r.incomplete, 3);
    }

    #[test]
    fn french_output_is_reported_not_misread() {
        let fr = "Nom de l'interface : Wi-Fi\r\n\
SSID 1 : Exemple\r\n\
    Type de réseau          : Infrastructure\r\n\
    BSSID 1                 : 02:00:00:00:00:01\r\n\
         Signal             : 95%\r\n\
         Type de radio      : 802.11ax\r\n\
         Bande              : 5 GHz\r\n\
         Canal              : 36\r\n";
        // "Signal" happens to be spelled the same, so the entry is still usable...
        let r = parse(fr).unwrap();
        assert_eq!(r.entries[0].signal_percent, Some(95));
        assert_eq!(r.entries[0].channel, None, "Canal is not Channel: unknown, not guessed");
        assert_eq!(r.entries[0].radio_type, None);
        assert_eq!(r.incomplete, 1);
    }

    #[test]
    fn output_with_no_recognised_fields_is_a_language_error_naming_the_fields_seen() {
        let de = "SSID 1 : Beispiel\r\n\
    BSSID 1                 : 02:00:00:00:00:01\r\n\
         Funktyp            : 802.11ax\r\n\
         Kanal              : 36\r\n\
         Empfang            : 95%\r\n";
        let err = parse(de).unwrap_err();
        assert!(matches!(err, NetshError::UnrecognisedLanguage { entries: 1, .. }), "{err:?}");
        let message = err.to_string();
        assert!(message.contains("Funktyp") && message.contains("Kanal") && message.contains("not running in English"), "{message}");
    }

    #[test]
    fn full_width_colons_mean_nothing_is_recognised() {
        let ja = "SSID 1 ： 例\r\n    BSSID 1 ： 02:00:00:00:00:01\r\n         シグナル ： 95%\r\n";
        assert_eq!(parse(ja), Err(NetshError::NothingRecognised));
    }

    #[test]
    fn zero_networks_is_a_valid_empty_answer() {
        let none = "\r\nInterface name : Wi-Fi \r\nThere are 0 networks currently visible. \r\n";
        assert_eq!(parse(none), Ok(NetshReport::default()));
    }

    #[test]
    fn empty_and_garbage_input_are_errors() {
        assert_eq!(parse(""), Err(NetshError::Empty));
        assert_eq!(parse("  \r\n \r\n"), Err(NetshError::Empty));
        assert_eq!(parse("The Wireless AutoConfig Service (wlansvc) is not running."), Err(NetshError::NothingRecognised));
    }

    #[test]
    fn malformed_values_are_skipped_not_invented() {
        let text = "SSID 1 : x\r\n    BSSID 1 : not-a-mac\r\n    BSSID 2 : 02:00:00:00:00:09\r\n         Signal : 140%\r\n         Channel : abc\r\n";
        let err = parse(text).unwrap_err();
        assert!(matches!(err, NetshError::UnrecognisedLanguage { entries: 1, .. }), "{err:?}");
    }

    #[test]
    fn mac_parsing_is_strict() {
        assert_eq!(parse_mac("AA-bb-0c-00-00-01"), Some(MacAddress([0xAA, 0xBB, 0x0C, 0, 0, 1])));
        for bad in ["", "aa:bb:cc:dd:ee", "aa:bb:cc:dd:ee:ff:00", "aa:bb:cc:dd:ee:fg", "a:bb:cc:dd:ee:ff"] {
            assert_eq!(parse_mac(bad), None, "{bad}");
        }
    }
}
