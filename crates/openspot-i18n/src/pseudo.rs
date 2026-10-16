//! Pseudo-localization: makes untranslated or hard-coded text stand out and exposes layouts that
//! break when a translation is longer than English.

/// Extra length added to every string; German and Finnish routinely run 30 % longer than English.
const EXPANSION_PERCENT: usize = 30;

/// Accents ASCII vowels (still readable), pads by [`EXPANSION_PERCENT`] and brackets the result.
/// Anything shown on screen *without* brackets and accents did not come through [`crate::tr`].
pub fn pseudo_localize(text: &str) -> String {
    let accented: String = text.chars().map(accent).collect();
    let padding = accented.chars().count() * EXPANSION_PERCENT / 100;
    let mut out = String::with_capacity(accented.len() + padding + 2);
    out.push('[');
    out.push_str(&accented);
    out.extend(std::iter::repeat_n('~', padding));
    out.push(']');
    out
}

fn accent(c: char) -> char {
    match c {
        'a' => 'á',
        'e' => 'é',
        'i' => 'í',
        'o' => 'ó',
        'u' => 'ú',
        'A' => 'Á',
        'E' => 'É',
        'I' => 'Í',
        'O' => 'Ó',
        'U' => 'Ú',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brackets_accents_and_expands() {
        assert_eq!(pseudo_localize("Open"), "[Ópén~]");
        assert_eq!(pseudo_localize(""), "[]");
    }

    #[test]
    fn expansion_is_about_thirty_percent() {
        let plain = "a".repeat(100);
        let out = pseudo_localize(&plain);
        assert_eq!(out.chars().count(), 100 + 30 + 2);
    }
}
