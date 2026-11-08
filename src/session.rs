//! Save and restore the window layout (tabs per side, split, terminal panels,
//! size) in `~/.local/state/omafile/session`.

use gtk::glib;
use std::path::PathBuf;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Session {
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    pub split: bool,
    pub sidebar: bool,
    pub active_side: usize,
    /// For each side: (folder uri, terminal panel open) per tab, and the selected tab.
    pub sides: [Vec<(String, bool)>; 2],
    pub selected: [usize; 2],
}

pub fn path() -> PathBuf {
    glib::user_state_dir().join("omafile/session")
}

impl Session {
    pub fn serialize(&self) -> String {
        let mut out = format!("size={} {}\nmaximized={}\nsplit={}\nsidebar={}\nactive={}\n", self.width, self.height, self.maximized as u8, self.split as u8, self.sidebar as u8, self.active_side);
        for side in 0..2 {
            out.push_str(&format!("selected{side}={}\n", self.selected[side]));
            for (uri, term) in &self.sides[side] {
                out.push_str(&format!("tab{side}={}\t{uri}\n", *term as u8));
            }
        }
        out
    }

    pub fn parse(text: &str) -> Session {
        let mut s = Session::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            match key {
                "size" => {
                    let mut it = value.split_whitespace();
                    s.width = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    s.height = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                }
                "maximized" => s.maximized = value == "1",
                "split" => s.split = value == "1",
                "sidebar" => s.sidebar = value == "1",
                "active" => s.active_side = value.parse().unwrap_or(0).min(1),
                "selected0" => s.selected[0] = value.parse().unwrap_or(0),
                "selected1" => s.selected[1] = value.parse().unwrap_or(0),
                "tab0" | "tab1" => {
                    let side = if key == "tab0" { 0 } else { 1 };
                    let (term, uri) = value.split_once('\t').unwrap_or(("0", value));
                    if !uri.is_empty() {
                        s.sides[side].push((uri.to_string(), term == "1"));
                    }
                }
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let p = path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(p, self.serialize());
    }

    pub fn load() -> Option<Session> {
        let s = Session::parse(&std::fs::read_to_string(path()).ok()?);
        if s.sides[0].is_empty() && s.sides[1].is_empty() { None } else { Some(s) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = Session {
            width: 1200,
            height: 800,
            maximized: false,
            split: true,
            sidebar: true,
            active_side: 1,
            sides: [vec![("file:///home/u".into(), false), ("file:///tmp".into(), true)], vec![("sftp://box/srv".into(), false)]],
            selected: [1, 0],
        };
        assert_eq!(Session::parse(&s.serialize()), s);
    }
}
