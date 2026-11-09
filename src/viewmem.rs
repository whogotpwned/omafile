//! Per-folder view memory, Finder style but kept in one file instead of
//! `.DS_Store` everywhere: `~/.config/omafile/views`, one line per folder:
//! `uri<TAB>view<TAB>sort<TAB>order<TAB>group<TAB>grid`.

use gtk::glib;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct ViewPrefs {
    pub view: String,
    pub sort: String,
    pub descending: bool,
    pub group: String,
    pub grid: i32,
}

pub fn path() -> PathBuf {
    glib::user_config_dir().join("omafile/views")
}

pub fn parse(text: &str) -> HashMap<String, ViewPrefs> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() < 6 || f[0].is_empty() {
                return None;
            }
            Some((f[0].to_string(), ViewPrefs { view: f[1].to_string(), sort: f[2].to_string(), descending: f[3] == "desc", group: f[4].to_string(), grid: f[5].parse().unwrap_or(72) }))
        })
        .collect()
}

pub fn serialize(map: &HashMap<String, ViewPrefs>) -> String {
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    let mut out = String::from("# uri\tview\tsort\torder\tgroup\tgrid\n");
    for k in keys {
        let v = &map[k];
        out.push_str(&format!("{k}\t{}\t{}\t{}\t{}\t{}\n", v.view, v.sort, if v.descending { "desc" } else { "asc" }, v.group, v.grid));
    }
    out
}

pub fn load() -> HashMap<String, ViewPrefs> {
    std::fs::read_to_string(path()).map(|t| parse(&t)).unwrap_or_default()
}

pub fn get(uri: &str) -> Option<ViewPrefs> {
    load().remove(uri)
}

/// Remember `prefs` for `uri`; a folder that matches the defaults is dropped
/// from the file so it keeps following the global settings.
pub fn set(uri: &str, prefs: ViewPrefs, defaults: &ViewPrefs) -> std::io::Result<()> {
    let mut map = load();
    if &prefs == defaults {
        if map.remove(uri).is_none() {
            return Ok(());
        }
    } else if map.get(uri) == Some(&prefs) {
        return Ok(());
    } else {
        map.insert(uri.to_string(), prefs);
    }
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, serialize(&map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let mut map = HashMap::new();
        map.insert("file:///home/u/Pictures".to_string(), ViewPrefs { view: "grid".into(), sort: "modified".into(), descending: true, group: "date".into(), grid: 120 });
        map.insert("sftp://host/srv".to_string(), ViewPrefs { view: "list".into(), sort: "size".into(), descending: false, group: "none".into(), grid: 72 });
        assert_eq!(parse(&serialize(&map)), map);
    }
}
