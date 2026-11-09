//! Finder-style colour tags for files and folders.
//!
//! A tag is stored twice: as GIO metadata on the file (`metadata::omafile-tag`,
//! moves with the file through GIO and gvfs) and in `~/.config/omafile/tags`
//! (`uri<TAB>colour` lines) so the sidebar can list everything with a tag
//! without crawling the disk.

use gtk::{gio, glib};
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

pub const ATTRIBUTE: &str = "metadata::omafile-tag";

/// Name, palette key and a readable fallback colour (used when the theme has no such key).
pub const COLORS: &[(&str, &str, &str)] = &[
    ("red", "red", "#f7768e"),
    ("orange", "orange", "#eb927b"),
    ("yellow", "yellow", "#e0af68"),
    ("green", "green", "#9ece6a"),
    ("blue", "blue", "#7aa2f7"),
    ("purple", "magenta", "#ad8ee6"),
    ("gray", "muted", "#565f89"),
];

pub fn is_color(name: &str) -> bool {
    COLORS.iter().any(|(n, _, _)| *n == name)
}

pub fn label(name: &str) -> String {
    let mut c = name.chars();
    let cap = match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    };
    crate::i18n::tr(&cap)
}

fn index_path() -> PathBuf {
    glib::user_config_dir().join("omafile/tags")
}

#[derive(Default)]
pub struct TagIndex {
    entries: RefCell<BTreeMap<String, String>>,
}

impl TagIndex {
    pub fn load() -> Rc<Self> {
        let index = TagIndex::default();
        if let Ok(text) = std::fs::read_to_string(index_path()) {
            let mut entries = index.entries.borrow_mut();
            for line in text.lines() {
                if let Some((uri, color)) = line.split_once('\t') {
                    if is_color(color.trim()) {
                        entries.insert(uri.to_string(), color.trim().to_string());
                    }
                }
            }
        }
        Rc::new(index)
    }

    fn save(&self) {
        let p = index_path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text: String = self.entries.borrow().iter().map(|(u, c)| format!("{u}\t{c}\n")).collect();
        let _ = std::fs::write(p, text);
    }

    pub fn get(&self, file: &gio::File) -> Option<String> {
        self.entries.borrow().get(file.uri().as_str()).cloned()
    }

    /// Set or clear the tag; writes the metadata and the index.
    pub fn set(&self, file: &gio::File, color: Option<&str>) -> Result<(), glib::Error> {
        match color {
            Some(c) => {
                if file.is_native() {
                    file.set_attribute_string(ATTRIBUTE, c, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)?;
                }
                self.entries.borrow_mut().insert(file.uri().to_string(), c.to_string());
            }
            None => {
                if file.is_native() {
                    let _ = file.set_attribute_string(ATTRIBUTE, "", gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE);
                }
                self.entries.borrow_mut().remove(file.uri().as_str());
            }
        }
        self.save();
        Ok(())
    }

    /// Keep the index in sync when omafile itself moves or renames a file.
    pub fn moved(&self, from: &gio::File, to: &gio::File) {
        let from_uri = from.uri().to_string();
        let to_uri = to.uri().to_string();
        let mut entries = self.entries.borrow_mut();
        let moved: Vec<(String, String)> = entries
            .iter()
            .filter(|(u, _)| **u == from_uri || u.starts_with(&format!("{from_uri}/")))
            .map(|(u, c)| (format!("{to_uri}{}", &u[from_uri.len()..]), c.clone()))
            .collect();
        if moved.is_empty() {
            return;
        }
        entries.retain(|u, _| !(*u == from_uri || u.starts_with(&format!("{from_uri}/"))));
        entries.extend(moved);
        drop(entries);
        self.save();
    }

    pub fn removed(&self, file: &gio::File) {
        let uri = file.uri().to_string();
        let before = self.entries.borrow().len();
        self.entries.borrow_mut().retain(|u, _| !(*u == uri || u.starts_with(&format!("{uri}/"))));
        if self.entries.borrow().len() != before {
            self.save();
        }
    }

    /// Every file carrying `color`, dropping entries whose file is gone.
    pub fn files_with(&self, color: &str) -> Vec<gio::File> {
        let mut gone = Vec::new();
        let files: Vec<gio::File> = self
            .entries
            .borrow()
            .iter()
            .filter(|(_, c)| c.as_str() == color)
            .map(|(u, _)| gio::File::for_uri(u))
            .filter(|f| {
                let exists = !f.is_native() || f.query_exists(gio::Cancellable::NONE);
                if !exists {
                    gone.push(f.uri().to_string());
                }
                exists
            })
            .collect();
        if !gone.is_empty() {
            let mut entries = self.entries.borrow_mut();
            for u in gone {
                entries.remove(&u);
            }
            drop(entries);
            self.save();
        }
        files
    }

    pub fn colors_in_use(&self) -> Vec<String> {
        let entries = self.entries.borrow();
        COLORS.iter().map(|(n, _, _)| n.to_string()).filter(|n| entries.values().any(|c| c == n)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_follow_prefix() {
        let index = TagIndex::default();
        index.entries.borrow_mut().insert("file:///a/dir".into(), "red".into());
        index.entries.borrow_mut().insert("file:///a/dir/x.txt".into(), "blue".into());
        index.entries.borrow_mut().insert("file:///a/dir2".into(), "green".into());
        index.moved(&gio::File::for_uri("file:///a/dir"), &gio::File::for_uri("file:///b/moved"));
        let e = index.entries.borrow();
        assert_eq!(e.get("file:///b/moved").map(String::as_str), Some("red"));
        assert_eq!(e.get("file:///b/moved/x.txt").map(String::as_str), Some("blue"));
        assert_eq!(e.get("file:///a/dir2").map(String::as_str), Some("green"));
    }
}
