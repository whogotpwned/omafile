//! Notes on files and folders (Finder "Comments"): stored as the GIO
//! attribute `metadata::annotation`, which Nautilus uses for its notes too,
//! plus an index at `~/.config/omafile/notes` so the palette can search them.

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

pub const ATTRIBUTE: &str = "metadata::annotation";

pub struct Notes {
    index: RefCell<BTreeMap<String, String>>,
}

fn path() -> PathBuf {
    glib::user_config_dir().join("omafile/notes")
}

impl Notes {
    pub fn load() -> Rc<Self> {
        let mut index = BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(path()) {
            for line in text.lines() {
                if let Some((uri, note)) = line.split_once('\t') {
                    index.insert(uri.to_string(), note.replace("\\n", "\n"));
                }
            }
        }
        Rc::new(Notes { index: RefCell::new(index) })
    }

    fn save(&self) {
        let p = path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text: String = self.index.borrow().iter().map(|(u, n)| format!("{u}\t{}\n", n.replace('\n', "\\n"))).collect();
        let _ = std::fs::write(p, text);
    }

    /// Index only, no file access: for list rows.
    pub fn cached(&self, file: &gio::File) -> Option<String> {
        self.index.borrow().get(file.uri().as_str()).cloned()
    }

    pub fn get(&self, file: &gio::File) -> Option<String> {
        if let Some(n) = self.index.borrow().get(file.uri().as_str()) {
            return Some(n.clone());
        }
        let info = file.query_info(ATTRIBUTE, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE).ok()?;
        info.attribute_string(ATTRIBUTE).map(|s| s.to_string()).filter(|s| !s.is_empty())
    }

    pub fn set(&self, file: &gio::File, note: &str) -> Result<(), glib::Error> {
        let note = note.trim();
        // An empty string clears the note (GIO metadata has no separate unset for strings).
        file.set_attribute_string(ATTRIBUTE, note, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)?;
        let mut index = self.index.borrow_mut();
        if note.is_empty() {
            index.remove(file.uri().as_str());
        } else {
            index.insert(file.uri().to_string(), note.to_string());
        }
        drop(index);
        self.save();
        Ok(())
    }

    pub fn moved(&self, from: &gio::File, to: &gio::File) {
        let note = self.index.borrow_mut().remove(from.uri().as_str());
        if let Some(n) = note {
            self.index.borrow_mut().insert(to.uri().to_string(), n);
            self.save();
        }
    }

    /// (file, note) pairs whose note contains `query` (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<(gio::File, String)> {
        let q = query.to_lowercase();
        self.index.borrow().iter().filter(|(_, n)| q.is_empty() || n.to_lowercase().contains(&q)).map(|(u, n)| (gio::File::for_uri(u), n.clone())).collect()
    }

    pub fn all(&self) -> Vec<(gio::File, String)> {
        self.search("")
    }
}
