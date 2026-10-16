use std::io::Cursor;

use super::*;
use crate::testdata::{TempDir, project};

const APP: &str = "9.9.9";
const CREATED: i64 = 1_792_152_000;

/// A fake database that passes the SQLite magic check.
fn fake_db(len: usize) -> Vec<u8> {
    let mut v = SQLITE_MAGIC.to_vec();
    v.extend((0..len).map(|i| (i % 251) as u8));
    v
}

fn sha(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn manifest_for(db: &[u8]) -> Manifest {
    Manifest {
        format_version: FORMAT_VERSION,
        app_version: APP.into(),
        created_unix: CREATED,
        members: vec![Member { name: DATABASE_MEMBER.into(), size: db.len() as u64, sha256: sha(db) }],
    }
}

/// Builds a zip with arbitrary entry names and contents (stored, so payload bytes are visible).
fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn good_entries(db: &[u8]) -> (String, Vec<u8>) {
    (serde_json::to_string(&manifest_for(db)).unwrap(), db.to_vec())
}

fn extract(bytes: &[u8]) -> (Result<Manifest, ArchiveError>, Vec<u8>) {
    let mut sink = Vec::new();
    let r = extract_database(Cursor::new(bytes), &mut sink, Limits::default());
    (r, sink)
}

#[test]
fn project_round_trips_through_an_archive() {
    let p = project();
    let db = database_bytes(&p).unwrap();
    let mut out = Cursor::new(Vec::new());
    let written = write_archive(&db, &mut out, APP, CREATED).unwrap();
    let (res, extracted) = extract(&out.into_inner());
    let manifest = res.unwrap();
    assert_eq!(manifest, written);
    assert_eq!((manifest.format_version, manifest.app_version.as_str(), manifest.created_unix), (1, APP, CREATED));
    assert_eq!(manifest.members.len(), 1);
    assert_eq!(manifest.members[0].size, db.len() as u64);
    assert_eq!(manifest.members[0].sha256, sha(&extracted));
    assert_eq!(extracted, db);

    // And the extracted bytes really are the project.
    let dir = TempDir::new();
    let path = dir.path().join("p.ospot");
    std::fs::write(&path, &extracted).unwrap();
    let loaded = Store::open(&path).unwrap().load().unwrap();
    assert_eq!(loaded, p);
}

#[test]
fn file_export_and_import_round_trip_and_leave_no_temp_files() {
    let p = project();
    let dir = TempDir::new();
    let archive = dir.path().join(format!("p.{ARCHIVE_EXTENSION}"));
    export_project(&p, &archive, APP, CREATED).unwrap();
    let restored = dir.path().join("restored.ospot");
    let manifest = import_project(&archive, &restored).unwrap();
    assert_eq!(manifest.app_version, APP);
    assert_eq!(Store::open(&restored).unwrap().load().unwrap(), p);
    let mut names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, vec!["p.ospotz", "restored.ospot"]);
}

#[test]
fn identical_input_gives_identical_archives() {
    let db = fake_db(5000);
    let build = || {
        let mut out = Cursor::new(Vec::new());
        write_archive(&db, &mut out, APP, CREATED).unwrap();
        out.into_inner()
    };
    assert_eq!(build(), build());
}

#[test]
fn flipping_a_payload_byte_is_detected() {
    let db = fake_db(4000);
    let (manifest, payload) = good_entries(&db);
    let mut bytes = zip_with(&[(MANIFEST_MEMBER, manifest.as_bytes()), (DATABASE_MEMBER, &payload)]);
    // Stored entries sit verbatim in the file; find the payload and damage one byte.
    let at = bytes.windows(payload.len()).position(|w| w == payload).unwrap() + 2000;
    bytes[at] ^= 0x40;
    let (res, _) = extract(&bytes);
    assert!(matches!(res, Err(ArchiveError::Corrupt(ref n)) if n == DATABASE_MEMBER), "{res:?}");
}

#[test]
fn consistent_zip_with_modified_database_fails_the_manifest_checksum() {
    // Someone rebuilt the zip (valid CRCs) around different database bytes but kept the
    // original manifest: only the SHA-256 can catch that.
    let original = fake_db(4000);
    let mut tampered = original.clone();
    tampered[1000] ^= 1;
    let bytes = zip_with(&[(MANIFEST_MEMBER, manifest_for(&original).to_json().as_bytes()), (DATABASE_MEMBER, &tampered)]);
    let (res, _) = extract(&bytes);
    assert!(matches!(res, Err(ArchiveError::ChecksumMismatch(ref n)) if n == DATABASE_MEMBER), "{res:?}");
}

#[test]
fn size_mismatch_is_reported_with_both_numbers() {
    let db = fake_db(100);
    let mut m = manifest_for(&db);
    m.members[0].size = 50;
    let bytes = zip_with(&[(MANIFEST_MEMBER, m.to_json().as_bytes()), (DATABASE_MEMBER, &db)]);
    let (res, _) = extract(&bytes);
    assert!(matches!(res, Err(ArchiveError::SizeMismatch { expected: 50, actual: 116, .. })), "{res:?}");
}

impl Manifest {
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[test]
fn zip_slip_and_other_unsafe_entry_names_are_rejected_before_any_data_is_written() {
    let db = fake_db(100);
    let (manifest, payload) = good_entries(&db);
    for evil in ["../evil.ospot", "../../etc/passwd", "/abs/path", "dir/../../x", "a\\b", "..\\win", "C:/windows/x", "C:evil", "./project.ospot", "a//b", "x\0y", "dir/", "..", "."] {
        let bytes = zip_with(&[(MANIFEST_MEMBER, manifest.as_bytes()), (evil, b"pwned"), (DATABASE_MEMBER, &payload)]);
        let (res, sink) = extract(&bytes);
        assert!(matches!(res, Err(ArchiveError::UnsafePath(ref n)) if n == evil), "{evil:?} -> {res:?}");
        assert!(sink.is_empty(), "{evil:?}: nothing may be extracted");
    }
}

#[test]
fn unsafe_member_name_in_the_manifest_is_rejected() {
    let db = fake_db(100);
    let mut m = manifest_for(&db);
    m.members[0].name = "../project.ospot".into();
    let bytes = zip_with(&[(MANIFEST_MEMBER, m.to_json().as_bytes()), (DATABASE_MEMBER, &db)]);
    assert!(matches!(extract(&bytes).0, Err(ArchiveError::UnsafePath(_))));
    m.members[0].name = "other.bin".into();
    let bytes = zip_with(&[(MANIFEST_MEMBER, m.to_json().as_bytes()), (DATABASE_MEMBER, &db)]);
    assert!(matches!(extract(&bytes).0, Err(ArchiveError::UnexpectedEntry(_))));
}

#[test]
fn unexpected_forged_duplicate_and_excessive_entries_are_handled() {
    let db = fake_db(100);
    let (manifest, payload) = good_entries(&db);
    let extra = zip_with(&[(MANIFEST_MEMBER, manifest.as_bytes()), (DATABASE_MEMBER, &payload), ("readme.txt", b"hi")]);
    assert!(matches!(extract(&extra).0, Err(ArchiveError::UnexpectedEntry(ref n)) if n == "readme.txt"));

    // The zip writer refuses duplicate names, so forge one by patching a same-length name.
    // The reader collapses duplicates into one entry; whichever it picks must still pass
    // the manifest check, so a smuggled second copy can never be imported unverified.
    let mut tampered = payload.clone();
    tampered[60] ^= 1;
    let mut dup = zip_with(&[(MANIFEST_MEMBER, manifest.as_bytes()), (DATABASE_MEMBER, &payload), ("project.ospoX", &tampered)]);
    let from = b"project.ospoX";
    for i in 0..dup.len() - from.len() {
        if &dup[i..i + from.len()] == from {
            dup[i..i + from.len()].copy_from_slice(DATABASE_MEMBER.as_bytes());
        }
    }
    let (res, sink) = extract(&dup);
    match res {
        Ok(m) => assert_eq!(sha(&sink), m.members[0].sha256, "an accepted archive delivers exactly the listed bytes"),
        Err(e) => assert!(matches!(e, ArchiveError::ChecksumMismatch(_) | ArchiveError::NotAnArchive(_) | ArchiveError::Corrupt(_)), "{e:?}"),
    }
    let names: Vec<String> = (0..=MAX_ENTRIES).map(|i| format!("f{i}")).collect();
    let many: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
    assert!(matches!(extract(&zip_with(&many)).0, Err(ArchiveError::TooManyEntries { .. })));
}

#[test]
fn oversized_databases_are_refused_by_declared_and_actual_size() {
    let db = fake_db(1000);
    let (manifest, payload) = good_entries(&db);
    let bytes = zip_with(&[(MANIFEST_MEMBER, manifest.as_bytes()), (DATABASE_MEMBER, &payload)]);
    let mut sink = Vec::new();
    let res = extract_database(Cursor::new(&bytes), &mut sink, Limits { max_database_bytes: 100 });
    assert!(matches!(res, Err(ArchiveError::TooLarge { limit: 100, .. })), "{res:?}");
    assert!(sink.is_empty());

    // A manifest that understates the size does not get past the zip's own length either.
    let mut lying = manifest_for(&db);
    lying.members[0].size = 10;
    let bytes = zip_with(&[(MANIFEST_MEMBER, lying.to_json().as_bytes()), (DATABASE_MEMBER, &payload)]);
    let res = extract_database(Cursor::new(&bytes), &mut Vec::new(), Limits { max_database_bytes: 100 });
    assert!(matches!(res, Err(ArchiveError::TooLarge { .. })), "{res:?}");

    // An oversized manifest is bounded too.
    let huge = vec![b' '; (MAX_MANIFEST_BYTES + 10) as usize];
    let bytes = zip_with(&[(MANIFEST_MEMBER, &huge), (DATABASE_MEMBER, &payload)]);
    assert!(matches!(extract(&bytes).0, Err(ArchiveError::TooLarge { ref name, .. }) if name == MANIFEST_MEMBER));
}

#[test]
fn bad_manifests_and_missing_members() {
    let db = fake_db(100);
    let m = manifest_for(&db);
    let version = |v: u32| Manifest { format_version: v, ..m.clone() }.to_json();
    for (v, ok) in [(0, false), (1, true), (2, false)] {
        let bytes = zip_with(&[(MANIFEST_MEMBER, version(v).as_bytes()), (DATABASE_MEMBER, &db)]);
        let res = extract(&bytes).0;
        if ok {
            assert!(res.is_ok());
        } else {
            assert!(matches!(res, Err(ArchiveError::UnsupportedVersion { found, supported: 1 }) if found == v), "{res:?}");
        }
    }
    let bytes = zip_with(&[(MANIFEST_MEMBER, b"{ not json"), (DATABASE_MEMBER, &db)]);
    assert!(matches!(extract(&bytes).0, Err(ArchiveError::ManifestInvalid(_))));
    let bytes = zip_with(&[(DATABASE_MEMBER, &db)]);
    assert!(matches!(extract(&bytes).0, Err(ArchiveError::MissingMember(ref n)) if n == MANIFEST_MEMBER));
    let bytes = zip_with(&[(MANIFEST_MEMBER, m.to_json().as_bytes())]);
    assert!(matches!(extract(&bytes).0, Err(ArchiveError::MissingMember(ref n)) if n == DATABASE_MEMBER));
    let empty = Manifest { members: vec![], ..m.clone() };
    let bytes = zip_with(&[(MANIFEST_MEMBER, empty.to_json().as_bytes()), (DATABASE_MEMBER, &db)]);
    assert!(matches!(extract(&bytes).0, Err(ArchiveError::MissingMember(_))));
    let twice = Manifest { members: vec![m.members[0].clone(), m.members[0].clone()], ..m };
    let bytes = zip_with(&[(MANIFEST_MEMBER, twice.to_json().as_bytes()), (DATABASE_MEMBER, &db)]);
    assert!(matches!(extract(&bytes).0, Err(ArchiveError::ManifestInvalid(_))));
}

#[test]
fn non_archives_and_non_databases_are_rejected() {
    assert!(matches!(extract(b"definitely not a zip").0, Err(ArchiveError::NotAnArchive(_))));
    assert!(matches!(extract(b"").0, Err(ArchiveError::NotAnArchive(_))));
    let db = fake_db(100);
    let (manifest, payload) = good_entries(&db);
    let mut bytes = zip_with(&[(MANIFEST_MEMBER, manifest.as_bytes()), (DATABASE_MEMBER, &payload)]);
    bytes.truncate(bytes.len() - 30); // loses the central directory
    assert!(matches!(extract(&bytes).0, Err(ArchiveError::NotAnArchive(_))));

    let text = b"just some text, not SQLite".to_vec();
    let bytes = zip_with(&[(MANIFEST_MEMBER, manifest_for(&text).to_json().as_bytes()), (DATABASE_MEMBER, &text)]);
    assert!(matches!(extract(&bytes).0, Err(ArchiveError::NotADatabase)));
}

#[test]
fn failed_import_leaves_the_destination_untouched() {
    let dir = TempDir::new();
    let target = dir.path().join("keep.ospot");
    std::fs::write(&target, b"precious existing project").unwrap();
    let db = fake_db(100);
    let (manifest, payload) = good_entries(&db);
    let evil = dir.path().join("evil.ospotz");
    std::fs::write(&evil, zip_with(&[(MANIFEST_MEMBER, manifest.as_bytes()), ("../escape", b"x"), (DATABASE_MEMBER, &payload)])).unwrap();
    assert!(matches!(import_project(&evil, &target), Err(ArchiveError::UnsafePath(_))));
    assert_eq!(std::fs::read(&target).unwrap(), b"precious existing project");

    // A correct checksum over bytes that are not a usable database is caught by the load check.
    let bogus = dir.path().join("bogus.ospotz");
    std::fs::write(&bogus, zip_with(&[(MANIFEST_MEMBER, manifest.as_bytes()), (DATABASE_MEMBER, &payload)])).unwrap();
    assert!(import_project(&bogus, &target).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"precious existing project");

    let mut left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, vec!["bogus.ospotz", "evil.ospotz", "keep.ospot"], "no .partial files remain");
    assert!(matches!(import_project(&dir.path().join("missing.ospotz"), &dir.path().join("new.ospot")), Err(ArchiveError::Io(_))));
    assert!(!dir.path().join("new.ospot").exists());
}
