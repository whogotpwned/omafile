//! Skins: how omafile itself looks.
//!
//! By default omafile wears the Omarchy theme that is on and changes with it. A skin is a
//! look of its own for omafile alone: chosen here, it changes nothing about the desktop,
//! the terminal or any other program, and Omarchy's theme stays what it is.
//!
//! A skin is a folder with a `colors.toml`, the format of an Omarchy theme, so every
//! theme Omarchy ships or the user has installed can be worn as a skin without being
//! turned on. Skins of omafile's own (made from a picture) live in
//! `~/.config/omafile/skins`. The choice is the preference `skin`: the folder's name, or
//! nothing to follow Omarchy.

use crate::omarchy::{self, ThemeInfo};
use std::path::{Path, PathBuf};

/// Where a skin comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Origin {
    /// One of omafile's own, in `~/.config/omafile/skins`: it can be deleted here.
    Own,
    /// An Omarchy theme, shipped or installed by the user: only read, never changed.
    Omarchy,
}

#[derive(Clone, Debug)]
pub struct Skin {
    pub info: ThemeInfo,
    pub origin: Origin,
}

/// omafile's own skins.
pub fn dir() -> PathBuf {
    gtk::glib::user_config_dir().join("omafile/skins")
}

/// Every skin there is: omafile's own first, then Omarchy's themes, each group by name.
/// One of omafile's own hides an Omarchy theme of the same name.
pub fn list_in(own: &Path, omarchy_user: &Path, omarchy_stock: &Path, backgrounds: &Path) -> Vec<Skin> {
    let nowhere = Path::new("/nonexistent/omafile");
    let mut out: Vec<Skin> = omarchy::list_themes_in(own, nowhere, nowhere).into_iter().map(|info| Skin { info, origin: Origin::Own }).collect();
    for info in omarchy::list_themes_in(omarchy_user, omarchy_stock, backgrounds) {
        if !out.iter().any(|s| s.info.name == info.name) {
            out.push(Skin { info, origin: Origin::Omarchy });
        }
    }
    out
}

pub fn list() -> Vec<Skin> {
    list_in(&dir(), &omarchy::install_dir(), &omarchy::stock_themes_dir(), &gtk::glib::home_dir().join(".config/omarchy/backgrounds"))
}

/// The folder of the skin with this name: the first of `places` that has it.
pub fn resolve_in(name: &str, places: &[PathBuf]) -> Option<PathBuf> {
    let name = name.trim();
    // A name, not a path: nothing outside the folders skins live in.
    if name.is_empty() || name.starts_with('.') || name.contains('/') {
        return None;
    }
    places.iter().map(|place| place.join(name)).find(|folder| omarchy::is_theme_dir(folder))
}

pub fn resolve(name: &str) -> Option<PathBuf> {
    resolve_in(name, &[dir(), omarchy::install_dir(), omarchy::stock_themes_dir()])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skin(root: &Path, name: &str, background: &str) {
        let folder = root.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("colors.toml"), format!("background = \"{background}\"\nforeground = \"#ffffff\"\naccent = \"#7aa2f7\"\n")).unwrap();
    }

    #[test]
    fn own_skins_and_omarchy_themes_are_all_skins() {
        let root = std::env::temp_dir().join(format!("omafile-skins-{}", std::process::id()));
        let (own, user, stock) = (root.join("own"), root.join("user"), root.join("stock"));
        skin(&own, "seaside", "#102030");
        skin(&own, "nord", "#111111");
        skin(&user, "my-omarchy-theme", "#202020");
        skin(&stock, "nord", "#2e3440");
        skin(&stock, "tokyo-night", "#1a1b26");
        std::fs::create_dir_all(stock.join("not-a-theme")).unwrap();
        let all = list_in(&own, &user, &stock, &root.join("backgrounds"));
        let names: Vec<(&str, Origin)> = all.iter().map(|s| (s.info.name.as_str(), s.origin)).collect();
        assert_eq!(names, vec![("nord", Origin::Own), ("seaside", Origin::Own), ("my-omarchy-theme", Origin::Omarchy), ("tokyo-night", Origin::Omarchy)], "own first; an own skin hides Omarchy's of the same name");
        assert_eq!(all[0].info.swatches[0], crate::theme::Rgb(0x11, 0x11, 0x11), "and it is the own one that is read");

        let places = [own.clone(), user.clone(), stock.clone()];
        assert_eq!(resolve_in("nord", &places), Some(own.join("nord")));
        assert_eq!(resolve_in("tokyo-night", &places), Some(stock.join("tokyo-night")));
        assert_eq!(resolve_in(" my-omarchy-theme\n", &places), Some(user.join("my-omarchy-theme")));
        assert_eq!(resolve_in("gone", &places), None, "a skin that was removed: omafile follows Omarchy again");
        assert_eq!(resolve_in("not-a-theme", &places), None);
        for bad in ["", "..", "../stock/nord", "/etc", ".hidden"] {
            assert_eq!(resolve_in(bad, &places), None, "{bad:?} is not a name");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
