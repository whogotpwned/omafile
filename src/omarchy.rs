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

/// Whether a file is a picture Omarchy can show as a background.
pub fn is_picture(p: &Path) -> bool {
    p.is_file() && is_image_name(p)
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

// ----- the themes that are installed -------------------------------------------------------

/// Omarchy's own folder (`$OMARCHY_PATH`, by default `~/.local/share/omarchy`).
pub fn omarchy_path() -> PathBuf {
    std::env::var_os("OMARCHY_PATH").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(|| glib::home_dir().join(".local/share/omarchy"))
}

/// The themes Omarchy ships.
pub fn stock_themes_dir() -> PathBuf {
    omarchy_path().join("themes")
}

/// Whether this Omarchy builds a theme's files from its `colors.toml` (the templates in
/// `default/themed`). Without them a theme needs every file written by hand.
pub fn templates_available() -> bool {
    glib::find_program_in_path("omarchy-theme-set-templates").is_some() || omarchy_path().join("default/themed").is_dir()
}

/// The name as Omarchy shows it: `tokyo-night` becomes `Tokyo Night`.
pub fn display_name(name: &str) -> String {
    name.split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A folder name `omarchy-theme-set` finds again: it lowercases what it is given and turns
/// spaces into dashes, so the folder must be lowercase without spaces. Everything that is
/// not a letter or a digit becomes a dash here.
pub fn folder_name(wanted: &str) -> String {
    let mut out = String::new();
    for c in wanted.trim().to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The name of the theme that is on, as `omarchy-theme-set` recorded it.
pub fn current_theme_name() -> Option<String> {
    let home = glib::home_dir();
    current_theme_name_in(&[home.join(".config/omarchy/current"), home.join(".local/state/omarchy/current")])
}

pub fn current_theme_name_in(current_dirs: &[PathBuf]) -> Option<String> {
    for dir in current_dirs {
        if let Ok(name) = std::fs::read_to_string(dir.join("theme.name")) {
            if !name.trim().is_empty() {
                return Some(name.trim().to_string());
            }
        }
        // Older releases: `current/theme` is a link to the theme's folder.
        if let Ok(target) = std::fs::read_link(dir.join("theme")) {
            if let Some(name) = target.file_name() {
                return Some(name.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// One installed theme, as the gallery shows it.
#[derive(Clone, Debug)]
pub struct ThemeInfo {
    /// The folder's name: what `omarchy-theme-set` takes.
    pub name: String,
    pub dir: PathBuf,
    /// In the user's folder (can be deleted); otherwise one Omarchy ships.
    pub user: bool,
    pub preview: Option<PathBuf>,
    pub light: bool,
    /// Background, foreground, accent, then red, yellow, green, cyan, blue, magenta.
    pub swatches: Vec<crate::theme::Rgb>,
    pub backgrounds: Vec<PathBuf>,
}

/// Where a user's own backgrounds for a theme go (Omarchy looks here before the theme's own).
pub fn user_backgrounds_dir(theme: &str) -> PathBuf {
    glib::home_dir().join(".config/omarchy/backgrounds").join(theme)
}

fn images_in(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.is_file() && is_image_name(p)).collect()).unwrap_or_default();
    files.sort();
    files
}

fn theme_info(dir: &Path, name: &str, user: bool, extra_backgrounds: &Path) -> ThemeInfo {
    let p = crate::theme::load_from(dir);
    let mut backgrounds = images_in(extra_backgrounds);
    backgrounds.extend(images_in(&dir.join("backgrounds")));
    // The first background shows the theme better than a screenshot of a terminal does.
    let preview = backgrounds.first().cloned().or_else(|| folder_preview(dir));
    ThemeInfo { name: name.to_string(), dir: dir.to_path_buf(), user, preview, light: p.light, swatches: vec![p.background, p.foreground, p.accent, p.red, p.yellow, p.green, p.cyan, p.blue, p.magenta], backgrounds }
}

/// Every theme: the user's and the ones Omarchy ships, by name; a user's theme hides a
/// shipped one of the same name, as it does for `omarchy-theme-set`.
pub fn list_themes() -> Vec<ThemeInfo> {
    list_themes_in(&install_dir(), &stock_themes_dir(), &glib::home_dir().join(".config/omarchy/backgrounds"))
}

pub fn list_themes_in(user_dir: &Path, stock_dir: &Path, backgrounds_dir: &Path) -> Vec<ThemeInfo> {
    let mut by_name: std::collections::BTreeMap<String, ThemeInfo> = std::collections::BTreeMap::new();
    for (dir, user) in [(stock_dir, false), (user_dir, true)] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
            if name.starts_with('.') || !is_theme_dir(&path) {
                continue;
            }
            by_name.insert(name.clone(), theme_info(&path, &name, user, &backgrounds_dir.join(&name)));
        }
    }
    by_name.into_values().collect()
}

/// Whether an icon set is installed (for the user or for everyone).
pub fn icon_theme_installed(name: &str) -> bool {
    let home = glib::home_dir();
    [home.join(".local/share/icons"), home.join(".icons"), PathBuf::from("/usr/share/icons"), PathBuf::from("/usr/local/share/icons")].iter().any(|dir| dir.join(name).join("index.theme").is_file())
}

/// Write a new theme into the user's folder: `colors.toml` (from which Omarchy's templates
/// build every program's file), the picture as its background, a preview, the icon set and
/// the marker for a light theme. Fails when the name is taken or empty.
pub fn write_theme(themes_dir: &Path, name: &str, scheme: &crate::scheme::Scheme, picture: &Path, preview_png: Option<&[u8]>, icons: Option<&str>) -> Result<PathBuf, String> {
    let folder = folder_name(name);
    if folder.is_empty() {
        return Err("the name needs a letter or a digit".into());
    }
    let dir = themes_dir.join(&folder);
    if dir.exists() {
        return Err(format!("a theme named {folder} exists already"));
    }
    let io = |e: std::io::Error| e.to_string();
    std::fs::create_dir_all(dir.join("backgrounds")).map_err(io)?;
    let result = (|| {
        std::fs::write(dir.join("colors.toml"), scheme.colors_toml()).map_err(io)?;
        if let Some(icons) = icons {
            std::fs::write(dir.join("icons.theme"), format!("{icons}\n")).map_err(io)?;
        }
        if scheme.light {
            std::fs::write(dir.join("light.mode"), "").map_err(io)?;
        }
        let extension = picture.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "jpg".into());
        std::fs::copy(picture, dir.join("backgrounds").join(format!("1-{folder}.{extension}"))).map_err(io)?;
        if let Some(png) = preview_png {
            std::fs::write(dir.join("preview.png"), png).map_err(io)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(dir),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omafile-omarchy-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_the_way_omarchy_does() {
        assert_eq!(display_name("tokyo-night"), "Tokyo Night");
        assert_eq!(display_name("retro-82"), "Retro 82");
        assert_eq!(display_name("white"), "White");
        assert_eq!(folder_name("Lake at Dusk"), "lake-at-dusk");
        assert_eq!(folder_name("  IMG_2041 (1).JPG "), "img-2041-1-jpg");
        assert_eq!(folder_name("Über—Nacht"), "über-nacht");
        assert_eq!(folder_name("!!!"), "");
        // What omarchy-theme-set makes of the display name leads back to the folder.
        assert_eq!(display_name(&folder_name("Lake at Dusk")).to_lowercase().replace(' ', "-"), "lake-at-dusk");
    }

    #[test]
    fn lists_themes_and_writes_a_new_one() {
        let root = scratch("themes");
        let (user, stock, backgrounds, current) = (root.join("user"), root.join("stock"), root.join("backgrounds"), root.join("current"));
        let tokyo = "accent = \"#7aa2f7\"\nforeground = \"#a9b1d6\"\nbackground = \"#1a1b26\"\ncolor1 = \"#f7768e\"\ncolor4 = \"#7aa2f7\"\n";
        for (dir, name, colors) in [(&stock, "tokyo-night", tokyo), (&stock, "white", "accent = \"#333333\"\nforeground = \"#111111\"\nbackground = \"#ffffff\"\n"), (&user, "tokyo-night", "accent = \"#ff0000\"\nforeground = \"#a9b1d6\"\nbackground = \"#1a1b26\"\n")] {
            std::fs::create_dir_all(dir.join(name).join("backgrounds")).unwrap();
            std::fs::write(dir.join(name).join("colors.toml"), colors).unwrap();
        }
        std::fs::write(stock.join("white/backgrounds/2.png"), "x").unwrap();
        std::fs::write(stock.join("white/backgrounds/1.jpg"), "x").unwrap();
        std::fs::write(stock.join("white/preview.png"), "x").unwrap();
        std::fs::create_dir_all(stock.join("not-a-theme")).unwrap();
        std::fs::create_dir_all(backgrounds.join("white")).unwrap();
        std::fs::write(backgrounds.join("white/mine.webp"), "x").unwrap();

        let themes = list_themes_in(&user, &stock, &backgrounds);
        assert_eq!(themes.iter().map(|t| (t.name.as_str(), t.user)).collect::<Vec<_>>(), vec![("tokyo-night", true), ("white", false)], "the user's theme hides the shipped one");
        assert_eq!(themes[0].swatches[2], crate::theme::Rgb(255, 0, 0));
        let white = &themes[1];
        assert!(white.light);
        assert_eq!(white.backgrounds.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect::<Vec<_>>(), vec!["mine.webp", "1.jpg", "2.png"], "the user's own backgrounds first, as Omarchy cycles them");
        assert_eq!(white.preview.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).as_deref(), Some("mine.webp"));

        // The current theme: the name file, or the link of older releases.
        assert_eq!(current_theme_name_in(std::slice::from_ref(&current)), None);
        std::fs::create_dir_all(&current).unwrap();
        std::os::unix::fs::symlink(stock.join("white"), current.join("theme")).unwrap();
        assert_eq!(current_theme_name_in(std::slice::from_ref(&current)).as_deref(), Some("white"));
        std::fs::write(current.join("theme.name"), "tokyo-night\n").unwrap();
        assert_eq!(current_theme_name_in(std::slice::from_ref(&current)).as_deref(), Some("tokyo-night"));

        // A new theme from a picture.
        let picture = root.join("Lake.JPG");
        std::fs::write(&picture, "jpeg bytes").unwrap();
        let analysis = crate::scheme::analyse(&[[240, 240, 235], [40, 90, 200]]);
        let scheme = crate::scheme::scheme(&analysis, true, None);
        let dir = write_theme(&user, "Lake at Dusk", &scheme, &picture, Some(b"png"), Some("Yaru-blue")).unwrap();
        assert_eq!(dir, user.join("lake-at-dusk"));
        assert!(is_theme_dir(&dir) && dir.join("light.mode").is_file() && dir.join("preview.png").is_file());
        assert_eq!(std::fs::read_to_string(dir.join("backgrounds/1-lake-at-dusk.jpg")).unwrap(), "jpeg bytes");
        assert_eq!(std::fs::read_to_string(dir.join("icons.theme")).unwrap(), "Yaru-blue\n");
        assert_eq!(crate::scheme::Scheme::from_colors_toml(&std::fs::read_to_string(dir.join("colors.toml")).unwrap()), Some(scheme.clone()));
        let listed = list_themes_in(&user, &stock, &backgrounds);
        let lake = listed.iter().find(|t| t.name == "lake-at-dusk").unwrap();
        assert!(lake.user && lake.light && lake.swatches[2] == scheme.accent && lake.backgrounds.len() == 1);
        assert!(!icon_theme_installed("no-such-icon-set-anywhere"));
        // Taken names and empty names are refused, and nothing half-written stays.
        assert!(write_theme(&user, "lake at dusk", &scheme, &picture, None, None).is_err());
        assert!(write_theme(&user, "???", &scheme, &picture, None, None).is_err());
        assert!(write_theme(&user, "Gone", &scheme, &root.join("missing.jpg"), None, None).is_err());
        // Without an icon set that is installed, none is named and Omarchy keeps its own.
        let plain = write_theme(&user, "Plain", &scheme, &picture, None, None).unwrap();
        assert!(!plain.join("icons.theme").exists() && !plain.join("preview.png").exists());
        assert!(!user.join("gone").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
