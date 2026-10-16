//! Project archives (`.ospotz`, R-07): a zip holding the project database and a manifest.
//!
//! ```text
//! manifest.json   {"format_version":1,"app_version":"..","created_unix":..,"members":[{"name","size","sha256"}]}
//! project.ospot   the SQLite project file
//! ```
//!
//! Import checks, in this order, before a single database byte is trusted: the entry list
//! (only the two known names, no duplicates, no path tricks), the manifest (valid JSON,
//! supported version, member names), then streams the database while enforcing a size cap
//! and comparing length and SHA-256 with the manifest.
//!
//! The checksums detect corruption and accidental edits. They are *not* a signature: someone
//! who rewrites both a member and the manifest produces a self-consistent archive, so only
//! import archives from sources you trust, as with any project file.

use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use openspot_model::{Project, Uuid};
use openspot_store::{Store, StoreError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const ARCHIVE_EXTENSION: &str = "ospotz";
/// Bumped when the archive layout changes incompatibly.
pub const FORMAT_VERSION: u32 = 1;
pub const DATABASE_MEMBER: &str = "project.ospot";
pub const MANIFEST_MEMBER: &str = "manifest.json";

/// Cap on the database member. A survey with dozens of floor-plan images is far below
/// 100 MiB; 2 GiB leaves room for huge plans while bounding what a hostile archive can
/// make us write to disk.
pub const MAX_DATABASE_BYTES: u64 = 2 << 30;
/// The manifest is a few hundred bytes; 1 MiB is generous and keeps JSON parsing cheap.
pub const MAX_MANIFEST_BYTES: u64 = 1 << 20;
/// An archive has exactly two entries; a little slack tolerates nothing else on purpose,
/// so this only bounds the work done before the "unexpected entry" rejection.
const MAX_ENTRIES: usize = 16;
/// First 16 bytes of every SQLite 3 database file (<https://www.sqlite.org/fileformat2.html#magic_header_string>).
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
const COPY_BUF: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a valid archive: {0}")]
    NotAnArchive(String),
    #[error("project database error: {0}")]
    Store(#[from] StoreError),
    #[error("archive entry has an unsafe path: {0:?}")]
    UnsafePath(String),
    #[error("archive contains an unexpected entry: {0:?}")]
    UnexpectedEntry(String),
    #[error("archive has {count} entries, more than the {limit} allowed")]
    TooManyEntries { count: usize, limit: usize },
    #[error("{name:?} is larger than the allowed {limit} bytes")]
    TooLarge { name: String, limit: u64 },
    #[error("archive is missing {0:?}")]
    MissingMember(String),
    #[error("{name:?} is {actual} bytes but the manifest says {expected}")]
    SizeMismatch { name: String, expected: u64, actual: u64 },
    #[error("checksum of {0:?} does not match the manifest; the archive is damaged or was modified")]
    ChecksumMismatch(String),
    #[error("{0:?} is damaged (failed the zip integrity check)")]
    Corrupt(String),
    #[error("manifest is invalid: {0}")]
    ManifestInvalid(String),
    #[error("archive format version {found} is newer than this build supports ({supported})")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("the project file inside the archive is not a SQLite database")]
    NotADatabase,
}

fn map_zip(e: ZipError) -> ArchiveError {
    match e {
        ZipError::Io(io) => ArchiveError::Io(io),
        other => ArchiveError::NotAnArchive(other.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub name: String,
    pub size: u64,
    /// Lower-case hex SHA-256 of the member's bytes.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format_version: u32,
    /// Version of the app that wrote the archive.
    pub app_version: String,
    /// Creation time, Unix seconds (UTC).
    pub created_unix: i64,
    pub members: Vec<Member>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Rejects names that could escape an extraction directory or that no writer of this format
/// produces: absolute paths, `..` or `.` components, empty components, backslashes (a
/// separator on Windows), drive letters and NUL.
fn validate_entry_name(name: &str) -> Result<(), ArchiveError> {
    let bad = name.is_empty()
        || name.contains(['\0', '\\'])
        || name.starts_with('/')
        || name.split('/').any(|c| c.is_empty() || c == "." || c == "..")
        || name.as_bytes().get(1) == Some(&b':');
    if bad { Err(ArchiveError::UnsafePath(name.to_owned())) } else { Ok(()) }
}

/// Settings for [`extract_database`]; the defaults are the module's limits.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_database_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_database_bytes: MAX_DATABASE_BYTES }
    }
}

/// Removes a temporary file when dropped.
#[derive(Debug)]
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        // Best effort: nothing useful can be done if the OS refuses to delete a temp file.
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Serialises `project` through the regular store into SQLite file bytes.
fn database_bytes(project: &Project) -> Result<Vec<u8>, ArchiveError> {
    let guard = TempFile(std::env::temp_dir().join(format!("openspot-archive-{}.ospot", Uuid::new_v4())));
    {
        let mut store = Store::open(&guard.0)?;
        store.save(project)?;
        // `store` is dropped here, closing the connection before the file is read.
    }
    Ok(std::fs::read(&guard.0)?)
}

/// Writes an archive holding `database` (SQLite file bytes) to `out`.
pub fn write_archive<W: Write + Seek>(database: &[u8], out: W, app_version: &str, created_unix: i64) -> Result<Manifest, ArchiveError> {
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        app_version: app_version.to_owned(),
        created_unix,
        members: vec![Member { name: DATABASE_MEMBER.to_owned(), size: database.len() as u64, sha256: hex(&Sha256::digest(database)) }],
    };
    let mut zip = ZipWriter::new(out);
    // Timestamps stay at the zip default so identical projects give identical archives.
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).large_file(database.len() as u64 > u64::from(u32::MAX));
    zip.start_file(MANIFEST_MEMBER, options).map_err(map_zip)?;
    zip.write_all(serde_json::to_string_pretty(&manifest).map_err(|e| ArchiveError::ManifestInvalid(e.to_string()))?.as_bytes())?;
    zip.start_file(DATABASE_MEMBER, options).map_err(map_zip)?;
    zip.write_all(database)?;
    zip.finish().map_err(map_zip)?;
    Ok(manifest)
}

/// Saves `project` as an archive at `path`. The file appears only when complete: it is
/// written next to the target and renamed.
pub fn export_project(project: &Project, path: &Path, app_version: &str, created_unix: i64) -> Result<Manifest, ArchiveError> {
    let db = database_bytes(project)?;
    let partial = TempFile(path.with_extension(format!("{ARCHIVE_EXTENSION}.partial")));
    let manifest = write_archive(&db, std::fs::File::create(&partial.0)?, app_version, created_unix)?;
    std::fs::rename(&partial.0, path)?;
    Ok(manifest)
}

struct Verified {
    manifest: Manifest,
    expected: Member,
}

/// Validates the entry list and the manifest, without touching member contents.
fn read_manifest<R: Read + Seek>(zip: &mut ZipArchive<R>) -> Result<Verified, ArchiveError> {
    if zip.len() > MAX_ENTRIES {
        return Err(ArchiveError::TooManyEntries { count: zip.len(), limit: MAX_ENTRIES });
    }
    // Duplicate names never reach this loop: the zip reader keeps one entry per name, and
    // whichever it keeps still has to match the manifest's checksum.
    let mut seen = std::collections::BTreeSet::new();
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).map_err(map_zip)?;
        // Names that are not valid text cannot be one of ours.
        let name = entry.name().map_err(|_| ArchiveError::UnsafePath("<invalid name>".to_owned()))?.into_owned();
        validate_entry_name(&name)?;
        // Second opinion from the zip crate's own sanitiser.
        if entry.enclosed_name().is_none() {
            return Err(ArchiveError::UnsafePath(name));
        }
        if name != MANIFEST_MEMBER && name != DATABASE_MEMBER {
            return Err(ArchiveError::UnexpectedEntry(name));
        }
        seen.insert(name);
    }
    if !seen.contains(MANIFEST_MEMBER) {
        return Err(ArchiveError::MissingMember(MANIFEST_MEMBER.to_owned()));
    }

    let mut text = Vec::new();
    let entry = zip.by_name(MANIFEST_MEMBER).map_err(map_zip)?;
    // `take` bounds the read even if the declared size lies.
    entry.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut text).map_err(|_| ArchiveError::Corrupt(MANIFEST_MEMBER.to_owned()))?;
    if text.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(ArchiveError::TooLarge { name: MANIFEST_MEMBER.to_owned(), limit: MAX_MANIFEST_BYTES });
    }
    let manifest: Manifest = serde_json::from_slice(&text).map_err(|e| ArchiveError::ManifestInvalid(e.to_string()))?;
    if manifest.format_version == 0 || manifest.format_version > FORMAT_VERSION {
        return Err(ArchiveError::UnsupportedVersion { found: manifest.format_version, supported: FORMAT_VERSION });
    }
    for m in &manifest.members {
        validate_entry_name(&m.name)?;
        if m.name != DATABASE_MEMBER {
            return Err(ArchiveError::UnexpectedEntry(m.name.clone()));
        }
    }
    let expected = manifest.members.iter().find(|m| m.name == DATABASE_MEMBER).cloned().ok_or_else(|| ArchiveError::MissingMember(DATABASE_MEMBER.to_owned()))?;
    if manifest.members.len() != 1 {
        return Err(ArchiveError::ManifestInvalid("database member listed more than once".to_owned()));
    }
    Ok(Verified { manifest, expected })
}

/// Verifies the archive in `reader` and streams the database into `sink`.
///
/// `sink` receives bytes *before* the final checksum is known, so on `Err` its contents must
/// be discarded; [`import_project`] does that for files.
pub fn extract_database<R: Read + Seek, W: Write>(reader: R, sink: &mut W, limits: Limits) -> Result<Manifest, ArchiveError> {
    let mut zip = ZipArchive::new(reader).map_err(map_zip)?;
    let Verified { manifest, expected } = read_manifest(&mut zip)?;
    if expected.size > limits.max_database_bytes {
        return Err(ArchiveError::TooLarge { name: DATABASE_MEMBER.to_owned(), limit: limits.max_database_bytes });
    }
    let entry = zip.by_name(DATABASE_MEMBER).map_err(|e| match e {
        ZipError::FileNotFound => ArchiveError::MissingMember(DATABASE_MEMBER.to_owned()),
        other => map_zip(other),
    })?;
    if entry.size() > limits.max_database_bytes {
        return Err(ArchiveError::TooLarge { name: DATABASE_MEMBER.to_owned(), limit: limits.max_database_bytes });
    }

    let mut hasher = Sha256::new();
    let mut total: u64 = 0;
    let mut header = Vec::with_capacity(SQLITE_MAGIC.len());
    let mut reader = entry.take(limits.max_database_bytes + 1);
    let mut buf = vec![0u8; COPY_BUF];
    loop {
        // A failed zip CRC or a truncated deflate stream surfaces here as an I/O error.
        let n = reader.read(&mut buf).map_err(|_| ArchiveError::Corrupt(DATABASE_MEMBER.to_owned()))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > limits.max_database_bytes {
            return Err(ArchiveError::TooLarge { name: DATABASE_MEMBER.to_owned(), limit: limits.max_database_bytes });
        }
        if header.len() < SQLITE_MAGIC.len() {
            header.extend_from_slice(&buf[..n.min(SQLITE_MAGIC.len() - header.len())]);
        }
        hasher.update(&buf[..n]);
        sink.write_all(&buf[..n])?;
    }
    if total != expected.size {
        return Err(ArchiveError::SizeMismatch { name: DATABASE_MEMBER.to_owned(), expected: expected.size, actual: total });
    }
    if !hex(&hasher.finalize()).eq_ignore_ascii_case(&expected.sha256) {
        return Err(ArchiveError::ChecksumMismatch(DATABASE_MEMBER.to_owned()));
    }
    if header != SQLITE_MAGIC {
        return Err(ArchiveError::NotADatabase);
    }
    Ok(manifest)
}

/// Verifies the archive at `archive` and writes its database to `database_path` (a normal
/// `.ospot` file the app can open). Nothing is left at `database_path` on failure, and an
/// existing file there is only replaced once the new one is complete and verified.
pub fn import_project(archive: &Path, database_path: &Path) -> Result<Manifest, ArchiveError> {
    let partial = TempFile(database_path.with_extension("ospot.partial"));
    let manifest = {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&partial.0)?);
        let manifest = extract_database(std::io::BufReader::new(std::fs::File::open(archive)?), &mut out, Limits::default())?;
        out.flush()?;
        manifest
    };
    // Prove the database is usable before it replaces anything.
    Store::open(&partial.0)?.load()?;
    std::fs::rename(&partial.0, database_path)?;
    Ok(manifest)
}

#[cfg(test)]
mod tests;
