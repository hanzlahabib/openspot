//! Localization for OpenSpot, built on Project Fluent.
//!
//! ```
//! use openspot_i18n::{Args, tr, tr_args};
//!
//! assert_eq!(tr("mode-inspector"), "Inspector");
//! let mut args = Args::new();
//! args.set("percent", 125);
//! assert_eq!(tr_args("prefs-scale-option", &args), "125 %");
//! ```
//!
//! The active locale is process-wide: UI code calls [`tr`] without threading a handle through
//! every function, which matters for an immediate-mode UI that formats text every frame.
//! See `docs/I18N.md` for how to migrate a file and add a language.

mod catalog;
mod locale;
mod pseudo;

use std::sync::{Arc, LazyLock, PoisonError, RwLock};

pub use catalog::{Catalog, EMBEDDED, LANGUAGE_NAME_KEY, Lookup, PSEUDO_TAG};
pub use fluent_bundle::FluentArgs as Args;
pub use locale::{FALLBACK_TAG, ParseLocaleError, negotiate, parse_locale};
pub use pseudo::pseudo_localize;
pub use unic_langid::LanguageIdentifier;

/// Prefix of text served from en-US because the active locale lacks the key. Debug builds only.
pub const FALLBACK_MARKER: &str = "[en] ";

struct State {
    /// What the user chose: a tag, or `None` for "follow the operating system".
    preference: Option<String>,
    catalog: Arc<Catalog>,
}

static STATE: LazyLock<RwLock<State>> = LazyLock::new(|| {
    let fallback = parse_locale(FALLBACK_TAG).expect("fallback tag is valid");
    RwLock::new(State { preference: None, catalog: Arc::new(Catalog::embedded(&fallback)) })
});

/// A language the user can pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleInfo {
    /// BCP 47 tag, the value stored in preferences.
    pub tag: String,
    /// The language's own name for itself.
    pub name: String,
}

/// Selects the language from the stored preference (`None` or an unusable tag follows the OS)
/// and returns the locale that became active.
pub fn set_preference(preference: Option<&str>) -> LanguageIdentifier {
    let catalog = Arc::new(Catalog::embedded(&resolve(preference)));
    let locale = catalog.locale().clone();
    let mut state = STATE.write().unwrap_or_else(PoisonError::into_inner);
    state.preference = preference.map(str::to_owned);
    state.catalog = catalog;
    locale
}

/// The stored preference exactly as given to [`set_preference`], for writing back to prefs.
pub fn preference() -> Option<String> {
    STATE.read().unwrap_or_else(PoisonError::into_inner).preference.clone()
}

/// The locale text is currently rendered in.
pub fn active_locale() -> LanguageIdentifier {
    catalog().locale().clone()
}

/// Shipped locales, each named in its own language. The pseudo-locale is listed in debug builds
/// only, so end users never see it.
pub fn available_locales() -> Vec<LocaleInfo> {
    let mut list: Vec<LocaleInfo> = EMBEDDED
        .iter()
        .filter_map(|(tag, _)| {
            let locale = parse_locale(tag).ok()?;
            let name = match Catalog::embedded(&locale).lookup(LANGUAGE_NAME_KEY, None) {
                Lookup::Found(name) => name,
                Lookup::Fallback(_) | Lookup::Missing => (*tag).to_owned(),
            };
            Some(LocaleInfo { tag: (*tag).to_owned(), name })
        })
        .collect();
    if cfg!(debug_assertions) {
        list.push(LocaleInfo { tag: PSEUDO_TAG.to_owned(), name: "Pseudo-locale (testing)".to_owned() });
    }
    list
}

/// Text for `key` in the active locale.
pub fn tr(key: &str) -> String {
    render(key, None)
}

/// Text for `key` with Fluent variables, e.g. a plural `count`.
pub fn tr_args(key: &str, args: &Args<'_>) -> String {
    render(key, Some(args))
}

fn render(key: &str, args: Option<&Args<'_>>) -> String {
    decorate(key, catalog().lookup(key, args), cfg!(debug_assertions))
}

/// Turns a lookup into display text. With `debug` set, problems are visible on screen: fallback
/// text is prefixed and a missing key is bracketed. Otherwise users get the best text available
/// and the bare key as a last resort.
fn decorate(key: &str, lookup: Lookup, debug: bool) -> String {
    match lookup {
        Lookup::Found(text) => text,
        Lookup::Fallback(text) if debug => format!("{FALLBACK_MARKER}{text}"),
        Lookup::Fallback(text) => text,
        Lookup::Missing if debug => format!("[missing:{key}]"),
        Lookup::Missing => key.to_owned(),
    }
}

fn catalog() -> Arc<Catalog> {
    Arc::clone(&STATE.read().unwrap_or_else(PoisonError::into_inner).catalog)
}

fn resolve(preference: Option<&str>) -> LanguageIdentifier {
    let fallback = parse_locale(FALLBACK_TAG).expect("fallback tag is valid");
    let mut available: Vec<LanguageIdentifier> = EMBEDDED.iter().filter_map(|(tag, _)| parse_locale(tag).ok()).collect();
    if let Some(pseudo) = parse_locale(PSEUDO_TAG).ok().filter(|_| preference == Some(PSEUDO_TAG)) {
        available.push(pseudo);
    }
    let requested = preference
        .and_then(|p| parse_locale(p).ok())
        .or_else(|| sys_locale::get_locale().and_then(|l| parse_locale(&l).ok()));
    requested.map_or(fallback.clone(), |r| negotiate(&r, &available, &fallback))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorate_marks_problems_only_in_debug() {
        assert_eq!(decorate("k", Lookup::Found("x".into()), true), "x");
        assert_eq!(decorate("k", Lookup::Fallback("x".into()), true), "[en] x");
        assert_eq!(decorate("k", Lookup::Fallback("x".into()), false), "x");
        assert_eq!(decorate("k", Lookup::Missing, true), "[missing:k]");
        assert_eq!(decorate("k", Lookup::Missing, false), "k");
    }
}
