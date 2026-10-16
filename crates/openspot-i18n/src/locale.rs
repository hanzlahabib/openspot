//! BCP 47 locale parsing and negotiation against the set of shipped locales.

use unic_langid::LanguageIdentifier;

/// The locale every key must exist in, and the last stop of the fallback chain.
pub const FALLBACK_TAG: &str = "en-US";

/// Why a locale string was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a valid BCP 47 locale tag")]
pub struct ParseLocaleError(pub String);

/// Parses a tag as found in prefs files or OS settings.
///
/// POSIX-style values (`de_DE.UTF-8@euro`) are accepted because `LANG` still looks like that on
/// Linux: the encoding and modifier are dropped and `_` becomes `-`.
pub fn parse_locale(raw: &str) -> Result<LanguageIdentifier, ParseLocaleError> {
    let trimmed = raw.trim();
    let base = trimmed.split(['.', '@']).next().unwrap_or(trimmed);
    base.replace('_', "-").parse().map_err(|_| ParseLocaleError(raw.to_owned()))
}

/// Picks the shipped locale that best serves `requested`: an exact match, else the first shipped
/// locale with the same language (`de-AT` gets `de-DE`), else `fallback`.
pub fn negotiate(
    requested: &LanguageIdentifier,
    available: &[LanguageIdentifier],
    fallback: &LanguageIdentifier,
) -> LanguageIdentifier {
    available
        .iter()
        .find(|a| *a == requested)
        .or_else(|| available.iter().find(|a| a.language == requested.language))
        .unwrap_or(fallback)
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(tag: &str) -> LanguageIdentifier {
        tag.parse().unwrap()
    }

    #[test]
    fn parses_bcp47_and_posix_forms() {
        assert_eq!(parse_locale("en-US").unwrap(), id("en-US"));
        assert_eq!(parse_locale("de_DE.UTF-8").unwrap(), id("de-DE"));
        assert_eq!(parse_locale("sr_RS@latin").unwrap(), id("sr-RS"));
        assert_eq!(parse_locale("  fr ").unwrap(), id("fr"));
        assert_eq!(parse_locale("xx-PSEUDO").unwrap().language.as_str(), "xx");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_locale("not a locale"), Err(ParseLocaleError("not a locale".into())));
        assert!(parse_locale("").is_err());
    }

    #[test]
    fn negotiation_prefers_exact_then_language_then_fallback() {
        let available = [id("en-US"), id("de-DE")];
        let fallback = id("en-US");
        assert_eq!(negotiate(&id("de-DE"), &available, &fallback), id("de-DE"));
        assert_eq!(negotiate(&id("de-AT"), &available, &fallback), id("de-DE"));
        assert_eq!(negotiate(&id("ja-JP"), &available, &fallback), id("en-US"));
    }
}
