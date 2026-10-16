//! A project file given on the command line (what a double-click in the file manager does
//! once `.ospot` / `.ospotz` are associated with the app).

use std::path::{Path, PathBuf};

const PROJECT_EXT: &str = "ospot";

/// First argument that names a project file or archive. Flags (`--fake`) and unrelated
/// arguments are ignored; matching is case-insensitive because Windows file names are.
pub fn project_arg(args: impl IntoIterator<Item = String>) -> Option<PathBuf> {
    args.into_iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).find(|p| has_ext(p, PROJECT_EXT) || has_ext(p, openspot_report::ARCHIVE_EXTENSION))
}

fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// A sibling `.ospot` path for `archive` that does not exist yet: `name.ospot`, then
/// `name (imported 2).ospot`, ... Never overwrites an existing project.
pub fn unique_project_path(archive: &Path) -> PathBuf {
    let dir = archive.parent().unwrap_or_else(|| Path::new("."));
    let stem = archive.file_stem().map_or_else(|| "imported".to_owned(), |s| s.to_string_lossy().into_owned());
    let first = dir.join(format!("{stem}.{PROJECT_EXT}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} (imported {n}).{PROJECT_EXT}")))
        .find(|p| !p.exists())
        .unwrap_or(first) // unreachable: the range is unbounded
}

/// The project database to open: the file itself for `.ospot`, or, for an archive, the
/// result of importing it next to the archive (checksums verified by the importer).
pub fn resolve(path: &Path) -> Result<PathBuf, String> {
    if has_ext(path, openspot_report::ARCHIVE_EXTENSION) {
        let target = unique_project_path(path);
        openspot_report::import_project(path, &target).map_err(|e| format!("could not import {}: {e}", path.display()))?;
        Ok(target)
    } else if path.is_file() {
        Ok(path.to_path_buf())
    } else {
        Err(format!("{} does not exist", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openspot_model::Project;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openspot-openarg-{}", openspot_model::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn picks_the_first_project_looking_argument() {
        assert_eq!(project_arg(args(&["--fake", "C:\\x\\Plan.OSPOT"])), Some(PathBuf::from("C:\\x\\Plan.OSPOT")));
        assert_eq!(project_arg(args(&["a.txt", "b.ospotz", "c.ospot"])), Some(PathBuf::from("b.ospotz")));
        assert_eq!(project_arg(args(&["--fake"])), None);
        assert_eq!(project_arg(args(&["notes.txt", "ospot"])), None);
        assert_eq!(project_arg(args(&[])), None);
    }

    #[test]
    fn unique_path_never_overwrites() {
        let dir = temp_dir();
        let archive = dir.join("house.ospotz");
        assert_eq!(unique_project_path(&archive), dir.join("house.ospot"));
        std::fs::write(dir.join("house.ospot"), b"x").unwrap();
        assert_eq!(unique_project_path(&archive), dir.join("house (imported 2).ospot"));
        std::fs::write(dir.join("house (imported 2).ospot"), b"x").unwrap();
        assert_eq!(unique_project_path(&archive), dir.join("house (imported 3).ospot"));
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }

    #[test]
    fn resolves_projects_archives_and_missing_files() {
        let dir = temp_dir();
        // A plain project file resolves to itself.
        let db = dir.join("a.ospot");
        openspot_store::Store::open(&db).unwrap().save(&Project::new("A")).unwrap();
        assert_eq!(resolve(&db).unwrap(), db);

        // An archive is imported next to itself and then opens as a project.
        let archive = dir.join("b.ospotz");
        let project = Project::new("B");
        openspot_report::export_project(&project, &archive, "test", 0).unwrap();
        let imported = resolve(&archive).unwrap();
        assert_eq!(imported, dir.join("b.ospot"));
        assert_eq!(openspot_store::Store::open(&imported).unwrap().load().unwrap().name, "B");
        // Opening the same archive again does not clobber the first import.
        assert_eq!(resolve(&archive).unwrap(), dir.join("b (imported 2).ospot"));

        assert!(resolve(&dir.join("missing.ospot")).unwrap_err().contains("does not exist"));
        std::fs::write(dir.join("bad.ospotz"), b"not a zip").unwrap();
        assert!(resolve(&dir.join("bad.ospotz")).unwrap_err().contains("could not import"));
        let _ = std::fs::remove_dir_all(dir); // best effort: Windows may still hold a handle
    }
}
