//! Folder rules, Hazel style: "PDFs older than 7 days in ~/Downloads go to
//! ~/Documents/Invoices", "*.iso gets the red tag", "screenshots sort into
//! YYYY-MM folders". Edited in a dialog, stored in `~/.config/omafile/rules`
//! (one per line: folder, glob, older-than-days, action, argument, enabled),
//! run when omafile starts and whenever a watched folder changes.

use crate::i18n::tr;
use crate::ops;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Move into a folder.
    MoveTo(PathBuf),
    /// Copy into a folder.
    CopyTo(PathBuf),
    /// Colour tag by name (red, green, …).
    Tag(String),
    Trash,
    /// Move into `<folder>/YYYY-MM` (argument: a base folder, empty = the rule's folder).
    ByMonth(PathBuf),
    /// Run a shell command with `$FILE` set.
    Run(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub folder: PathBuf,
    /// Glob on the file name; empty matches everything.
    pub glob: String,
    /// Only files whose modification time is at least this old (0 = any).
    pub older_days: u32,
    pub action: Action,
    pub enabled: bool,
}

pub fn path() -> PathBuf {
    glib::user_config_dir().join("omafile/rules")
}

pub const ACTION_IDS: &[&str] = &["move", "copy", "tag", "trash", "month", "run"];

impl Action {
    pub fn id(&self) -> &'static str {
        match self {
            Action::MoveTo(_) => "move",
            Action::CopyTo(_) => "copy",
            Action::Tag(_) => "tag",
            Action::Trash => "trash",
            Action::ByMonth(_) => "month",
            Action::Run(_) => "run",
        }
    }

    pub fn arg(&self) -> String {
        match self {
            Action::MoveTo(p) | Action::CopyTo(p) | Action::ByMonth(p) => p.to_string_lossy().to_string(),
            Action::Tag(t) | Action::Run(t) => t.clone(),
            Action::Trash => String::new(),
        }
    }

    pub fn from_parts(id: &str, arg: &str) -> Option<Action> {
        Some(match id {
            "move" => Action::MoveTo(expand(arg)),
            "copy" => Action::CopyTo(expand(arg)),
            "tag" => Action::Tag(arg.to_string()),
            "trash" => Action::Trash,
            "month" => Action::ByMonth(expand(arg)),
            "run" => Action::Run(arg.to_string()),
            _ => return None,
        })
    }

    /// Short sentence for lists and toasts.
    pub fn describe(&self) -> String {
        match self {
            Action::MoveTo(p) => trf!("move to {folder}", folder = shorten(p)),
            Action::CopyTo(p) => trf!("copy to {folder}", folder = shorten(p)),
            Action::Tag(t) => trf!("tag {color}", color = crate::tags::label(t)),
            Action::Trash => tr("move to trash"),
            Action::ByMonth(p) => trf!("sort into {folder}/YYYY-MM", folder = if p.as_os_str().is_empty() { "…".to_string() } else { shorten(p) }),
            Action::Run(c) => trf!("run {command}", command = c.clone()),
        }
    }
}

pub fn expand(arg: &str) -> PathBuf {
    let arg = arg.trim();
    if let Some(rest) = arg.strip_prefix("~/") {
        glib::home_dir().join(rest)
    } else if arg == "~" {
        glib::home_dir()
    } else {
        PathBuf::from(arg)
    }
}

pub fn shorten(p: &Path) -> String {
    let home = glib::home_dir();
    match p.strip_prefix(&home) {
        Ok(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.to_string_lossy()),
        Ok(_) => "~".into(),
        Err(_) => p.to_string_lossy().to_string(),
    }
}

pub fn parse(text: &str) -> Vec<Rule> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() < 4 || f[0].is_empty() {
                return None;
            }
            let action = Action::from_parts(f[3], f.get(4).copied().unwrap_or(""))?;
            Some(Rule { folder: expand(f[0]), glob: f[1].to_string(), older_days: f[2].parse().unwrap_or(0), action, enabled: f.get(5).map(|e| *e != "0").unwrap_or(true) })
        })
        .collect()
}

pub fn serialize(rules: &[Rule]) -> String {
    let mut out = String::from("# folder\tglob\tolder_than_days\taction\targument\tenabled\n");
    for r in rules {
        out.push_str(&format!("{}\t{}\t{}\t{}\t{}\t{}\n", r.folder.to_string_lossy(), r.glob.replace('\t', " "), r.older_days, r.action.id(), r.action.arg().replace('\t', " "), if r.enabled { 1 } else { 0 }));
    }
    out
}

pub fn load() -> Vec<Rule> {
    std::fs::read_to_string(path()).map(|t| parse(&t)).unwrap_or_default()
}

pub fn save(rules: &[Rule]) -> std::io::Result<()> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, serialize(rules))
}

/// Files in `rule.folder` that match `rule` right now (top level, no hidden files).
pub fn matches(rule: &Rule, now_unix: i64) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(&rule.folder) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name.ends_with(".part") || name.ends_with(".crdownload") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() && !matches!(rule.action, Action::MoveTo(_) | Action::CopyTo(_) | Action::Tag(_) | Action::Trash) {
            continue;
        }
        if !crate::smart::glob_matches(&rule.glob, &name) {
            continue;
        }
        if rule.older_days > 0 {
            let modified = meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(now_unix);
            if now_unix - modified < rule.older_days as i64 * 86_400 {
                continue;
            }
        }
        // Never move a folder into itself or a file onto itself.
        if let Action::MoveTo(dest) | Action::CopyTo(dest) = &rule.action {
            if dest == &rule.folder || dest.starts_with(entry.path()) {
                continue;
            }
        }
        out.push(entry.path());
    }
    out.sort();
    out
}

/// Where `file` ends up under `rule`, for moves, copies and month sorting.
pub fn destination(rule: &Rule, file: &Path, modified_unix: i64) -> Option<PathBuf> {
    let name = file.file_name()?;
    match &rule.action {
        Action::MoveTo(d) | Action::CopyTo(d) => Some(d.join(name)),
        Action::ByMonth(base) => {
            let base = if base.as_os_str().is_empty() { rule.folder.clone() } else { base.clone() };
            let dt = glib::DateTime::from_unix_local(modified_unix).ok()?;
            Some(base.join(format!("{:04}-{:02}", dt.year(), dt.month())).join(name))
        }
        _ => None,
    }
}

/// Run every enabled rule for `folder` (or all folders when None). Returns a summary for a toast.
pub async fn run(window: &Rc<Window>, only: Option<&Path>) -> Option<String> {
    let rules: Vec<Rule> = load().into_iter().filter(|r| r.enabled && only.map(|f| f == r.folder).unwrap_or(true)).collect();
    if rules.is_empty() {
        return None;
    }
    let now = glib::real_time() / 1_000_000;
    let mut moved: Vec<(gio::File, gio::File)> = Vec::new();
    let mut copied: Vec<(gio::File, gio::File)> = Vec::new();
    let mut trashed: Vec<gio::File> = Vec::new();
    let mut tagged: Vec<(gio::File, Option<String>, Option<String>)> = Vec::new();
    let mut ran = 0;
    let mut errors = 0;
    for rule in &rules {
        let r = rule.clone();
        let files = gio::spawn_blocking(move || matches(&r, now)).await.unwrap_or_default();
        for path in files {
            let src = gio::File::for_path(&path);
            let modified = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(now);
            match &rule.action {
                Action::MoveTo(_) | Action::ByMonth(_) => {
                    let Some(dest) = destination(rule, &path, modified) else { continue };
                    if dest.exists() {
                        continue;
                    }
                    if let Some(parent) = dest.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let dst = gio::File::for_path(&dest);
                    match ops::move_file(src.clone(), dst.clone()).await {
                        Ok(()) => {
                            window.tags.moved(&src, &dst);
                            moved.push((src, dst));
                        }
                        Err(_) => errors += 1,
                    }
                }
                Action::CopyTo(_) => {
                    let Some(dest) = destination(rule, &path, modified) else { continue };
                    if dest.exists() {
                        continue;
                    }
                    let _ = std::fs::create_dir_all(dest.parent().unwrap_or(&rule.folder));
                    let dst = gio::File::for_path(&dest);
                    match ops::copy_recursive(src.clone(), dst.clone()).await {
                        Ok(()) => copied.push((src, dst)),
                        Err(_) => errors += 1,
                    }
                }
                Action::Tag(color) => {
                    let old = window.tags.get(&src);
                    if old.as_deref() == Some(color.as_str()) {
                        continue;
                    }
                    match window.tags.set(&src, Some(color)) {
                        Ok(()) => tagged.push((src, old, Some(color.clone()))),
                        Err(_) => errors += 1,
                    }
                }
                Action::Trash => match ops::trash(src.clone()).await {
                    Ok(()) => {
                        window.tags.removed(&src);
                        trashed.push(src);
                    }
                    Err(_) => errors += 1,
                },
                Action::Run(command) => {
                    let ok = std::process::Command::new("bash").arg("-lc").arg(command).env("FILE", &path).current_dir(&rule.folder).stdin(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
                    if ok { ran += 1 } else { errors += 1 }
                }
            }
        }
    }
    let total = moved.len() + copied.len() + trashed.len() + tagged.len() + ran;
    if total == 0 && errors == 0 {
        return None;
    }
    // One undo entry per kind of change.
    if !moved.is_empty() {
        window.journal.record(Op::Move(moved.clone()));
    }
    if !copied.is_empty() {
        window.journal.record(Op::Copy(copied.clone()));
    }
    if !trashed.is_empty() {
        window.journal.record(Op::Trash(trashed.clone()));
    }
    if !tagged.is_empty() {
        window.journal.record(Op::Tag(tagged.clone()));
    }
    let mut parts = Vec::new();
    if !moved.is_empty() { parts.push(trf!("{n} moved", n = moved.len())); }
    if !copied.is_empty() { parts.push(trf!("{n} copied", n = copied.len())); }
    if !tagged.is_empty() { parts.push(trf!("{n} tagged", n = tagged.len())); }
    if !trashed.is_empty() { parts.push(trf!("{n} trashed", n = trashed.len())); }
    if ran > 0 { parts.push(trf!("{n} commands run", n = ran)); }
    if errors > 0 { parts.push(trf!("{n} failed", n = errors)); }
    Some(format!("{}: {}", tr("Folder rules"), parts.join(", ")))
}

/// Watch every rule folder; a change runs its rules after a short pause.
pub fn watch(window: &Rc<Window>) {
    let mut monitors = window.rule_monitors.borrow_mut();
    monitors.clear();
    let mut folders: Vec<PathBuf> = load().into_iter().filter(|r| r.enabled).map(|r| r.folder).collect();
    folders.sort();
    folders.dedup();
    for folder in folders {
        let file = gio::File::for_path(&folder);
        let Ok(monitor) = file.monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) else { continue };
        let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        let weak = Rc::downgrade(window);
        monitor.connect_changed(move |_, _, _, event| {
            if !matches!(event, gio::FileMonitorEvent::Created | gio::FileMonitorEvent::MovedIn | gio::FileMonitorEvent::ChangesDoneHint | gio::FileMonitorEvent::Renamed) {
                return;
            }
            if let Some(id) = pending.borrow_mut().take() {
                id.remove();
            }
            let weak = weak.clone();
            let folder = folder.clone();
            let pending2 = pending.clone();
            let id = glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                pending2.borrow_mut().take();
                if let Some(w) = weak.upgrade() {
                    glib::spawn_future_local(async move {
                        if let Some(summary) = run(&w, Some(&folder)).await {
                            w.toast(&summary);
                            for p in w.panes.borrow().clone() {
                                if p.cwd().path().as_deref() == Some(folder.as_path()) {
                                    p.reload();
                                }
                            }
                        }
                    });
                }
            });
            pending.borrow_mut().replace(id);
        });
        monitors.push(monitor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_rules() {
        let rules = vec![
            Rule { folder: PathBuf::from("/tmp/dl"), glob: "*.pdf".into(), older_days: 7, action: Action::MoveTo(PathBuf::from("/tmp/docs")), enabled: true },
            Rule { folder: PathBuf::from("/tmp/dl"), glob: "*.iso".into(), older_days: 0, action: Action::Tag("red".into()), enabled: false },
            Rule { folder: PathBuf::from("/tmp/shots"), glob: "".into(), older_days: 0, action: Action::ByMonth(PathBuf::new()), enabled: true },
            Rule { folder: PathBuf::from("/tmp/dl"), glob: "*.tmp".into(), older_days: 1, action: Action::Trash, enabled: true },
            Rule { folder: PathBuf::from("/tmp/dl"), glob: "*.zip".into(), older_days: 0, action: Action::Run("unzip \"$FILE\"".into()), enabled: true },
        ];
        assert_eq!(parse(&serialize(&rules)), rules);
    }

    #[test]
    fn matches_by_glob_and_age() {
        let dir = std::env::temp_dir().join(format!("omafile-rules-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.pdf"), b"x").unwrap();
        std::fs::write(dir.join("b.txt"), b"x").unwrap();
        std::fs::write(dir.join(".hidden.pdf"), b"x").unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let rule = Rule { folder: dir.clone(), glob: "*.pdf".into(), older_days: 0, action: Action::Trash, enabled: true };
        assert_eq!(matches(&rule, now), vec![dir.join("a.pdf")]);
        let old = Rule { older_days: 3, ..rule.clone() };
        assert!(matches(&old, now).is_empty());
        assert_eq!(matches(&old, now + 4 * 86_400), vec![dir.join("a.pdf")]);
        let month = Rule { folder: dir.clone(), glob: String::new(), older_days: 0, action: Action::ByMonth(PathBuf::new()), enabled: true };
        let dest = destination(&month, &dir.join("a.pdf"), 0).unwrap();
        assert!(dest.starts_with(&dir) && dest.ends_with("a.pdf"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
