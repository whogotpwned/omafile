//! Directory listing and file operations.

use std::cmp::Ordering;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub mode: u32,
    pub is_exec: bool,
}

impl Entry {
    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('.')
    }

    pub fn ext(&self) -> String {
        self.path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default()
    }

    pub fn from_path(path: &Path) -> io::Result<Entry> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let link_meta = std::fs::symlink_metadata(path)?;
        let is_symlink = link_meta.file_type().is_symlink();
        // Follow symlinks for type/size; fall back to the link itself when broken.
        let meta = if is_symlink {
            std::fs::metadata(path).unwrap_or(link_meta)
        } else {
            link_meta
        };
        let is_dir = meta.is_dir();
        let mode = meta.permissions().mode();
        Ok(Entry {
            name,
            path: path.to_path_buf(),
            is_dir,
            is_symlink,
            size: meta.len(),
            modified: meta.modified().ok(),
            mode,
            is_exec: !is_dir && mode & 0o111 != 0,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    Name,
    Size,
    Modified,
    Extension,
}

impl SortMode {
    pub fn next(self) -> Self {
        match self {
            SortMode::Name => SortMode::Size,
            SortMode::Size => SortMode::Modified,
            SortMode::Modified => SortMode::Extension,
            SortMode::Extension => SortMode::Name,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Name => "name",
            SortMode::Size => "size",
            SortMode::Modified => "modified",
            SortMode::Extension => "ext",
        }
    }
}

fn cmp_name(a: &Entry, b: &Entry) -> Ordering {
    a.name
        .to_lowercase()
        .cmp(&b.name.to_lowercase())
        .then_with(|| a.name.cmp(&b.name))
}

pub fn sort_entries(entries: &mut [Entry], sort: SortMode) {
    entries.sort_by(|a, b| {
        // Directories always first.
        b.is_dir.cmp(&a.is_dir).then_with(|| match sort {
            SortMode::Name => cmp_name(a, b),
            SortMode::Size => b.size.cmp(&a.size).then_with(|| cmp_name(a, b)),
            SortMode::Modified => b.modified.cmp(&a.modified).then_with(|| cmp_name(a, b)),
            SortMode::Extension => a.ext().cmp(&b.ext()).then_with(|| cmp_name(a, b)),
        })
    });
}

pub fn read_dir(dir: &Path, show_hidden: bool, sort: SortMode) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in std::fs::read_dir(dir)? {
        let item = match item {
            Ok(i) => i,
            Err(_) => continue,
        };
        let path = item.path();
        if let Ok(entry) = Entry::from_path(&path) {
            if !show_hidden && entry.is_hidden() {
                continue;
            }
            entries.push(entry);
        }
    }
    sort_entries(&mut entries, sort);
    Ok(entries)
}

pub fn human_size(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn human_time(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = t.into();
    let now = chrono::Local::now();
    if (now - dt).num_days() < 180 && dt <= now {
        dt.format("%d %b %H:%M").to_string()
    } else {
        dt.format("%d %b  %Y").to_string()
    }
}

pub fn perm_string(mode: u32, is_dir: bool, is_symlink: bool) -> String {
    let mut s = String::with_capacity(10);
    s.push(if is_symlink {
        'l'
    } else if is_dir {
        'd'
    } else {
        '-'
    });
    for shift in [6u32, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

/// True when `name` resolves on `$PATH`.
pub fn has_cmd(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(name).is_file())
}

/// Pick a destination in `dir` for `name` that does not exist yet.
pub fn unique_dest(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() && std::fs::symlink_metadata(&candidate).is_err() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for i in 1.. {
        let candidate = dir.join(format!("{stem} ({i}){ext}"));
        if std::fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
    }
    unreachable!()
}

pub fn copy_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(src)?;
        return std::os::unix::fs::symlink(target, dst);
    }
    if meta.is_dir() {
        if dst.starts_with(src) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot copy a directory into itself",
            ));
        }
        std::fs::create_dir(dst)?;
        for item in std::fs::read_dir(src)? {
            let item = item?;
            copy_recursive(&item.path(), &dst.join(item.file_name()))?;
        }
        std::fs::set_permissions(dst, meta.permissions())?;
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

pub fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        // Cross-device: copy then delete.
        Err(e) if e.raw_os_error() == Some(18) => {
            copy_recursive(src, dst)?;
            remove(src)
        }
        Err(e) => Err(e),
    }
}

pub fn remove(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

pub fn trash_available() -> bool {
    has_cmd("gio")
}

/// Move to the freedesktop trash via `gio trash` (part of glib2, present on Omarchy).
pub fn trash(path: &Path) -> io::Result<()> {
    let status = Command::new("gio")
        .arg("trash")
        .arg("--")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("gio trash failed for {}", path.display())))
    }
}

/// Launch a program fully detached from the terminal.
pub fn spawn_detached(program: &str, args: &[&str], cwd: Option<&Path>) -> io::Result<()> {
    let mut cmd = if has_cmd("setsid") {
        let mut c = Command::new("setsid");
        c.arg("-f").arg(program);
        c
    } else {
        Command::new(program)
    };
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.spawn().map(|_| ())
}

/// Shorten `$HOME` to `~`.
pub fn display_path(path: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(rest) = path.strip_prefix(&home) {
            return if rest.as_os_str().is_empty() {
                "~".into()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1.0 K");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 M");
        assert_eq!(human_size(250 * 1024 * 1024), "250 M");
    }

    #[test]
    fn perms() {
        assert_eq!(perm_string(0o755, true, false), "drwxr-xr-x");
        assert_eq!(perm_string(0o644, false, false), "-rw-r--r--");
        assert_eq!(perm_string(0o777, false, true), "lrwxrwxrwx");
    }

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir().join(format!("omafile-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        assert_eq!(unique_dest(&dir, "a.txt"), dir.join("a (1).txt"));
        assert_eq!(unique_dest(&dir, "b.txt"), dir.join("b.txt"));
        std::fs::write(dir.join("noext"), b"x").unwrap();
        assert_eq!(unique_dest(&dir, "noext"), dir.join("noext (1)"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copy_and_move_dirs() {
        let base = std::env::temp_dir().join(format!("omafile-cp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("src/inner")).unwrap();
        std::fs::write(base.join("src/inner/f.txt"), b"hello").unwrap();
        copy_recursive(&base.join("src"), &base.join("dst")).unwrap();
        assert_eq!(std::fs::read(base.join("dst/inner/f.txt")).unwrap(), b"hello");
        assert!(copy_recursive(&base.join("src"), &base.join("src/inner/loop")).is_err());
        move_path(&base.join("dst"), &base.join("moved")).unwrap();
        assert!(!base.join("dst").exists());
        assert!(base.join("moved/inner/f.txt").exists());
        remove(&base.join("moved")).unwrap();
        assert!(!base.join("moved").exists());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn sorting_dirs_first() {
        let mk = |name: &str, is_dir: bool, size: u64| Entry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink: false,
            size,
            modified: None,
            mode: 0o644,
            is_exec: false,
        };
        let mut v = vec![mk("b.txt", false, 10), mk("Zed", true, 0), mk("a.txt", false, 50), mk("alpha", true, 0)];
        sort_entries(&mut v, SortMode::Name);
        let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Zed", "a.txt", "b.txt"]);
        sort_entries(&mut v, SortMode::Size);
        let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Zed", "a.txt", "b.txt"]);
    }
}
