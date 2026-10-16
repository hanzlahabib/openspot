//! Opt-in update check against the GitHub releases API.
//!
//! Off by default. The only thing sent is one HTTPS GET with a `User-Agent` header (GitHub
//! requires one); no identifiers, no usage data. Runs on a background thread so it can never
//! delay startup, with a hard timeout.

use std::cmp::Ordering;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use eframe::egui;
use serde::Deserialize;
use thiserror::Error;

/// `owner/name` of the GitHub repository whose releases are checked. Placeholder until the
/// project is published; the lead sets the real value.
pub const GITHUB_REPOSITORY: &str = "hanzlahabib/openspot";

/// Give up after this long overall; a hung network must not leave a thread around forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// This build's version, from Cargo.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UpdateError {
    #[error("not a valid version: \"{0}\"")]
    BadVersion(String),
    #[error("unexpected response from the release server: {0}")]
    BadResponse(String),
    #[error("could not reach the release server: {0}")]
    Network(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Dot-separated pre-release identifiers (`rc.1`), empty for a final release.
    pub pre: Vec<String>,
}

impl Version {
    /// Parses `1.2.3`, `v1.2.3`, `1.2.3-rc.1`; build metadata (`+abc`) is ignored.
    pub fn parse(text: &str) -> Result<Self, UpdateError> {
        let bad = || UpdateError::BadVersion(text.to_owned());
        let t = text.trim().trim_start_matches(['v', 'V']);
        let t = t.split('+').next().unwrap_or(t);
        let (core, pre) = match t.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (t, None),
        };
        let mut nums = core.split('.');
        let mut next = || nums.next().and_then(|n| n.parse::<u64>().ok()).ok_or_else(bad);
        let (major, minor, patch) = (next()?, next()?, next()?);
        if core.split('.').count() != 3 {
            return Err(bad());
        }
        let pre: Vec<String> = pre.map(|p| p.split('.').map(str::to_owned).collect()).unwrap_or_default();
        if pre.iter().any(String::is_empty) {
            return Err(bad());
        }
        Ok(Self { major, minor, patch, pre })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    /// SemVer 2.0 precedence: a pre-release sorts before its release; numeric identifiers
    /// compare as numbers and sort before alphanumeric ones; a longer list wins a tie.
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch)).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => compare_pre(&self.pre, &other.pre),
            }
        })
    }
}

fn compare_pre(a: &[String], b: &[String]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(m), Ok(n)) => m.cmp(&n),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    a.len().cmp(&b.len())
}

#[derive(Debug, Deserialize)]
struct ReleaseJson {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate,
    Available { version: String, url: String },
}

/// Compares the running version with the body of `GET /repos/{repo}/releases/latest`.
/// Drafts and pre-releases never count as an update.
pub fn evaluate(current: &str, response_json: &str) -> Result<UpdateStatus, UpdateError> {
    let release: ReleaseJson = serde_json::from_str(response_json).map_err(|e| UpdateError::BadResponse(e.to_string()))?;
    if release.draft || release.prerelease {
        return Ok(UpdateStatus::UpToDate);
    }
    // Only ever offer a link to the project's GitHub pages, whatever the response says.
    if !release.html_url.starts_with("https://github.com/") {
        return Err(UpdateError::BadResponse("release link is not a GitHub URL".to_owned()));
    }
    let latest = Version::parse(&release.tag_name)?;
    Ok(if latest > Version::parse(current)? {
        UpdateStatus::Available { version: release.tag_name.trim_start_matches(['v', 'V']).to_owned(), url: release.html_url }
    } else {
        UpdateStatus::UpToDate
    })
}

fn latest_release_url(repository: &str) -> String {
    format!("https://api.github.com/repos/{repository}/releases/latest")
}

fn fetch_and_evaluate(repository: &str) -> Result<UpdateStatus, UpdateError> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(REQUEST_TIMEOUT)).build().into();
    let mut response = agent
        .get(&latest_release_url(repository))
        .header("User-Agent", concat!("OpenSpot/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    let body = response.body_mut().read_to_string().map_err(|e| UpdateError::Network(e.to_string()))?;
    evaluate(CURRENT_VERSION, &body)
}

/// Background check plus its latest outcome.
#[derive(Debug, Default)]
pub struct UpdateChecker {
    pending: Option<Receiver<Result<UpdateStatus, UpdateError>>>,
    pub result: Option<Result<UpdateStatus, UpdateError>>,
}

impl UpdateChecker {
    pub fn is_checking(&self) -> bool {
        self.pending.is_some()
    }

    /// Starts a check on a new thread unless one is already running.
    pub fn start(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.pending = Some(rx);
        self.result = None;
        thread::spawn(move || {
            // The receiver is gone only if the app is closing.
            let _ = tx.send(fetch_and_evaluate(GITHUB_REPOSITORY));
            ctx.request_repaint();
        });
    }

    /// Collects a finished check. Returns true when a new result arrived.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.pending else { return false };
        match rx.try_recv() {
            Ok(result) => {
                self.result = Some(result);
                self.pending = None;
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.result = Some(Err(UpdateError::Network("the check ended unexpectedly".to_owned())));
                self.pending = None;
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn release_json(tag: &str, draft: bool, prerelease: bool, url: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","html_url":"{url}","draft":{draft},"prerelease":{prerelease},"name":"x","assets":[]}}"#)
    }

    const URL: &str = "https://github.com/o/openspot/releases/tag/v0.2.0";

    #[test]
    fn parses_common_forms() {
        assert_eq!(v("1.2.3"), Version { major: 1, minor: 2, patch: 3, pre: vec![] });
        assert_eq!(v("v10.0.1"), v("10.0.1"));
        assert_eq!(v("1.0.0-rc.1").pre, vec!["rc", "1"]);
        assert_eq!(v("1.0.0+build.5"), v("1.0.0"));
        for bad in ["", "1.2", "1.2.3.4", "a.b.c", "1.2.x", "1.2.3-", "1.2.3-a..b", "-1.2.3"] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn precedence_follows_semver() {
        // The ordering example from the SemVer 2.0 specification, item 11.
        let ordered = ["1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta", "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0"];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert!(v("0.9.9") < v("0.10.0"), "numeric, not lexical");
        assert!(v("1.2.3") < v("2.0.0"));
        assert_eq!(v("1.2.3").cmp(&v("v1.2.3+meta")), Ordering::Equal);
    }

    #[test]
    fn newer_release_is_reported_with_its_link() {
        let status = evaluate("0.1.0", &release_json("v0.2.0", false, false, URL)).unwrap();
        assert_eq!(status, UpdateStatus::Available { version: "0.2.0".into(), url: URL.into() });
    }

    #[test]
    fn same_or_older_release_is_up_to_date() {
        assert_eq!(evaluate("0.2.0", &release_json("v0.2.0", false, false, URL)).unwrap(), UpdateStatus::UpToDate);
        assert_eq!(evaluate("0.3.0", &release_json("v0.2.0", false, false, URL)).unwrap(), UpdateStatus::UpToDate);
    }

    #[test]
    fn a_stable_build_is_not_offered_a_prerelease_of_the_same_version() {
        assert_eq!(evaluate("0.2.0", &release_json("v0.2.0-rc.1", false, false, URL)).unwrap(), UpdateStatus::UpToDate);
        assert!(matches!(evaluate("0.2.0-rc.1", &release_json("v0.2.0", false, false, URL)).unwrap(), UpdateStatus::Available { .. }));
    }

    #[test]
    fn drafts_and_flagged_prereleases_are_ignored() {
        assert_eq!(evaluate("0.1.0", &release_json("v9.9.9", true, false, URL)).unwrap(), UpdateStatus::UpToDate);
        assert_eq!(evaluate("0.1.0", &release_json("v9.9.9", false, true, URL)).unwrap(), UpdateStatus::UpToDate);
    }

    #[test]
    fn malformed_and_hostile_responses_are_errors() {
        assert!(matches!(evaluate("0.1.0", "not json"), Err(UpdateError::BadResponse(_))));
        assert!(matches!(evaluate("0.1.0", r#"{"message":"Not Found"}"#), Err(UpdateError::BadResponse(_))));
        assert!(matches!(evaluate("0.1.0", &release_json("latest", false, false, URL)), Err(UpdateError::BadVersion(_))));
        let evil = release_json("v0.2.0", false, false, "https://evil.example/download");
        assert!(matches!(evaluate("0.1.0", &evil), Err(UpdateError::BadResponse(_))));
        let js = release_json("v0.2.0", false, false, "javascript:alert(1)");
        assert!(evaluate("0.1.0", &js).is_err());
    }

    #[test]
    fn minimal_response_with_only_required_fields_parses() {
        let body = format!(r#"{{"tag_name":"v0.2.0","html_url":"{URL}"}}"#);
        assert!(matches!(evaluate("0.1.0", &body), Ok(UpdateStatus::Available { .. })));
    }

    #[test]
    fn request_url_and_current_version_are_well_formed() {
        assert_eq!(latest_release_url("a/b"), "https://api.github.com/repos/a/b/releases/latest");
        assert!(Version::parse(CURRENT_VERSION).is_ok());
    }
}
