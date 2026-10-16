//! File-name sanitising for exports (Windows rules are the strictest, so they apply everywhere).

/// Characters Windows forbids in file names (plus the path separators of every OS).
const FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Device names Windows reserves, with or without an extension (`CON.txt` is also reserved).
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4",
    "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Longest sanitised component in bytes. File systems cap a whole name at 255 bytes
/// (ext4) or UTF-16 units (NTFS); exports join three such components with separators,
/// the BSSID and an extension, which stays at about 215 bytes.
pub const MAX_COMPONENT_BYTES: usize = 60;

const FALLBACK: &str = "unnamed";

/// Turns arbitrary text (an SSID, an alias, a zone name) into one safe file-name component.
///
/// Forbidden characters and control characters become `_`, whitespace runs collapse to one
/// space, leading/trailing spaces and trailing dots (which Windows strips silently) are
/// removed, reserved device names get a `_` prefix, and the result is cut to
/// [`MAX_COMPONENT_BYTES`] on a character boundary. Never returns an empty string.
pub fn sanitize_file_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_space = false;
    for c in name.chars() {
        // Whitespace is checked first: tab and newline are also control characters.
        let c = if c.is_whitespace() || !(FORBIDDEN.contains(&c) || c.is_control()) { c } else { '_' };
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    let mut out = trim_name(&out).to_owned();
    if out.is_empty() {
        return FALLBACK.to_owned();
    }
    let stem = out.split('.').next().unwrap_or("").trim_end().to_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    if out.len() > MAX_COMPONENT_BYTES {
        let mut cut = MAX_COMPONENT_BYTES;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out = trim_name(&out).to_owned();
    }
    if out.is_empty() { FALLBACK.to_owned() } else { out }
}

fn trim_name(s: &str) -> &str {
    s.trim_matches(' ').trim_end_matches(['.', ' '])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_and_control_characters_are_replaced() {
        assert_eq!(sanitize_file_name(r#"a<b>c:d"e/f\g|h?i*j"#), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize_file_name("tab\there\nnew\u{0}nul"), "tab here new_nul");
    }

    #[test]
    fn whitespace_collapses_and_edges_are_trimmed() {
        assert_eq!(sanitize_file_name("  my   network  "), "my network");
        assert_eq!(sanitize_file_name("name..."), "name");
        assert_eq!(sanitize_file_name("name. . "), "name");
        assert_eq!(sanitize_file_name(".hidden"), ".hidden");
    }

    #[test]
    fn reserved_device_names_are_defused_case_insensitively() {
        for n in ["CON", "con", "Nul", "aux.txt", "COM1", "lpt9", "PRN.tar.gz", "con "] {
            let s = sanitize_file_name(n);
            assert!(s.starts_with('_'), "{n:?} -> {s:?}");
        }
        // Not reserved: longer names and COM10/LPT0.
        for n in ["console", "COM10", "LPT0", "communal"] {
            assert_eq!(sanitize_file_name(n), n);
        }
    }

    #[test]
    fn empty_and_all_invalid_input_gets_a_placeholder() {
        assert_eq!(sanitize_file_name(""), "unnamed");
        assert_eq!(sanitize_file_name("   "), "unnamed");
        assert_eq!(sanitize_file_name("..."), "unnamed");
        assert_eq!(sanitize_file_name("???"), "___");
    }

    #[test]
    fn unicode_is_kept_and_truncation_respects_char_boundaries() {
        assert_eq!(sanitize_file_name("Caf\u{e9} Z\u{fc}rich \u{65e5}\u{672c}"), "Caf\u{e9} Z\u{fc}rich \u{65e5}\u{672c}");
        let long = "\u{1f4f6}".repeat(40); // 4 bytes each
        let s = sanitize_file_name(&long);
        assert!(s.len() <= MAX_COMPONENT_BYTES && !s.is_empty());
        assert!(s.chars().all(|c| c == '\u{1f4f6}'), "no split code point");
        assert_eq!(sanitize_file_name(&"a".repeat(200)).len(), MAX_COMPONENT_BYTES);
        // Cutting must not leave a trailing dot or space either.
        let edge = format!("{}. tail", "a".repeat(MAX_COMPONENT_BYTES - 1));
        assert!(!sanitize_file_name(&edge).ends_with(['.', ' ']));
    }
}
