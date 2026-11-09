//! Command palette (Ctrl+K): fuzzy search over actions, places, recent folders,
//! project commands and bookmarks; `>` runs a shell command in a terminal here.

use crate::i18n::tr;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
pub enum Target {
    Action(String),
    Folder(gio::File),
    Connect(gio::File),
    Project(String),
    Command(String),
    /// Open the folder of this file and select it.
    Reveal(gio::File),
    /// A bulk command typed into the palette (rename / chmod / touch / mkdir).
    Bulk(String),
}

#[derive(Clone)]
pub struct Entry {
    pub title: String,
    pub detail: String,
    pub icon: &'static str,
    pub target: Target,
}

/// Subsequence match: every query character in order; bonus for prefix and word starts.
pub fn score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let text_l = text.to_lowercase();
    let query_l = query.to_lowercase();
    if text_l.starts_with(&query_l) {
        return Some(1000 - text.len() as i32);
    }
    if let Some(pos) = text_l.find(&query_l) {
        return Some(500 - pos as i32 - text.len() as i32 / 4);
    }
    let mut score = 0;
    let mut last = None::<usize>;
    let mut chars = text_l.char_indices();
    for q in query_l.chars() {
        let found = chars.by_ref().find(|(_, c)| *c == q)?;
        let i = found.0;
        score += match last {
            Some(l) if i == l + 1 => 12,
            _ => 1,
        };
        if i == 0 || text_l.as_bytes()[i - 1] == b' ' || text_l.as_bytes()[i - 1] == b'/' {
            score += 6;
        }
        last = Some(i);
    }
    Some(score)
}

pub struct Palette {
    dialog: adw::Dialog,
    entry: gtk::Entry,
    list: gtk::ListBox,
    entries: RefCell<Vec<Entry>>,
    shown: RefCell<Vec<Entry>>,
    window: std::rc::Weak<Window>,
}

impl Palette {
    pub fn new(window: &Rc<Window>) -> Rc<Self> {
        let entry = gtk::Entry::builder().placeholder_text(tr("Type an action, a folder, a project command, or > a shell command")).hexpand(true).build();
        entry.add_css_class("omafile-palette-entry");
        let list = gtk::ListBox::new();
        list.add_css_class("omafile-palette-list");
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_can_focus(false);
        let scroll = gtk::ScrolledWindow::builder().child(&list).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build();
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
        vbox.set_margin_top(12);
        vbox.set_margin_bottom(12);
        vbox.set_margin_start(12);
        vbox.set_margin_end(12);
        vbox.append(&entry);
        vbox.append(&scroll);
        let dialog = adw::Dialog::builder().title(tr("Command Palette")).child(&vbox).content_width(640).content_height(480).build();
        dialog.add_css_class("omafile-palette");
        let this = Rc::new(Palette { dialog, entry: entry.clone(), list: list.clone(), entries: RefCell::new(Vec::new()), shown: RefCell::new(Vec::new()), window: Rc::downgrade(window) });

        let weak = Rc::downgrade(&this);
        entry.connect_changed(move |_| {
            if let Some(p) = weak.upgrade() {
                p.filter();
            }
        });
        let weak = Rc::downgrade(&this);
        entry.connect_activate(move |_| {
            if let Some(p) = weak.upgrade() {
                p.run_selected();
            }
        });
        let weak = Rc::downgrade(&this);
        list.connect_row_activated(move |_, row| {
            if let Some(p) = weak.upgrade() {
                p.list.select_row(Some(row));
                p.run_selected();
            }
        });
        let weak = Rc::downgrade(&this);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(p) = weak.upgrade() else { return glib::Propagation::Proceed };
            match key {
                gdk::Key::Down | gdk::Key::Tab => {
                    p.step(1);
                    glib::Propagation::Stop
                }
                gdk::Key::Up | gdk::Key::ISO_Left_Tab => {
                    p.step(-1);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        this.dialog.add_controller(keys);
        this
    }

    pub fn open(self: &Rc<Self>, entries: Vec<Entry>) {
        *self.entries.borrow_mut() = entries;
        self.entry.set_text("");
        self.filter();
        if let Some(w) = self.window.upgrade() {
            w.present_dialog(&self.dialog);
        }
        self.entry.grab_focus();
    }

    fn filter(&self) {
        let query = self.entry.text().to_string();
        let mut shown: Vec<(i32, Entry)> = Vec::new();
        if let Some(cmd) = query.strip_prefix('>') {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                shown.push((0, Entry { title: trf!("Run in terminal: {cmd}", cmd = cmd), detail: tr("Opens a terminal in this folder"), icon: "utilities-terminal-symbolic", target: Target::Command(cmd.into()) }));
            }
        } else if let Some((title, detail)) = self.window.upgrade().and_then(|w| w.bulk_preview(&query)) {
            shown.push((0, Entry { title, detail, icon: "edit-find-replace-symbolic", target: Target::Bulk(query.clone()) }));
        } else {
            for e in self.entries.borrow().iter() {
                if let Some(s) = score(&query, &e.title).or_else(|| score(&query, &e.detail).map(|s| s - 200)) {
                    shown.push((s, e.clone()));
                }
            }
            shown.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
            shown.truncate(40);
        }
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        for (_, e) in &shown {
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            hbox.set_margin_top(4);
            hbox.set_margin_bottom(4);
            hbox.set_margin_start(6);
            hbox.set_margin_end(6);
            hbox.append(&gtk::Image::from_icon_name(e.icon));
            let title = gtk::Label::builder().label(&e.title).xalign(0.0).hexpand(true).ellipsize(pango::EllipsizeMode::Middle).build();
            hbox.append(&title);
            let detail = gtk::Label::builder().label(&e.detail).xalign(1.0).ellipsize(pango::EllipsizeMode::Start).max_width_chars(28).css_classes(["omafile-dim", "caption", "numeric"]).build();
            hbox.append(&detail);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&hbox));
            self.list.append(&row);
        }
        self.list.select_row(self.list.row_at_index(0).as_ref());
        *self.shown.borrow_mut() = shown.into_iter().map(|(_, e)| e).collect();
    }

    fn step(&self, delta: i32) {
        let n = self.shown.borrow().len() as i32;
        if n == 0 {
            return;
        }
        let current = self.list.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = (current + delta).rem_euclid(n);
        if let Some(row) = self.list.row_at_index(next) {
            self.list.select_row(Some(&row));
            row.grab_focus();
            self.entry.grab_focus_without_selecting();
        }
    }

    fn run_selected(self: &Rc<Self>) {
        let index = self.list.selected_row().map(|r| r.index()).unwrap_or(0) as usize;
        let Some(entry) = self.shown.borrow().get(index).cloned() else { return };
        self.dialog.close();
        if let Some(w) = self.window.upgrade() {
            w.run_palette_target(entry.target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoring() {
        assert!(score("new", "New Folder").unwrap() > score("new", "Open in New Tab").unwrap());
        assert!(score("nf", "New Folder").is_some());
        assert!(score("xyz", "New Folder").is_none());
        assert_eq!(score("", "anything"), Some(0));
    }
}
