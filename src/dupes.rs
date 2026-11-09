//! Duplicate finder: files below a folder with the same size and SHA-256,
//! grouped, with one "original" preselected to keep and the rest ticked for
//! the trash.

use crate::i18n::tr;
use crate::ops;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub size: u64,
    pub hash: String,
    pub files: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Keep {
    Oldest,
    Newest,
    ShortestPath,
}

/// Group `files` (path, size, modified) by size, then by hash of the
/// size-collisions only. `hash` is called for candidates; `progress` counts them.
pub fn group_by<F>(files: Vec<(PathBuf, u64)>, mut hash: F, progress: &AtomicUsize, cancel: &AtomicBool) -> Vec<Group>
where
    F: FnMut(&Path) -> Option<String>,
{
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for (path, size) in files {
        if size > 0 {
            by_size.entry(size).or_default().push(path);
        }
    }
    let mut groups = Vec::new();
    for (size, paths) in by_size {
        if paths.len() < 2 {
            continue;
        }
        let mut by_hash: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for p in paths {
            if cancel.load(Ordering::Relaxed) {
                return groups;
            }
            progress.fetch_add(1, Ordering::Relaxed);
            if let Some(h) = hash(&p) {
                by_hash.entry(h).or_default().push(p);
            }
        }
        for (hash, mut files) in by_hash {
            if files.len() > 1 {
                files.sort();
                groups.push(Group { size, hash, files });
            }
        }
    }
    // Most wasted space first.
    groups.sort_by(|a, b| (b.size * (b.files.len() as u64 - 1)).cmp(&(a.size * (a.files.len() as u64 - 1))).then(a.files[0].cmp(&b.files[0])));
    groups
}

/// Every regular file below `root` (symlinks and hidden entries skipped).
pub fn walk(root: &Path, cancel: &AtomicBool) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                out.push((entry.path(), meta.len()));
            }
        }
    }
    out
}

fn sha256_blocking(path: &Path) -> Option<String> {
    let out = std::process::Command::new("sha256sum").arg(path).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(|s| s.to_lowercase())
}

fn modified_unix(p: &Path) -> i64 {
    std::fs::metadata(p).ok().and_then(|m| m.modified().ok()).and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Index of the file to keep in `group` under `policy`.
pub fn keeper(group: &Group, policy: Keep) -> usize {
    let mut best = 0;
    for (i, f) in group.files.iter().enumerate() {
        let better = match policy {
            Keep::Oldest => modified_unix(f) < modified_unix(&group.files[best]),
            Keep::Newest => modified_unix(f) > modified_unix(&group.files[best]),
            Keep::ShortestPath => f.as_os_str().len() < group.files[best].as_os_str().len(),
        };
        if better {
            best = i;
        }
    }
    best
}

pub fn wasted(groups: &[Group]) -> u64 {
    groups.iter().map(|g| g.size * (g.files.len() as u64 - 1)).sum()
}

pub struct Finder {
    dialog: adw::Dialog,
    list: gtk::ListBox,
    status: gtk::Label,
    trash_btn: gtk::Button,
    keep: gtk::DropDown,
    groups: std::cell::RefCell<Vec<Group>>,
    checks: std::cell::RefCell<Vec<Vec<gtk::CheckButton>>>,
    cancel: Arc<AtomicBool>,
    owner: std::rc::Weak<Window>,
}

impl Finder {
    pub fn open(owner: &Rc<Window>, folder: PathBuf) {
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&adw::WindowTitle::new(&tr("Find Duplicates"), &crate::rules::shorten(&folder))));
        let keep = gtk::DropDown::from_strings(&[tr("Keep the oldest").as_str(), tr("Keep the newest").as_str(), tr("Keep the shortest path").as_str()]);
        keep.set_tooltip_text(Some(&tr("Which copy in each group stays unticked")));
        header.pack_start(&keep);
        let trash_btn = gtk::Button::with_label(&tr("Move Ticked to Trash"));
        trash_btn.add_css_class("destructive-action");
        trash_btn.set_sensitive(false);
        header.pack_end(&trash_btn);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        list.set_margin_start(12);
        list.set_margin_end(12);
        list.set_margin_top(12);
        list.set_margin_bottom(12);
        let scroll = gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
        let status = gtk::Label::builder().label(tr("Scanning…")).css_classes(["omafile-dim", "caption"]).margin_bottom(8).wrap(true).build();
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&scroll));
        view.add_bottom_bar(&status);
        let dialog = adw::Dialog::builder().title(tr("Find Duplicates")).content_width(760).content_height(640).child(&view).build();
        let this = Rc::new(Finder { dialog, list, status, trash_btn, keep, groups: Default::default(), checks: Default::default(), cancel: Arc::new(AtomicBool::new(false)), owner: Rc::downgrade(owner) });
        // The dialog keeps the finder alive until it closes.
        let keep = std::cell::RefCell::new(Some(this.clone()));
        let cancel = this.cancel.clone();
        this.dialog.connect_closed(move |_| {
            cancel.store(true, Ordering::Relaxed);
            keep.borrow_mut().take();
        });
        let weak = Rc::downgrade(&this);
        this.keep.connect_selected_notify(move |_| { if let Some(f) = weak.upgrade() { f.apply_policy(); } });
        let weak = Rc::downgrade(&this);
        this.trash_btn.connect_clicked(move |_| { if let Some(f) = weak.upgrade() { f.trash_ticked(); } });
        owner.present_dialog(&this.dialog);

        let progress = Arc::new(AtomicUsize::new(0));
        let (cancel, progress2) = (this.cancel.clone(), progress.clone());
        let weak = Rc::downgrade(&this);
        // Progress line while hashing.
        let ticker = glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            match weak.upgrade() {
                Some(f) => {
                    let n = progress2.load(Ordering::Relaxed);
                    if n > 0 {
                        f.status.set_text(&trf!("Hashing {n} candidates…", n = n));
                    }
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            }
        });
        let weak = Rc::downgrade(&this);
        glib::spawn_future_local(async move {
            let groups = gio::spawn_blocking(move || {
                let files = walk(&folder, &cancel);
                group_by(files, sha256_blocking, &progress, &cancel)
            })
            .await
            .unwrap_or_default();
            ticker.remove();
            let Some(f) = weak.upgrade() else { return };
            f.groups.replace(groups);
            f.rebuild();
        });
    }

    fn policy(&self) -> Keep {
        match self.keep.selected() {
            1 => Keep::Newest,
            2 => Keep::ShortestPath,
            _ => Keep::Oldest,
        }
    }

    fn rebuild(self: &Rc<Self>) {
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        let groups = self.groups.borrow().clone();
        let mut checks = Vec::new();
        for g in &groups {
            let header = gtk::Label::builder().xalign(0.0).css_classes(["heading"]).label(format!("{} × {}  ·  {}", g.files.len(), ops::human_size(g.size), g.files[0].file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())).margin_top(10).margin_start(12).margin_bottom(4).build();
            let header_row = gtk::ListBoxRow::builder().child(&header).activatable(false).selectable(false).build();
            self.list.append(&header_row);
            let mut group_checks = Vec::new();
            for file in &g.files {
                let check = gtk::CheckButton::new();
                check.set_valign(gtk::Align::Center);
                let row = adw::ActionRow::builder().title(glib::markup_escape_text(&crate::rules::shorten(file))).subtitle(glib::markup_escape_text(&glib::DateTime::from_unix_local(modified_unix(file)).map(|d| ops::format_time(&d)).unwrap_or_default())).activatable_widget(&check).build();
                row.add_prefix(&check);
                let weak = Rc::downgrade(self);
                check.connect_toggled(move |_| { if let Some(f) = weak.upgrade() { f.update_summary(); } });
                let reveal = gtk::Button::builder().icon_name("folder-open-symbolic").valign(gtk::Align::Center).tooltip_text(tr("Reveal in omafile")).build();
                reveal.add_css_class("flat");
                let weak = Rc::downgrade(self);
                let p = file.clone();
                reveal.connect_clicked(move |_| {
                    if let Some(f) = weak.upgrade() {
                        if let Some(w) = f.owner.upgrade() {
                            w.reveal(gio::File::for_path(&p));
                        }
                        f.dialog.close();
                    }
                });
                row.add_suffix(&reveal);
                self.list.append(&row);
                group_checks.push(check);
            }
            checks.push(group_checks);
        }
        self.checks.replace(checks);
        if groups.is_empty() {
            let empty = adw::StatusPage::builder().icon_name("emblem-ok-symbolic").title(tr("No duplicates")).description(tr("Every file below this folder has unique contents.")).build();
            empty.add_css_class("compact");
            self.list.append(&gtk::ListBoxRow::builder().child(&empty).activatable(false).selectable(false).build());
        }
        self.apply_policy();
    }

    fn apply_policy(&self) {
        let policy = self.policy();
        let groups = self.groups.borrow();
        for (g, checks) in groups.iter().zip(self.checks.borrow().iter()) {
            let keep = keeper(g, policy);
            for (i, c) in checks.iter().enumerate() {
                c.set_active(i != keep);
            }
        }
        drop(groups);
        self.update_summary();
    }

    fn ticked(&self) -> Vec<PathBuf> {
        let groups = self.groups.borrow();
        let mut out = Vec::new();
        for (g, checks) in groups.iter().zip(self.checks.borrow().iter()) {
            for (f, c) in g.files.iter().zip(checks.iter()) {
                if c.is_active() {
                    out.push(f.clone());
                }
            }
        }
        out
    }

    fn update_summary(&self) {
        let groups = self.groups.borrow();
        let ticked = self.ticked();
        let ticked_bytes: u64 = groups.iter().zip(self.checks.borrow().iter()).map(|(g, checks)| g.size * checks.iter().filter(|c| c.is_active()).count() as u64).sum();
        self.trash_btn.set_sensitive(!ticked.is_empty());
        self.status.set_text(&if groups.is_empty() {
            tr("No duplicates found")
        } else {
            trf!("{groups} groups, {wasted} in extra copies  ·  {ticked} ticked ({bytes})", groups = groups.len(), wasted = ops::human_size(wasted(&groups)), ticked = ticked.len(), bytes = ops::human_size(ticked_bytes))
        });
    }

    fn trash_ticked(self: &Rc<Self>) {
        let Some(w) = self.owner.upgrade() else { return };
        let files: Vec<gio::File> = self.ticked().iter().map(gio::File::for_path).collect();
        if files.is_empty() {
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let mut done = Vec::new();
            let mut error = None;
            for f in &files {
                match ops::trash(f.clone()).await {
                    Ok(()) => {
                        w.tags.removed(f);
                        done.push(f.clone());
                    }
                    Err(e) => {
                        error = Some(e.message().to_string());
                        break;
                    }
                }
            }
            let n = done.len();
            if n > 0 {
                w.record(Op::Trash(done.clone()), &trf!("Moved {n} duplicates to trash", n = n));
            }
            if let Some(e) = error {
                w.toast_error(&e);
            }
            // Drop trashed files from the groups and redraw.
            let gone: Vec<PathBuf> = done.iter().filter_map(|f| f.path()).collect();
            let mut groups = this.groups.borrow().clone();
            for g in groups.iter_mut() {
                g.files.retain(|f| !gone.contains(f));
            }
            groups.retain(|g| g.files.len() > 1);
            this.groups.replace(groups);
            this.rebuild();
            for p in w.panes.borrow().clone() {
                p.reload();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_size_collisions_by_hash() {
        let files = vec![
            (PathBuf::from("/a/1.txt"), 10),
            (PathBuf::from("/a/2.txt"), 10),
            (PathBuf::from("/b/3.txt"), 10),
            (PathBuf::from("/a/4.txt"), 20),
            (PathBuf::from("/a/5.txt"), 0),
        ];
        let progress = AtomicUsize::new(0);
        let groups = group_by(files, |p| Some(if p.ends_with("3.txt") { "h2".into() } else { "h1".into() }), &progress, &AtomicBool::new(false));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].files, vec![PathBuf::from("/a/1.txt"), PathBuf::from("/a/2.txt")]);
        assert_eq!(progress.load(Ordering::Relaxed), 3, "only size collisions are hashed");
        assert_eq!(wasted(&groups), 10);
    }

    #[test]
    fn keeper_prefers_the_shortest_path() {
        let g = Group { size: 1, hash: "h".into(), files: vec![PathBuf::from("/a/very/long/x.txt"), PathBuf::from("/a/x.txt")] };
        assert_eq!(keeper(&g, Keep::ShortestPath), 1);
    }
}
