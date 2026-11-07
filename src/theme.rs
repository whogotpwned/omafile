//! Omarchy theme integration.
//!
//! `omarchy-theme-set` repoints `~/.config/omarchy/current/theme`. Every theme
//! ships an `alacritty.toml` with a 16-colour palette; light themes also contain
//! a `light.mode` marker. We turn the palette into libadwaita named colours and
//! install them as a CSS provider, then re-apply whenever the link changes.

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

    /// Linear blend towards `other` by `t` in 0..=1.
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

#[derive(Clone, Debug)]
pub struct Palette {
    pub name: String,
    pub light: bool,
    pub bg: Rgb,
    pub fg: Rgb,
    pub black: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
    pub white: Rgb,
    pub bright_black: Rgb,
    pub bright_blue: Rgb,
}

impl Default for Palette {
    /// Tokyo Night, the Omarchy default.
    fn default() -> Self {
        Palette {
            name: "tokyo-night".into(),
            light: false,
            bg: Rgb(0x1a, 0x1b, 0x26),
            fg: Rgb(0xa9, 0xb1, 0xd6),
            black: Rgb(0x32, 0x34, 0x4a),
            red: Rgb(0xf7, 0x76, 0x8e),
            green: Rgb(0x9e, 0xce, 0x6a),
            yellow: Rgb(0xe0, 0xaf, 0x68),
            blue: Rgb(0x7a, 0xa2, 0xf7),
            magenta: Rgb(0xad, 0x8e, 0xe6),
            cyan: Rgb(0x44, 0x9d, 0xab),
            white: Rgb(0x78, 0x7c, 0x99),
            bright_black: Rgb(0x44, 0x4b, 0x6a),
            bright_blue: Rgb(0x7d, 0xa6, 0xff),
        }
    }
}

pub fn theme_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("OMAFILE_THEME_DIR") {
        return PathBuf::from(dir);
    }
    glib::home_dir().join(".config/omarchy/current/theme")
}

/// Minimal parser for the `[colors.*]` tables of an Alacritty config.
pub fn parse_alacritty(src: &str) -> HashMap<String, Rgb> {
    let mut out = HashMap::new();
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
            if let Some(color) = Rgb::parse(value) {
                let key = key.trim().trim_matches('"');
                let full = if section.is_empty() { key.to_string() } else { format!("{section}.{key}") };
                out.insert(full, color);
            }
        }
    }
    out
}

fn theme_name(dir: &Path) -> String {
    std::fs::read_link(dir)
        .ok()
        .or_else(|| std::fs::canonicalize(dir).ok())
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "custom".into())
}

pub fn load_from(dir: &Path) -> Palette {
    let mut p = Palette::default();
    let Ok(src) = std::fs::read_to_string(dir.join("alacritty.toml")) else {
        return p;
    };
    let map = parse_alacritty(&src);
    if map.is_empty() {
        return p;
    }
    p.name = theme_name(dir);
    p.light = dir.join("light.mode").exists();
    let pick = |key: &str, fallback: Rgb| map.get(key).copied().unwrap_or(fallback);
    p.bg = pick("primary.background", p.bg);
    p.fg = pick("primary.foreground", p.fg);
    p.black = pick("normal.black", p.black);
    p.red = pick("normal.red", p.red);
    p.green = pick("normal.green", p.green);
    p.yellow = pick("normal.yellow", p.yellow);
    p.blue = pick("normal.blue", p.blue);
    p.magenta = pick("normal.magenta", p.magenta);
    p.cyan = pick("normal.cyan", p.cyan);
    p.white = pick("normal.white", p.white);
    p.bright_black = pick("bright.black", p.black);
    p.bright_blue = pick("bright.blue", p.blue);
    // Trust the palette over the marker if they disagree wildly.
    if !p.light && p.bg.luminance() > 0.6 {
        p.light = true;
    }
    p
}

/// libadwaita named colours derived from the palette.
pub fn css(p: &Palette) -> String {
    let dark = !p.light;
    // Surfaces: step away from the window background in the direction of the
    // foreground so sidebars, cards and popovers stay distinguishable.
    let toward = if dark { Rgb(255, 255, 255) } else { Rgb(0, 0, 0) };
    let view_bg = if dark { p.bg.mix(Rgb(0, 0, 0), 0.18) } else { p.bg.mix(Rgb(255, 255, 255), 0.6) };
    let sidebar_bg = p.bg.mix(toward, if dark { 0.045 } else { 0.03 });
    let headerbar_bg = sidebar_bg;
    let card_bg = p.bg.mix(toward, if dark { 0.06 } else { 0.4 });
    let popover_bg = p.bg.mix(toward, if dark { 0.08 } else { 0.5 });
    let dialog_bg = p.bg.mix(toward, if dark { 0.05 } else { 0.3 });
    let border = p.bright_black.rgba(if dark { 0.55 } else { 0.35 });
    let accent = p.blue;
    // Text on the accent must stay readable on light and dark accents alike.
    let accent_fg = if accent.luminance() > 0.55 { p.bg.mix(Rgb(0, 0, 0), 0.6) } else { Rgb(255, 255, 255) };
    let accent_text = if dark { p.bright_blue } else { p.blue.mix(Rgb(0, 0, 0), 0.25) };
    let selected_row = accent.rgba(if dark { 0.22 } else { 0.18 });

    format!(
        r#"/* generated by omafile from the Omarchy theme "{name}" */
@define-color window_bg_color {bg};
@define-color window_fg_color {fg};
@define-color view_bg_color {view_bg};
@define-color view_fg_color {fg};
@define-color headerbar_bg_color {headerbar_bg};
@define-color headerbar_fg_color {fg};
@define-color headerbar_border_color {border};
@define-color headerbar_backdrop_color {bg};
@define-color headerbar_shade_color rgba(0, 0, 0, 0.12);
@define-color headerbar_darker_shade_color rgba(0, 0, 0, 0.2);
@define-color sidebar_bg_color {sidebar_bg};
@define-color sidebar_fg_color {fg};
@define-color sidebar_backdrop_color {bg};
@define-color sidebar_border_color {border};
@define-color sidebar_shade_color rgba(0, 0, 0, 0.12);
@define-color secondary_sidebar_bg_color {sidebar_bg};
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

.omafile-files listview > row:selected,
.omafile-files gridview > child:selected {{
  background-color: {selected_row};
  color: @window_fg_color;
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
  border-radius: 9px;
  padding: 6px;
}}
.omafile-files columnview > header {{
  background-color: @view_bg_color;
}}
.omafile-pathbar button {{
  min-height: 24px;
  padding: 0 8px;
  border-radius: 6px;
}}
.omafile-pathbar button.current {{
  font-weight: bold;
  color: @accent_color;
}}
.omafile-pathbar .separator {{
  color: alpha(@window_fg_color, 0.45);
  padding: 0 2px;
}}
.omafile-hidden {{
  opacity: 0.6;
}}
.omafile-symlink {{
  font-style: italic;
}}
.omafile-exec {{
  color: {green};
}}
.omafile-dim {{
  color: alpha(@window_fg_color, 0.55);
}}
"#,
        name = p.name,
        bg = p.bg.hex(),
        fg = p.fg.hex(),
        view_bg = view_bg.hex(),
        headerbar_bg = headerbar_bg.hex(),
        sidebar_bg = sidebar_bg.hex(),
        card_bg = card_bg.hex(),
        popover_bg = popover_bg.hex(),
        dialog_bg = dialog_bg.hex(),
        accent = accent.hex(),
        accent_fg = accent_fg.hex(),
        accent_text = accent_text.hex(),
        red = p.red.hex(),
        green = p.green.hex(),
        yellow = p.yellow.hex(),
    )
}

/// Installs the theme CSS and keeps it in sync with the active Omarchy theme.
pub struct ThemeManager {
    provider: gtk::CssProvider,
    dir: PathBuf,
    monitors: RefCell<Vec<gio::FileMonitor>>,
}

impl ThemeManager {
    pub fn install() -> Rc<Self> {
        let provider = gtk::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 10,
            );
        }
        let manager = Rc::new(ThemeManager {
            provider,
            dir: theme_dir(),
            monitors: RefCell::new(Vec::new()),
        });
        manager.apply();
        manager.watch();
        manager
    }

    fn apply(&self) {
        let palette = load_from(&self.dir);
        let style = adw::StyleManager::default();
        style.set_color_scheme(if palette.light { adw::ColorScheme::ForceLight } else { adw::ColorScheme::ForceDark });
        self.provider.load_from_string(&css(&palette));
    }

    fn watch(self: &Rc<Self>) {
        // The `current` directory holds the `theme` symlink; the palette file is
        // watched too so edits to a custom theme show up immediately.
        let mut targets = vec![self.dir.join("alacritty.toml")];
        if let Some(parent) = self.dir.parent() {
            targets.push(parent.to_path_buf());
        }
        for target in targets {
            let file = gio::File::for_path(&target);
            let Ok(monitor) = file.monitor(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) else {
                continue;
            };
            let weak = Rc::downgrade(self);
            let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
            monitor.connect_changed(move |_, _, _, _| {
                let Some(manager) = weak.upgrade() else { return };
                // Debounce: theme switches touch several files in a burst.
                if let Some(id) = pending.borrow_mut().take() {
                    id.remove();
                }
                let pending2 = pending.clone();
                let id = glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                    pending2.borrow_mut().take();
                    manager.apply();
                });
                *pending.borrow_mut() = Some(id);
            });
            self.monitors.borrow_mut().push(monitor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_omarchy_palette() {
        let src = "[colors]\n[colors.primary]\nbackground = \"#1a1b26\"\nforeground = '#a9b1d6' # c\n[colors.normal]\nblack = \"0x32344a\"\n";
        let map = parse_alacritty(src);
        assert_eq!(map["primary.background"], Rgb(0x1a, 0x1b, 0x26));
        assert_eq!(map["primary.foreground"], Rgb(0xa9, 0xb1, 0xd6));
        assert_eq!(map["normal.black"], Rgb(0x32, 0x34, 0x4a));
    }

    #[test]
    fn css_mentions_accent() {
        let p = Palette::default();
        let css = css(&p);
        assert!(css.contains("@define-color accent_bg_color #7aa2f7;"));
        assert!(css.contains("window_bg_color #1a1b26"));
    }

    #[test]
    fn mix_and_luminance() {
        assert_eq!(Rgb(0, 0, 0).mix(Rgb(255, 255, 255), 0.5), Rgb(128, 128, 128));
        assert!(Rgb(255, 255, 255).luminance() > 0.99);
    }
}
