//! Nerd Font glyphs (Omarchy ships CaskaydiaMono Nerd Font) with ASCII fallback.

use crate::fs::Entry;

pub fn for_entry(entry: &Entry, enabled: bool) -> &'static str {
    if !enabled {
        return if entry.is_dir { "/" } else { " " };
    }
    if entry.is_dir {
        return match entry.name.as_str() {
            ".git" => "",
            ".config" => "",
            "node_modules" => "",
            "Downloads" => "󰉍",
            "Documents" => "󱔗",
            "Pictures" | "Images" => "󰉏",
            "Music" => "󱍙",
            "Videos" | "Movies" => "󱧺",
            "Desktop" => "󰇄",
            "Projects" | "Code" | "src" => "󰲋",
            _ if entry.is_symlink => "",
            _ => "",
        };
    }
    let by_name = match entry.name.as_str() {
        ".gitignore" | ".gitattributes" | ".gitmodules" => Some(""),
        "Dockerfile" | "docker-compose.yml" | "compose.yml" => Some(""),
        "Makefile" | "justfile" | "Justfile" => Some(""),
        "Cargo.toml" | "Cargo.lock" => Some(""),
        "package.json" | "package-lock.json" => Some(""),
        "LICENSE" | "LICENSE.md" | "COPYING" => Some(""),
        "PKGBUILD" => Some(""),
        _ => None,
    };
    if let Some(icon) = by_name {
        return icon;
    }
    match entry.ext().as_str() {
        "rs" => "",
        "py" | "pyi" => "",
        "js" | "mjs" | "cjs" => "",
        "ts" | "mts" => "",
        "jsx" | "tsx" => "",
        "rb" | "erb" | "gemspec" => "",
        "go" => "",
        "c" | "h" => "",
        "cpp" | "cc" | "cxx" | "hpp" => "",
        "java" | "jar" => "",
        "lua" => "",
        "sh" | "bash" | "zsh" | "fish" => "",
        "html" | "htm" => "",
        "css" | "scss" | "sass" => "",
        "md" | "markdown" => "",
        "json" | "jsonc" => "",
        "toml" => "",
        "yml" | "yaml" => "",
        "xml" => "󰗀",
        "sql" | "db" | "sqlite" => "",
        "txt" | "log" => "",
        "pdf" => "",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" => "",
        "mp4" | "mkv" | "webm" | "mov" | "avi" => "",
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "opus" => "",
        "zip" | "tar" | "gz" | "xz" | "zst" | "bz2" | "7z" | "rar" => "",
        "iso" | "img" => "",
        "deb" | "rpm" | "pkg" => "",
        "conf" | "cfg" | "ini" | "rc" => "",
        "lock" => "",
        "key" | "pem" | "gpg" | "asc" => "",
        "ttf" | "otf" | "woff" | "woff2" => "",
        "desktop" => "",
        _ if entry.is_symlink => "",
        _ if entry.is_exec => "",
        _ => "",
    }
}

pub const DIR_OPEN: &str = "";
pub const ASCII_DIR_OPEN: &str = ">";
