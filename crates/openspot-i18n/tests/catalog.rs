//! Catalog behaviour over in-memory sources, independent of the shipped locales.

use openspot_i18n::{Args, Catalog, LanguageIdentifier, Lookup, PSEUDO_TAG, parse_locale};

const EN: &str = "
greeting = Hello
plain-only-en = English only
files = { $count ->
    [one] { $count } file
   *[other] { $count } files
}
welcome = Welcome, { $name }!
";
const DE: &str = "
greeting = Hallo
files = { $count ->
    [one] { $count } Datei
   *[other] { $count } Dateien
}
";

fn locale(tag: &str) -> LanguageIdentifier {
    parse_locale(tag).unwrap()
}

fn catalog(tag: &str) -> Catalog {
    Catalog::from_sources(&locale(tag), &[("en-US", EN), ("de", DE)])
}

fn text(lookup: Lookup) -> String {
    match lookup {
        Lookup::Found(t) | Lookup::Fallback(t) => t,
        Lookup::Missing => panic!("missing"),
    }
}

#[test]
fn looks_up_a_plain_message() {
    assert_eq!(catalog("en-US").lookup("greeting", None), Lookup::Found("Hello".into()));
    assert_eq!(catalog("de").lookup("greeting", None), Lookup::Found("Hallo".into()));
}

#[test]
fn substitutes_arguments() {
    let mut args = Args::new();
    args.set("name", "Ada");
    // No Unicode isolation marks around the placeable: egui's fonts cannot draw them.
    assert_eq!(text(catalog("en-US").lookup("welcome", Some(&args))), "Welcome, Ada!");
}

#[test]
fn english_plurals_distinguish_one_from_many() {
    let c = catalog("en-US");
    let count = |n: i32| {
        let mut args = Args::new();
        args.set("count", n);
        text(c.lookup("files", Some(&args)))
    };
    assert_eq!(count(1), "1 file");
    assert_eq!(count(0), "0 files");
    assert_eq!(count(2), "2 files");
}

#[test]
fn plural_rules_follow_the_active_locale() {
    let c = catalog("de");
    let mut args = Args::new();
    args.set("count", 1);
    assert_eq!(text(c.lookup("files", Some(&args))), "1 Datei");
    args.set("count", 5);
    assert_eq!(text(c.lookup("files", Some(&args))), "5 Dateien");
}

#[test]
fn missing_key_in_locale_falls_back_to_english() {
    assert_eq!(catalog("de").lookup("plain-only-en", None), Lookup::Fallback("English only".into()));
}

#[test]
fn locale_without_a_file_is_all_fallback() {
    assert_eq!(catalog("ja").lookup("greeting", None), Lookup::Fallback("Hello".into()));
}

#[test]
fn unknown_key_is_reported_as_missing() {
    assert_eq!(catalog("de").lookup("no-such-key", None), Lookup::Missing);
}

#[test]
fn pseudo_locale_transforms_every_message() {
    let c = Catalog::from_sources(&locale(PSEUDO_TAG), &[("en-US", EN)]);
    // Pseudo output counts as found: it is the point of the locale, not a fallback.
    assert_eq!(c.lookup("greeting", None), Lookup::Found("[Hélló~]".into()));
    assert_eq!(c.lookup("no-such-key", None), Lookup::Missing);
}
