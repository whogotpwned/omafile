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
}

impl Default for Config {
    fn default() -> Self {
        Config { schedule: Schedule::Daily, dest: PathBuf::new(), uuid: String::new(), subdir: String::new(), skip_caches: true, sources: Vec::new(), excludes: Vec::new() }
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
                "source" if !value.is_empty() => c.sources.push(PathBuf::from(value)),
                "exclude" if !value.is_empty() => c.excludes.push(value.to_string()),
                _ => {}
            }
        }
        c
    }

    pub fn serialize(&self) -> String {
        let mut out = format!("schedule={}\ndest={}\nuuid={}\nsubdir={}\nskip_caches={}\n", self.schedule.id(), self.dest.display(), self.uuid, self.subdir, self.skip_caches);
        for s in &self.sources {
            out.push_str(&format!("source={}\n", s.display()));
        }
        for e in &self.excludes {
            out.push_str(&format!("exclude={e}\n"));
        }
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

const CACHE_NAMES: &[&str] = &[".cache", "node_modules", "__pycache__", ".snapshots", "lost+found", ".Trash-1000", ".thumbnails"];

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
/// for thirty days, then the newest of each week. The newest three always stay.
pub fn to_prune(backups: &[(u32, i64)], now: i64) -> Vec<u32> {
    let mut sorted: Vec<(u32, i64)> = backups.to_vec();
    sorted.sort_by_key(|(_, date)| std::cmp::Reverse(*date));
    let mut keep_slots: std::collections::HashSet<(u8, i64)> = std::collections::HashSet::new();
    let mut prune = Vec::new();
    for (i, (number, date)) in sorted.iter().enumerate() {
        let age = now - date;
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

/// Whether the schedule calls for a backup now.
pub fn due(schedule: Schedule, last_success: i64, now: i64) -> bool {
    match schedule.interval() {
        // A little slack, so an hourly timer that fires a minute early still counts.
        Some(interval) => now - last_success >= interval - interval / 20,
        None => false,
    }
}

/// Ask a running backup to stop after the file it is on.
pub fn request_stop() {
    if let Some(dir) = stop_path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(stop_path(), b"stop");
}

// ----- running by itself -------------------------------------------------------------------

/// The systemd user units that run `omafile --backup --scheduled` on a schedule, also when
/// omafile is closed. `Persistent` makes up for runs missed while the computer was off.
pub fn units(exe: &str, schedule: Schedule) -> Option<(String, String)> {
    let calendar = match schedule {
        Schedule::Manual => return None,
        Schedule::Hourly => "hourly",
        Schedule::Daily => "daily",
        Schedule::Weekly => "weekly",
    };
    let service = format!("[Unit]\nDescription=omafile backup\n\n[Service]\nType=oneshot\nExecStart={exe} --backup --scheduled\nNice=10\nIOSchedulingClass=idle\n");
    let timer = format!("[Unit]\nDescription=omafile backup ({calendar})\n\n[Timer]\nOnCalendar={calendar}\nPersistent=true\nRandomizedDelaySec=180\n\n[Install]\nWantedBy=timers.target\n");
    Some((service, timer))
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
            if systemctl(&["daemon-reload"]) && systemctl(&["enable", "--now", "omafile-backup.timer"]) {
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
    if scheduled && !due(config.schedule, last_success(), now()) {
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
                    for number in to_prune(&all, finished) {
                        let _ = std::fs::remove_dir_all(store.join(".snapshots").join(number.to_string()));
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
                        let _ = std::process::Command::new("notify-send").args(["-a", "omafile", "-i", "dialog-warning", "Backup failed", &e]).status();
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
        let c = Config { schedule: Schedule::Hourly, dest: "/run/media/me/T7/Backups".into(), uuid: "1234-ABCD".into(), subdir: "Backups".into(), skip_caches: false, sources: vec!["/home/me".into(), "/etc".into()], excludes: vec!["*.iso".into(), "~/Downloads/big".into()] };
        assert_eq!(Config::parse(&c.serialize()), c);
        assert!(c.is_set_up());
        let empty = Config::parse("");
        assert_eq!((empty.schedule, empty.skip_caches, empty.is_set_up()), (Schedule::Daily, true, false));
        assert_eq!(Schedule::from_id("nonsense"), Schedule::Manual);
    }

    #[test]
    fn patterns_and_exclusions() {
        assert!(glob("*.iso", "arch.iso") && !glob("*.iso", "arch.iso.sig"));
        assert!(glob("backup-*", "backup-2026") && glob("a*b*c", "aXXbYYc") && !glob("a*b*c", "aXXc"));
        assert!(glob("exact", "exact") && !glob("exact", "exactly"));
        let mut config = Config { excludes: vec!["*.tmp".into(), "/srv/private".into()], ..Default::default() };
        assert!(excluded(Path::new("/home/me/x.tmp"), false, &config));
        assert!(excluded(Path::new("/srv/private/deep/file"), false, &config));
        assert!(!excluded(Path::new("/srv/public"), true, &config));
        assert!(excluded(Path::new("/home/me/.cache"), true, &config) && excluded(Path::new("/home/me/p/node_modules"), true, &config));
        assert!(excluded(Path::new("/home/me/.local/share/Trash"), true, &config));
        assert!(!excluded(Path::new("/home/me/.cache"), false, &config), "a file that happens to be called .cache is kept");
        config.skip_caches = false;
        assert!(!excluded(Path::new("/home/me/.cache"), true, &config));
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
        let pruned = to_prune(&all, now);
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
        assert!(to_prune(&[(1, 0), (2, 10), (3, 20)], now).is_empty());
        assert_eq!(to_prune(&[(1, 5 * day), (2, 5 * day + 60), (3, 6 * day), (4, 7 * day), (5, 8 * day)], 1_000 * day), vec![1], "the newest three stay; of two old ones in the same week the newer stays");
    }

    #[test]
    fn schedule_units_status_and_journal() {
        assert!(due(Schedule::Hourly, 0, 10_000) && !due(Schedule::Hourly, 9_000, 10_000) && due(Schedule::Hourly, 6_500, 10_000));
        assert!(!due(Schedule::Manual, 0, 10_000));
        let (service, timer) = units("/usr/bin/omafile", Schedule::Daily).unwrap();
        assert!(service.contains("ExecStart=/usr/bin/omafile --backup --scheduled") && service.contains("Type=oneshot"));
        assert!(timer.contains("OnCalendar=daily") && timer.contains("Persistent=true") && timer.contains("WantedBy=timers.target"));
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
