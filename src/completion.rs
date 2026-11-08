//! Path completion for the location entry (Ctrl+L): a popover under the entry
//! lists matching folders as you type; Tab accepts, Up/Down pick, Enter opens.

use adw::prelude::*;
use gtk::{gio, glib, pango};
use std::cell::RefCell;
use std::rc::Rc;

const MAX_SUGGESTIONS: usize = 10;

pub struct Completion {
    pub popover: gtk::Popover,
    pub list: gtk::ListBox,
    entry: gtk::Entry,
    pending: RefCell<Option<glib::SourceId>>,
    candidates: RefCell<Vec<String>>,
}

/// Expand `~` and resolve relative input against `cwd`; returns (dir, prefix, typed-dir-text).
fn split_input(text: &str, cwd: &gio::File) -> Option<(std::path::PathBuf, String, String)> {
    if text.contains("://") {
        return None;
    }
    let expanded = if let Some(rest) = text.strip_prefix('~') {
        format!("{}{}", glib::home_dir().display(), rest)
    } else {
        text.to_string()
    };
    let (dir_text, prefix) = match expanded.rfind('/') {
        Some(i) => (expanded[..=i].to_string(), expanded[i + 1..].to_string()),
        None => (String::new(), expanded.clone()),
    };
    let typed_dir = match text.rfind('/') {
        Some(i) => text[..=i].to_string(),
        None => String::new(),
    };
    let dir = if dir_text.is_empty() {
        cwd.path()?
    } else if dir_text.starts_with('/') {
        std::path::PathBuf::from(&dir_text)
    } else {
        cwd.path()?.join(&dir_text)
    };
    Some((dir, prefix, typed_dir))
}

impl Completion {
    pub fn new(entry: &gtk::Entry) -> Rc<Self> {
        let list = gtk::ListBox::new();
        list.add_css_class("omafile-completions");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_can_focus(false);
        let scroll = gtk::ScrolledWindow::builder().child(&list).hscrollbar_policy(gtk::PolicyType::Never).propagate_natural_height(true).max_content_height(320).build();
        let popover = gtk::Popover::builder().child(&scroll).autohide(false).has_arrow(false).position(gtk::PositionType::Bottom).build();
        popover.set_parent(entry);
        popover.add_css_class("menu");
        let this = Rc::new(Completion { popover, list, entry: entry.clone(), pending: RefCell::new(None), candidates: RefCell::new(Vec::new()) });
        let weak = Rc::downgrade(&this);
        this.list.connect_row_activated(move |_, row| {
            if let Some(c) = weak.upgrade() {
                c.accept(row.index() as usize, true);
            }
        });
        this
    }

    /// Recompute suggestions for the entry text (debounced).
    pub fn refresh(self: &Rc<Self>, cwd: gio::File) {
        if let Some(id) = self.pending.borrow_mut().take() {
            id.remove();
        }
        let weak = Rc::downgrade(self);
        *self.pending.borrow_mut() = Some(glib::timeout_add_local_once(std::time::Duration::from_millis(60), move || {
            let Some(c) = weak.upgrade() else { return };
            c.pending.borrow_mut().take();
            c.compute(&cwd);
        }));
    }

    fn compute(&self, cwd: &gio::File) {
        let text = self.entry.text().to_string();
        let Some((dir, prefix, typed_dir)) = split_input(&text, cwd) else {
            self.hide();
            return;
        };
        let lower = prefix.to_lowercase();
        let show_hidden = prefix.starts_with('.');
        let mut names: Vec<String> = match std::fs::read_dir(&dir) {
            Ok(entries) => entries
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir() || (t.is_symlink() && e.path().is_dir())).unwrap_or(false))
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| (show_hidden || !n.starts_with('.')) && n.to_lowercase().starts_with(&lower))
                .collect(),
            Err(_) => Vec::new(),
        };
        names.sort_by_key(|n| n.to_lowercase());
        names.truncate(MAX_SUGGESTIONS);
        // Exactly the typed folder and nothing else: nothing to add.
        if names.is_empty() || (names.len() == 1 && names[0].to_lowercase() == lower && text.ends_with('/')) {
            self.hide();
            return;
        }
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        for name in &names {
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            hbox.set_margin_start(6);
            hbox.set_margin_end(6);
            hbox.append(&gtk::Image::from_icon_name("folder-symbolic"));
            hbox.append(&gtk::Label::builder().label(name).xalign(0.0).ellipsize(pango::EllipsizeMode::Middle).build());
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&hbox));
            self.list.append(&row);
        }
        *self.candidates.borrow_mut() = names.into_iter().map(|n| format!("{typed_dir}{n}/")).collect();
        self.list.select_row(self.list.row_at_index(0).as_ref());
        let width = self.entry.width().max(240);
        self.popover.set_size_request(width, -1);
        self.popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(0, self.entry.height(), width, 1)));
        if !self.popover.is_visible() {
            self.popover.popup();
        }
    }

    pub fn is_open(&self) -> bool {
        self.popover.is_visible()
    }

    pub fn hide(&self) {
        if self.popover.is_visible() {
            self.popover.popdown();
        }
    }

    /// Move the highlighted suggestion by `delta` rows.
    pub fn step(&self, delta: i32) {
        let n = self.candidates.borrow().len() as i32;
        if n == 0 {
            return;
        }
        let current = self.list.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = (current + delta).rem_euclid(n);
        self.list.select_row(self.list.row_at_index(next).as_ref());
    }

    /// Put the highlighted suggestion into the entry; `activate` also submits it.
    pub fn accept_selected(&self, activate: bool) -> bool {
        match self.list.selected_row() {
            Some(row) => {
                self.accept(row.index() as usize, activate);
                true
            }
            None => false,
        }
    }

    fn accept(&self, index: usize, activate: bool) {
        let Some(text) = self.candidates.borrow().get(index).cloned() else { return };
        self.entry.set_text(&text);
        self.entry.set_position(-1);
        self.hide();
        if activate {
            self.entry.emit_activate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_input() {
        let cwd = gio::File::for_path("/tmp");
        let (dir, prefix, typed) = split_input("/usr/sh", &cwd).unwrap();
        assert_eq!(dir, std::path::PathBuf::from("/usr/"));
        assert_eq!(prefix, "sh");
        assert_eq!(typed, "/usr/");
        let (dir, prefix, _) = split_input("doc", &cwd).unwrap();
        assert_eq!(dir, std::path::PathBuf::from("/tmp"));
        assert_eq!(prefix, "doc");
        assert!(split_input("sftp://host/", &cwd).is_none());
        let (_, _, typed) = split_input("~/Pro", &cwd).unwrap();
        assert_eq!(typed, "~/");
    }
}
