//! Backups to another drive, Time Machine style.
//!
//! Every run makes a complete, browsable copy of the chosen folders in a new
//! folder on the destination; files that did not change since the run before are
//! hard links to it and take no space. Old backups thin out by themselves: all
//! of the last day, one a day for a month, one a week beyond that.
//!
//! The layout is the one snapper uses (`.snapshots/<number>/snapshot` with an
//! `info.xml` beside it), so "Earlier Versions" reads backups exactly like it
//! reads Btrfs snapshots. No rsync, no other tool: the copying is done here,
//! with nothing but the standard library, so `omafile --backup` runs without a
//! window from a systemd timer.
//!
//! Where this stands next to Omarchy: Omarchy snapshots the system (snapper,
//! root only, before every update, a handful kept, on the same disk) and leaves
//! `/home` alone on purpose. This is the other half: the user's own files, on
//! another drive, with a history. The two do not touch: snapshot folders are
//! never copied, nothing here calls snapper, and the page says who does what.

use crate::i18n::tr;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ----- what is set up --------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Schedule {
    Manual,
    Hourly,
    Daily,
    Weekly,
}

impl Schedule {
    pub fn id(self) -> &'static str {
        match self {
            Schedule::Manual => "manual",
            Schedule::Hourly => "hourly",
            Schedule::Daily => "daily",
            Schedule::Weekly => "weekly",
        }
    }

    pub fn from_id(id: &str) -> Schedule {
        match id {
            "hourly" => Schedule::Hourly,
            "daily" => Schedule::Daily,
            "weekly" => Schedule::Weekly,
            _ => Schedule::Manual,
        }
    }

    /// Seconds between two runs; None for manual.
    pub fn interval(self) -> Option<i64> {
        match self {
            Schedule::Manual => None,
            Schedule::Hourly => Some(3600),
            Schedule::Daily => Some(86400),
            Schedule::Weekly => Some(7 * 86400),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub schedule: Schedule,
    /// The folder on the other drive, as it was when it was chosen.
    pub dest: PathBuf,
    /// The file system it is on, to find the drive again wherever it gets mounted.
    pub uuid: String,
    /// `dest` relative to the mount point of that file system.
    pub subdir: String,
    pub skip_caches: bool,
    pub sources: Vec<PathBuf>,
    /// Names (with `*`) or absolute paths to leave out.
    pub excludes: Vec<String>,
    /// Leave out hidden files and folders (names that start with a dot) at the top of each
    /// source: the settings of programs, caches, keys. What lies deeper is copied.
    pub skip_hidden: bool,
    /// Daily and weekly backups: not before this hour of the day (local time).
    pub hour: u8,
    /// Weekly backups: the day, Monday = 0.
    pub weekday: u8,
    /// Backups older than this many days are deleted; 0 keeps them as long as there is room.
    pub keep_days: u32,
    /// Delete the oldest backups before a run when the drive is nearly full.
    pub make_room: bool,
    /// Automatic backups wait while the computer runs on battery.
    pub ac_only: bool,
    /// A notification when a backup has finished (a failed one is always reported).
    pub notify_done: bool,
    /// Note the installed packages next to each backup.
    pub package_list: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { schedule: Schedule::Daily, dest: PathBuf::new(), uuid: String::new(), subdir: String::new(), skip_caches: true, sources: Vec::new(), excludes: Vec::new(), skip_hidden: false, hour: 12, weekday: 0, keep_days: 0, make_room: true, ac_only: false, notify_done: false, package_list: true }
    }
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let mut c = Config::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "schedule" => c.schedule = Schedule::from_id(value),
                "dest" => c.dest = PathBuf::from(value),
                "uuid" => c.uuid = value.to_string(),
                "subdir" => c.subdir = value.to_string(),
                "skip_caches" => c.skip_caches = value != "false",
                "hour" => c.hour = value.parse::<u8>().unwrap_or(12).min(23),
                "weekday" => c.weekday = value.parse::<u8>().unwrap_or(0).min(6),
                "keep_days" => c.keep_days = value.parse().unwrap_or(0),
                "make_room" => c.make_room = value != "false",
                "ac_only" => c.ac_only = value == "true",
                "notify_done" => c.notify_done = value == "true",
                "package_list" => c.package_list = value != "false",
                "source" if !value.is_empty() => c.sources.push(PathBuf::from(value)),
                "exclude" if !value.is_empty() => c.excludes.push(value.to_string()),
                "skip_hidden" => c.skip_hidden = value == "true",
                _ => {}
            }
        }
        c
    }

    pub fn serialize(&self) -> String {
        let mut out = format!("schedule={}\ndest={}\nuuid={}\nsubdir={}\nskip_caches={}\n", self.schedule.id(), self.dest.display(), self.uuid, self.subdir, self.skip_caches);
        out.push_str(&format!("hour={}\nweekday={}\nkeep_days={}\nmake_room={}\nac_only={}\nnotify_done={}\npackage_list={}\n", self.hour, self.weekday, self.keep_days, self.make_room, self.ac_only, self.notify_done, self.package_list));
        for s in &self.sources {
            out.push_str(&format!("source={}\n", s.display()));
        }
        for e in &self.excludes {
            out.push_str(&format!("exclude={e}\n"));
        }
        out.push_str(&format!("skip_hidden={}\n", self.skip_hidden));
        out
    }

    pub fn path() -> PathBuf {
        config_dir().join("backup")
    }

    pub fn load() -> Config {
        std::fs::read_to_string(Self::path()).map(|t| Config::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, self.serialize());
    }

    pub fn is_set_up(&self) -> bool {
        !self.dest.as_os_str().is_empty() && !self.sources.is_empty()
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("omafile")
}

fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/state")).join("omafile")
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn host_and_user() -> String {
    let host = std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).unwrap_or_default();
    let user = std::env::var("USER").unwrap_or_default();
    let clean = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect::<String>();
    format!("omafile-{}-{}", if host.is_empty() { "computer".to_string() } else { clean(&host) }, if user.is_empty() { "user".to_string() } else { clean(&user) })
}

/// Where this computer's backups live on the destination: one folder per computer and user,
/// so one drive can serve several.
pub fn store_dir(dest: &Path) -> PathBuf {
    dest.join(host_and_user())
}

// ----- what to leave out -----------------------------------------------------------------

/// `*` matches any run of characters; everything else must match exactly.
pub fn glob(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let mut rest = name;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    true
}

const CACHE_NAMES: &[&str] = &[".cache", "node_modules", "__pycache__", "lost+found", ".Trash-1000", ".thumbnails"];

/// Whether a file or folder stays out of the backup.
pub fn excluded(path: &Path, is_dir: bool, config: &Config) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    for pattern in &config.excludes {
        let pattern = pattern.trim();
        let absolute = pattern.strip_prefix("~/").map(|rest| home().join(rest)).or_else(|| pattern.starts_with('/').then(|| PathBuf::from(pattern)));
        match absolute {
            Some(p) => {
                if path.starts_with(&p) {
                    return true;
                }
            }
            None => {
                if glob(pattern, &name) {
                    return true;
                }
            }
        }
    }
    // Snapshots (snapper's, so Omarchy's, and other backups in this layout) are never copied,
    // whatever the settings say: each one is a whole copy of the folder it sits in.
    if is_dir && name == ".snapshots" {
        return true;
    }
    if config.skip_hidden && name.starts_with('.') && config.sources.iter().any(|source| path.parent() == Some(source.as_path())) {
        return true;
    }
    if config.skip_caches && is_dir {
        if CACHE_NAMES.contains(&name.as_str()) {
            return true;
        }
        // The trash, and anything a program has marked as a cache (the CACHEDIR.TAG convention,
        // which Rust's target folders follow).
        if path.ends_with(".local/share/Trash") || path.join("CACHEDIR.TAG").is_file() {
            return true;
        }
    }
    false
}

// ----- the backups on the destination ----------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub number: u32,
    pub date: i64,
    pub dir: PathBuf,
    pub files: u64,
    pub changed: u64,
    pub bytes: u64,
    pub seconds: u64,
    pub errors: u64,
}

fn number_tag(xml: &str, name: &str) -> u64 {
    crate::versions::tag(xml, name).and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// Every finished backup in a store, newest first.
pub fn entries(store: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(dirs) = std::fs::read_dir(store.join(".snapshots")) else { return out };
    for dir in dirs.flatten() {
        let Some(number) = dir.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let Ok(xml) = std::fs::read_to_string(dir.path().join("info.xml")) else { continue };
        let (date, _) = crate::versions::parse_info(&xml);
        out.push(Entry { number, date, dir: dir.path().join("snapshot"), files: number_tag(&xml, "files"), changed: number_tag(&xml, "changed"), bytes: number_tag(&xml, "bytes"), seconds: number_tag(&xml, "seconds"), errors: number_tag(&xml, "errors") });
    }
    out.sort_by_key(|e| std::cmp::Reverse(e.number));
    out
}

/// Which backups to delete so that all of the last day stay, then the newest of each day
/// for thirty days, then the newest of each week. With `keep_days`, nothing older than that
/// stays. The newest three always stay.
pub fn to_prune(backups: &[(u32, i64)], now: i64, keep_days: u32) -> Vec<u32> {
    let mut sorted: Vec<(u32, i64)> = backups.to_vec();
    sorted.sort_by_key(|(_, date)| std::cmp::Reverse(*date));
    let mut keep_slots: std::collections::HashSet<(u8, i64)> = std::collections::HashSet::new();
    let mut prune = Vec::new();
    for (i, (number, date)) in sorted.iter().enumerate() {
        let age = now - date;
        if i >= 3 && keep_days > 0 && age > keep_days as i64 * 86400 {
            prune.push(*number);
            continue;
        }
        let slot = if i < 3 || age < 86400 {
            None
        } else if age < 30 * 86400 {
            Some((1u8, date.div_euclid(86400)))
        } else {
            Some((2u8, date.div_euclid(7 * 86400)))
        };
        if let Some(slot) = slot {
            // Newest first: the first backup seen in a slot is the one that stays.
            if !keep_slots.insert(slot) {
                prune.push(*number);
            }
        }
    }
    prune
}

/// Delete one backup (the folder that holds `snapshot` and `info.xml`).
pub fn remove(entry_dir: &Path) -> std::io::Result<()> {
    let holder = entry_dir.parent().unwrap_or(entry_dir);
    std::fs::remove_dir_all(holder)
}

// ----- the drive --------------------------------------------------------------------------

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The file system a path is on: (uuid, mount point, type, device).
pub fn mount_info(path: &Path) -> Option<(String, PathBuf, String, String)> {
    let line = command_output("findmnt", &["-rno", "UUID,TARGET,FSTYPE,SOURCE", "--target", &path.to_string_lossy()])?;
    parse_findmnt(&line)
}

/// One line of `findmnt -rno UUID,TARGET,FSTYPE,SOURCE` (spaces inside a field come as \x20).
pub fn parse_findmnt(line: &str) -> Option<(String, PathBuf, String, String)> {
    let fields: Vec<String> = line.lines().next()?.split(' ').map(|f| f.replace("\\x20", " ")).collect();
    match fields.as_slice() {
        [uuid, target, fstype, source] => Some((uuid.clone(), PathBuf::from(target), fstype.clone(), source.clone())),
        // No UUID (tmpfs, overlay): three fields.
        [target, fstype, source] => Some((String::new(), PathBuf::from(target), fstype.clone(), source.clone())),
        _ => None,
    }
}

/// Free and total bytes of the file system a path is on.
pub fn space(path: &Path) -> Option<(u64, u64)> {
    let out = command_output("df", &["-B1", "--output=avail,size", &path.to_string_lossy()])?;
    let mut numbers = out.lines().nth(1)?.split_whitespace().filter_map(|n| n.parse::<u64>().ok());
    Some((numbers.next()?, numbers.next()?))
}

/// Whether a folder's file system can hard-link, which is what makes many backups cheap.
/// FAT and exFAT cannot: every backup there is a full copy.
pub fn supports_hard_links(dir: &Path) -> bool {
    let a = dir.join(format!(".omafile-linktest-{}", std::process::id()));
    let b = dir.join(format!(".omafile-linktest-{}-2", std::process::id()));
    let ok = std::fs::write(&a, b"x").is_ok() && std::fs::hard_link(&a, &b).is_ok();
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    ok
}

#[derive(Debug, PartialEq)]
pub enum Destination {
    Ready(PathBuf),
    /// The drive is not plugged in (or not mounted).
    NotConnected,
    NotSetUp,
}

/// Find the destination folder: where it was, or wherever its file system is mounted now.
pub fn destination(config: &Config) -> Destination {
    if config.dest.as_os_str().is_empty() {
        return Destination::NotSetUp;
    }
    if !config.uuid.is_empty() {
        // The same drive may come back under another mount point.
        if let Some(target) = command_output("findmnt", &["-rno", "TARGET", "-S", &format!("UUID={}", config.uuid)]).and_then(|t| t.lines().next().map(|l| l.replace("\\x20", " "))) {
            let dir = if config.subdir.is_empty() { PathBuf::from(target) } else { PathBuf::from(target).join(&config.subdir) };
            return if dir.is_dir() || std::fs::create_dir_all(&dir).is_ok() { Destination::Ready(dir) } else { Destination::NotConnected };
        }
        return Destination::NotConnected;
    }
    if config.dest.is_dir() {
        Destination::Ready(config.dest.clone())
    } else {
        Destination::NotConnected
    }
}

/// Mount the destination's file system through udisks (no password needed for removable
/// drives). True when this call mounted it, so the caller can unmount it again afterwards.
pub fn try_mount(config: &Config) -> bool {
    if config.uuid.is_empty() {
        return false;
    }
    let device = format!("/dev/disk/by-uuid/{}", config.uuid);
    Path::new(&device).exists() && command_output("udisksctl", &["mount", "--no-user-interaction", "-b", &device]).is_some()
}

pub fn unmount(config: &Config) {
    let device = format!("/dev/disk/by-uuid/{}", config.uuid);
    let _ = command_output("udisksctl", &["unmount", "--no-user-interaction", "-b", &device]);
}

// ----- one run -----------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_copied: u64,
    pub current: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub number: u32,
    pub files: u64,
    pub changed: u64,
    pub bytes: u64,
    pub errors: u64,
    pub seconds: u64,
}

struct Run<'a> {
    config: &'a Config,
    /// The backup before this one, to link what has not changed.
    previous: Option<PathBuf>,
    target: PathBuf,
    store: PathBuf,
    progress: Progress,
    outcome: Outcome,
    last_report: std::time::Instant,
    report: &'a mut dyn FnMut(&Progress),
    cancelled: &'a dyn Fn() -> bool,
}

fn mtime(meta: &std::fs::Metadata) -> i64 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Same size and (within the two seconds FAT can tell apart) same date: taken as unchanged.
fn same(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    a.len() == b.len() && (mtime(a) - mtime(b)).abs() <= 2
}

/// Where a path of the computer goes inside a backup: its absolute path, below the root.
fn inside(root: &Path, path: &Path) -> PathBuf {
    root.join(path.strip_prefix("/").unwrap_or(path))
}

fn count_files(dir: &Path, device: u64, config: &Config, store: &Path, cancelled: &dyn Fn() -> bool) -> u64 {
    let mut total = 0;
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    for entry in entries.flatten() {
        if cancelled() {
            return total;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        let is_dir = meta.is_dir();
        if excluded(&path, is_dir, config) || path == store {
            continue;
        }
        if is_dir {
            if meta.dev() == device {
                total += count_files(&path, device, config, store, cancelled);
            }
        } else {
            total += 1;
        }
    }
    total
}

impl Run<'_> {
    fn tick(&mut self, current: &Path) {
        if self.last_report.elapsed() >= Duration::from_millis(250) {
            self.last_report = std::time::Instant::now();
            self.progress.current = current.to_string_lossy().to_string();
            (self.report)(&self.progress);
        }
    }

    fn file(&mut self, path: &Path, meta: &std::fs::Metadata) {
        let to = inside(&self.target, path);
        self.progress.files_done += 1;
        self.outcome.files += 1;
        // A run that was interrupted is continued: what is already there is not copied again.
        if std::fs::symlink_metadata(&to).map(|there| same(&there, meta)).unwrap_or(false) {
            return;
        }
        if meta.file_type().is_symlink() {
            match std::fs::read_link(path).and_then(|link| std::os::unix::fs::symlink(link, &to)) {
                Ok(()) => {}
                // File systems without links (FAT): the link is left out, not counted as a failure.
                Err(e) if e.raw_os_error() == Some(1) || e.kind() == std::io::ErrorKind::Unsupported => {}
                Err(_) => self.outcome.errors += 1,
            }
            return;
        }
        if !meta.is_file() {
            return; // sockets, pipes, devices
        }
        if let Some(previous) = &self.previous {
            let before = inside(previous, path);
            if std::fs::metadata(&before).map(|b| b.is_file() && same(&b, meta)).unwrap_or(false) && std::fs::hard_link(&before, &to).is_ok() {
                return;
            }
        }
        match std::fs::copy(path, &to) {
            Ok(bytes) => {
                // Keep the date: it is what tells changed from unchanged next time.
                if let (Ok(file), Ok(modified)) = (std::fs::File::open(&to), meta.modified()) {
                    let _ = file.set_modified(modified);
                }
                self.outcome.changed += 1;
                self.outcome.bytes += bytes;
                self.progress.bytes_copied += bytes;
            }
            Err(_) => self.outcome.errors += 1,
        }
    }

    fn folder(&mut self, dir: &Path, device: u64) -> Result<(), String> {
        let to = inside(&self.target, dir);
        std::fs::create_dir_all(&to).map_err(|e| format!("{}: {e}", to.display()))?;
        let Ok(entries) = std::fs::read_dir(dir) else {
            self.outcome.errors += 1;
            return Ok(());
        };
        for entry in entries.flatten() {
            if (self.cancelled)() {
                return Err("stopped".into());
            }
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                self.outcome.errors += 1;
                continue;
            };
            let is_dir = meta.is_dir();
            // Never the backup into itself, never what was asked to be left out.
            if excluded(&path, is_dir, self.config) || path == self.store {
                continue;
            }
            self.tick(&path);
            if is_dir {
                // Other file systems mounted inside (an unlocked vault, a network share) stay out.
                if meta.dev() == device {
                    self.folder(&path, device)?;
                }
            } else {
                self.file(&path, &meta);
            }
        }
        Ok(())
    }
}

/// Make one backup of `config.sources` in `store`. `report` is called a few times a second;
/// `cancelled` is asked between files. An interrupted run leaves a `.partial` folder that
/// the next run continues.
pub fn run(config: &Config, store: &Path, report: &mut dyn FnMut(&Progress), cancelled: &dyn Fn() -> bool) -> Result<Outcome, String> {
    let started = std::time::Instant::now();
    let snapshots = store.join(".snapshots");
    std::fs::create_dir_all(&snapshots).map_err(|e| format!("{}: {e}", snapshots.display()))?;
    let done = entries(store);
    let number = done.first().map(|e| e.number + 1).unwrap_or(1);
    let partial = snapshots.join(format!("{number}.partial"));
    // Leftovers of an older, abandoned run under another number are of no use.
    if let Ok(dirs) = std::fs::read_dir(&snapshots) {
        for dir in dirs.flatten() {
            let name = dir.file_name().to_string_lossy().to_string();
            if name.ends_with(".partial") && dir.path() != partial {
                let _ = std::fs::remove_dir_all(dir.path());
            }
        }
    }
    let target = partial.join("snapshot");
    std::fs::create_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    let mut total = 0;
    for source in &config.sources {
        if let Ok(meta) = std::fs::metadata(source) {
            total += count_files(source, meta.dev(), config, store, cancelled);
        }
    }
    let mut run = Run { config, previous: done.first().map(|e| e.dir.clone()), target, store: store.to_path_buf(), progress: Progress { files_total: total, ..Default::default() }, outcome: Outcome { number, ..Default::default() }, last_report: std::time::Instant::now(), report, cancelled };
    let mut any = false;
    for source in &config.sources {
        match std::fs::metadata(source) {
            Ok(meta) if meta.is_dir() => {
                any = true;
                run.folder(source, meta.dev())?;
            }
            _ => run.outcome.errors += 1,
        }
    }
    if !any {
        let _ = std::fs::remove_dir_all(&partial);
        return Err("none of the folders to back up exists".into());
    }
    let mut outcome = run.outcome.clone();
    let progress = run.progress.clone();
    (run.report)(&progress);
    outcome.seconds = started.elapsed().as_secs();
    let info = format!(
        "<?xml version=\"1.0\"?>\n<snapshot>\n  <type>single</type>\n  <num>{number}</num>\n  <date>{}</date>\n  <description>omafile backup</description>\n  <files>{}</files>\n  <changed>{}</changed>\n  <bytes>{}</bytes>\n  <seconds>{}</seconds>\n  <errors>{}</errors>\n</snapshot>\n",
        crate::feeds::utc_stamp(now()),
        outcome.files,
        outcome.changed,
        outcome.bytes,
        outcome.seconds,
        outcome.errors
    );
    std::fs::write(partial.join("info.xml"), info).map_err(|e| e.to_string())?;
    // Only a finished backup gets its number: the rename is what makes it count.
    std::fs::rename(&partial, snapshots.join(number.to_string())).map_err(|e| e.to_string())?;
    Ok(outcome)
}

// ----- what happened: status for the window, a journal for the list ------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// "running", "ok", "failed", "waiting" (drive not connected), or empty.
    pub state: String,
    pub started: i64,
    pub finished: i64,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_copied: u64,
    pub message: String,
}

pub fn status_path() -> PathBuf {
    state_dir().join("backup-status")
}

fn stop_path() -> PathBuf {
    state_dir().join("backup-stop")
}

impl Status {
    pub fn parse(text: &str) -> Status {
        let mut s = Status::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            match key {
                "state" => s.state = value.to_string(),
                "started" => s.started = value.parse().unwrap_or(0),
                "finished" => s.finished = value.parse().unwrap_or(0),
                "files_done" => s.files_done = value.parse().unwrap_or(0),
                "files_total" => s.files_total = value.parse().unwrap_or(0),
                "bytes_copied" => s.bytes_copied = value.parse().unwrap_or(0),
                "message" => s.message = value.to_string(),
                _ => {}
            }
        }
        s
    }

    pub fn serialize(&self) -> String {
        format!("state={}\nstarted={}\nfinished={}\nfiles_done={}\nfiles_total={}\nbytes_copied={}\nmessage={}\n", self.state, self.started, self.finished, self.files_done, self.files_total, self.bytes_copied, self.message.replace('\n', " "))
    }

    pub fn load() -> Status {
        let mut status = std::fs::read_to_string(status_path()).map(|t| Status::parse(&t)).unwrap_or_default();
        // A run that died without saying so: its status file stopped changing.
        if status.state == "running" {
            let stale = std::fs::metadata(status_path()).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|age| age > Duration::from_secs(120)).unwrap_or(false);
            if stale {
                status.state = "failed".into();
                status.message = "interrupted".into();
            }
        }
        status
    }

    fn save(&self) {
        let path = status_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Written beside and renamed: a reader never sees half a file.
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, self.serialize()).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

/// One line of the journal: a run and how it went. Kept on this computer, so the list of
/// the last backups is there even when the drive is not.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub time: i64,
    pub ok: bool,
    pub files: u64,
    pub changed: u64,
    pub bytes: u64,
    pub seconds: u64,
    pub message: String,
}

pub fn journal_path() -> PathBuf {
    state_dir().join("backup-log")
}

pub fn parse_journal(text: &str) -> Vec<Record> {
    let mut out: Vec<Record> = text
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            (f.len() >= 6).then(|| Record { time: f[0].parse().unwrap_or(0), ok: f[1] == "ok", files: f[2].parse().unwrap_or(0), changed: f[3].parse().unwrap_or(0), bytes: f[4].parse().unwrap_or(0), seconds: f[5].parse().unwrap_or(0), message: f.get(6).unwrap_or(&"").to_string() })
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.time));
    out
}

pub fn journal() -> Vec<Record> {
    std::fs::read_to_string(journal_path()).map(|t| parse_journal(&t)).unwrap_or_default()
}

fn journal_add(record: &Record) {
    let mut records = journal();
    records.insert(0, record.clone());
    records.truncate(200);
    let text: String = records.iter().rev().map(|r| format!("{}\t{}\t{}\t{}\t{}\t{}\t{}\n", r.time, if r.ok { "ok" } else { "failed" }, r.files, r.changed, r.bytes, r.seconds, r.message.replace(['\t', '\n'], " "))).collect();
    if let Some(dir) = journal_path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(journal_path(), text);
}

/// When the last backup that worked was made; 0 if never.
pub fn last_success() -> i64 {
    journal().iter().find(|r| r.ok).map(|r| r.time).unwrap_or(0)
}

/// The moment the schedule last called for a backup: the top of the hour, today's (or
/// yesterday's) chosen hour, or the chosen weekday's. `offset` is local time minus UTC.
pub fn latest_slot(config: &Config, now: i64, offset: i64) -> Option<i64> {
    let local = now + offset;
    let day = local.div_euclid(86400);
    let at = config.hour.min(23) as i64 * 3600;
    let slot = match config.schedule {
        Schedule::Manual => return None,
        Schedule::Hourly => local - local.rem_euclid(3600),
        Schedule::Daily => {
            let today = day * 86400 + at;
            if today > local { today - 86400 } else { today }
        }
        Schedule::Weekly => {
            // 1 January 1970 was a Thursday; Monday is 0 here.
            let weekday = (day + 3).rem_euclid(7);
            let back = (weekday - config.weekday.min(6) as i64).rem_euclid(7);
            let this_week = (day - back) * 86400 + at;
            if this_week > local { this_week - 7 * 86400 } else { this_week }
        }
    };
    Some(slot - offset)
}

/// Whether the schedule calls for a backup now: none has worked since the last moment one
/// was due. A run that was missed (computer off, drive away, on battery) stays due.
pub fn due(config: &Config, last_success: i64, now: i64, offset: i64) -> bool {
    latest_slot(config, now, offset).is_some_and(|slot| last_success < slot)
}

/// When the next automatic backup starts: `now` when one is due.
pub fn next_run(config: &Config, last_success: i64, now: i64, offset: i64) -> Option<i64> {
    let slot = latest_slot(config, now, offset)?;
    Some(if last_success < slot { now } else { slot + config.schedule.interval().unwrap_or(0) })
}

/// Local time minus UTC, in seconds.
pub fn utc_offset() -> i64 {
    gtk::glib::DateTime::now_local().map(|d| d.utc_offset().as_seconds()).unwrap_or(0)
}

/// Whether the computer runs on battery: a battery of its own (not a mouse's) is discharging.
pub fn on_battery() -> bool {
    on_battery_in(Path::new("/sys/class/power_supply"))
}

pub fn on_battery_in(dir: &Path) -> bool {
    let Ok(supplies) = std::fs::read_dir(dir) else { return false };
    let read = |p: PathBuf| std::fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default();
    supplies.flatten().any(|s| read(s.path().join("type")) == "Battery" && read(s.path().join("scope")) != "Device" && read(s.path().join("status")) == "Discharging")
}

/// Whether an automatic backup should start now: one is due, and the settings allow it.
pub fn should_start(config: &Config) -> bool {
    config.is_set_up() && due(config, last_success(), now(), utc_offset()) && !(config.ac_only && on_battery())
}

/// Ask a running backup to stop after the file it is on.
pub fn request_stop() {
    if let Some(dir) = stop_path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(stop_path(), b"stop");
}

// ----- running by itself -------------------------------------------------------------------

/// The systemd user units that run `omafile --backup --scheduled`, also when omafile is
/// closed. The timer only asks, every ten minutes; whether a backup is due is decided by
/// `due`, so a run that could not happen (drive away, on battery) is made up for soon after.
pub fn units(exe: &str, schedule: Schedule) -> Option<(String, String)> {
    if schedule == Schedule::Manual {
        return None;
    }
    let service = format!("[Unit]\nDescription=omafile backup\n\n[Service]\nType=oneshot\nExecStart={exe} --backup --scheduled\nNice=10\nIOSchedulingClass=idle\n");
    let timer = "[Unit]\nDescription=omafile backup (asks every ten minutes whether one is due)\n\n[Timer]\nOnCalendar=*:0/10\nPersistent=true\nRandomizedDelaySec=60\n\n[Install]\nWantedBy=timers.target\n".to_string();
    Some((service, timer))
}

/// Whether the installed timer is one an older version wrote: then it is written again.
pub fn units_outdated(schedule: Schedule) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let Some((service, timer)) = units(&exe.to_string_lossy(), schedule) else { return false };
    let dir = unit_dir();
    match (std::fs::read_to_string(dir.join("omafile-backup.service")), std::fs::read_to_string(dir.join("omafile-backup.timer"))) {
        (Ok(s), Ok(t)) => s != service || t != timer,
        _ => false,
    }
}

fn unit_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("systemd/user")
}

/// Write or remove the timer to match the schedule. Err when systemd would not take it:
/// then backups run only while omafile is open.
pub fn apply_schedule(schedule: Schedule) -> Result<(), String> {
    let dir = unit_dir();
    let (service, timer) = (dir.join("omafile-backup.service"), dir.join("omafile-backup.timer"));
    let systemctl = |args: &[&str]| std::process::Command::new("systemctl").arg("--user").args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).stdout(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    match units(&exe.to_string_lossy(), schedule) {
        Some((service_text, timer_text)) => {
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(&service, service_text).map_err(|e| e.to_string())?;
            std::fs::write(&timer, timer_text).map_err(|e| e.to_string())?;
            if systemctl(&["daemon-reload"]) && systemctl(&["enable", "omafile-backup.timer"]) && systemctl(&["restart", "omafile-backup.timer"]) {
                Ok(())
            } else {
                Err("systemd would not start the timer".into())
            }
        }
        None => {
            let _ = systemctl(&["disable", "--now", "omafile-backup.timer"]);
            let _ = std::fs::remove_file(&service);
            let _ = std::fs::remove_file(&timer);
            let _ = systemctl(&["daemon-reload"]);
            Ok(())
        }
    }
}

fn notify(icon: &str, title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send").args(["-a", "omafile", "-i", icon, title, body]).stdin(std::process::Stdio::null()).status();
}

/// Delete the oldest backups until a twentieth of the drive (10 GB at most) is free. The
/// newest three always stay. Returns how many went.
pub fn make_room(store: &Path, space: &dyn Fn() -> Option<(u64, u64)>) -> u32 {
    let mut removed = 0;
    while let Some((free, total)) = space() {
        if total == 0 || free >= (total / 20).min(10 << 30) {
            break;
        }
        let list = entries(store);
        let Some(oldest) = list.last().filter(|_| list.len() > 3) else { break };
        if remove(&oldest.dir).is_err() {
            break;
        }
        removed += 1;
    }
    removed
}

/// What is installed, next to a backup: `packages.txt` (from the repositories) and
/// `packages-aur.txt` (everything else), each in the form `pacman -S --needed - <` reads.
/// A backup holds the user's files; with these lists a fresh system gets its programs back.
pub fn write_package_lists(backup_dir: &Path, pacman: &dyn Fn(&[&str]) -> Option<String>) -> u32 {
    let mut written = 0;
    for (file, flags) in [("packages.txt", "-Qqen"), ("packages-aur.txt", "-Qqem")] {
        if let Some(list) = pacman(&[flags]).filter(|l| !l.trim().is_empty()) {
            if std::fs::write(backup_dir.join(file), list).is_ok() {
                written += 1;
            }
        }
    }
    written
}

fn in_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

pub fn has_pacman() -> bool {
    in_path("pacman")
}

// ----- what the system does by itself ------------------------------------------------------

/// Snapshots of the system (snapper's; on Omarchy taken before every update). They are a
/// different thing from this backup: on the same disk, of the system, for going back after an
/// update. The page shows them so that it is clear who looks after what.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemSnapshots {
    pub omarchy: bool,
    /// How many, when the folder may be read (it usually may not); 0 otherwise.
    pub count: usize,
    /// When the newest was made, as far as can be told without being root.
    pub newest: i64,
}

pub fn system_snapshots() -> Option<SystemSnapshots> {
    let omarchy = in_path("omarchy-snapshot") || home().join(".local/share/omarchy/bin/omarchy-snapshot").is_file();
    system_snapshots_in(Path::new("/"), omarchy)
}

pub fn system_snapshots_in(root: &Path, omarchy: bool) -> Option<SystemSnapshots> {
    let dir = root.join(".snapshots");
    let meta = std::fs::metadata(&dir).ok().filter(|m| m.is_dir())?;
    let (mut count, mut newest) = (0, 0);
    if let Ok(dirs) = std::fs::read_dir(&dir) {
        for entry in dirs.flatten() {
            if entry.file_name().to_str().is_some_and(|n| n.parse::<u32>().is_ok()) {
                count += 1;
                let date = std::fs::read_to_string(entry.path().join("info.xml")).map(|xml| crate::versions::parse_info(&xml).0).unwrap_or(0);
                newest = newest.max(date);
            }
        }
    }
    if newest == 0 {
        // The folder is root's: its date still tells when a snapshot was last added or removed.
        newest = mtime(&meta);
    }
    Some(SystemSnapshots { omarchy, count, newest })
}

/// Whether a folder lies in an area snapper takes snapshots of. Backups kept there end up
/// inside those snapshots: deleting an old backup frees nothing until the snapshots are gone.
pub fn inside_snapshotted(dir: &Path) -> bool {
    mount_info(dir).is_some_and(|(_, target, _, _)| target.join(".snapshots").is_dir())
}

fn lock_path() -> PathBuf {
    state_dir().join("backup.lock")
}

/// One backup at a time: the lock holds the process number, and a lock whose process is
/// gone does not count.
fn take_lock() -> bool {
    let path = lock_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(pid) = std::fs::read_to_string(&path) {
        if Path::new(&format!("/proc/{}", pid.trim())).exists() && pid.trim() != std::process::id().to_string() {
            return false;
        }
    }
    std::fs::write(&path, std::process::id().to_string()).is_ok()
}

/// `omafile --backup [--scheduled]`: one run without a window. Scheduled runs stay quiet
/// when nothing is due or the drive is away; a manual run says why it cannot start.
/// Returns the exit code.
pub fn main_headless(scheduled: bool) -> u8 {
    let config = Config::load();
    if !config.is_set_up() {
        eprintln!("omafile: no backup is set up (open omafile › Backup)");
        return 2;
    }
    if scheduled && !should_start(&config) {
        return 0;
    }
    if !take_lock() {
        eprintln!("omafile: a backup is running already");
        return if scheduled { 0 } else { 4 };
    }
    let _ = std::fs::remove_file(stop_path());
    let mut mounted_here = false;
    let mut dest = destination(&config);
    if dest == Destination::NotConnected && try_mount(&config) {
        mounted_here = true;
        dest = destination(&config);
    }
    let code = match dest {
        Destination::Ready(dir) => {
            let store = store_dir(&dir);
            let _awake = crate::inhibit::take("Backing up");
            if config.make_room {
                make_room(&store, &|| space(&dir));
            }
            let started = now();
            let mut status = Status { state: "running".into(), started, ..Default::default() };
            status.save();
            let mut report = |p: &Progress| {
                status.files_done = p.files_done;
                status.files_total = p.files_total;
                status.bytes_copied = p.bytes_copied;
                status.message = p.current.clone();
                status.save();
            };
            let result = run(&config, &store, &mut report, &|| stop_path().exists());
            let finished = now();
            match result {
                Ok(outcome) => {
                    let all: Vec<(u32, i64)> = entries(&store).iter().map(|e| (e.number, e.date)).collect();
                    for number in to_prune(&all, finished, config.keep_days) {
                        let _ = std::fs::remove_dir_all(store.join(".snapshots").join(number.to_string()));
                    }
                    if config.package_list {
                        write_package_lists(&store.join(".snapshots").join(outcome.number.to_string()), &|args| command_output("pacman", args));
                    }
                    if config.notify_done {
                        notify("emblem-ok-symbolic", &tr("Backup finished"), &format!("{} {}  ·  {} {}", outcome.files, tr("files"), outcome.changed, tr("new or changed")));
                    }
                    journal_add(&Record { time: finished, ok: true, files: outcome.files, changed: outcome.changed, bytes: outcome.bytes, seconds: outcome.seconds, message: if outcome.errors > 0 { format!("{} could not be read", outcome.errors) } else { String::new() } });
                    Status { state: "ok".into(), started, finished, files_done: outcome.files, files_total: outcome.files, bytes_copied: outcome.bytes, message: String::new() }.save();
                    0
                }
                Err(e) => {
                    let stopped = e == "stopped";
                    journal_add(&Record { time: finished, ok: false, files: 0, changed: 0, bytes: 0, seconds: (finished - started).max(0) as u64, message: e.clone() });
                    Status { state: "failed".into(), started, finished, message: e.clone(), ..Default::default() }.save();
                    if !stopped {
                        notify("dialog-warning", &tr("Backup failed"), &e);
                    }
                    1
                }
            }
        }
        Destination::NotConnected => {
            // Not an error for a timer: the drive is simply not there. Say so, once.
            let mut status = Status::load();
            if status.state != "waiting" {
                status.state = "waiting".into();
                status.message = String::new();
                status.save();
            }
            if scheduled {
                0
            } else {
                eprintln!("omafile: the backup drive is not connected");
                3
            }
        }
        Destination::NotSetUp => 2,
    };
    let _ = std::fs::remove_file(stop_path());
    let _ = std::fs::remove_file(lock_path());
    if mounted_here {
        unmount(&config);
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(path: &Path, content: &str, modified: u64) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        std::fs::File::open(path).unwrap().set_modified(UNIX_EPOCH + Duration::from_secs(modified)).unwrap();
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omafile-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn config_round_trips() {
        let c = Config { schedule: Schedule::Hourly, dest: "/run/media/me/T7/Backups".into(), uuid: "1234-ABCD".into(), subdir: "Backups".into(), skip_caches: false, sources: vec!["/home/me".into(), "/etc".into()], excludes: vec!["*.iso".into(), "~/Downloads/big".into()], ..Default::default() };
        assert_eq!(Config::parse(&c.serialize()), c);
        assert!(c.is_set_up());
        let empty = Config::parse("");
        assert_eq!((empty.schedule, empty.skip_caches, empty.is_set_up()), (Schedule::Daily, true, false));
        assert_eq!((empty.hour, empty.weekday, empty.keep_days, empty.make_room, empty.ac_only, empty.notify_done, empty.package_list), (12, 0, 0, true, false, false, true));
        let tuned = Config::parse("hour=7\nweekday=6\nkeep_days=92\nmake_room=false\nac_only=true\nnotify_done=true\npackage_list=false\n");
        assert_eq!((tuned.hour, tuned.weekday, tuned.keep_days, tuned.make_room, tuned.ac_only, tuned.notify_done, tuned.package_list), (7, 6, 92, false, true, true, false));
        assert_eq!(Config::parse(&tuned.serialize()), tuned);
        assert_eq!(Config::parse("hour=99\nweekday=9\n").hour, 23);
        assert_eq!(Schedule::from_id("nonsense"), Schedule::Manual);
    }

    #[test]
    fn patterns_and_exclusions() {
        assert!(glob("*.iso", "arch.iso") && !glob("*.iso", "arch.iso.sig"));
        assert!(glob("backup-*", "backup-2026") && glob("a*b*c", "aXXbYYc") && !glob("a*b*c", "aXXc"));
        assert!(glob("exact", "exact") && !glob("exact", "exactly"));
        // Hidden folders at the top of a source stay out when asked; deeper ones are copied.
        let hidden = Config { skip_hidden: true, sources: vec!["/home/me".into()], ..Default::default() };
        assert!(excluded(Path::new("/home/me/.config"), true, &hidden) && excluded(Path::new("/home/me/.bashrc"), false, &hidden));
        assert!(!excluded(Path::new("/home/me/Projects/.git"), true, &hidden), "only at the top of a source");
        assert!(!excluded(Path::new("/home/me/Documents"), true, &hidden));
        assert!(!excluded(Path::new("/home/me/.config"), true, &Config { sources: vec!["/home/me".into()], ..Default::default() }), "off by default");
        assert!(Config::parse(&hidden.serialize()).skip_hidden);
        let mut config = Config { excludes: vec!["*.tmp".into(), "/srv/private".into()], ..Default::default() };
        assert!(excluded(Path::new("/home/me/x.tmp"), false, &config));
        assert!(excluded(Path::new("/srv/private/deep/file"), false, &config));
        assert!(!excluded(Path::new("/srv/public"), true, &config));
        assert!(excluded(Path::new("/home/me/.cache"), true, &config) && excluded(Path::new("/home/me/p/node_modules"), true, &config));
        assert!(excluded(Path::new("/home/me/.local/share/Trash"), true, &config));
        assert!(!excluded(Path::new("/home/me/.cache"), false, &config), "a file that happens to be called .cache is kept");
        config.skip_caches = false;
        assert!(!excluded(Path::new("/home/me/.cache"), true, &config));
        assert!(excluded(Path::new("/home/.snapshots"), true, &config), "snapshots stay out whatever the settings say");
    }

    #[test]
    fn backs_up_links_what_is_unchanged_and_keeps_history() {
        let root = scratch("run");
        let (source, dest) = (root.join("home"), root.join("drive"));
        put(&source.join("docs/report.md"), "first", 1_000);
        put(&source.join("docs/same.txt"), "never changes", 500);
        put(&source.join("photo.jpg"), "JPEG", 700);
        put(&source.join(".cache/junk"), "junk", 100);
        put(&source.join("build/CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55", 100);
        put(&source.join("build/big.o"), "object", 100);
        std::os::unix::fs::symlink("docs/report.md", source.join("link")).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        let config = Config { dest: dest.clone(), sources: vec![source.clone()], ..Default::default() };
        let store = store_dir(&dest);
        let mut reports = 0;

        let first = run(&config, &store, &mut |_| reports += 1, &|| false).unwrap();
        assert_eq!((first.number, first.files, first.changed, first.errors), (1, 4, 3, 0), "three files and a link; caches stay out");
        assert!(reports >= 1);
        let one = store.join(".snapshots/1/snapshot");
        assert_eq!(std::fs::read_to_string(inside(&one, &source.join("docs/report.md"))).unwrap(), "first");
        assert!(!inside(&one, &source.join(".cache")).exists() && !inside(&one, &source.join("build")).exists());
        assert_eq!(std::fs::read_link(inside(&one, &source.join("link"))).unwrap(), PathBuf::from("docs/report.md"));
        assert!(!store.join(".snapshots/1.partial").exists());

        // One file changes, one is deleted, one is new.
        put(&source.join("docs/report.md"), "second, longer", 2_000);
        std::fs::remove_file(source.join("photo.jpg")).unwrap();
        put(&source.join("new.txt"), "new", 2_100);
        let second = run(&config, &store, &mut |_| {}, &|| false).unwrap();
        assert_eq!((second.number, second.files, second.changed), (2, 4, 2), "only the changed and the new file are copied");
        let two = store.join(".snapshots/2/snapshot");
        // Unchanged: the very same file on disk, not a second copy.
        let (a, b) = (std::fs::metadata(inside(&one, &source.join("docs/same.txt"))).unwrap(), std::fs::metadata(inside(&two, &source.join("docs/same.txt"))).unwrap());
        assert_eq!(a.ino(), b.ino());
        assert_eq!(a.nlink(), 2);
        // History: both states of the report are there, and the deleted photo is still in the first.
        assert_eq!(std::fs::read_to_string(inside(&one, &source.join("docs/report.md"))).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(inside(&two, &source.join("docs/report.md"))).unwrap(), "second, longer");
        assert!(inside(&one, &source.join("photo.jpg")).exists() && !inside(&two, &source.join("photo.jpg")).exists());
        assert_eq!(mtime(&std::fs::metadata(inside(&two, &source.join("docs/report.md"))).unwrap()), 2_000, "dates are kept");

        let list = entries(&store);
        assert_eq!(list.iter().map(|e| (e.number, e.files, e.changed)).collect::<Vec<_>>(), vec![(2, 4, 2), (1, 4, 3)]);
        assert!(list[0].date > 1_700_000_000);

        // Earlier Versions reads backups like snapshots: the store's root is "/".
        let as_versions = crate::versions::Store { root: PathBuf::from("/"), snapshots: store.join(".snapshots") };
        let versions = crate::versions::versions_of(&as_versions, &source.join("docs/report.md"));
        assert_eq!(versions.len(), 2);
        assert!(versions[0].current && versions[0].modified == 2_000 && versions[1].modified == 1_000);
        let gone = crate::versions::deleted_in(&as_versions, &source);
        assert!(gone.iter().any(|d| d.name == "photo.jpg"), "{gone:?}");

        // The backup never goes into itself, even when the drive is inside what is backed up.
        let inside_source = Config { dest: source.join("drive"), sources: vec![source.clone()], ..Default::default() };
        std::fs::create_dir_all(source.join("drive")).unwrap();
        let nested_store = store_dir(&source.join("drive"));
        let nested = run(&inside_source, &nested_store, &mut |_| {}, &|| false).unwrap();
        assert!(!inside(&nested_store.join(".snapshots/1/snapshot"), &nested_store).exists());
        assert_eq!(nested.files, 4);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_stopped_run_does_not_count_and_is_continued() {
        let root = scratch("stop");
        let (source, dest) = (root.join("src"), root.join("drive"));
        for i in 0..20 {
            put(&source.join(format!("f{i:02}.txt")), "content", 1_000 + i);
        }
        std::fs::create_dir_all(&dest).unwrap();
        let config = Config { dest: dest.clone(), sources: vec![source.clone()], ..Default::default() };
        let store = store_dir(&dest);
        let asked = std::cell::Cell::new(0);
        // Stop after a few files.
        let stopped = run(&config, &store, &mut |_| {}, &|| {
            asked.set(asked.get() + 1);
            asked.get() > 30
        });
        assert_eq!(stopped, Err("stopped".into()));
        assert!(entries(&store).is_empty(), "an unfinished backup is not listed");
        assert!(store.join(".snapshots/1.partial").is_dir());
        let again = run(&config, &store, &mut |_| {}, &|| false).unwrap();
        assert_eq!((again.number, again.files), (1, 20));
        assert!(again.changed < 20, "what the first attempt copied is not copied again");
        assert!(run(&Config { sources: vec![root.join("missing")], ..config.clone() }, &store, &mut |_| {}, &|| false).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn old_backups_thin_out() {
        let day = 86400;
        let now = 1_000 * day;
        // One backup an hour for three days, then one a day back to day 60.
        let mut all: Vec<(u32, i64)> = Vec::new();
        let mut n = 0;
        for hour in 0..72 {
            n += 1;
            all.push((n, now - hour * 3600));
        }
        for d in 3..60 {
            n += 1;
            all.push((n, now - d * day));
        }
        let pruned = to_prune(&all, now, 0);
        let kept: Vec<(u32, i64)> = all.iter().filter(|(n, _)| !pruned.contains(n)).cloned().collect();
        let within_day = kept.iter().filter(|(_, d)| now - d < day).count();
        assert_eq!(within_day, 24, "everything from the last day stays");
        let days_2_to_30: Vec<i64> = kept.iter().filter(|(_, d)| now - d >= day && now - d < 30 * day).map(|(_, d)| d.div_euclid(day)).collect();
        let mut unique = days_2_to_30.clone();
        unique.dedup();
        assert_eq!(days_2_to_30, unique, "one a day beyond that");
        let older = kept.iter().filter(|(_, d)| now - d >= 30 * day).count();
        assert!((4..=6).contains(&older), "one a week beyond a month: {older}");
        // Few backups: nothing is deleted, however old.
        assert!(to_prune(&[(1, 0), (2, 10), (3, 20)], now, 0).is_empty());
        assert_eq!(to_prune(&[(1, 5 * day), (2, 5 * day + 60), (3, 6 * day), (4, 7 * day), (5, 8 * day)], 1_000 * day, 0), vec![1], "the newest three stay; of two old ones in the same week the newer stays");
        // Kept for a month only: nothing older stays, except that the newest three always do.
        let month = to_prune(&all, now, 31);
        assert!(all.iter().filter(|(n, _)| !month.contains(n)).all(|(_, d)| now - d <= 31 * day));
        assert!(month.len() > pruned.len());
        assert!(to_prune(&[(1, 0), (2, 10), (3, 20)], now, 31).is_empty());
    }

    #[test]
    fn a_backup_is_due_from_the_chosen_time_until_one_has_worked() {
        let (hour, day) = (3600, 86400);
        // Day 19996 is a Monday (30 September 2024); local time is UTC+2.
        let (monday, offset) = (19_996 * day, 2 * hour);
        let at = |d: i64, h: i64, m: i64| monday + d * day + h * hour + m * 60 - offset;
        let daily = Config { schedule: Schedule::Daily, hour: 18, ..Default::default() };
        assert!(!due(&daily, at(0, 9, 0), at(0, 17, 59), offset), "not before the chosen hour");
        assert!(due(&daily, at(0, 9, 0), at(0, 18, 0), offset), "from then on");
        assert!(due(&daily, at(0, 9, 0), at(1, 7, 0), offset), "and still the next morning, when the drive was away in the evening");
        assert!(!due(&daily, at(0, 18, 5), at(1, 7, 0), offset), "not again once it has worked");
        assert!(due(&daily, 0, at(0, 3, 0), offset), "never backed up: due at once");
        assert_eq!(next_run(&daily, at(0, 18, 5), at(1, 7, 0), offset), Some(at(1, 18, 0)));
        assert_eq!(next_run(&daily, at(0, 9, 0), at(0, 19, 0), offset), Some(at(0, 19, 0)), "due: now");

        let weekly = Config { schedule: Schedule::Weekly, hour: 10, weekday: 4, ..Default::default() };
        assert_eq!(latest_slot(&weekly, at(5, 0, 0), offset), Some(at(4, 10, 0)), "Saturday: last Friday at ten");
        assert_eq!(latest_slot(&weekly, at(4, 9, 0), offset), Some(at(-3, 10, 0)), "Friday before ten: the Friday before");
        assert!(!due(&weekly, at(4, 11, 0), at(6, 12, 0), offset) && due(&weekly, at(3, 11, 0), at(4, 10, 1), offset));
        assert_eq!(next_run(&weekly, at(4, 11, 0), at(6, 12, 0), offset), Some(at(11, 10, 0)));

        let hourly = Config { schedule: Schedule::Hourly, ..Default::default() };
        assert!(due(&hourly, at(0, 9, 50), at(0, 10, 0), offset) && !due(&hourly, at(0, 10, 1), at(0, 10, 59), offset));
        assert!(!due(&Config { schedule: Schedule::Manual, ..Default::default() }, 0, at(0, 10, 0), offset));
    }

    #[test]
    fn battery_room_packages_and_the_system_snapshots() {
        let root = scratch("extras");
        // On battery: only a discharging battery of the computer itself counts.
        let power = root.join("power");
        let supply = |name: &str, kind: &str, status: &str, scope: &str| {
            for (file, text) in [("type", kind), ("status", status), ("scope", scope)] {
                put(&power.join(name).join(file), text, 1);
            }
        };
        supply("AC", "Mains", "", "");
        supply("hidpp_battery_0", "Battery", "Discharging", "Device");
        assert!(!on_battery_in(&power), "a mouse's battery is not the computer's");
        supply("BAT0", "Battery", "Charging", "");
        assert!(!on_battery_in(&power));
        supply("BAT0", "Battery", "Discharging", "");
        assert!(on_battery_in(&power));
        assert!(!on_battery_in(&root.join("no-such-folder")), "a desktop has none");

        // Making room: the oldest go until enough is free; three always stay.
        let store = root.join("store");
        for n in 1..=6 {
            put(&store.join(format!(".snapshots/{n}/info.xml")), &format!("<snapshot><num>{n}</num><date>2026-01-0{n} 10:00:00</date></snapshot>"), 1);
            put(&store.join(format!(".snapshots/{n}/snapshot/file")), "x", 1);
        }
        let free = std::cell::Cell::new(10u64);
        let removed = make_room(&store, &|| {
            free.set(free.get() + 20);
            Some((free.get(), 1000))
        });
        assert_eq!(removed, 1, "30 of 1000 is too little, 50 is enough");
        assert_eq!(entries(&store).iter().map(|e| e.number).collect::<Vec<_>>(), vec![6, 5, 4, 3, 2]);
        assert_eq!(make_room(&store, &|| Some((0, 1000))), 2);
        assert_eq!(entries(&store).len(), 3, "the newest three stay however full the drive is");
        assert_eq!(make_room(&store, &|| Some((900, 1000))), 0);
        // A big drive: 10 GB free is enough, not a twentieth.
        assert_eq!(make_room(&store, &|| Some((11 << 30, 4000 << 30))), 0);

        // The package lists, in the form pacman reads back.
        let dir = store.join(".snapshots/6");
        let pacman = |args: &[&str]| Some(if args == ["-Qqen"] { "firefox\nhelix\n".to_string() } else { "omafile-git\n".to_string() });
        assert_eq!(write_package_lists(&dir, &pacman), 2);
        assert_eq!(std::fs::read_to_string(dir.join("packages.txt")).unwrap(), "firefox\nhelix\n");
        assert_eq!(std::fs::read_to_string(dir.join("packages-aur.txt")).unwrap(), "omafile-git\n");
        assert_eq!(write_package_lists(&dir, &|_| None), 0, "no pacman: nothing is written");
        assert_eq!(entries(&store).first().map(|e| e.number), Some(6), "the lists do not disturb the backup");

        // The system's own snapshots are only looked at.
        let system = root.join("system");
        assert_eq!(system_snapshots_in(&system, true), None);
        put(&system.join(".snapshots/41/info.xml"), "<snapshot><num>41</num><date>2026-03-01 08:00:00</date></snapshot>", 1);
        put(&system.join(".snapshots/42/info.xml"), "<snapshot><num>42</num><date>2026-03-09 08:00:00</date></snapshot>", 1);
        let seen = system_snapshots_in(&system, true).unwrap();
        assert_eq!((seen.omarchy, seen.count), (true, 2));
        assert_eq!(seen.newest, crate::versions::parse_info("<date>2026-03-09 08:00:00</date>").0);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn schedule_units_status_and_journal() {
        let (service, timer) = units("/usr/bin/omafile", Schedule::Daily).unwrap();
        assert!(service.contains("ExecStart=/usr/bin/omafile --backup --scheduled") && service.contains("Type=oneshot"));
        assert!(timer.contains("OnCalendar=*:0/10") && timer.contains("Persistent=true") && timer.contains("WantedBy=timers.target"));
        assert!(units("/usr/bin/omafile", Schedule::Manual).is_none());
        let status = Status { state: "running".into(), started: 5, finished: 0, files_done: 10, files_total: 40, bytes_copied: 1234, message: "a\nb".into() };
        let back = Status::parse(&status.serialize());
        assert_eq!((back.state.as_str(), back.files_done, back.files_total, back.message.as_str()), ("running", 10, 40, "a b"));
        let records = parse_journal("100\tok\t10\t2\t300\t4\t\n200\tfailed\t0\t0\t0\t1\tdrive full\nbroken line\n");
        assert_eq!(records.len(), 2);
        assert_eq!((records[0].time, records[0].ok, records[0].message.as_str()), (200, false, "drive full"), "newest first");
        assert_eq!(parse_findmnt("1234-ABCD /run/media/me/My\\x20Drive exfat /dev/sdb1"), Some(("1234-ABCD".into(), PathBuf::from("/run/media/me/My Drive"), "exfat".into(), "/dev/sdb1".into())));
        assert_eq!(parse_findmnt("/ overlay overlay").map(|m| m.0), Some(String::new()));
    }
}
