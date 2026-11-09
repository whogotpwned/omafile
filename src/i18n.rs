//! Translations, following Omarchy's language choice. The language comes
//! from `OMAFILE_LANG`, else `~/.config/omarchy/language` (what *Setup >
//! Language* writes), else `LC_ALL` / `LC_MESSAGES` / `LANG`. Catalogs are
//! flat JSON files keyed by the English string, compiled in from
//! `data/i18n/<code>.json`; `~/.config/omafile/i18n/<code>.json` overrides
//! or adds. A regional code (pt-BR) falls back to its base (pt), then to
//! English, then to the key itself, exactly like Omarchy's own catalogs.

use std::collections::HashMap;
use std::sync::OnceLock;

/// (code, catalog) compiled into the binary.
const BUILTIN: &[(&str, &str)] = &[
    ("de", include_str!("../data/i18n/de.json")),
    ("es", include_str!("../data/i18n/es.json")),
    ("fr", include_str!("../data/i18n/fr.json")),
    ("it", include_str!("../data/i18n/it.json")),
    ("pt", include_str!("../data/i18n/pt.json")),
    ("nl", include_str!("../data/i18n/nl.json")),
];

struct Catalog {
    code: String,
    strings: HashMap<String, String>,
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// The language code omafile runs in ("en" when nothing else applies).
pub fn language() -> String {
    CATALOG.get_or_init(load).code.clone()
}

/// Hand the language to gettext before GTK starts, so GTK's and GIO's own
/// strings (file chooser, content-type descriptions, "Cancel") follow the
/// Omarchy setting even when the shell's LANG is still something else.
/// `LANGUAGE` wins over `LANG` in gettext but is ignored under the C locale.
pub fn export_language() {
    if std::env::var_os("LANGUAGE").is_some() {
        return;
    }
    let code = detect();
    if code == "en" {
        return;
    }
    let (base, region) = match code.split_once('-') {
        Some((b, r)) => (b.to_string(), Some(r.to_uppercase())),
        None => (code.clone(), None),
    };
    let value = match region {
        Some(r) => format!("{base}_{r}:{base}"),
        None => base,
    };
    // SAFETY: called first thing in main, before any other thread exists.
    unsafe { std::env::set_var("LANGUAGE", value) };
}

/// Detect the language code the way Omarchy does.
pub fn detect() -> String {
    if let Some(v) = std::env::var_os("OMAFILE_LANG").map(|v| v.to_string_lossy().trim().to_string()).filter(|v| !v.is_empty()) {
        return v;
    }
    let config = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).unwrap_or_else(|| gtk::glib::home_dir().join(".config"));
    if let Ok(text) = std::fs::read_to_string(config.join("omarchy/language")) {
        let code = text.lines().next().unwrap_or("").trim();
        if !code.is_empty() {
            return code.to_string();
        }
    }
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(v) = std::env::var_os(var).map(|v| v.to_string_lossy().to_string()) {
            if let Some(code) = code_from_locale(&v) {
                return code;
            }
        }
    }
    "en".into()
}

/// `de_DE.UTF-8` → `de`, `pt_BR.UTF-8` → `pt-BR`, `C` → None.
pub fn code_from_locale(locale: &str) -> Option<String> {
    let base = locale.split(['.', '@']).next()?.trim();
    if base.is_empty() || base == "C" || base == "POSIX" {
        return None;
    }
    let mut parts = base.split('_');
    let lang = parts.next()?.to_lowercase();
    match parts.next() {
        Some(region) if lang == "pt" || lang == "zh" || lang == "es" => Some(format!("{lang}-{}", region.to_uppercase())),
        _ => Some(lang),
    }
}

/// Minimal parser for the flat `{"key": "value", …}` catalogs (no nesting needed).
pub fn parse_catalog(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut chars = text.chars().peekable();
    let mut key: Option<String> = None;
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut s = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some('u') => {
                        let hex: String = chars.by_ref().take(4).collect();
                        if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            s.push(ch);
                        }
                    }
                    Some(o) => s.push(o),
                    None => {}
                },
                '"' => break,
                c => s.push(c),
            }
        }
        match key.take() {
            None => key = Some(s),
            Some(k) => {
                map.insert(k, s);
            }
        }
    }
    map
}

fn load() -> Catalog {
    let code = detect();
    let mut strings = HashMap::new();
    let base = code.split('-').next().unwrap_or("en").to_string();
    for layer in [base.clone(), code.clone()] {
        if layer == "en" {
            continue;
        }
        if let Some((_, text)) = BUILTIN.iter().find(|(c, _)| *c == layer) {
            strings.extend(parse_catalog(text));
        }
        let config = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).unwrap_or_else(|| gtk::glib::home_dir().join(".config"));
        if let Ok(text) = std::fs::read_to_string(config.join(format!("omafile/i18n/{layer}.json"))) {
            strings.extend(parse_catalog(&text));
        }
    }
    Catalog { code, strings }
}

/// Translate an English UI string; unknown strings come back unchanged.
pub fn tr(english: &str) -> String {
    match CATALOG.get_or_init(load).strings.get(english) {
        Some(t) if !t.is_empty() => t.clone(),
        _ => english.to_string(),
    }
}

/// `trf!("Copied {n} items", n = 3)`: translate, then fill `{name}` holes.
#[macro_export]
macro_rules! trf {
    ($s:expr $(, $k:ident = $v:expr)* $(,)?) => {{
        #[allow(unused_mut)]
        let mut t = $crate::i18n::tr($s);
        $( t = t.replace(concat!("{", stringify!($k), "}"), &($v).to_string()); )*
        t
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_codes() {
        assert_eq!(code_from_locale("de_DE.UTF-8").as_deref(), Some("de"));
        assert_eq!(code_from_locale("pt_BR.UTF-8").as_deref(), Some("pt-BR"));
        assert_eq!(code_from_locale("C.UTF-8"), None);
        assert_eq!(code_from_locale("fr").as_deref(), Some("fr"));
    }

    #[test]
    fn parses_flat_catalog() {
        let m = parse_catalog("{\n  \"Open\": \"Öffnen\",\n  \"Say \\\"hi\\\"\": \"Sag \\\"hallo\\\"\\n\"\n}");
        assert_eq!(m.get("Open").map(String::as_str), Some("Öffnen"));
        assert_eq!(m.get("Say \"hi\"").map(String::as_str), Some("Sag \"hallo\"\n"));
    }

    #[test]
    fn every_builtin_catalog_parses_and_matches_english_keys() {
        let en = parse_catalog(include_str!("../data/i18n/en.json"));
        assert!(!en.is_empty());
        for (code, text) in BUILTIN {
            let m = parse_catalog(text);
            assert!(!m.is_empty(), "{code} is empty");
            for k in m.keys() {
                assert!(en.contains_key(k), "{code}: key not in en.json: {k:?}");
            }
        }
    }
}
