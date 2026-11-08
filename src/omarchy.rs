//! Omarchy integration: theme folders, applying a theme, setting a wallpaper.

use gtk::glib;
use std::path::{Path, PathBuf};

/// Where the user's Omarchy themes live (first existing wins).
pub fn themes_dir() -> Option<PathBuf> {
    let home = glib::home_dir();
    [home.join(".config/omarchy/themes"), home.join(".local/share/omarchy/themes")].into_iter().find(|p| p.is_dir())
}

/// A folder is a theme when it carries the palette files Omarchy reads.
pub fn is_theme_dir(dir: &Path) -> bool {
    dir.is_dir() && (dir.join("colors.toml").is_file() || dir.join("alacritty.toml").is_file() || dir.join("hyprland.conf").is_file())
}

/// Image used to preview a folder: Omarchy's `preview.png`, else the first
/// wallpaper in `backgrounds/`, else common cover names.
pub fn folder_preview(dir: &Path) -> Option<PathBuf> {
    for name in ["preview.png", "preview.jpg", "cover.jpg", "cover.png", "folder.jpg", "folder.png", ".folder.png"] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    let backgrounds = dir.join("backgrounds");
    if backgrounds.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&backgrounds).ok()?.flatten().map(|e| e.path()).filter(|p| is_image_name(p)).collect();
        files.sort();
        return files.into_iter().next();
    }
    None
}

fn is_image_name(p: &Path) -> bool {
    matches!(p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("png" | "jpg" | "jpeg" | "webp"))
}

pub fn theme_set_available() -> bool {
    glib::find_program_in_path("omarchy-theme-set").is_some()
}

pub fn bg_set_available() -> bool {
    glib::find_program_in_path("omarchy-theme-bg-set").is_some()
}

/// `omarchy-theme-set <name>` wants the folder name of the theme.
pub fn apply_theme_argv(dir: &Path) -> Option<Vec<String>> {
    let name = dir.file_name()?.to_str()?.to_string();
    Some(vec!["omarchy-theme-set".into(), name])
}

pub fn set_wallpaper_argv(image: &Path) -> Vec<String> {
    vec!["omarchy-theme-bg-set".into(), image.to_string_lossy().to_string()]
}

/// Does an archive carry an Omarchy theme (colors.toml / alacritty.toml at its top level)?
pub fn archive_is_theme(archive: &Path) -> bool {
    let Ok(out) = std::process::Command::new("bsdtar").arg("-tf").arg(archive).output() else { return false };
    if !out.status.success() {
        return false;
    }
    String::from_utf8_lossy(&out.stdout).lines().any(|l| {
        let l = l.trim_end_matches('/');
        let depth = l.matches('/').count();
        depth <= 1 && (l.ends_with("colors.toml") || l.ends_with("alacritty.toml"))
    })
}

/// Folder name an installed theme archive would get: its top-level directory, else the archive stem.
pub fn archive_theme_name(archive: &Path) -> String {
    let fallback = archive.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let fallback = fallback.trim_end_matches(".tar.gz").trim_end_matches(".tgz").trim_end_matches(".zip").trim_end_matches(".tar.zst").to_string();
    let Ok(out) = std::process::Command::new("bsdtar").arg("-tf").arg(archive).output() else { return fallback };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut tops: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for l in text.lines() {
        if let Some((top, _)) = l.split_once('/') {
            tops.insert(top.to_string());
        } else {
            return fallback;
        }
    }
    if tops.len() == 1 { tops.into_iter().next().unwrap() } else { fallback }
}

/// Where a theme gets installed (Omarchy reads `~/.config/omarchy/themes/<name>`).
pub fn install_dir() -> PathBuf {
    glib::home_dir().join(".config/omarchy/themes")
}
