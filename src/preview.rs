//! Right-hand preview pane content.

use crate::fs::{self, Entry, SortMode};
use std::io::Read;
use std::path::Path;

pub enum Preview {
    Empty,
    Text(Vec<String>),
    Dir(Vec<Entry>),
    Info(Vec<(String, String)>),
    Error(String),
}

const SNIFF_BYTES: usize = 64 * 1024;
const MAX_LINE_CHARS: usize = 400;

/// Heuristic text detection on the first 64 KiB: no NUL bytes and valid-ish UTF-8.
pub fn looks_like_text(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; 8192];
    let Ok(n) = file.read(&mut buf) else {
        return false;
    };
    if n == 0 {
        return true;
    }
    let chunk = &buf[..n];
    if chunk.contains(&0) {
        return false;
    }
    match std::str::from_utf8(chunk) {
        Ok(_) => true,
        // A cut-off multibyte sequence at the end is fine.
        Err(e) => e.valid_up_to() + 4 >= n,
    }
}

pub fn build(entry: Option<&Entry>, show_hidden: bool, sort: SortMode, max_lines: usize) -> Preview {
    let Some(entry) = entry else {
        return Preview::Empty;
    };
    if entry.is_dir {
        return match fs::read_dir(&entry.path, show_hidden, sort) {
            Ok(entries) => Preview::Dir(entries),
            Err(e) => Preview::Error(format!("{}", e)),
        };
    }
    if looks_like_text(&entry.path) && entry.size > 0 {
        match read_head(&entry.path, max_lines) {
            Ok(lines) => Preview::Text(lines),
            Err(e) => Preview::Error(format!("{}", e)),
        }
    } else {
        Preview::Info(info(entry))
    }
}

fn read_head(path: &Path, max_lines: usize) -> std::io::Result<Vec<String>> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = Vec::with_capacity(SNIFF_BYTES);
    file.by_ref().take(SNIFF_BYTES as u64).read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    Ok(text
        .lines()
        .take(max_lines)
        .map(|l| {
            let expanded = l.replace('\t', "    ");
            let cleaned: String = expanded.chars().filter(|c| !c.is_control()).collect();
            if cleaned.chars().count() > MAX_LINE_CHARS {
                cleaned.chars().take(MAX_LINE_CHARS).collect()
            } else {
                cleaned
            }
        })
        .collect())
}

pub fn info(entry: &Entry) -> Vec<(String, String)> {
    let mut rows = vec![
        ("Name".to_string(), entry.name.clone()),
        (
            "Type".to_string(),
            if entry.is_symlink {
                "symbolic link".into()
            } else if entry.is_dir {
                "directory".into()
            } else if entry.ext().is_empty() {
                "file".into()
            } else {
                format!("{} file", entry.ext())
            },
        ),
        ("Size".to_string(), fs::human_size(entry.size)),
        (
            "Modified".to_string(),
            entry.modified.map(fs::human_time).unwrap_or_else(|| "-".into()),
        ),
        (
            "Mode".to_string(),
            format!(
                "{} ({:o})",
                fs::perm_string(entry.mode, entry.is_dir, entry.is_symlink),
                entry.mode & 0o7777
            ),
        ),
    ];
    if entry.is_symlink {
        if let Ok(target) = std::fs::read_link(&entry.path) {
            rows.push(("Target".into(), target.display().to_string()));
        }
    }
    rows.push(("Path".into(), fs::display_path(&entry.path)));
    rows
}
