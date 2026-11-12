//! Tiny preferences store: `~/.config/omafile/omafile.conf`, `key = value` lines.
//! Only things the Omarchy theme cannot decide for us live here.

use gtk::glib;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Default)]
pub struct Prefs {
    values: RefCell<BTreeMap<String, String>>,
}

fn path() -> PathBuf {
    glib::user_config_dir().join("omafile/omafile.conf")
}

impl Prefs {
    pub fn load() -> Rc<Self> {
        let prefs = Prefs::default();
        // A fresh installation: omafile has never kept anything on this computer. Noted
        // before anything is written, and remembered, so that the tour is shown with the
        // first window even when this first start has none (a start from the bar's menu,
        // or by another program asking to show a folder).
        let fresh = is_fresh(&glib::user_config_dir().join("omafile"), &glib::user_state_dir().join("omafile"));
        if let Ok(text) = std::fs::read_to_string(path()) {
            let mut values = prefs.values.borrow_mut();
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    values.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
        if fresh {
            prefs.set("tour", Some("pending"));
        }
        Rc::new(prefs)
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.values.borrow().get(key).cloned().filter(|v| !v.is_empty())
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        match self.get(key).as_deref() {
            Some("true") | Some("1") | Some("yes") => true,
            Some("false") | Some("0") | Some("no") => false,
            _ => default,
        }
    }

    pub fn set(&self, key: &str, value: Option<&str>) {
        {
            let mut values = self.values.borrow_mut();
            match value {
                Some(v) if !v.is_empty() => values.insert(key.to_string(), v.to_string()),
                _ => values.remove(key),
            };
        }
        self.save();
    }

    fn save(&self) {
        let p = path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut text = String::from("# omafile preferences. Everything else follows the Omarchy theme.\n");
        for (k, v) in self.values.borrow().iter() {
            text.push_str(&format!("{k} = {v}\n"));
        }
        let _ = std::fs::write(p, text);
    }
}

/// Whether omafile has never run here: neither its configuration nor its state folder holds
/// anything. An upgrade from any earlier version leaves at least one of them filled.
pub fn is_fresh(config: &std::path::Path, state: &std::path::Path) -> bool {
    let empty = |dir: &std::path::Path| std::fs::read_dir(dir).map(|mut entries| entries.next().is_none()).unwrap_or(true);
    empty(config) && empty(state)
}

/// Icon themes installed on this system (directories with an index.theme).
pub fn installed_icon_themes() -> Vec<String> {
    let mut dirs: Vec<PathBuf> = vec![glib::home_dir().join(".icons"), glib::user_data_dir().join("icons")];
    for d in glib::system_data_dirs() {
        dirs.push(d.join("icons"));
    }
    let mut names = std::collections::BTreeSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            let index = p.join("index.theme");
            if !index.is_file() {
                continue;
            }
            // Cursor-only themes have no icon directories; skip them.
            let Ok(text) = std::fs::read_to_string(&index) else { continue };
            if !text.contains("Directories=") {
                continue;
            }
            if let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_string()) {
                if name != "default" && name != "hicolor" {
                    names.insert(name);
                }
            }
        }
    }
    names.into_iter().collect()
}

#[cfg(test)]
mod fresh_tests {
    use super::*;

    #[test]
    fn an_installation_is_fresh_until_omafile_has_kept_something() {
        let root = std::env::temp_dir().join(format!("omafile-fresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (config, state) = (root.join("config/omafile"), root.join("state/omafile"));
        assert!(is_fresh(&config, &state), "nothing there at all");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        assert!(is_fresh(&config, &state), "empty folders are still nothing");
        std::fs::write(state.join("status"), "busy=0\n").unwrap();
        assert!(!is_fresh(&config, &state), "an earlier version ran here: an upgrade, not a first start");
        std::fs::remove_file(state.join("status")).unwrap();
        std::fs::write(config.join("bookmarks"), "").unwrap();
        assert!(!is_fresh(&config, &state));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
