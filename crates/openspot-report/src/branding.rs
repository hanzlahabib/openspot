//! Company and client details printed on the cover page (R-05).

use serde::{Deserialize, Serialize};

/// Persisted in the app preferences. Every field defaults to empty so older preference
/// files keep loading.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReportBranding {
    pub company: String,
    /// Path of a PNG or JPEG logo. Stored as text so preferences stay valid JSON on
    /// every platform; the image itself is read when a report is generated.
    pub logo_path: String,
    pub client: String,
    pub location: String,
    /// Phone, e-mail or web address of the author.
    pub contact: String,
}

impl ReportBranding {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Reads the logo file, `Ok(None)` when none is configured.
    pub fn load_logo(&self) -> Result<Option<Vec<u8>>, crate::ReportError> {
        let path = self.logo_path.trim();
        if path.is_empty() {
            return Ok(None);
        }
        std::fs::read(path).map(Some).map_err(|source| crate::ReportError::Io { path: path.into(), source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_default_so_old_prefs_load() {
        let b: ReportBranding = serde_json::from_str(r#"{"company":"Acme"}"#).unwrap();
        assert_eq!(b.company, "Acme");
        assert!(b.client.is_empty());
        assert!(!b.is_empty());
        assert!(ReportBranding::default().is_empty());
    }

    #[test]
    fn json_round_trip_keeps_unicode() {
        let b = ReportBranding {
            company: "Caf\u{e9} \u{65e5}\u{672c}".into(),
            logo_path: "C:\\logos\\a.png".into(),
            client: "c".into(),
            location: "l".into(),
            contact: "x@y.z".into(),
        };
        assert_eq!(serde_json::from_str::<ReportBranding>(&serde_json::to_string(&b).unwrap()).unwrap(), b);
    }

    #[test]
    fn no_logo_configured_is_not_an_error_but_missing_file_is() {
        assert!(ReportBranding::default().load_logo().unwrap().is_none());
        let b = ReportBranding { logo_path: "/definitely/not/here.png".into(), ..Default::default() };
        assert!(matches!(b.load_logo(), Err(crate::ReportError::Io { .. })));
    }
}
