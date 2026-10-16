//! Checks on the `.ftl` files compiled into the binary, and the process-wide API.

use std::collections::BTreeSet;

use openspot_i18n::{Args, EMBEDDED, FALLBACK_TAG, LANGUAGE_NAME_KEY, PSEUDO_TAG, available_locales, tr, tr_args};
use openspot_i18n::{active_locale, parse_locale, preference, set_preference};

fn message_ids(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter(|l| l.starts_with(|c: char| c.is_ascii_alphabetic()))
        .filter_map(|l| l.split_once('=').map(|(id, _)| id.trim().to_owned()))
        .collect()
}

#[test]
fn every_shipped_file_parses_without_errors() {
    for (tag, source) in EMBEDDED {
        assert!(parse_locale(tag).is_ok(), "{tag} is not a valid tag");
        fluent_bundle::FluentResource::try_new((*source).to_owned())
            .unwrap_or_else(|(_, errors)| panic!("{tag}.ftl has syntax errors: {errors:?}"));
    }
}

#[test]
fn no_duplicate_ids_and_every_locale_names_itself() {
    for (tag, source) in EMBEDDED {
        let ids: Vec<_> = source
            .lines()
            .filter(|l| l.starts_with(|c: char| c.is_ascii_alphabetic()))
            .filter_map(|l| l.split_once('=').map(|(id, _)| id.trim()))
            .collect();
        let unique: BTreeSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len(), "{tag}.ftl repeats a message id");
        assert!(ids.contains(&LANGUAGE_NAME_KEY), "{tag}.ftl lacks `{LANGUAGE_NAME_KEY}`");
    }
}

#[test]
fn other_locales_only_define_keys_that_exist_in_en_us() {
    let (_, en) = EMBEDDED.iter().find(|(tag, _)| *tag == FALLBACK_TAG).unwrap();
    let en = message_ids(en);
    for (tag, source) in EMBEDDED.iter().filter(|(tag, _)| *tag != FALLBACK_TAG) {
        let extra: Vec<_> = message_ids(source).difference(&en).cloned().collect();
        assert!(extra.is_empty(), "{tag}.ftl has keys unknown to en-US: {extra:?}");
    }
}

/// One test owns the process-wide locale; parallel tests would race on it.
#[test]
fn global_locale_switching() {
    set_preference(Some("en-US"));
    assert_eq!(preference().as_deref(), Some("en-US"));
    assert_eq!(tr("mode-survey"), "Survey");
    let mut args = Args::new();
    args.set("percent", 130);
    assert_eq!(tr_args("prefs-scale-option", &args), "130 %");

    // An unusable preference degrades to a shipped locale instead of failing.
    set_preference(Some("not a locale"));
    assert!(EMBEDDED.iter().any(|(tag, _)| parse_locale(tag).unwrap() == active_locale()));

    assert_eq!(set_preference(Some(PSEUDO_TAG)).to_string(), "xx-pseudo");
    assert_eq!(tr("mode-survey"), "[Súrvéy~]");
    assert!(tr("definitely-not-a-key").contains("definitely-not-a-key"));

    set_preference(Some("en-US"));
    assert!(available_locales().iter().any(|l| l.tag == FALLBACK_TAG && l.name == "English (United States)"));
    set_preference(None);
    assert_eq!(preference(), None);
}
