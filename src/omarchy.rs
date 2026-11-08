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
