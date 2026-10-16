//! A set of Fluent bundles for one locale plus the English fallback.

use std::fmt;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use unic_langid::LanguageIdentifier;

use crate::locale::{FALLBACK_TAG, parse_locale};
use crate::pseudo::pseudo_localize;

type Bundle = FluentBundle<FluentResource>;

/// Locale that is not backed by a file: English run through [`pseudo_localize`].
pub const PSEUDO_TAG: &str = "xx-PSEUDO";

const EN_US: &str = include_str!("../locales/en-US.ftl");

/// Every `.ftl` shipped in the binary, as `(tag, source)`. Add new languages here.
pub const EMBEDDED: &[(&str, &str)] = &[(FALLBACK_TAG, EN_US)];

/// Message that holds the locale's own name, shown in the language selector.
pub const LANGUAGE_NAME_KEY: &str = "language-name";

/// Result of resolving one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// Found in the active locale.
    Found(String),
    /// Missing in the active locale, found in en-US.
    Fallback(String),
    /// Missing everywhere: a bug in the calling code or in `en-US.ftl`.
    Missing,
}

pub struct Catalog {
    locale: LanguageIdentifier,
    /// Active locale first (when it has a file), en-US last.
    chain: Vec<Bundle>,
    pseudo: bool,
}

impl fmt::Debug for Catalog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Catalog").field("locale", &self.locale.to_string()).field("pseudo", &self.pseudo).finish()
    }
}

impl Catalog {
    /// Catalog over the sources compiled into the binary.
    pub fn embedded(locale: &LanguageIdentifier) -> Self {
        Self::from_sources(locale, EMBEDDED)
    }

    /// Catalog over arbitrary `(tag, ftl)` sources; the `en-US` entry is the fallback.
    /// Tests use this to exercise fallback without shipping a half-translated file.
    pub fn from_sources(locale: &LanguageIdentifier, sources: &[(&str, &str)]) -> Self {
        let pseudo = parse_locale(PSEUDO_TAG).as_ref() == Ok(locale);
        let fallback = parse_locale(FALLBACK_TAG).expect("fallback tag is valid");
        let mut chain = Vec::new();
        if !pseudo && *locale != fallback {
            chain.extend(bundle_for(locale, sources));
        }
        chain.extend(bundle_for(&fallback, sources));
        Self { locale: locale.clone(), chain, pseudo }
    }

    pub fn locale(&self) -> &LanguageIdentifier {
        &self.locale
    }

    pub fn lookup(&self, key: &str, args: Option<&FluentArgs<'_>>) -> Lookup {
        for bundle in &self.chain {
            let Some(pattern) = bundle.get_message(key).and_then(|m| m.value()) else { continue };
            // Formatting errors (e.g. a missing argument) still yield best-effort text, which is
            // more useful on screen than nothing; the lint test catches broken messages.
            let mut errors = Vec::new();
            let text = bundle.format_pattern(pattern, args, &mut errors).into_owned();
            let text = if self.pseudo { pseudo_localize(&text) } else { text };
            let native = self.pseudo || bundle.locales.first() == Some(&self.locale);
            return if native { Lookup::Found(text) } else { Lookup::Fallback(text) };
        }
        Lookup::Missing
    }
}

/// `None` when the sources have no file for `locale`.
fn bundle_for(locale: &LanguageIdentifier, sources: &[(&str, &str)]) -> Option<Bundle> {
    let (_, ftl) = sources.iter().find(|(tag, _)| parse_locale(tag).as_ref() == Ok(locale))?;
    // A file with a syntax error still yields every message that did parse.
    let resource = FluentResource::try_new((*ftl).to_owned()).unwrap_or_else(|(partial, _)| partial);
    let mut bundle = FluentBundle::new_concurrent(vec![locale.clone()]);
    // Directional isolation marks render as boxes in egui's bundled fonts. Revisit with RTL support.
    bundle.set_use_isolating(false);
    // Overriding is an error only for duplicate keys, which the lint test rejects.
    let _ = bundle.add_resource(resource);
    Some(bundle)
}
