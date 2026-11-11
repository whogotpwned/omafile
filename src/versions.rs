//! Earlier versions of files, from Btrfs snapshots made by snapper.
//!
//! Omarchy snapshots the system (`/`) so an update can be rolled back, and leaves
//! `/home` alone on purpose: rolling a home folder back would throw away work.
//! This goes the other way round: with a snapper configuration for the home
//! folder, every file keeps its history, and single files or folders are copied
//! back out of a snapshot. Nothing is ever rolled back.
//!
//! Reading needs no snapper at all: a configuration keeps its snapshots under
//! `<subvolume>/.snapshots/<number>/snapshot`, each with an `info.xml` beside it.
//! Everything here works on that layout, which also makes it testable anywhere.

use std::path::{Path, PathBuf};

/// The snapshots that cover a path: the subvolume they belong to and where they are.
#[derive(Clone, Debug, PartialEq)]
pub struct Store {
    pub root: PathBuf,
    pub snapshots: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub number: u32,
    /// When it was taken, seconds since 1970 (0 when unknown).
    pub date: i64,
    pub description: String,
    /// What the snapshot holds: the subvolume as it was.
    pub dir: PathBuf,
}

/// One distinct earlier state of a file or folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    /// The newest snapshot that holds this state.
    pub snapshot: Snapshot,
    pub path: PathBuf,
    pub size: u64,
    pub modified: i64,
    /// In how many snapshots this very state is kept.
    pub kept_in: usize,
    /// The same as what is there now.
    pub current: bool,
}

/// Something that was in a folder at the time of a snapshot and is gone now.
#[derive(Clone, Debug, PartialEq)]
pub struct Deleted {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// The newest snapshot that still has it.
    pub snapshot: Snapshot,
    pub path: PathBuf,
}

/// The snapshot store that covers `path`: the nearest folder above it with a readable
/// `.snapshots` folder inside.
pub fn store_for(path: &Path) -> Option<Store> {
    let mut dir = if path.is_dir() { Some(path) } else { path.parent() };
    while let Some(d) = dir {
        let snapshots = d.join(".snapshots");
        // Readable, not merely there: without snapper's ACL the folder belongs to root alone.
        if snapshots.is_dir() && std::fs::read_dir(&snapshots).is_ok() {
            return Some(Store { root: d.to_path_buf(), snapshots });
        }
        dir = d.parent();
    }
    None
}

/// A folder above `path` that has snapshots which this user may not read: versions exist,
/// but snapper was not told to let the user see them.
pub fn locked_store_for(path: &Path) -> Option<PathBuf> {
    let mut dir = if path.is_dir() { Some(path) } else { path.parent() };
    while let Some(d) = dir {
        let snapshots = d.join(".snapshots");
        if snapshots.is_dir() && std::fs::read_dir(&snapshots).is_err() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(xml[start..end].trim())
}

/// What snapper writes beside a snapshot: its date (UTC, "2026-11-11 12:00:00") and description.
pub fn parse_info(xml: &str) -> (i64, String) {
    // snapper writes UTC without saying so.
    let date = tag(xml, "date").map(|d| crate::feeds::parse_date(&format!("{}Z", d.replacen(' ', "T", 1)))).unwrap_or(0);
    let description = tag(xml, "description").map(crate::feeds::decode_entities).unwrap_or_default();
    (date, description)
}

/// Every snapshot of a store, newest first.
pub fn snapshots(store: &Store) -> Vec<Snapshot> {
    let mut out: Vec<Snapshot> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&store.snapshots) else { return out };
    for entry in entries.flatten() {
        let Some(number) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let dir = entry.path().join("snapshot");
        if !dir.is_dir() {
            continue;
        }
        let (date, description) = std::fs::read_to_string(entry.path().join("info.xml")).map(|x| parse_info(&x)).unwrap_or((0, String::new()));
        out.push(Snapshot { number, date, description, dir });
    }
    out.sort_by_key(|s| std::cmp::Reverse((s.date, s.number)));
    out
}

fn stat(path: &Path) -> Option<(u64, i64, bool)> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
    Some((meta.len(), modified, meta.is_dir()))
}

/// Where `path` is inside a snapshot.
pub fn in_snapshot(store: &Store, snapshot: &Snapshot, path: &Path) -> Option<PathBuf> {
    path.strip_prefix(&store.root).ok().map(|relative| snapshot.dir.join(relative))
}

/// The distinct earlier states of a file, newest first. A state is told apart by size and
/// modification time; snapshots that hold the same state are counted, not listed.
pub fn versions_of(store: &Store, path: &Path) -> Vec<Version> {
    let now = stat(path);
    let mut out: Vec<Version> = Vec::new();
    for snapshot in snapshots(store) {
        let Some(inside) = in_snapshot(store, &snapshot, path) else { continue };
        let Some((size, modified, is_dir)) = stat(&inside) else { continue };
        if is_dir {
            // A folder's own date says nothing about what is inside: every snapshot is a state
            // of its own, and they stay in the order of the snapshots.
            out.push(Version { snapshot, path: inside, size, modified, kept_in: 1, current: false });
            continue;
        }
        match out.iter_mut().find(|v| v.modified == modified && v.size == size) {
            Some(v) => v.kept_in += 1,
            None => {
                let current = now.map(|(s, m, _)| m == modified && s == size).unwrap_or(false);
                out.push(Version { snapshot, path: inside, size, modified, kept_in: 1, current });
            }
        }
    }
    if !path.is_dir() {
        out.sort_by_key(|v| std::cmp::Reverse(v.modified));
    }
    out
}

/// What was in `dir` in some snapshot and is not there now, newest loss first.
pub fn deleted_in(store: &Store, dir: &Path) -> Vec<Deleted> {
    let present: std::collections::HashSet<std::ffi::OsString> = std::fs::read_dir(dir).map(|entries| entries.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
    let mut out: Vec<Deleted> = Vec::new();
    for snapshot in snapshots(store) {
        let Some(inside) = in_snapshot(store, &snapshot, dir) else { continue };
        let Ok(entries) = std::fs::read_dir(&inside) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if present.contains(&name) || name == ".snapshots" {
                continue;
            }
            let name = name.to_string_lossy().to_string();
            // Snapshots come newest first: the first one that has it is the one to restore from.
            if out.iter().any(|d| d.name == name) {
                continue;
            }
            let (size, _, is_dir) = stat(&entry.path()).unwrap_or((0, 0, false));
            out.push(Deleted { name, is_dir, size, snapshot: snapshot.clone(), path: entry.path() });
        }
    }
    out
}

/// A name for a restored copy that does not collide: "report (11 Nov 14:30).md".
pub fn copy_name(original: &Path, label: &str) -> PathBuf {
    let dir = original.parent().unwrap_or(Path::new("/"));
    let stem = original.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = original.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let label = label.replace(['/', ':'], ".");
    let mut candidate = dir.join(format!("{stem} ({label}){ext}"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem} ({label}) {n}{ext}"));
        n += 1;
    }
    candidate
}

// ----- turning it on ---------------------------------------------------------------------

/// Why versions cannot be offered for a path, or how they could be.
#[derive(Debug, PartialEq)]
pub enum Readiness {
    /// Snapshots are there and readable.
    Ready,
    /// Snapshots exist above this path but this user may not read them.
    Locked(PathBuf),
    /// A Btrfs subvolume without snapshots: `setup_script` turns them on.
    CanSetUp(PathBuf),
    /// Not on Btrfs, or snapper is missing: (what is wrong).
    Unavailable(String),
}

fn filesystem_of(path: &Path) -> String {
    std::process::Command::new("stat").args(["-f", "-c", "%T"]).arg(path).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// The Btrfs subvolume a path lives in: the nearest folder above it whose inode number is
/// 256, which is what every subvolume root has.
fn subvolume_of(path: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let mut dir = if path.is_dir() { Some(path) } else { path.parent() };
    while let Some(d) = dir {
        if std::fs::metadata(d).map(|m| m.ino() == 256).unwrap_or(false) {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

pub fn readiness(path: &Path) -> Readiness {
    if store_for(path).is_some() {
        return Readiness::Ready;
    }
    // For looking at the setup dialog on a machine without Btrfs (screenshots, translations).
    if std::env::var_os("OMAFILE_VERSIONS_PREVIEW_SETUP").is_some() {
        return Readiness::CanSetUp(gtk::glib::home_dir());
    }
    if let Some(root) = locked_store_for(path) {
        return Readiness::Locked(root);
    }
    if filesystem_of(path) != "btrfs" {
        return Readiness::Unavailable("not-btrfs".into());
    }
    if gtk::glib::find_program_in_path("snapper").is_none() {
        return Readiness::Unavailable("no-snapper".into());
    }
    match subvolume_of(path) {
        Some(root) => Readiness::CanSetUp(root),
        None => Readiness::Unavailable("no-subvolume".into()),
    }
}

/// A configuration name snapper accepts for a subvolume: "home" for /home, else from the path.
pub fn config_name(subvolume: &Path) -> String {
    let name: String = subvolume.to_string_lossy().trim_matches('/').chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    if name.is_empty() {
        "root".into()
    } else {
        name
    }
}

/// The commands that turn versions on for a subvolume, as a script a person can read
/// before giving their password: an hourly timeline, pruned by snapper itself, readable
/// by this user. `locked`: the configuration exists and only the permission is missing.
pub fn setup_script(subvolume: &Path, user: &str, locked: bool) -> String {
    let name = config_name(subvolume);
    // Inside single quotes in the script: a quote in a folder name must not end them.
    let quoted = |text: &str| text.replace('\'', "'\\''");
    let dir = quoted(&subvolume.to_string_lossy());
    let user = quoted(user);
    let mut script = String::from("#!/bin/bash\nset -e\n");
    script.push_str(&format!("echo 'omafile: turning on versions for {dir}'\necho\n"));
    if !locked {
        script.push_str(&format!("sudo snapper -c {name} create-config '{dir}'\n"));
    }
    script.push_str(&format!(
        "sudo snapper -c {name} set-config ALLOW_USERS='{user}' SYNC_ACL=yes TIMELINE_CREATE=yes TIMELINE_CLEANUP=yes TIMELINE_LIMIT_HOURLY=24 TIMELINE_LIMIT_DAILY=14 TIMELINE_LIMIT_WEEKLY=4 TIMELINE_LIMIT_MONTHLY=6 TIMELINE_LIMIT_YEARLY=0 NUMBER_CLEANUP=yes NUMBER_LIMIT=20\n"
    ));
    script.push_str(&format!("sudo chmod a+rx '{dir}/.snapshots'\n"));
    script.push_str("sudo systemctl enable --now snapper-timeline.timer snapper-cleanup.timer\n");
    script.push_str(&format!("snapper -c {name} create --description 'omafile: versions turned on' || sudo snapper -c {name} create --description 'omafile: versions turned on'\n"));
    script.push_str("echo\necho 'Done. A snapshot is taken every hour; old ones are pruned automatically.'\n");
    script
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn touch(path: &Path, content: &str, modified: u64) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(modified)).unwrap();
    }

    fn snapshot(root: &Path, number: u32, date: &str, description: &str) -> PathBuf {
        let dir = root.join(".snapshots").join(number.to_string());
        std::fs::create_dir_all(dir.join("snapshot")).unwrap();
        std::fs::write(dir.join("info.xml"), format!("<?xml version=\"1.0\"?>\n<snapshot>\n  <type>single</type>\n  <num>{number}</num>\n  <date>{date}</date>\n  <description>{description}</description>\n  <cleanup>timeline</cleanup>\n</snapshot>\n")).unwrap();
        dir.join("snapshot")
    }

    #[test]
    fn reads_what_snapper_writes() {
        let (date, description) = parse_info("<snapshot><type>single</type><num>7</num><date>2026-11-10 13:30:00</date><description>before &amp; after</description></snapshot>");
        assert_eq!(date, crate::feeds::parse_date("2026-11-10T13:30:00Z"));
        assert_eq!(description, "before & after");
        assert_eq!(parse_info("<snapshot></snapshot>"), (0, String::new()));
    }

    #[test]
    fn finds_versions_and_deleted_files() {
        let root = std::env::temp_dir().join(format!("omafile-versions-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // Three snapshots: the report changes between 1 and 2, stays the same in 3; notes.txt is deleted after 2.
        let s1 = snapshot(&root, 1, "2026-11-09 08:00:00", "timeline");
        let s2 = snapshot(&root, 2, "2026-11-10 08:00:00", "timeline");
        let s3 = snapshot(&root, 3, "2026-11-11 08:00:00", "timeline");
        touch(&s1.join("docs/report.md"), "first draft", 1_000);
        touch(&s2.join("docs/report.md"), "second draft, longer", 2_000);
        touch(&s3.join("docs/report.md"), "second draft, longer", 2_000);
        touch(&s1.join("docs/notes.txt"), "old notes", 500);
        touch(&s2.join("docs/notes.txt"), "newer notes", 900);
        std::fs::create_dir_all(s2.join("docs/old-folder")).unwrap();
        // Now: the report was edited again, notes.txt and old-folder are gone.
        touch(&root.join("docs/report.md"), "third draft", 3_000);
        touch(&root.join("docs/keep.txt"), "x", 100);

        let store = store_for(&root.join("docs/report.md")).unwrap();
        assert_eq!(store, Store { root: root.clone(), snapshots: root.join(".snapshots") });
        assert_eq!(store_for(&root.join("docs")), Some(store.clone()));
        let all = snapshots(&store);
        assert_eq!(all.iter().map(|s| s.number).collect::<Vec<_>>(), vec![3, 2, 1], "newest first");

        let versions = versions_of(&store, &root.join("docs/report.md"));
        assert_eq!(versions.len(), 2, "two distinct states, not three snapshots");
        assert_eq!((versions[0].modified, versions[0].kept_in, versions[0].snapshot.number, versions[0].current), (2_000, 2, 3, false));
        assert_eq!((versions[1].modified, versions[1].kept_in, versions[1].snapshot.number), (1_000, 1, 1));
        assert_eq!(std::fs::read_to_string(&versions[1].path).unwrap(), "first draft");
        // A file that has not changed since the snapshot is marked as the current one.
        touch(&root.join("docs/report.md"), "second draft, longer", 2_000);
        assert!(versions_of(&store, &root.join("docs/report.md"))[0].current);
        assert!(versions_of(&store, &root.join("docs/never-existed.md")).is_empty());

        // A folder is listed once per snapshot that has it, newest first.
        let folder: Vec<u32> = versions_of(&store, &root.join("docs")).iter().map(|v| v.snapshot.number).collect();
        assert_eq!(folder, vec![3, 2, 1]);

        let gone = deleted_in(&store, &root.join("docs"));
        let names: Vec<(&str, bool, u32)> = gone.iter().map(|d| (d.name.as_str(), d.is_dir, d.snapshot.number)).collect();
        assert!(names.contains(&("notes.txt", false, 2)), "the newest snapshot that has it: {names:?}");
        assert!(names.contains(&("old-folder", true, 2)));
        assert_eq!(gone.len(), 2, "what is still there is not listed: {names:?}");
        assert_eq!(std::fs::read_to_string(&gone.iter().find(|d| d.name == "notes.txt").unwrap().path).unwrap(), "newer notes");

        // The snapshot folder of the subvolume itself never counts as deleted.
        assert!(deleted_in(&store, &root).iter().all(|d| d.name != ".snapshots"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn restored_copies_get_a_free_name() {
        let dir = std::env::temp_dir().join(format!("omafile-versions-name-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("report.md");
        let first = copy_name(&original, "11 Nov 14:30");
        assert_eq!(first, dir.join("report (11 Nov 14.30).md"));
        std::fs::write(&first, "x").unwrap();
        assert_eq!(copy_name(&original, "11 Nov 14:30"), dir.join("report (11 Nov 14.30) 2.md"));
        assert_eq!(copy_name(&dir.join("Makefile"), "yesterday"), dir.join("Makefile (yesterday)"));
        std::fs::remove_dir_all(&dir).unwrap();
        let _ = SystemTime::now();
    }

    #[test]
    fn the_setup_script_says_what_it_does() {
        assert_eq!(config_name(Path::new("/home")), "home");
        assert_eq!(config_name(Path::new("/")), "root");
        assert_eq!(config_name(Path::new("/mnt/data disk")), "mnt_data_disk");
        let script = setup_script(Path::new("/home"), "nora", false);
        assert!(script.contains("sudo snapper -c home create-config '/home'"));
        assert!(script.contains("ALLOW_USERS='nora'") && script.contains("SYNC_ACL=yes") && script.contains("TIMELINE_CREATE=yes"));
        assert!(script.contains("snapper-timeline.timer snapper-cleanup.timer"));
        assert!(script.starts_with("#!/bin/bash\nset -e\n"), "stops at the first failure");
        // Only the permission is missing: the configuration is not created a second time.
        assert!(!setup_script(Path::new("/home"), "nora", true).contains("create-config"));
        assert!(setup_script(Path::new("/mnt/it's"), "nora", false).contains("create-config '/mnt/it'\\''s'"), "a quote cannot break out");
    }
}
