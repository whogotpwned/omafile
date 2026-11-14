//! Omarchy theme integration.
//!
//! Current Omarchy keeps the active theme at `~/.local/state/omarchy/current/theme`
//! (a symlink that `omarchy-theme-set` repoints) with a semantic `colors.toml`,
//! an `icons.theme` naming the icon theme and `mode = "light" | "dark"`.
//! Older releases used `~/.config/omarchy/current/theme` with an
//! `alacritty.toml` palette and a `light.mode` marker. Both are supported.
//! The palette is turned into libadwaita named colours, installed as a CSS
//! provider, and re-applied whenever the theme link changes.

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    pub fn rgba(self, alpha: f32) -> String {
        format!("rgba({}, {}, {}, {alpha})", self.0, self.1, self.2)
    }

    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let f = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
        Rgb(f(self.0, other.0), f(self.1, other.1), f(self.2, other.2))
    }

    pub fn luminance(self) -> f32 {
        (0.2126 * self.0 as f32 + 0.7152 * self.1 as f32 + 0.0722 * self.2 as f32) / 255.0
    }

    fn parse(raw: &str) -> Option<Rgb> {
        let v = raw.trim();
        let v = if let Some(start) = v.find(['"', '\'']) {
            let rest = &v[start + 1..];
            let end = rest.find(['"', '\''])?;
            &rest[..end]
        } else {
            v.split('#').next().unwrap_or("").trim()
        };
        let hex = v.trim().trim_start_matches('#').trim_start_matches("0x").trim_start_matches("0X");
        if hex.len() != 6 {
            return None;
        }
        Some(Rgb(
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ))
    }
}

/// Semantic palette, following the keys of Omarchy's `colors.toml`.
#[derive(Clone, Debug)]
pub struct Palette {
    pub name: String,
    pub light: bool,
    pub icon_theme: Option<String>,
    pub accent: Rgb,
    pub selection: Rgb,
    pub muted: Rgb,
    pub background: Rgb,
    pub dark_background: Rgb,
    pub lighter_background: Rgb,
    pub foreground: Rgb,
    pub dark_foreground: Rgb,
    pub bright_foreground: Rgb,
    pub red: Rgb,
    pub orange: Rgb,
    pub yellow: Rgb,
    pub green: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
    pub bright_red: Rgb,
    pub bright_yellow: Rgb,
    pub bright_green: Rgb,
    pub bright_blue: Rgb,
    pub bright_magenta: Rgb,
    pub bright_cyan: Rgb,
}

impl Default for Palette {
    /// Tokyo Night, the Omarchy default.
    fn default() -> Self {
        Palette {
            name: "tokyo-night".into(),
            light: false,
            icon_theme: None,
            accent: Rgb(0x7a, 0xa2, 0xf7),
            selection: Rgb(0x29, 0x2e, 0x42),
            muted: Rgb(0x41, 0x48, 0x68),
            background: Rgb(0x1a, 0x1b, 0x26),
            dark_background: Rgb(0x13, 0x14, 0x1c),
            lighter_background: Rgb(0x24, 0x28, 0x3b),
            foreground: Rgb(0xa9, 0xb1, 0xd6),
            dark_foreground: Rgb(0x56, 0x5f, 0x89),
            bright_foreground: Rgb(0xc0, 0xca, 0xf5),
            red: Rgb(0xf7, 0x76, 0x8e),
            orange: Rgb(0xeb, 0x92, 0x7b),
            yellow: Rgb(0xe0, 0xaf, 0x68),
            green: Rgb(0x9e, 0xce, 0x6a),
            blue: Rgb(0x7a, 0xa2, 0xf7),
            magenta: Rgb(0xad, 0x8e, 0xe6),
            cyan: Rgb(0x44, 0x9d, 0xab),
            bright_red: Rgb(0xff, 0x7a, 0x93),
            bright_yellow: Rgb(0xff, 0x9e, 0x64),
            bright_green: Rgb(0xb9, 0xf2, 0x7c),
            bright_blue: Rgb(0x7d, 0xa6, 0xff),
            bright_magenta: Rgb(0xbb, 0x9a, 0xf7),
            bright_cyan: Rgb(0x0d, 0xb9, 0xd7),
        }
    }
}

/// Candidate theme directories, most current first.
pub fn theme_dirs() -> Vec<PathBuf> {
    if let Ok(dir) = std::env::var("OMAFILE_THEME_DIR") {
        return vec![PathBuf::from(dir)];
    }
    let home = glib::home_dir();
    vec![home.join(".local/state/omarchy/current/theme"), home.join(".config/omarchy/current/theme")]
}

/// The directory of the active theme: the first candidate that holds a palette.
pub fn theme_dir() -> PathBuf {
    let dirs = theme_dirs();
    dirs.iter()
        .find(|d| d.join("colors.toml").is_file() || d.join("alacritty.toml").is_file())
        .cloned()
        .unwrap_or_else(|| dirs[0].clone())
}

/// Minimal TOML-ish parser: `key = "#hex"` lines, optionally under `[sections]`.
/// Returns `section.key` (or bare `key` for the top level) → colour, and the
/// raw string values of non-colour keys such as `mode`.
pub fn parse_colors(src: &str) -> (HashMap<String, Rgb>, HashMap<String, String>) {
    let mut colors = HashMap::new();
    let mut strings = HashMap::new();
    let mut section = String::new();
    for line in src.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = inner.trim().trim_start_matches("colors.").trim_matches('"').to_string();
            if section == "colors" {
                section.clear();
            }
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().trim_matches('"');
            let full = if section.is_empty() { key.to_string() } else { format!("{section}.{key}") };
            if let Some(color) = Rgb::parse(value) {
                colors.insert(full, color);
            } else {
                let v = value.trim().split('#').next().unwrap_or("").trim().trim_matches(['"', '\'']).to_string();
                strings.insert(full, v);
            }
        }
    }
    (colors, strings)
}

fn theme_name(dir: &Path) -> String {
    if let Some(parent) = dir.parent() {
        if let Ok(name) = std::fs::read_to_string(parent.join("theme.name")) {
            let name = name.trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    std::fs::read_link(dir)
        .ok()
        .or_else(|| std::fs::canonicalize(dir).ok())
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "custom".into())
}

pub fn load_from(dir: &Path) -> Palette {
    let mut p = Palette::default();
    let icon_theme = std::fs::read_to_string(dir.join("icons.theme")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if let Ok(src) = std::fs::read_to_string(dir.join("colors.toml")) {
        let (colors, strings) = parse_colors(&src);
        if !colors.is_empty() {
            p.name = theme_name(dir);
            p.icon_theme = icon_theme;
            let pick = |key: &str, fallback: Rgb| colors.get(key).copied().unwrap_or(fallback);
            p.background = pick("background", p.background);
            p.foreground = pick("foreground", p.foreground);
            // Today's colors.toml names the terminal colours color0..color15; older ones
            // (and other tools) use words. Both are read.
            let named = |word: &str, number: &str, fallback: Rgb| colors.get(word).or_else(|| colors.get(number)).copied().unwrap_or(fallback);
            p.blue = named("blue", "color4", p.blue);
            p.accent = pick("accent", p.blue);
            p.red = named("red", "color1", p.red);
            p.yellow = named("yellow", "color3", p.yellow);
            p.orange = pick("orange", p.red.mix(p.yellow, 0.5));
            p.green = named("green", "color2", p.green);
            p.magenta = named("magenta", "color5", p.magenta);
            p.cyan = named("cyan", "color6", p.cyan);
            p.bright_red = named("bright_red", "color9", p.red);
            p.bright_yellow = named("bright_yellow", "color11", p.yellow);
            p.bright_green = named("bright_green", "color10", p.green);
            p.bright_blue = named("bright_blue", "color12", p.blue);
            p.bright_magenta = named("bright_magenta", "color13", p.magenta);
            p.bright_cyan = named("bright_cyan", "color14", p.cyan);
            p.light = match strings.get("mode").or_else(|| strings.get("theme_type")).map(|s| s.as_str()) {
                Some("light") => true,
                Some("dark") => false,
                _ => dir.join("light.mode").exists() || p.background.luminance() > 0.6,
            };
            p.dark_background = pick("dark_background", p.background.mix(Rgb(0, 0, 0), if p.light { 0.04 } else { 0.25 }));
            p.lighter_background = pick("lighter_background", p.background.mix(if p.light { Rgb(0, 0, 0) } else { Rgb(255, 255, 255) }, 0.06));
            p.selection = pick("selection", p.accent.mix(p.background, 0.7));
            p.muted = pick("muted", p.foreground.mix(p.background, 0.6));
            p.dark_foreground = pick("dark_foreground", p.foreground.mix(p.background, 0.45));
            p.bright_foreground = pick("bright_foreground", p.foreground.mix(if p.light { Rgb(0, 0, 0) } else { Rgb(255, 255, 255) }, 0.3));
            return p;
        }
    }
    // Legacy layout: alacritty.toml with a 16-colour palette.
    if let Ok(src) = std::fs::read_to_string(dir.join("alacritty.toml")) {
        let (colors, _) = parse_colors(&src);
        if !colors.is_empty() {
            p.name = theme_name(dir);
            p.icon_theme = icon_theme;
            let pick = |key: &str, fallback: Rgb| colors.get(key).copied().unwrap_or(fallback);
            p.background = pick("primary.background", p.background);
            p.foreground = pick("primary.foreground", p.foreground);
            p.blue = pick("normal.blue", p.blue);
            p.accent = p.blue;
            p.red = pick("normal.red", p.red);
            p.yellow = pick("normal.yellow", p.yellow);
            p.orange = p.red.mix(p.yellow, 0.5);
            p.green = pick("normal.green", p.green);
            p.magenta = pick("normal.magenta", p.magenta);
            p.cyan = pick("normal.cyan", p.cyan);
            p.bright_red = pick("bright.red", p.red);
            p.bright_yellow = pick("bright.yellow", p.yellow);
            p.bright_green = pick("bright.green", p.green);
            p.bright_blue = pick("bright.blue", p.blue);
            p.bright_magenta = pick("bright.magenta", p.magenta);
            p.bright_cyan = pick("bright.cyan", p.cyan);
            p.light = dir.join("light.mode").exists() || p.background.luminance() > 0.6;
            let toward = if p.light { Rgb(0, 0, 0) } else { Rgb(255, 255, 255) };
            p.dark_background = p.background.mix(Rgb(0, 0, 0), if p.light { 0.04 } else { 0.25 });
            p.lighter_background = p.background.mix(toward, 0.06);
            p.selection = p.accent.mix(p.background, 0.7);
            p.muted = pick("bright.black", p.foreground.mix(p.background, 0.6));
            p.dark_foreground = pick("normal.white", p.foreground.mix(p.background, 0.45));
            p.bright_foreground = pick("bright.white", p.foreground.mix(toward, 0.3));
        }
    }
    p
}

/// libadwaita named colours and omafile widget styles derived from the palette.
pub fn css(p: &Palette) -> String {
    let dark = !p.light;
    let border = p.muted.rgba(if dark { 0.45 } else { 0.35 });
    let accent_fg = if p.accent.luminance() > 0.55 { p.dark_background.mix(Rgb(0, 0, 0), 0.5) } else { Rgb(255, 255, 255) };
    let accent_text = if dark { p.accent.mix(Rgb(255, 255, 255), 0.1) } else { p.accent.mix(Rgb(0, 0, 0), 0.25) };
    let selected_row = p.selection.hex();
    let hover = p.foreground.rgba(0.05);
    let popover_bg = p.lighter_background;
    let dialog_bg = p.background.mix(p.lighter_background, 0.5);

    format!(
        r#"/* generated by omafile from the Omarchy theme "{name}" */
@define-color window_bg_color {bg};
@define-color window_fg_color {fg};
@define-color view_bg_color {view_bg};
@define-color view_fg_color {fg};
@define-color headerbar_bg_color {bg};
@define-color headerbar_fg_color {fg};
@define-color headerbar_border_color {border};
@define-color headerbar_backdrop_color {bg};
@define-color headerbar_shade_color rgba(0, 0, 0, 0.12);
@define-color headerbar_darker_shade_color rgba(0, 0, 0, 0.2);
@define-color sidebar_bg_color {bg};
@define-color sidebar_fg_color {fg};
@define-color sidebar_backdrop_color {bg};
@define-color sidebar_border_color {border};
@define-color sidebar_shade_color rgba(0, 0, 0, 0.12);
@define-color secondary_sidebar_bg_color {bg};
@define-color secondary_sidebar_fg_color {fg};
@define-color secondary_sidebar_backdrop_color {bg};
@define-color secondary_sidebar_border_color {border};
@define-color card_bg_color {card_bg};
@define-color card_fg_color {fg};
@define-color card_shade_color rgba(0, 0, 0, 0.2);
@define-color popover_bg_color {popover_bg};
@define-color popover_fg_color {fg};
@define-color popover_shade_color rgba(0, 0, 0, 0.2);
@define-color dialog_bg_color {dialog_bg};
@define-color dialog_fg_color {fg};
@define-color thumbnail_bg_color {card_bg};
@define-color thumbnail_fg_color {fg};
@define-color accent_bg_color {accent};
@define-color accent_fg_color {accent_fg};
@define-color accent_color {accent_text};
@define-color destructive_bg_color {red};
@define-color destructive_fg_color #ffffff;
@define-color destructive_color {red};
@define-color success_bg_color {green};
@define-color success_fg_color {bg};
@define-color success_color {green};
@define-color warning_bg_color {yellow};
@define-color warning_fg_color {bg};
@define-color warning_color {yellow};
@define-color error_bg_color {red};
@define-color error_fg_color #ffffff;
@define-color error_color {red};
@define-color shade_color rgba(0, 0, 0, 0.36);
@define-color scrollbar_outline_color rgba(0, 0, 0, 0.5);

/* ---- omafile widgets ------------------------------------------------- */
.omafile-header {{
  box-shadow: none;
}}
.omafile-card {{
  background-color: @view_bg_color;
  border: 1px solid {border};
  border-radius: 12px;
  margin: 0 10px 10px 4px;
}}
.omafile-card scrolledwindow {{
  background: transparent;
}}
.omafile-files {{
  background: transparent;
}}
.omafile-files columnview > header {{
  background-color: transparent;
  border-bottom: 1px solid {border};
}}
.omafile-files columnview > header button {{
  padding: 4px 10px;
  font-size: 0.85em;
  font-weight: 600;
  color: {dim};
}}
.omafile-files listview > row {{
  border-radius: 8px;
  margin: 1px 6px;
  padding: 0 4px;
}}
.omafile-files columnview > listview > row > cell {{
  padding: 4px 6px;
}}
.omafile-files listview > row:hover,
.omafile-files gridview > child:hover {{
  background-color: {hover};
}}
.omafile-files listview > row:selected,
.omafile-files gridview > child:selected {{
  background-color: {selected_row};
  color: {bright_fg};
}}
.omafile-files listview > row:selected:focus-within,
.omafile-files gridview > child:selected:focus-within {{
  background-color: {accent};
  color: {accent_fg};
}}
.omafile-files listview > row:selected .omafile-dim,
.omafile-files listview > row:selected .omafile-exec,
.omafile-files gridview > child:selected .omafile-exec {{
  color: inherit;
  opacity: 0.85;
}}
.omafile-files gridview > child {{
  border-radius: 10px;
  padding: 10px 6px;
  margin: 4px;
}}
.omafile-thumb {{
  border-radius: 6px;
}}
.omafile-grid-item .omafile-thumb {{
  border-radius: 8px;
}}
.omafile-thumb-page {{
  background-color: alpha(currentColor, 0.06);
}}
.omafile-feed-page {{
  background-color: {view_bg};
}}
.omafile-feed-list row {{
  padding: 8px 10px;
  border-radius: 10px;
}}
.omafile-feed-row-title {{
  font-weight: 500;
  opacity: 0.72;
}}
.omafile-feed-unread .omafile-feed-row-title {{
  font-weight: 800;
  opacity: 1;
}}
.omafile-feed-dot {{
  min-width: 8px;
  min-height: 8px;
  border-radius: 4px;
  background-color: transparent;
}}
.omafile-feed-unread .omafile-feed-dot {{
  background-color: {accent};
}}
.omafile-feed-title {{
  font-size: 20pt;
  font-weight: 800;
  letter-spacing: -0.4px;
}}
.omafile-feed-body {{
  font-size: 11.5pt;
  line-height: 1.5;
}}
.omafile-feed-image {{
  border-radius: 12px;
}}
.omafile-keycap {{
  font-size: 0.86em;
  font-weight: 700;
  padding: 2px 7px;
  border-radius: 6px;
  background-color: alpha(currentColor, 0.08);
  border: 1px solid alpha(currentColor, 0.14);
  border-bottom-width: 2px;
}}
.omafile-help-title {{
  font-weight: 700;
}}
.omafile-help-icon {{
  color: {accent};
}}
.omafile-help-version {{
  font-size: 13pt;
  font-weight: 700;
  color: {accent};
}}
.omafile-help-found {{
  color: {green};
}}
.omafile-theme-page {{
  background-color: {view_bg};
}}
.omafile-theme-flow > flowboxchild {{
  padding: 0;
  border-radius: 14px;
  background: none;
  transition: transform 180ms cubic-bezier(0.22, 1, 0.36, 1);
}}
.omafile-theme-flow > flowboxchild:hover {{
  transform: scale(1.025);
}}
.omafile-theme-flow > flowboxchild:active {{
  transform: scale(0.985);
}}
.omafile-theme-flow > flowboxchild:focus-visible {{
  outline: 2px solid {accent};
  outline-offset: 3px;
}}
.omafile-theme-card {{
  border-radius: 14px;
  box-shadow: 0 0 0 1px alpha(black, 0.25), 0 6px 18px alpha(black, 0.22);
}}
.omafile-theme-card.omafile-theme-current {{
  box-shadow: 0 0 0 3px {accent}, 0 8px 22px alpha(black, 0.3);
}}
.omafile-theme-picture {{
  opacity: 0;
  transition: opacity 420ms ease-out;
}}
.omafile-theme-picture.omafile-theme-loaded {{
  opacity: 1;
}}
.omafile-theme-caption {{
  padding: 26px 12px 9px 12px;
  color: white;
  background-image: linear-gradient(to bottom, alpha(black, 0), alpha(black, 0.72));
}}
.omafile-theme-name {{
  font-weight: 700;
  text-shadow: 0 1px 3px alpha(black, 0.6);
}}
.omafile-theme-badge {{
  margin: 9px;
  padding: 5px;
  border-radius: 999px;
  color: {accent_fg};
  background-color: {accent};
}}
.omafile-theme-own {{
  margin: 9px;
  padding: 1px 8px;
  border-radius: 999px;
  font-size: 0.8em;
  font-weight: 700;
  color: white;
  background-color: alpha(black, 0.5);
}}
.omafile-theme-mock {{
  border-radius: 12px;
}}
.omafile-theme-accent {{
  padding: 3px;
  min-width: 0;
  min-height: 0;
}}
.omafile-theme-accent:checked {{
  box-shadow: 0 0 0 2px {accent};
  background: none;
}}
.omafile-menu-item {{
  padding: 6px 12px;
  font-weight: normal;
  border-radius: 6px;
}}
.omafile-capture-day {{
  font-size: 1.15em;
  font-weight: 800;
  margin: 12px 2px 6px 2px;
}}
.omafile-capture-actions {{
  margin: 8px;
  opacity: 0;
  transition: opacity 160ms ease-out;
}}
.omafile-capture-actions button {{
  min-width: 30px;
  min-height: 30px;
  padding: 0;
}}
flowboxchild:hover .omafile-capture-actions,
flowboxchild:focus-within .omafile-capture-actions {{
  opacity: 1;
}}
.omafile-capture-play {{
  padding: 11px;
  border-radius: 999px;
  color: white;
  background-color: alpha(black, 0.5);
}}
@keyframes omafile-capture-arrive {{
  from {{ opacity: 0; transform: scale(0.82); }}
  60% {{ opacity: 1; transform: scale(1.04); }}
  to {{ opacity: 1; transform: scale(1); }}
}}
.omafile-capture-new {{
  animation: omafile-capture-arrive 520ms cubic-bezier(0.22, 1, 0.36, 1);
}}
.omafile-capture-card {{
  transition: box-shadow 900ms ease-out;
}}
.omafile-capture-new .omafile-capture-card {{
  box-shadow: 0 0 0 3px {accent}, 0 8px 22px alpha(black, 0.3);
}}
.omafile-update-command {{
  padding: 10px 8px 10px 14px;
}}
.omafile-radio-air {{
  margin: 2px 18px 8px 18px;
  padding: 10px 14px;
  border-radius: 14px;
  background-color: alpha({accent}, 0.1);
  box-shadow: 0 0 0 1px alpha({accent}, 0.28);
}}
.omafile-radio-art {{
  border-radius: 10px;
}}
.omafile-radio-kind {{
  opacity: 0.8;
}}
.omafile-radio-title {{
  font-weight: 700;
}}
.omafile-tour-bubble {{
  padding: 14px;
  border-radius: 999px;
  color: {accent};
  background-color: alpha({accent}, 0.14);
  opacity: 0;
}}
.omafile-tour-bubble.omafile-tour-main {{
  padding: 22px;
  background-color: alpha({accent}, 0.22);
}}
.omafile-tour-art {{
  opacity: 0;
  box-shadow: 0 8px 24px alpha(black, 0.3);
}}
.omafile-tour-text {{
  font-size: 1.08em;
  opacity: 0.86;
}}
@keyframes omafile-tour-pop {{
  from {{ opacity: 0; transform: scale(0.6); }}
  70% {{ opacity: 1; transform: scale(1.06); }}
  to {{ opacity: 1; transform: scale(1); }}
}}
.omafile-tour-in .omafile-tour-bubble,
.omafile-tour-in .omafile-tour-art {{
  animation: omafile-tour-pop 480ms cubic-bezier(0.22, 1, 0.36, 1) both;
}}
.omafile-tour-in .omafile-tour-bubble:nth-child(1) {{
  animation-delay: 120ms;
}}
.omafile-tour-in .omafile-tour-bubble:nth-child(3) {{
  animation-delay: 200ms;
}}
@keyframes omafile-egg-turn {{
  from {{ transform: rotate(0deg) scale(1); }}
  50% {{ transform: rotate(180deg) scale(1.18); }}
  to {{ transform: rotate(360deg) scale(1); }}
}}
.omafile-egg-spin {{
  animation: omafile-egg-turn 900ms cubic-bezier(0.22, 1, 0.36, 1);
}}
.omafile-egg {{
  font-size: 1.5em;
  font-weight: 800;
  color: {accent};
  margin-top: 4px;
}}
.omafile-vault-page {{
  background-color: {view_bg};
}}
.omafile-vault-title {{
  font-size: 22pt;
  font-weight: 800;
  letter-spacing: -0.5px;
}}
.omafile-vault-entry {{
  min-height: 38px;
  border-radius: 999px;
  padding-left: 14px;
  padding-right: 8px;
}}
.omafile-vault-open {{
  color: {green};
}}
.omafile-pathbar {{
  background-color: {hover};
  border-radius: 999px;
  padding: 1px 6px;
  min-height: 30px;
}}
.omafile-pathbar button {{
  min-height: 24px;
  padding: 0 8px;
  border-radius: 999px;
  font-weight: 500;
}}
.omafile-pathbar button.current {{
  font-weight: 700;
  color: @accent_color;
}}
.omafile-pathbar .separator {{
  color: {dim};
  padding: 0 1px;
}}
.omafile-path-entry {{
  border-radius: 999px;
}}
.omafile-tabs tabbar {{
  background: transparent;
}}
.omafile-sidebar row {{
  border-radius: 8px;
}}
.omafile-preview {{
  background-color: @sidebar_bg_color;
}}
.omafile-preview-image {{
  border-radius: 10px;
}}
.omafile-preview-card {{
  background-color: @view_bg_color;
  border: 1px solid {border};
  border-radius: 10px;
}}
.omafile-preview-text {{
  background: transparent;
  font-size: 0.85em;
}}
.omafile-hidden {{
  opacity: 0.5;
  font-style: italic;
}}
.omafile-hidden image,
.omafile-hidden picture {{
  filter: grayscale(0.9);
}}
/* Motion: hover and selection colours ease instead of snapping. */
.omafile-files row, .omafile-files child, .omafile-sidebar row, button, .omafile-tag-dot, .omafile-git-badge, .omafile-thumb {{
  transition: background-color 150ms ease-out, color 150ms ease-out, opacity 150ms ease-out, box-shadow 150ms ease-out;
}}
.omafile-files row:hover, .omafile-files child:hover {{
  background-color: {hover};
}}
.omafile-play-badge {{
  background-color: rgba(0, 0, 0, 0.55);
  color: white;
  border-radius: 999px;
  padding: 0;
  min-width: 0;
  min-height: 0;
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.35);
  opacity: 0.85;
  -gtk-icon-size: 12px;
}}
.omafile-play-badge:hover {{
  background-color: {accent};
  opacity: 1;
}}
.omafile-play-badge-large {{
  min-width: 30px;
  min-height: 30px;
  -gtk-icon-size: 18px;
}}
.omafile-play-badge-small {{
  min-width: 14px;
  min-height: 14px;
  margin: -3px;
  -gtk-icon-size: 9px;
}}
.omafile-video {{
  border-radius: 10px;
  background-color: black;
}}
.omafile-media-controls {{
  margin-bottom: 4px;
}}
.omafile-symlink {{
  font-style: italic;
}}
.omafile-exec {{
  color: {green};
}}
.omafile-dim {{
  color: {dim};
}}
.omafile-split-active .omafile-card {{
  border-color: alpha(@accent_bg_color, 0.55);
}}
.omafile-icon-choice {{
  border-radius: 10px;
  padding: 8px;
}}
.omafile-icon-choice:checked {{
  background-color: alpha(@accent_bg_color, 0.25);
}}
toast {{
  border-radius: 999px;
}}
.omafile-quicklook .omafile-preview {{
  background-color: @dialog_bg_color;
}}
.omafile-git-badge {{
  font-size: 0.75em;
  font-weight: 700;
  min-width: 14px;
  padding: 0 5px;
  border-radius: 6px;
  background-color: {hover};
}}
.omafile-git-modified {{ color: {yellow}; }}
.omafile-git-added {{ color: {green}; }}
.omafile-git-deleted {{ color: {red}; }}
.omafile-git-ignored {{ color: {dim}; }}
.omafile-palette-entry {{
  font-size: 1.1em;
  border-radius: 10px;
}}
.omafile-palette-list {{
  background: transparent;
}}
.omafile-palette-list row {{
  border-radius: 8px;
}}
.omafile-master-key {{
  font-family: monospace;
  font-size: 1.05em;
}}
.omafile-terminal-panel {{
  background-color: @view_bg_color;
  border: 1px solid {border};
  border-radius: 12px;
  margin: 4px 10px 10px 4px;
}}
.omafile-terminal-header {{
  padding: 4px 6px 2px 10px;
  border-bottom: 1px solid {border};
}}
.omafile-terminal-header button {{
  min-height: 22px;
  min-width: 22px;
  padding: 0 4px;
}}
.omafile-terminal {{
  padding: 6px 8px;
}}
.omafile-git-badge.omafile-git-staged {{
  box-shadow: inset 0 0 0 1px {accent};
  text-decoration: underline;
}}
.omafile-branch {{
  font-size: 0.85em;
  padding: 1px 8px;
  min-height: 0;
  color: {accent};
}}
.omafile-qr {{
  background-color: white;
  border-radius: 10px;
  padding: 8px;
}}
.omafile-share-url {{
  font-family: monospace;
  font-size: 0.95em;
}}
.omafile-commit-body {{
  font-family: monospace;
}}
.omafile-secret {{
  color: {yellow};
}}
.omafile-rename-popover > contents {{
  padding: 4px;
}}
.omafile-progress {{
  background-color: @view_bg_color;
  border-top: 1px solid {border};
  padding: 8px 14px;
}}
.omafile-progress-bar trough {{
  min-height: 6px;
  border-radius: 3px;
  background-color: {border};
}}
.omafile-progress-bar progress {{
  min-height: 6px;
  border-radius: 3px;
  background-color: {accent};
}}
.omafile-tag-dot {{
  min-width: 10px;
  min-height: 10px;
  border-radius: 999px;
  margin: 0 2px;
  box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.25);
}}
.omafile-tag-sidebar {{
  min-width: 14px;
  min-height: 14px;
  margin: 0 1px;
}}
.omafile-tag-red {{ background-color: {red}; }}
.omafile-tag-orange {{ background-color: {orange}; }}
.omafile-tag-yellow {{ background-color: {yellow}; }}
.omafile-tag-green {{ background-color: {green}; }}
.omafile-tag-blue {{ background-color: {blue}; }}
.omafile-tag-purple {{ background-color: {magenta}; }}
.omafile-tag-gray {{ background-color: {dim}; }}
.omafile-sidebar-eject {{
  min-width: 22px;
  min-height: 22px;
  padding: 0;
  opacity: 0.6;
}}
.omafile-sidebar-eject:hover {{
  opacity: 1;
}}
@keyframes omafile-pulse {{
  from {{ opacity: 0.45; }}
  to {{ opacity: 1.0; }}
}}
.omafile-skeleton-bar {{
  background-color: alpha(currentColor, 0.10);
  border-radius: 6px;
  animation: omafile-pulse 900ms ease-in-out infinite alternate;
}}
.omafile-np-mini {{
  margin: 6px 8px 8px 8px;
  padding: 10px;
  border-radius: 12px;
  background-color: alpha(currentColor, 0.06);
  border: 1px solid alpha(currentColor, 0.08);
}}
.omafile-np-mini {{
  transition: background-color 220ms ease, border-color 220ms ease;
}}
.omafile-np-mini:hover {{
  background-color: alpha(currentColor, 0.10);
  border-color: alpha({accent}, 0.45);
}}
@keyframes omafile-np-arrive {{
  0% {{ background-color: alpha({accent}, 0.34); border-color: alpha({accent}, 0.95); box-shadow: 0 0 0 0 alpha({accent}, 0.0); }}
  35% {{ background-color: alpha({accent}, 0.20); border-color: alpha({accent}, 0.80); box-shadow: 0 0 9px 1px alpha({accent}, 0.55); }}
  100% {{ background-color: alpha(currentColor, 0.06); border-color: alpha(currentColor, 0.08); box-shadow: 0 0 0 0 alpha({accent}, 0.0); }}
}}
.omafile-np-arrive {{
  animation: omafile-np-arrive 1400ms ease-out 1;
}}
@keyframes omafile-np-pop-a {{
  0% {{ transform: scale(0.70); }}
  55% {{ transform: scale(1.14); }}
  100% {{ transform: scale(1.0); }}
}}
@keyframes omafile-np-pop-b {{
  0% {{ transform: scale(0.70); }}
  55% {{ transform: scale(1.14); }}
  100% {{ transform: scale(1.0); }}
}}
.omafile-np-pop-a {{
  animation: omafile-np-pop-a 340ms ease-out 1;
}}
.omafile-np-pop-b {{
  animation: omafile-np-pop-b 340ms ease-out 1;
}}
.omafile-np-mini button {{
  transition: transform 150ms cubic-bezier(0.2, 0.9, 0.3, 1.4), background-color 150ms ease;
}}
.omafile-np-mini button:hover {{
  transform: scale(1.14);
}}
.omafile-np-mini button:active {{
  transform: scale(0.90);
}}
.omafile-np-mini-title {{
  font-weight: 700;
}}
.omafile-np-mini button {{
  min-width: 28px;
  min-height: 28px;
  padding: 0;
}}
.omafile-np-mini-play {{
  background-color: {accent};
  color: {accent_fg};
}}
.omafile-np-progress trough {{
  min-height: 3px;
  border-radius: 2px;
  background-color: alpha(currentColor, 0.12);
}}
.omafile-np-progress progress {{
  min-height: 3px;
  border-radius: 2px;
  background-color: {accent};
}}
.omafile-np-header {{
  padding: 4px 8px;
}}
.omafile-np-page {{
  background-color: {view_bg};
}}
.omafile-np-backdrop {{
  opacity: 0.30;
}}
.omafile-np-title {{
  font-size: 22pt;
  font-weight: 800;
  letter-spacing: -0.5px;
}}
.omafile-np-subtitle {{
  font-size: 12pt;
  opacity: 0.75;
}}
.omafile-np-source {{
  letter-spacing: 2px;
  text-transform: uppercase;
  margin-top: 2px;
}}
.omafile-np-play {{
  min-width: 64px;
  min-height: 64px;
  background-color: {accent};
  color: {accent_fg};
  box-shadow: 0 6px 22px alpha({accent}, 0.45);
  transition: transform 160ms cubic-bezier(0.2, 0.9, 0.3, 1.4), box-shadow 200ms ease;
  -gtk-icon-size: 26px;
}}
.omafile-np-play:hover {{
  transform: scale(1.08);
  box-shadow: 0 8px 30px alpha({accent}, 0.65);
}}
.omafile-np-play:active {{
  transform: scale(0.94);
}}
.omafile-np-skip {{
  min-width: 44px;
  min-height: 44px;
  -gtk-icon-size: 20px;
  transition: transform 160ms cubic-bezier(0.2, 0.9, 0.3, 1.4);
}}
.omafile-np-skip:hover {{
  transform: scale(1.12);
}}
.omafile-np-on, .omafile-np-page togglebutton:checked {{
  color: {accent};
  background-color: alpha({accent}, 0.16);
}}
.omafile-np-seek trough {{
  min-height: 5px;
  border-radius: 3px;
  background-color: alpha(currentColor, 0.14);
}}
.omafile-np-seek highlight {{
  min-height: 5px;
  border-radius: 3px;
  background-color: {accent};
}}
.omafile-np-seek slider {{
  min-width: 14px;
  min-height: 14px;
  margin: -5px;
  background-color: {fg};
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
  transition: transform 120ms ease;
}}
.omafile-np-seek:hover slider {{
  transform: scale(1.25);
}}
"#,
        name = p.name,
        bg = p.background.hex(),
        fg = p.foreground.hex(),
        view_bg = p.dark_background.hex(),
        card_bg = p.lighter_background.hex(),
        popover_bg = popover_bg.hex(),
        dialog_bg = dialog_bg.hex(),
        accent = p.accent.hex(),
        accent_fg = accent_fg.hex(),
        accent_text = accent_text.hex(),
        red = p.red.hex(),
        orange = p.orange.hex(),
        green = p.green.hex(),
        yellow = p.yellow.hex(),
        blue = p.blue.hex(),
        magenta = p.magenta.hex(),
        dim = p.dark_foreground.hex(),
        bright_fg = p.bright_foreground.hex(),
    )
}

type Listener = Box<dyn Fn(&Palette)>;

thread_local! {
    static CURRENT: RefCell<Option<Palette>> = const { RefCell::new(None) };
}

/// The palette last applied, for widgets that need raw colours (diff view).
pub fn current() -> Option<Palette> {
    CURRENT.with(|c| c.borrow().clone())
}

pub struct ThemeManager {
    provider: gtk::CssProvider,
    palette: RefCell<Palette>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
    icon_override: RefCell<Option<String>>,
    /// A skin: omafile's own look, worn instead of the Omarchy theme that is on.
    skin: RefCell<Option<PathBuf>>,
    pending: RefCell<Option<glib::SourceId>>,
    listeners: RefCell<Vec<Listener>>,
}

impl ThemeManager {
    pub fn install(icon_override: Option<String>, skin: Option<PathBuf>) -> Rc<Self> {
        let provider = gtk::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 10);
        }
        let manager = Rc::new(ThemeManager {
            provider,
            palette: RefCell::new(Palette::default()),
            monitors: RefCell::new(Vec::new()),
            icon_override: RefCell::new(icon_override),
            skin: RefCell::new(skin),
            pending: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
        });
        manager.apply();
        manager.watch();
        manager
    }

    pub fn palette(&self) -> Palette {
        self.palette.borrow().clone()
    }

    /// Called with the new palette after every theme change (and once right away).
    pub fn connect_changed(&self, f: impl Fn(&Palette) + 'static) {
        f(&self.palette.borrow());
        self.listeners.borrow_mut().push(Box::new(f));
    }

    /// `None` follows the Omarchy theme's `icons.theme`; `Some(name)` forces an icon theme.
    pub fn set_icon_override(&self, name: Option<String>) {
        *self.icon_override.borrow_mut() = name;
        self.apply_icon_theme();
    }

    /// `Some(folder)`: omafile wears this skin, whatever theme Omarchy has on. `None`: it
    /// follows Omarchy's theme. Only omafile changes; nothing of the desktop is touched.
    pub fn set_skin(&self, skin: Option<PathBuf>) {
        *self.skin.borrow_mut() = skin;
        self.apply();
    }

    /// The folder the colours come from: the skin while it exists, else Omarchy's theme.
    fn source(&self) -> PathBuf {
        let skin = self.skin.borrow().clone();
        skin.filter(|d| d.join("colors.toml").is_file() || d.join("alacritty.toml").is_file()).unwrap_or_else(theme_dir)
    }

    fn apply_icon_theme(&self) {
        let Some(settings) = gtk::Settings::default() else { return };
        let wanted = self.icon_override.borrow().clone().or_else(|| self.palette.borrow().icon_theme.clone());
        if let Some(name) = wanted {
            if settings.gtk_icon_theme_name().as_deref() != Some(name.as_str()) {
                settings.set_gtk_icon_theme_name(Some(&name));
            }
        }
    }

    fn apply(&self) {
        let palette = load_from(&self.source());
        let style = adw::StyleManager::default();
        style.set_color_scheme(if palette.light { adw::ColorScheme::ForceLight } else { adw::ColorScheme::ForceDark });
        self.provider.load_from_string(&css(&palette));
        CURRENT.with(|c| *c.borrow_mut() = Some(palette.clone()));
        *self.palette.borrow_mut() = palette;
        self.apply_icon_theme();
        let palette = self.palette.borrow().clone();
        for listener in self.listeners.borrow().iter() {
            listener(&palette);
        }
    }

    fn watch(self: &Rc<Self>) {
        // Watch the `current` directories (the `theme` link lives there) and the
        // palette files themselves, so edits to a custom theme show up too.
        let mut targets = Vec::new();
        for dir in theme_dirs() {
            if let Some(parent) = dir.parent() {
                targets.push(parent.to_path_buf());
            }
            targets.push(dir.join("colors.toml"));
            targets.push(dir.join("alacritty.toml"));
            targets.push(dir.join("icons.theme"));
        }
        for target in targets {
            let file = gio::File::for_path(&target);
            let Ok(monitor) = file.monitor(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) else { continue };
            let weak = Rc::downgrade(self);
            monitor.connect_changed(move |_, _, _, _| {
                let Some(manager) = weak.upgrade() else { return };
                // Debounce: a theme switch touches several files in a burst.
                if let Some(id) = manager.pending.borrow_mut().take() {
                    id.remove();
                }
                let weak2 = weak.clone();
                let id = glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                    if let Some(m) = weak2.upgrade() {
                        m.pending.borrow_mut().take();
                        m.apply();
                    }
                });
                *manager.pending.borrow_mut() = Some(id);
            });
            self.monitors.borrow_mut().push(monitor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colors_toml() {
        let src = "mode = \"dark\"\n\naccent = \"#7aa2f7\"\nbackground = \"#1a1b26\" # bg\nforeground = '#a9b1d6'\n";
        let (colors, strings) = parse_colors(src);
        assert_eq!(colors["accent"], Rgb(0x7a, 0xa2, 0xf7));
        assert_eq!(colors["background"], Rgb(0x1a, 0x1b, 0x26));
        assert_eq!(strings["mode"], "dark");
    }

    #[test]
    fn parses_legacy_alacritty() {
        let src = "[colors]\n[colors.primary]\nbackground = \"#1a1b26\"\n[colors.normal]\nblack = \"0x32344a\"\n";
        let (colors, _) = parse_colors(src);
        assert_eq!(colors["primary.background"], Rgb(0x1a, 0x1b, 0x26));
        assert_eq!(colors["normal.black"], Rgb(0x32, 0x34, 0x4a));
    }

    #[test]
    fn loads_current_layout() {
        let dir = std::env::temp_dir().join(format!("omafile-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("colors.toml"), "mode = \"light\"\naccent = \"#56949f\"\nbackground = \"#faf4ed\"\nforeground = \"#575279\"\n").unwrap();
        std::fs::write(dir.join("icons.theme"), "Yaru-sage\n").unwrap();
        let p = load_from(&dir);
        assert!(p.light);
        assert_eq!(p.accent, Rgb(0x56, 0x94, 0x9f));
        assert_eq!(p.icon_theme.as_deref(), Some("Yaru-sage"));
        assert!(css(&p).contains("@define-color accent_bg_color #56949f;"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mix_and_luminance() {
        assert_eq!(Rgb(0, 0, 0).mix(Rgb(255, 255, 255), 0.5), Rgb(128, 128, 128));
        assert!(Rgb(255, 255, 255).luminance() > 0.99);
    }
}
