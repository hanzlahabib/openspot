//! Alias import and export as CSV (`BSSID,alias`).
//!
//! Export keeps aliases byte-for-byte so a file round-trips exactly; it deliberately does not
//! apply the spreadsheet formula guard used for table exports, which would alter the names.

use std::collections::BTreeMap;

use openspot_wlan::MacAddress;
use thiserror::Error;

use crate::aliases::Aliases;
use crate::csvio::{self, CsvError, ParseMode};

pub const HEADER: [&str; 2] = ["BSSID", "alias"];

/// Strict accepts only a clean file and applies nothing otherwise; lenient applies every good
/// line and reports the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportMode {
    Strict,
    Lenient,
}

/// What to do when an imported BSSID already has an alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictPolicy {
    #[default]
    Overwrite,
    KeepExisting,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ImportError {
    #[error(transparent)]
    Csv(#[from] CsvError),
    #[error("line {line}: {reason}")]
    BadLine { line: usize, reason: SkipReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SkipReason {
    #[error("not a valid MAC address")]
    InvalidBssid,
    #[error("the alias is empty")]
    EmptyAlias,
    #[error("expected two columns (BSSID, alias)")]
    MissingColumn,
    #[error("more than two columns")]
    ExtraColumns,
    #[error("BSSID appears more than once")]
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedLine {
    pub line: usize,
    pub reason: SkipReason,
}

/// A file's valid lines plus what was left out. Pure data: nothing has been applied yet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedAliases {
    pub entries: Vec<(MacAddress, String)>,
    pub skipped: Vec<SkippedLine>,
}

/// Outcome of applying a [`ParsedAliases`] to the live aliases.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    pub imported: usize,
    /// Of `imported`, how many replaced a different existing alias.
    pub overwritten: usize,
    pub kept_existing: usize,
    pub skipped: Vec<SkippedLine>,
}

impl ImportReport {
    pub fn summary(&self) -> String {
        let mut text = format!("{} imported", self.imported);
        if self.overwritten > 0 {
            text.push_str(&format!(" ({} replaced existing)", self.overwritten));
        }
        if self.kept_existing > 0 {
            text.push_str(&format!(", {} kept existing", self.kept_existing));
        }
        if !self.skipped.is_empty() {
            text.push_str(&format!(", {} skipped", self.skipped.len()));
        }
        text
    }
}

/// Parses `AA:BB:CC:DD:EE:FF`, `aa-bb-cc-dd-ee-ff` or bare `aabbccddeeff` (any case).
pub fn parse_bssid(text: &str) -> Option<MacAddress> {
    let text = text.trim();
    let hex: String = if text.len() == 17 {
        let sep = text.chars().nth(2)?;
        if sep != ':' && sep != '-' {
            return None;
        }
        let mut digits = String::with_capacity(12);
        for (i, group) in text.split(sep).enumerate() {
            if group.len() != 2 || i > 5 {
                return None;
            }
            digits.push_str(group);
        }
        digits
    } else {
        text.to_owned()
    };
    if hex.len() != 12 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0u8; 6];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(MacAddress(bytes))
}

/// All aliases as CSV with a header, ordered by BSSID.
pub fn export(aliases: &Aliases) -> String {
    let mut out = csvio::write_record(&HEADER, ',');
    for (bssid, name) in aliases.iter() {
        out.push_str(&csvio::write_record(&[bssid, name], ','));
    }
    out
}

/// Reads aliases from CSV text. A first row whose first cell is `BSSID` is a header.
///
/// Strict mode fails on the first problem (nothing is returned); lenient mode detects the
/// delimiter, tolerates malformed quoting, and lists every skipped line instead. A BSSID that
/// appears twice keeps its last alias and the earlier line is reported as skipped.
pub fn parse(text: &str, mode: ImportMode) -> Result<ParsedAliases, ImportError> {
    let (delimiter, csv_mode) = match mode {
        ImportMode::Strict => (',', ParseMode::Strict),
        ImportMode::Lenient => (csvio::detect_delimiter(text), ParseMode::Lenient),
    };
    let mut records = csvio::parse(text, delimiter, csv_mode)?;
    if records.first().is_some_and(|r| r.fields.first().is_some_and(|f| f.trim().eq_ignore_ascii_case(HEADER[0]))) {
        records.remove(0);
    }

    let mut result = ParsedAliases::default();
    // Position in `entries` of each BSSID, so a later duplicate can replace the earlier one.
    let mut index_of: BTreeMap<MacAddress, usize> = BTreeMap::new();
    for rec in records {
        let line = rec.line;
        let reject = |reason: SkipReason, result: &mut ParsedAliases| match mode {
            ImportMode::Strict => Err(ImportError::BadLine { line, reason }),
            ImportMode::Lenient => {
                result.skipped.push(SkippedLine { line, reason });
                Ok(())
            }
        };
        if rec.fields.len() < 2 {
            reject(SkipReason::MissingColumn, &mut result)?;
            continue;
        }
        if rec.fields.len() > 2 && mode == ImportMode::Strict {
            reject(SkipReason::ExtraColumns, &mut result)?;
        }
        let Some(bssid) = parse_bssid(&rec.fields[0]) else {
            reject(SkipReason::InvalidBssid, &mut result)?;
            continue;
        };
        let alias = rec.fields[1].trim();
        if alias.is_empty() {
            reject(SkipReason::EmptyAlias, &mut result)?;
            continue;
        }
        if let Some(&i) = index_of.get(&bssid) {
            // Strict mode returns here; lenient mode keeps going with the later alias.
            reject(SkipReason::Duplicate, &mut result)?;
            result.entries[i].1 = alias.to_owned();
        } else {
            index_of.insert(bssid, result.entries.len());
            result.entries.push((bssid, alias.to_owned()));
        }
    }
    Ok(result)
}

/// Applies parsed entries to `aliases` under `policy`.
pub fn apply(parsed: ParsedAliases, aliases: &mut Aliases, policy: ConflictPolicy) -> ImportReport {
    let mut report = ImportReport { skipped: parsed.skipped, ..ImportReport::default() };
    for (bssid, name) in parsed.entries {
        match aliases.get(&bssid) {
            Some(existing) if existing == name => report.imported += 1,
            Some(_) if policy == ConflictPolicy::KeepExisting => report.kept_existing += 1,
            Some(_) => {
                aliases.set(&bssid, &name);
                report.imported += 1;
                report.overwritten += 1;
            }
            None => {
                aliases.set(&bssid, &name);
                report.imported += 1;
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: MacAddress = MacAddress([0xAA, 0xBB, 0xCC, 0, 0, 1]);
    const B: MacAddress = MacAddress([0xAA, 0xBB, 0xCC, 0, 0, 2]);

    #[test]
    fn export_and_import_act_on_the_active_scope_only() {
        use crate::aliases::{self, AliasScope};
        let mut app = Aliases::default();
        app.set(&A, "Global");
        let mut project = openspot_model::ProjectAliases::default();
        project.by_bssid.insert(B, "Local".into());

        let csv = export(&aliases::active(AliasScope::Project, &app, &project));
        assert!(csv.contains("Local") && !csv.contains("Global"), "{csv}");

        let mut working = aliases::active(AliasScope::Project, &app, &project);
        let parsed = parse("BSSID,alias\nAA:BB:CC:00:00:03,Imported\n", ImportMode::Strict).unwrap();
        apply(parsed, &mut working, ConflictPolicy::Overwrite);
        aliases::commit(AliasScope::Project, working, &mut app, &mut project);
        assert_eq!(project.len(), 2, "imported into the project");
        assert_eq!(app.len(), 1, "the app-wide list did not grow");
    }

    #[test]
    fn bssid_formats() {
        assert_eq!(parse_bssid("AA:BB:CC:00:00:01"), Some(A));
        assert_eq!(parse_bssid(" aa-bb-cc-00-00-01 "), Some(A));
        assert_eq!(parse_bssid("aabbcc000001"), Some(A));
        for bad in ["", "AA:BB:CC:00:00", "AA:BB:CC:00:00:01:02", "AA:BB:CC:00:00:0G", "AA:BB-CC:00:00:01", "AABBCC00000", "AA.BB.CC.00.00.01"] {
            assert_eq!(parse_bssid(bad), None, "{bad}");
        }
        // Non-ASCII must not panic on byte slicing.
        assert_eq!(parse_bssid("ÄÄ:BB:CC:00:00:01"), None);
    }

    #[test]
    fn export_has_header_sorted_rows_and_escapes() {
        let mut a = Aliases::default();
        a.set(&B, "Lobby, \"east\"");
        a.set(&A, "Office");
        assert_eq!(export(&a), "BSSID,alias\r\nAA:BB:CC:00:00:01,Office\r\nAA:BB:CC:00:00:02,\"Lobby, \"\"east\"\"\"\r\n");
    }

    #[test]
    fn export_import_round_trip_is_exact() {
        let mut a = Aliases::default();
        a.set(&A, "=Office");
        a.set(&B, "multi\nline, \"quoted\"");
        let parsed = parse(&export(&a), ImportMode::Strict).unwrap();
        let mut b = Aliases::default();
        let report = apply(parsed, &mut b, ConflictPolicy::Overwrite);
        assert_eq!(b, a);
        assert_eq!((report.imported, report.overwritten, report.skipped.len()), (2, 0, 0));
    }

    #[test]
    fn header_is_optional() {
        let with = parse("BSSID,alias\nAA:BB:CC:00:00:01,One", ImportMode::Strict).unwrap();
        let without = parse("AA:BB:CC:00:00:01,One", ImportMode::Strict).unwrap();
        assert_eq!(with, without);
        assert_eq!(with.entries, vec![(A, "One".to_owned())]);
    }

    #[test]
    fn strict_fails_on_the_first_bad_line_and_names_it() {
        let text = "BSSID,alias\nAA:BB:CC:00:00:01,Good\nnot-a-mac,Bad\n";
        assert_eq!(parse(text, ImportMode::Strict), Err(ImportError::BadLine { line: 3, reason: SkipReason::InvalidBssid }));
        assert_eq!(
            parse("AA:BB:CC:00:00:01,", ImportMode::Strict),
            Err(ImportError::BadLine { line: 1, reason: SkipReason::EmptyAlias })
        );
        assert_eq!(
            parse("AA:BB:CC:00:00:01", ImportMode::Strict),
            Err(ImportError::BadLine { line: 1, reason: SkipReason::MissingColumn })
        );
        assert_eq!(
            parse("AA:BB:CC:00:00:01,x,extra", ImportMode::Strict),
            Err(ImportError::BadLine { line: 1, reason: SkipReason::ExtraColumns })
        );
        assert!(matches!(parse("AA:BB:CC:00:00:01,\"open", ImportMode::Strict), Err(ImportError::Csv(_))));
    }

    #[test]
    fn strict_rejects_duplicates() {
        let text = "AA:BB:CC:00:00:01,One\naa:bb:cc:00:00:01,Two";
        assert_eq!(parse(text, ImportMode::Strict), Err(ImportError::BadLine { line: 2, reason: SkipReason::Duplicate }));
    }

    #[test]
    fn lenient_keeps_good_lines_and_reports_the_rest() {
        let text = "BSSID;alias\nAA:BB:CC:00:00:01;One\nbad;Two\nAA:BB:CC:00:00:02;\nAA:BB:CC:00:00:01;Uno\nAA:BB:CC:00:00:02;Dos;extra\nlonely";
        let p = parse(text, ImportMode::Lenient).unwrap();
        assert_eq!(p.entries, vec![(A, "Uno".to_owned()), (B, "Dos".to_owned())], "semicolons detected, duplicate keeps the last alias");
        let reasons: Vec<_> = p.skipped.iter().map(|s| (s.line, s.reason)).collect();
        assert_eq!(
            reasons,
            vec![
                (3, SkipReason::InvalidBssid),
                (4, SkipReason::EmptyAlias),
                (5, SkipReason::Duplicate),
                (7, SkipReason::MissingColumn),
            ]
        );
    }

    #[test]
    fn apply_respects_conflict_policy_and_counts() {
        let mut live = Aliases::default();
        live.set(&A, "Mine");
        live.set(&B, "Same");
        let parsed = ParsedAliases {
            entries: vec![(A, "Theirs".into()), (B, "Same".into()), (MacAddress([1, 2, 3, 4, 5, 6]), "New".into())],
            skipped: vec![SkippedLine { line: 9, reason: SkipReason::InvalidBssid }],
        };

        let mut keep = live.clone();
        let r = apply(parsed.clone(), &mut keep, ConflictPolicy::KeepExisting);
        assert_eq!(keep.get(&A), Some("Mine"));
        assert_eq!((r.imported, r.overwritten, r.kept_existing, r.skipped.len()), (2, 0, 1, 1));

        let r = apply(parsed, &mut live, ConflictPolicy::Overwrite);
        assert_eq!(live.get(&A), Some("Theirs"));
        assert_eq!((r.imported, r.overwritten, r.kept_existing), (3, 1, 0));
        assert_eq!(live.len(), 3);
        assert_eq!(r.summary(), "3 imported (1 replaced existing), 1 skipped");
    }

    #[test]
    fn crlf_bom_and_blank_lines_are_fine() {
        let p = parse("\u{feff}BSSID,alias\r\n\r\nAA:BB:CC:00:00:01,One\r\n", ImportMode::Strict).unwrap();
        assert_eq!(p.entries.len(), 1);
    }

    #[test]
    fn empty_file_imports_nothing() {
        assert_eq!(parse("", ImportMode::Strict).unwrap(), ParsedAliases::default());
    }
}
