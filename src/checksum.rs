//! SHA-256 for Properties and for verifying downloads against a sidecar
//! `.sha256` / `SHA256SUMS` file or a hash on the clipboard.

use gtk::gio;
use std::path::{Path, PathBuf};

pub async fn sha256(path: PathBuf) -> Option<String> {
    gio::spawn_blocking(move || {
        let out = std::process::Command::new("sha256sum").arg(&path).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(|s| s.to_lowercase())
    })
    .await
    .ok()
    .flatten()
}

pub fn looks_like_sha256(text: &str) -> Option<String> {
    let t = text.split_whitespace().next()?.to_lowercase();
    if t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()) { Some(t) } else { None }
}

/// Sidecar files that may carry the expected hash for `file`.
pub fn sidecars(file: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let dir = file.parent().unwrap_or(Path::new("."));
    for ext in ["sha256", "sha256sum", "sha256sums", "sha", "digest"] {
        v.push(dir.join(format!("{name}.{ext}")));
    }
    for common in ["SHA256SUMS", "sha256sums.txt", "SHA256SUMS.txt", "checksums.txt", "CHECKSUMS"] {
        v.push(dir.join(common));
    }
    v.into_iter().filter(|p| p.is_file()).collect()
}

/// Find the hash for `name` in sidecar text (`<hash>  <name>` lines, or a bare hash).
pub fn expected_from(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(hash), rest) = (parts.next(), parts.next()) else { continue };
        let Some(hash) = looks_like_sha256(hash) else { continue };
        match rest {
            None => return Some(hash),
            Some(n) if n.trim_start_matches('*') == name || n.ends_with(&format!("/{name}")) => return Some(hash),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sums_files() {
        let h = "a".repeat(64);
        assert_eq!(expected_from(&format!("{h}  arch.iso\n"), "arch.iso"), Some(h.clone()));
        assert_eq!(expected_from(&format!("{h} *arch.iso\n"), "arch.iso"), Some(h.clone()));
        assert_eq!(expected_from(&format!("{h}\n"), "x"), Some(h.clone()));
        assert_eq!(expected_from(&format!("{h}  other.iso\n"), "arch.iso"), None);
        assert_eq!(looks_like_sha256("  ABCD "), None);
    }
}
