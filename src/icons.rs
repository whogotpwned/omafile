//! Nerd Font glyphs (Omarchy ships CaskaydiaMono Nerd Font) with ASCII fallback.
//! Code points are written as escapes so the file stays editor-safe.

use crate::fs::Entry;

pub const DIR_OPEN: &str = "\u{f07c}"; // nf-fa-folder_open
pub const ASCII_DIR_OPEN: &str = ">";

const FOLDER: &str = "\u{f07b}"; // nf-fa-folder
const FOLDER_LINK: &str = "\u{f482}"; // nf-oct-file_symlink_directory
const FILE: &str = "\u{f15b}"; // nf-fa-file
const FILE_TEXT: &str = "\u{f15c}"; // nf-fa-file_text
const FILE_LINK: &str = "\u{f481}"; // nf-oct-file_symlink_file
const TERMINAL: &str = "\u{f489}"; // nf-oct-terminal
const CONFIG: &str = "\u{e615}"; // nf-seti-config
const GIT: &str = "\u{e702}"; // nf-dev-git
const NODE: &str = "\u{e718}"; // nf-dev-nodejs_small
const RUST: &str = "\u{e7a8}"; // nf-dev-rust
const ARCHIVE: &str = "\u{f410}"; // nf-oct-file_zip

pub fn for_entry(entry: &Entry, enabled: bool) -> &'static str {
    if !enabled {
        return if entry.is_dir { "/" } else { " " };
    }
    if entry.is_dir {
        return match entry.name.as_str() {
            ".git" => GIT,
            ".config" => CONFIG,
            "node_modules" => NODE,
            "Downloads" => "\u{f024d}", // nf-md-folder_download
            "Documents" => "\u{f1517}", // nf-md-folder_file
            "Pictures" | "Images" => "\u{f024f}", // nf-md-folder_image
            "Music" => "\u{f1359}", // nf-md-folder_music
            "Videos" | "Movies" => "\u{f19fa}", // nf-md-folder_play
            "Desktop" => "\u{f01c4}", // nf-md-desktop_classic
            "Projects" | "Code" | "src" => "\u{f0c8b}", // nf-md-folder_cog... (folder with wrench)
            _ if entry.is_symlink => FOLDER_LINK,
            _ => FOLDER,
        };
    }
    let by_name = match entry.name.as_str() {
        ".gitignore" | ".gitattributes" | ".gitmodules" => Some(GIT),
        "Dockerfile" | "docker-compose.yml" | "compose.yml" => Some("\u{e7b0}"), // nf-dev-docker
        "Makefile" | "justfile" | "Justfile" => Some(CONFIG),
        "Cargo.toml" | "Cargo.lock" => Some(RUST),
        "package.json" | "package-lock.json" => Some(NODE),
        "LICENSE" | "LICENSE.md" | "COPYING" => Some("\u{f24e}"), // nf-fa-balance_scale
        "PKGBUILD" => Some("\u{f303}"), // nf-linux-archlinux
        _ => None,
    };
    if let Some(icon) = by_name {
        return icon;
    }
    match entry.ext().as_str() {
        "rs" => RUST,
        "py" | "pyi" => "\u{e73c}", // nf-dev-python
        "js" | "mjs" | "cjs" => "\u{e74e}", // nf-dev-javascript
        "ts" | "mts" => "\u{e628}", // nf-seti-typescript
        "jsx" | "tsx" => "\u{e7ba}", // nf-dev-react
        "rb" | "erb" | "gemspec" => "\u{e739}", // nf-dev-ruby
        "go" => "\u{e627}", // nf-seti-go
        "c" | "h" => "\u{e61e}", // nf-custom-c
        "cpp" | "cc" | "cxx" | "hpp" => "\u{e61d}", // nf-custom-cpp
        "java" | "jar" => "\u{e738}", // nf-dev-java
        "lua" => "\u{e620}", // nf-seti-lua
        "sh" | "bash" | "zsh" | "fish" => TERMINAL,
        "html" | "htm" => "\u{e736}", // nf-dev-html5
        "css" | "scss" | "sass" => "\u{e749}", // nf-dev-css3
        "md" | "markdown" => "\u{e73e}", // nf-dev-markdown
        "json" | "jsonc" => "\u{e60b}", // nf-seti-json
        "toml" => "\u{e6b2}", // nf-seti-toml
        "yml" | "yaml" => "\u{e6a8}", // nf-seti-yml
        "xml" => "\u{f05c0}", // nf-md-xml
        "sql" | "db" | "sqlite" => "\u{f1c0}", // nf-fa-database
        "txt" | "log" => FILE_TEXT,
        "pdf" => "\u{f1c1}", // nf-fa-file_pdf_o
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" => "\u{f1c5}", // nf-fa-file_image_o
        "mp4" | "mkv" | "webm" | "mov" | "avi" => "\u{f03d}", // nf-fa-video_camera
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "opus" => "\u{f001}", // nf-fa-music
        "zip" | "tar" | "gz" | "xz" | "zst" | "bz2" | "7z" | "rar" => ARCHIVE,
        "iso" | "img" => "\u{f0a0}", // nf-fa-hdd_o
        "deb" | "rpm" | "pkg" => "\u{f187}", // nf-fa-archive
        "conf" | "cfg" | "ini" | "rc" => CONFIG,
        "lock" => "\u{f023}", // nf-fa-lock
        "key" | "pem" | "gpg" | "asc" => "\u{f084}", // nf-fa-key
        "ttf" | "otf" | "woff" | "woff2" => "\u{f031}", // nf-fa-font
        "desktop" => "\u{f108}", // nf-fa-desktop
        _ if entry.is_symlink => FILE_LINK,
        _ if entry.is_exec => TERMINAL,
        _ => FILE,
    }
}
