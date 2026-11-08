//! Smart folders: saved searches that show up in the sidebar and list their
//! matches as a virtual folder. Stored in `~/.config/omafile/smart-folders`,
//! one per line: `name<TAB>root<TAB>query<TAB>max_age_days<TAB>glob`.

use gtk::glib;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct SmartFolder {
    pub name: String,
    pub root: PathBuf,
    /// Substring matched against the name (fd / walk), may be empty with a glob.
    pub query: String,
    /// Only files modified within this many days (0 = any).
    pub max_age_days: u32,
    /// Optional glob on the file name, e.g. `*.pdf`.
    pub glob: String,
}

pub fn path() -> PathBuf {
    glib::user_config_dir().join("omafile/smart-folders")
}

pub fn parse(text: &str) -> Vec<SmartFolder> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() < 3 || f[0].is_empty() {
                return None;
            }
            Some(SmartFolder {
                name: f[0].to_string(),
                root: PathBuf::from(f[1]),
                query: f[2].to_string(),
                max_age_days: f.get(3).and_then(|v| v.parse().ok()).unwrap_or(0),
                glob: f.get(4).map(|s| s.to_string()).unwrap_or_default(),
            })
        })
        .collect()
}

pub fn serialize(list: &[SmartFolder]) -> String {
    let mut out = String::from("# omafile smart folders: name, root, query, max age in days, glob\n");
    for s in list {
        out.push_str(&format!("{}\t{}\t{}\t{}\t{}\n", s.name.replace('\t', " "), s.root.display(), s.query.replace('\t', " "), s.max_age_days, s.glob));
    }
    out
}

pub fn load() -> Vec<SmartFolder> {
    std::fs::read_to_string(path()).map(|t| parse(&t)).unwrap_or_default()
}

pub fn save(list: &[SmartFolder]) -> std::io::Result<()> {
    let p = path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(p, serialize(list))
}

pub fn add(folder: SmartFolder) -> std::io::Result<()> {
    let mut list = load();
    list.retain(|s| s.name != folder.name);
    list.push(folder);
    save(&list)
}

pub fn remove(name: &str) -> std::io::Result<()> {
    let mut list = load();
    list.retain(|s| s.name != name);
    save(&list)
}

/// The search term handed to fd / the GIO walk: the query, or the glob's
/// literal part so `*.pdf` still narrows the walk to names containing `.pdf`.
pub fn search_term(s: &SmartFolder) -> String {
    if !s.query.is_empty() {
        return s.query.clone();
    }
    s.glob.split('*').filter(|p| !p.is_empty() && !p.contains('?')).max_by_key(|p| p.len()).unwrap_or("").to_string()
}

/// Shell-style glob on a file name: `*` any run, `?` one char, case-insensitive.
pub fn glob_matches(glob: &str, name: &str) -> bool {
    if glob.is_empty() {
        return true;
    }
    let (g, n): (Vec<char>, Vec<char>) = (glob.to_lowercase().chars().collect(), name.to_lowercase().chars().collect());
    fn go(g: &[char], n: &[char]) -> bool {
        match g.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|i| go(&g[1..], &n[i..])),
            Some('?') => !n.is_empty() && go(&g[1..], &n[1..]),
            Some(c) => n.first() == Some(c) && go(&g[1..], &n[1..]),
        }
    }
    go(&g, &n)
}

/// Does a hit belong in the smart folder?
pub fn accepts(s: &SmartFolder, name: &str, modified_unix: i64) -> bool {
    if !glob_matches(&s.glob, name) {
        return false;
    }
    if s.max_age_days > 0 {
        let now = glib::real_time() / 1_000_000;
        if now - modified_unix > s.max_age_days as i64 * 86_400 {
            return false;
        }
    }
    true
}

pub fn uri(s: &SmartFolder) -> String {
    format!("omafile-smart:///{}", glib::uri_escape_string(&s.name, None::<&str>, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_glob() {
        let list = vec![SmartFolder { name: "Recent PDFs".into(), root: "/home/u".into(), query: String::new(), max_age_days: 7, glob: "*.pdf".into() }];
        assert_eq!(parse(&serialize(&list)), list);
        assert!(glob_matches("*.pdf", "Paper.PDF"));
        assert!(!glob_matches("*.pdf", "paper.pdf.bak"));
        assert!(glob_matches("report-??.md", "report-01.md"));
        assert_eq!(search_term(&list[0]), ".pdf");
        assert!(accepts(&list[0], "a.pdf", glib::real_time() / 1_000_000 - 3600));
        assert!(!accepts(&list[0], "a.pdf", 0));
        assert!(uri(&list[0]).starts_with("omafile-smart:///"));
    }
}
