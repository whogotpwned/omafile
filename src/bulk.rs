//! Bulk commands typed into the palette: `rename *.jpeg *.jpg`, `chmod +x`,
//! `touch a b`, `mkdir docs`. Parsed into a preview first, run with undo.

use crate::item::FileItem;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub enum Bulk {
    /// (pattern, replacement) with `*` captured and reinserted.
    Rename(String, String),
    /// Mode text as chmod understands it (`+x`, `644`, `u-w`).
    Chmod(String),
    Touch(Vec<String>),
    Mkdir(Vec<String>),
}

pub fn parse(text: &str) -> Option<Bulk> {
    let mut words = text.split_whitespace();
    let verb = words.next()?.to_lowercase();
    let rest: Vec<String> = words.map(str::to_string).collect();
    match verb.as_str() {
        "rename" | "mv" | "ren" if rest.len() == 2 => Some(Bulk::Rename(rest[0].clone(), rest[1].clone())),
        "chmod" if rest.len() == 1 && looks_like_mode(&rest[0]) => Some(Bulk::Chmod(rest[0].clone())),
        "touch" if !rest.is_empty() => Some(Bulk::Touch(rest)),
        "mkdir" if !rest.is_empty() => Some(Bulk::Mkdir(rest)),
        _ => None,
    }
}

fn looks_like_mode(m: &str) -> bool {
    m.chars().all(|c| c.is_ascii_digit()) && (3..=4).contains(&m.len()) || m.chars().all(|c| "ugoa+-=rwxXst".contains(c)) && m.contains(['+', '-', '='])
}

/// Apply `pattern → replacement` to one name. `*` in the pattern captures a
/// run of characters; the n-th `*` in the replacement reinserts it. Without
/// any `*` the pattern is a literal substring to replace.
pub fn rename_one(pattern: &str, replacement: &str, name: &str) -> Option<String> {
    if !pattern.contains('*') {
        return if name.contains(pattern) { Some(name.replacen(pattern, replacement, 1)) } else { None };
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut captures: Vec<String> = Vec::new();
    let mut rest = name;
    // First literal must be a prefix.
    if !rest.starts_with(parts[0]) {
        return None;
    }
    rest = &rest[parts[0].len()..];
    for (i, lit) in parts.iter().enumerate().skip(1) {
        let last = i == parts.len() - 1;
        if lit.is_empty() {
            if last {
                captures.push(rest.to_string());
                rest = "";
            } else {
                captures.push(String::new());
            }
            continue;
        }
        let pos = if last { rest.rfind(lit).filter(|p| p + lit.len() == rest.len()) } else { rest.find(lit) };
        let pos = pos?;
        captures.push(rest[..pos].to_string());
        rest = &rest[pos + lit.len()..];
    }
    if !rest.is_empty() {
        return None;
    }
    let mut out = String::new();
    let mut n = 0;
    for c in replacement.chars() {
        if c == '*' {
            out.push_str(captures.get(n).map(String::as_str).unwrap_or(""));
            n += 1;
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// Which items a rename would touch: (old name, new name).
pub fn rename_plan(items: &[FileItem], pattern: &str, replacement: &str) -> Vec<(FileItem, String)> {
    let mut seen = BTreeSet::new();
    items
        .iter()
        .filter_map(|i| {
            let old = i.display_name();
            let new = rename_one(pattern, replacement, &old)?;
            if new == old || new.is_empty() || new.contains('/') || !seen.insert(new.clone()) {
                return None;
            }
            Some((i.clone(), new))
        })
        .collect()
}

/// One-line preview for the palette row.
pub fn describe(bulk: &Bulk, plan_len: usize, sample: &[(String, String)]) -> (String, String) {
    match bulk {
        Bulk::Rename(p, r) => {
            let ex = sample.iter().take(2).map(|(a, b)| format!("{a} → {b}")).collect::<Vec<_>>().join(", ");
            (format!("Rename {plan_len} {}: {p} → {r}", if plan_len == 1 { "item" } else { "items" }), ex)
        }
        Bulk::Chmod(m) => (format!("chmod {m} on {plan_len} selected {}", if plan_len == 1 { "item" } else { "items" }), "Undo restores the old modes".into()),
        Bulk::Touch(n) => (format!("Create {} empty {}", n.len(), if n.len() == 1 { "file" } else { "files" }), n.join(", ")),
        Bulk::Mkdir(n) => (format!("Create {} {}", n.len(), if n.len() == 1 { "folder" } else { "folders" }), n.join(", ")),
    }
}

/// Parse a chmod mode against a current mode; returns the new permission bits.
pub fn apply_mode(current: u32, mode: &str) -> Option<u32> {
    if mode.chars().all(|c| c.is_ascii_digit()) {
        return u32::from_str_radix(mode, 8).ok().map(|m| (current & !0o7777) | (m & 0o7777));
    }
    let mut bits = current & 0o7777;
    for clause in mode.split(',') {
        let who_end = clause.find(['+', '-', '=']).unwrap_or(0);
        let (who, rest) = clause.split_at(who_end);
        let op = rest.chars().next()?;
        let perms = &rest[1..];
        let mut mask = 0u32;
        let who = if who.is_empty() || who.contains('a') { "ugo" } else { who };
        let mut value = 0u32;
        for p in perms.chars() {
            let bit = match p {
                'r' => 4,
                'w' => 2,
                'x' => 1,
                'X' => {
                    if current & 0o111 != 0 || current & 0o40000 != 0 { 1 } else { 0 }
                }
                _ => return None,
            };
            value |= bit;
        }
        for w in who.chars() {
            let shift = match w {
                'u' => 6,
                'g' => 3,
                'o' => 0,
                _ => continue,
            };
            mask |= value << shift;
            if op == '=' {
                bits &= !(7 << shift);
            }
        }
        match op {
            '+' | '=' => bits |= mask,
            '-' => bits &= !mask,
            _ => return None,
        }
    }
    Some((current & !0o7777) | bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_rename() {
        assert_eq!(rename_one("*.jpeg", "*.jpg", "photo.jpeg").as_deref(), Some("photo.jpg"));
        assert_eq!(rename_one("*.jpeg", "*.jpg", "photo.png"), None);
        assert_eq!(rename_one("IMG_*.JPG", "holiday-*.jpg", "IMG_0042.JPG").as_deref(), Some("holiday-0042.jpg"));
        assert_eq!(rename_one("draft", "final", "draft-v2.md").as_deref(), Some("final-v2.md"));
        assert_eq!(rename_one("* - *.mp3", "* – *.mp3", "A - B.mp3").as_deref(), Some("A – B.mp3"));
    }

    #[test]
    fn parse_and_modes() {
        assert_eq!(parse("rename *.jpeg *.jpg"), Some(Bulk::Rename("*.jpeg".into(), "*.jpg".into())));
        assert_eq!(parse("chmod +x"), Some(Bulk::Chmod("+x".into())));
        assert_eq!(parse("chmod hello"), None);
        assert_eq!(parse("touch a.txt b.txt"), Some(Bulk::Touch(vec!["a.txt".into(), "b.txt".into()])));
        assert_eq!(apply_mode(0o100644, "+x"), Some(0o100755));
        assert_eq!(apply_mode(0o100755, "u-x"), Some(0o100655));
        assert_eq!(apply_mode(0o100644, "600"), Some(0o100600));
        assert_eq!(apply_mode(0o100644, "g=rw"), Some(0o100664));
    }
}
