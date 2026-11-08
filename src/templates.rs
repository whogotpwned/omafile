//! "New from Template": entries of `~/Templates` (the XDG templates folder,
//! files and whole folders) plus built-in project scaffolds.

use gtk::glib;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum Template {
    /// A file or folder under the templates directory, copied as is.
    User(PathBuf),
    /// Generated on the fly: (id, label, default name).
    Builtin(&'static str, &'static str, &'static str),
}

pub const BUILTINS: &[(&str, &str, &str)] = &[
    ("rust", "Rust crate (Cargo.toml, src/main.rs)", "my-crate"),
    ("python", "Python project (pyproject.toml, main.py, .gitignore)", "my-app"),
    ("omarchy-plugin", "Omarchy shell plugin (manifest.json, BarWidget.qml)", "my-plugin"),
    ("hyprland", "Hyprland config snippet (bindings, windowrules)", "hyprland-snippet.conf"),
    ("web", "Static web page (index.html, style.css)", "site"),
];

pub fn user_dir() -> PathBuf {
    glib::user_special_dir(glib::UserDirectory::Templates).unwrap_or_else(|| glib::home_dir().join("Templates"))
}

pub fn list() -> Vec<Template> {
    let mut v: Vec<Template> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(user_dir()) {
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| !p.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(true)).collect();
        paths.sort();
        v.extend(paths.into_iter().map(Template::User));
    }
    v.extend(BUILTINS.iter().map(|(id, label, name)| Template::Builtin(id, label, name)));
    v
}

pub fn label(t: &Template) -> String {
    match t {
        Template::User(p) => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        Template::Builtin(_, label, _) => label.to_string(),
    }
}

pub fn default_name(t: &Template) -> String {
    match t {
        Template::User(p) => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        Template::Builtin(_, _, name) => name.to_string(),
    }
}

/// Files of a built-in scaffold: (relative path, content).
pub fn builtin_files(id: &str, name: &str) -> Vec<(String, String)> {
    let ident = name.replace([' ', '.'], "-").to_lowercase();
    match id {
        "rust" => vec![
            ("Cargo.toml".into(), format!("[package]\nname = \"{ident}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n")),
            ("src/main.rs".into(), "fn main() {\n    println!(\"Hello from {}!\", env!(\"CARGO_PKG_NAME\"));\n}\n".into()),
            (".gitignore".into(), "/target\n".into()),
        ],
        "python" => vec![
            ("pyproject.toml".into(), format!("[project]\nname = \"{ident}\"\nversion = \"0.1.0\"\nrequires-python = \">=3.11\"\ndependencies = []\n")),
            ("main.py".into(), "def main() -> None:\n    print(\"hello\")\n\n\nif __name__ == \"__main__\":\n    main()\n".into()),
            (".gitignore".into(), "__pycache__/\n.venv/\n*.egg-info/\n".into()),
        ],
        "omarchy-plugin" => vec![
            ("manifest.json".into(), format!("{{\n  \"schemaVersion\": 1,\n  \"id\": \"you.{ident}\",\n  \"name\": \"{name}\",\n  \"version\": \"0.1.0\",\n  \"description\": \"What the widget does\",\n  \"kinds\": [\"bar-widget\"],\n  \"entryPoints\": {{ \"barWidget\": \"BarWidget.qml\" }},\n  \"barWidget\": {{ \"defaultSection\": \"right\" }}\n}}\n")),
            ("BarWidget.qml".into(), format!("import QtQuick\nimport Quickshell\nimport qs.Commons\nimport qs.Ui\n\nBarWidget {{\n  id: root\n  moduleName: \"you.{ident}\"\n  implicitWidth: button.implicitWidth\n  implicitHeight: button.implicitHeight\n\n  BarIconButton {{\n    id: button\n    anchors.fill: parent\n    bar: root.bar\n    text: \"\\uf0eb\"\n    onPressed: function(mouseButton) {{\n      if (root.bar) root.bar.run(\"notify-send '{name}'\")\n    }}\n  }}\n}}\n")),
            ("README.md".into(), format!("# {name}\n\nInstall: `omarchy plugin add <git url>`  ·  Validate: `omarchy plugin validate .`\n")),
        ],
        "hyprland" => vec![(
            String::new(),
            "# Drop into ~/.config/hypr/ and `source` it from hyprland.conf\n\n# bindd = SUPER SHIFT, F, File manager, exec, uwsm-app -- omafile\n\nwindowrule = float, class:^(io\\.github\\.whogotpwned\\.Omafile)$\nwindowrule = size 1100 700, class:^(io\\.github\\.whogotpwned\\.Omafile)$\n".into(),
        )],
        "web" => vec![
            ("index.html".into(), format!("<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>{name}</title>\n  <link rel=\"stylesheet\" href=\"style.css\">\n</head>\n<body>\n  <main><h1>{name}</h1></main>\n</body>\n</html>\n")),
            ("style.css".into(), ":root {{ color-scheme: light dark; font-family: system-ui, sans-serif; }}\nmain {{ max-width: 60ch; margin: 4rem auto; padding: 0 1rem; }}\n".replace("{{", "{").replace("}}", "}")),
        ],
        _ => Vec::new(),
    }
}

/// Create `template` as `dest` (a new file or folder). Returns what was created.
pub fn instantiate(t: &Template, dest: &Path) -> std::io::Result<()> {
    match t {
        Template::User(src) => copy_path(src, dest),
        Template::Builtin(id, _, _) => {
            let name = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let files = builtin_files(id, &name);
            if files.len() == 1 && files[0].0.is_empty() {
                return std::fs::write(dest, &files[0].1);
            }
            std::fs::create_dir(dest)?;
            for (rel, content) in files {
                let p = dest.join(rel);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(p, content)?;
            }
            Ok(())
        }
    }
}

fn copy_path(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_path(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_instantiate() {
        let dir = std::env::temp_dir().join(format!("omafile-tpl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (id, _, name) in BUILTINS {
            let dest = dir.join(name);
            instantiate(&Template::Builtin(id, "", name), &dest).unwrap();
            assert!(dest.exists(), "{id}");
        }
        assert!(dir.join("my-crate/src/main.rs").is_file());
        assert!(std::fs::read_to_string(dir.join("my-plugin/manifest.json")).unwrap().contains("\"schemaVersion\": 1"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
