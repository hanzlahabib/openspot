//! Every key passed to `tr("…")` / `tr_args("…", …)` anywhere in the workspace must exist in
//! `en-US.ftl`, so a typo or a forgotten entry fails CI instead of showing `[missing:key]`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use openspot_i18n::EMBEDDED;

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// String-literal first arguments of `tr(` and `tr_args(` calls. Keys built at run time cannot be
/// checked here, which is why the docs ask for literal keys.
fn used_keys(source: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for callee in ["tr(\"", "tr_args(\""] {
        for (at, _) in source.match_indices(callee) {
            let before = source[..at].chars().next_back();
            // `str(`, `my_tr(` and `x.tr(` are other functions; `::tr(` is ours.
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                continue;
            }
            let rest = &source[at + callee.len()..];
            if let Some(end) = rest.find('"') {
                keys.push(rest[..end].to_owned());
            }
        }
    }
    keys
}

fn defined_keys() -> BTreeSet<String> {
    let (_, en) = EMBEDDED.iter().find(|(tag, _)| *tag == "en-US").unwrap();
    en.lines()
        .filter(|l| l.starts_with(|c: char| c.is_ascii_alphabetic()))
        .filter_map(|l| l.split_once('=').map(|(id, _)| id.trim().to_owned()))
        .collect()
}

#[test]
fn every_key_used_in_code_exists_in_en_us() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let defined = defined_keys();
    let mut files = Vec::new();
    for entry in fs::read_dir(crates).unwrap().flatten() {
        // This crate's own sources hold examples and deliberately bad keys.
        if entry.file_name() != "openspot-i18n" {
            rust_files(&entry.path().join("src"), &mut files);
        }
    }
    let mut missing = Vec::new();
    let mut checked = 0;
    for file in &files {
        let source = fs::read_to_string(file).unwrap();
        for key in used_keys(&source) {
            checked += 1;
            if !defined.contains(&key) {
                missing.push(format!("{}: `{key}`", file.display()));
            }
        }
    }
    assert!(checked > 0, "the scan found no tr(\"…\") calls; is the path wrong?");
    assert!(missing.is_empty(), "keys used in code but absent from en-US.ftl:\n{}", missing.join("\n"));
}

#[test]
fn scanner_recognises_calls_and_ignores_lookalikes() {
    let src = r#"tr("a") + openspot_i18n::tr("b") + tr_args("c", &x) + str("d") + self.tr("e") + my_tr("f")"#;
    assert_eq!(used_keys(src), ["a", "b", "c"]);
}
