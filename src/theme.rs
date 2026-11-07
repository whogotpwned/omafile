//! Omarchy theme integration.
//!
//! Omarchy keeps the active theme in `~/.config/omarchy/current/theme` (a symlink
//! that `omarchy-theme-set` repoints). Every theme ships an `alacritty.toml`
//! with a full 16-colour palette, which is the most reliable colour source we
//! can read without pulling in a TOML dependency: the format is trivial.

use ratatui::style::Color;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct Theme {
    pub name: String,
    pub bg: Color,
    pub fg: Color,
    pub black: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub blue: Color,
    pub magenta: Color,
    pub cyan: Color,
    pub white: Color,
    pub bright_black: Color,
    pub bright_red: Color,
    pub bright_green: Color,
    pub bright_yellow: Color,
    pub bright_blue: Color,
    pub bright_magenta: Color,
    pub bright_cyan: Color,
    pub bright_white: Color,
}

impl Default for Theme {
    /// Tokyo Night, the Omarchy default.
    fn default() -> Self {
        Theme {
            name: "tokyo-night".into(),
            bg: Color::Rgb(0x1a, 0x1b, 0x26),
            fg: Color::Rgb(0xa9, 0xb1, 0xd6),
            black: Color::Rgb(0x32, 0x34, 0x4a),
            red: Color::Rgb(0xf7, 0x76, 0x8e),
            green: Color::Rgb(0x9e, 0xce, 0x6a),
            yellow: Color::Rgb(0xe0, 0xaf, 0x68),
            blue: Color::Rgb(0x7a, 0xa2, 0xf7),
            magenta: Color::Rgb(0xad, 0x8e, 0xe6),
            cyan: Color::Rgb(0x44, 0x9d, 0xab),
            white: Color::Rgb(0x78, 0x7c, 0x99),
            bright_black: Color::Rgb(0x44, 0x4b, 0x6a),
            bright_red: Color::Rgb(0xff, 0x7a, 0x93),
            bright_green: Color::Rgb(0xb9, 0xf2, 0x7c),
            bright_yellow: Color::Rgb(0xff, 0x9e, 0x64),
            bright_blue: Color::Rgb(0x7d, 0xa6, 0xff),
            bright_magenta: Color::Rgb(0xbb, 0x9a, 0xf7),
            bright_cyan: Color::Rgb(0x0d, 0xb9, 0xd7),
            bright_white: Color::Rgb(0xac, 0xb0, 0xd0),
        }
    }
}

/// Directory of the active Omarchy theme.
pub fn theme_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("OMAFILE_THEME_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    PathBuf::from(home).join(".config/omarchy/current/theme")
}

fn palette_file(dir: &Path) -> PathBuf {
    dir.join("alacritty.toml")
}

/// Identity of the currently linked theme: resolved directory + palette mtime.
fn stamp(dir: &Path) -> Option<(PathBuf, SystemTime)> {
    let resolved = std::fs::canonicalize(dir).ok()?;
    let mtime = std::fs::metadata(palette_file(dir)).ok()?.modified().ok()?;
    Some((resolved, mtime))
}

fn parse_color(raw: &str) -> Option<Color> {
    let v = raw.trim();
    // Take the first quoted token if the value is quoted (ignores trailing comments).
    let v = if let Some(start) = v.find(['"', '\'']) {
        let rest = &v[start + 1..];
        let end = rest.find(['"', '\''])?;
        &rest[..end]
    } else {
        v.split('#').next().unwrap_or("").trim()
    };
    let hex = v
        .trim()
        .trim_start_matches('#')
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

/// Minimal parser for the `[colors.*]` tables of an Alacritty config.
/// Returns keys like `primary.background` or `normal.red`.
pub fn parse_alacritty(src: &str) -> HashMap<String, Color> {
    let mut out = HashMap::new();
    let mut section = String::new();
    for line in src.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = inner
                .trim()
                .trim_start_matches("colors.")
                .trim_matches('"')
                .to_string();
            if section == "colors" {
                section.clear();
            }
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            if let Some(color) = parse_color(value) {
                let key = key.trim().trim_matches('"');
                let full = if section.is_empty() {
                    key.to_string()
                } else {
                    format!("{section}.{key}")
                };
                out.insert(full, color);
            }
        }
    }
    out
}

fn theme_name(dir: &Path) -> String {
    let resolved = std::fs::read_link(dir)
        .ok()
        .or_else(|| std::fs::canonicalize(dir).ok())
        .unwrap_or_else(|| dir.to_path_buf());
    resolved
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "custom".into())
}

pub fn load_from(dir: &Path) -> Theme {
    let mut theme = Theme::default();
    let Ok(src) = std::fs::read_to_string(palette_file(dir)) else {
        return theme;
    };
    let map = parse_alacritty(&src);
    if map.is_empty() {
        return theme;
    }
    theme.name = theme_name(dir);
    let pick = |key: &str, fallback: Color| map.get(key).copied().unwrap_or(fallback);
    theme.bg = pick("primary.background", theme.bg);
    theme.fg = pick("primary.foreground", theme.fg);
    theme.black = pick("normal.black", theme.black);
    theme.red = pick("normal.red", theme.red);
    theme.green = pick("normal.green", theme.green);
    theme.yellow = pick("normal.yellow", theme.yellow);
    theme.blue = pick("normal.blue", theme.blue);
    theme.magenta = pick("normal.magenta", theme.magenta);
    theme.cyan = pick("normal.cyan", theme.cyan);
    theme.white = pick("normal.white", theme.white);
    theme.bright_black = pick("bright.black", theme.black);
    theme.bright_red = pick("bright.red", theme.red);
    theme.bright_green = pick("bright.green", theme.green);
    theme.bright_yellow = pick("bright.yellow", theme.yellow);
    theme.bright_blue = pick("bright.blue", theme.blue);
    theme.bright_magenta = pick("bright.magenta", theme.magenta);
    theme.bright_cyan = pick("bright.cyan", theme.cyan);
    theme.bright_white = pick("bright.white", theme.white);
    theme
}

/// Tracks the active theme and reloads it when `omarchy-theme-set` switches it.
pub struct ThemeWatcher {
    dir: PathBuf,
    stamp: Option<(PathBuf, SystemTime)>,
}

impl ThemeWatcher {
    pub fn new() -> Self {
        ThemeWatcher { dir: theme_dir(), stamp: None }
    }

    pub fn load(&mut self) -> Theme {
        self.stamp = stamp(&self.dir);
        load_from(&self.dir)
    }

    pub fn changed(&self) -> bool {
        stamp(&self.dir) != self.stamp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_omarchy_palette() {
        let src = r##"
[colors]

[colors.primary]
background = "#1a1b26"
foreground = '#a9b1d6' # trailing comment

[colors.normal]
black = "0x32344a"
red = "#f7768e"
"##;
        let map = parse_alacritty(src);
        assert_eq!(map["primary.background"], Color::Rgb(0x1a, 0x1b, 0x26));
        assert_eq!(map["primary.foreground"], Color::Rgb(0xa9, 0xb1, 0xd6));
        assert_eq!(map["normal.black"], Color::Rgb(0x32, 0x34, 0x4a));
        assert_eq!(map["normal.red"], Color::Rgb(0xf7, 0x76, 0x8e));
    }

    #[test]
    fn falls_back_to_default_on_missing_dir() {
        let theme = load_from(Path::new("/nonexistent/omafile-theme"));
        assert_eq!(theme.name, "tokyo-night");
    }
}
