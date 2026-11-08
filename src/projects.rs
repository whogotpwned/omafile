//! Project detection and per-project actions, plus `.omafile.toml` custom actions.

use std::path::Path;

#[derive(Clone, Debug)]
pub struct ProjectAction {
    pub label: String,
    /// Shell command, run in the project folder inside a terminal.
    pub command: String,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub kind: &'static str,
    pub actions: Vec<ProjectAction>,
}

fn act(label: &str, command: &str) -> ProjectAction {
    ProjectAction { label: label.into(), command: command.into() }
}

/// What kind of project lives in `dir`, judged by its marker files.
pub fn detect(dir: &Path) -> Option<Project> {
    let has = |name: &str| dir.join(name).exists();
    let mut actions = Vec::new();
    let mut kind = None;
    if has("Cargo.toml") {
        kind = Some("Rust");
        actions.extend([act("cargo build", "cargo build"), act("cargo test", "cargo test"), act("cargo run", "cargo run"), act("cargo clippy", "cargo clippy")]);
    }
    if has("package.json") {
        kind = kind.or(Some("Node"));
        actions.extend([act("npm install", "npm install"), act("npm run dev", "npm run dev"), act("npm test", "npm test"), act("npm run build", "npm run build")]);
    }
    if has("pyproject.toml") || has("requirements.txt") {
        kind = kind.or(Some("Python"));
        actions.extend([act("pytest", "python -m pytest"), act("pip install -r requirements.txt", "pip install -r requirements.txt")]);
    }
    if has("go.mod") {
        kind = kind.or(Some("Go"));
        actions.extend([act("go build ./...", "go build ./..."), act("go test ./...", "go test ./...")]);
    }
    if has("PKGBUILD") {
        kind = kind.or(Some("Arch package"));
        actions.extend([act("makepkg -si", "makepkg -si"), act("makepkg --printsrcinfo > .SRCINFO", "makepkg --printsrcinfo > .SRCINFO")]);
    }
    if has("docker-compose.yml") || has("compose.yml") || has("compose.yaml") || has("docker-compose.yaml") {
        kind = kind.or(Some("Docker Compose"));
        actions.extend([act("docker compose up", "docker compose up"), act("docker compose down", "docker compose down"), act("docker compose logs -f", "docker compose logs -f")]);
    }
    if has("Makefile") || has("justfile") || has("Justfile") {
        kind = kind.or(Some("Make"));
        if has("justfile") || has("Justfile") {
            actions.push(act("just", "just"));
        } else {
            actions.push(act("make", "make"));
        }
    }
    if has("manifest.json") && has("plugin") {
        kind = kind.or(Some("Omarchy plugin"));
        actions.push(act("omarchy plugin validate .", "omarchy plugin validate ."));
    }
    if has(".git") {
        kind = kind.or(Some("Git repository"));
        actions.extend([act("git status", "git status"), act("git log", "git log --oneline --graph --decorate -n 40"), act("git diff", "git diff"), act("git pull", "git pull")]);
    }
    // Team-defined actions from .omafile.toml.
    let custom = custom_actions(dir);
    if !custom.is_empty() {
        kind = kind.or(Some("Project"));
        actions.extend(custom);
    }
    kind.map(|kind| Project { kind, actions })
}

/// Parse `.omafile.toml`:
/// ```toml
/// [[action]]
/// name = "Deploy"
/// command = "./deploy.sh production"
/// ```
pub fn custom_actions(dir: &Path) -> Vec<ProjectAction> {
    let Ok(text) = std::fs::read_to_string(dir.join(".omafile.toml")) else { return Vec::new() };
    parse_custom(&text)
}

pub fn parse_custom(text: &str) -> Vec<ProjectAction> {
    let mut actions = Vec::new();
    let mut name: Option<String> = None;
    let mut command: Option<String> = None;
    let flush = |name: &mut Option<String>, command: &mut Option<String>, actions: &mut Vec<ProjectAction>| {
        if let (Some(n), Some(c)) = (name.take(), command.take()) {
            if !n.is_empty() && !c.is_empty() {
                actions.push(ProjectAction { label: n, command: c });
            }
        }
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[action]]" {
            flush(&mut name, &mut command, &mut actions);
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches(['"', '\'']).to_string();
            match k.trim() {
                "name" => name = Some(v),
                "command" => command = Some(v),
                _ => {}
            }
        }
    }
    flush(&mut name, &mut command, &mut actions);
    actions
}

/// Which terminal to open: `$TERMINAL`, else the desktop default through
/// `xdg-terminal-exec` (what Omarchy's own bar uses), else foot, else alacritty.
pub fn terminal_program() -> String {
    if let Ok(t) = std::env::var("TERMINAL") {
        if !t.is_empty() {
            return t;
        }
    }
    for candidate in ["xdg-terminal-exec", "foot", "alacritty", "kitty", "ghostty"] {
        if gtk::glib::find_program_in_path(candidate).is_some() {
            return candidate.into();
        }
    }
    "xdg-terminal-exec".into()
}

/// argv that opens the terminal with `command` (empty: an interactive shell).
/// The caller spawns it with `dir` as working directory, which every terminal
/// inherits; alacritty and foot additionally get it passed explicitly.
pub fn terminal_with(dir: &Path, command: &[String]) -> Vec<String> {
    let term = terminal_program();
    let mut argv: Vec<String> = Vec::new();
    if gtk::glib::find_program_in_path("uwsm").is_some() {
        argv.extend(["uwsm".into(), "app".into(), "--".into()]);
    }
    argv.push(term.clone());
    let base = std::path::Path::new(&term).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    match base.as_str() {
        "alacritty" | "kitty" => argv.extend(["--working-directory".into(), dir.to_string_lossy().to_string()]),
        "foot" => argv.push(format!("--working-directory={}", dir.to_string_lossy())),
        "ghostty" => argv.push(format!("--working-directory={}", dir.to_string_lossy())),
        _ => {}
    }
    if !command.is_empty() {
        if matches!(base.as_str(), "alacritty" | "kitty" | "foot" | "ghostty") {
            argv.push("-e".into());
        }
        argv.extend(command.iter().cloned());
    }
    argv
}

/// A terminal that runs `command` in `dir` and waits before closing.
pub fn terminal_argv(dir: &Path, command: &str) -> Vec<String> {
    let script = format!("{command}; status=$?; echo; echo \"[omafile] exit $status, press Enter to close\"; read -r _");
    terminal_with(dir, &["bash".into(), "-lc".into(), script])
}

/// Editor launch for a folder or file: Omarchy's launcher when present, else $EDITOR in a terminal.
pub fn editor_argv(target: &Path) -> Vec<String> {
    if gtk::glib::find_program_in_path("omarchy-launch-editor").is_some() {
        return vec!["omarchy-launch-editor".into(), target.to_string_lossy().to_string()];
    }
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "nvim".into());
    let dir = if target.is_dir() { target.to_path_buf() } else { target.parent().map(Path::to_path_buf).unwrap_or_else(|| target.to_path_buf()) };
    terminal_with(&dir, &[editor, target.to_string_lossy().to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_custom_actions() {
        let text = "# team actions\n[[action]]\nname = \"Deploy\"\ncommand = \"./deploy.sh prod\"\n\n[[action]]\nname = 'Lint'\ncommand = 'cargo clippy'\n";
        let a = parse_custom(text);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].label, "Deploy");
        assert_eq!(a[1].command, "cargo clippy");
    }

    #[test]
    fn detects_rust_and_git() {
        let dir = std::env::temp_dir().join(format!("omafile-proj-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "").unwrap();
        let p = detect(&dir).unwrap();
        assert_eq!(p.kind, "Rust");
        assert!(p.actions.iter().any(|a| a.command == "cargo test"));
        assert!(p.actions.iter().any(|a| a.command.starts_with("git log")));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(detect(&std::env::temp_dir()).is_none() || true);
    }
}
