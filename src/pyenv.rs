//! Python virtual environments: find them, read `pyvenv.cfg` and the
//! installed packages without running Python, match requirements against
//! them, and build the commands (uv first, then python -m venv / pip).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Venv {
    pub root: PathBuf,
    pub version: String,
    pub base: String,
    pub prompt: String,
    /// "uv", "virtualenv", "poetry" or "venv".
    pub tool: String,
    /// Normalised name → version.
    pub packages: BTreeMap<String, String>,
}

pub const VENV_NAMES: &[&str] = &[".venv", "venv", "env", ".env-py"];

pub fn is_venv_dir(dir: &Path) -> bool {
    dir.join("pyvenv.cfg").is_file()
}

/// PEP 503 normalisation: `Foo_Bar.baz` → `foo-bar-baz`.
pub fn normalize(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.trim().chars() {
        if c == '-' || c == '_' || c == '.' {
            if !dash {
                out.push('-');
                dash = true;
            }
        } else {
            out.push(c.to_ascii_lowercase());
            dash = false;
        }
    }
    out
}

pub fn parse_pyvenv_cfg(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_lowercase(), v.trim().to_string()))
        })
        .collect()
}

/// Installed distributions from `lib/python*/site-packages/*.dist-info`.
pub fn packages_in(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(libs) = std::fs::read_dir(root.join("lib")) else { return out };
    for lib in libs.flatten() {
        let site = lib.path().join("site-packages");
        let Ok(entries) = std::fs::read_dir(&site) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".dist-info") else { continue };
            let (pkg, version) = match stem.rsplit_once('-') {
                Some((p, v)) => (p.to_string(), v.to_string()),
                None => (stem.to_string(), String::new()),
            };
            out.insert(normalize(&pkg), version);
        }
    }
    out
}

pub fn read(root: &Path) -> Option<Venv> {
    let cfg = parse_pyvenv_cfg(&std::fs::read_to_string(root.join("pyvenv.cfg")).ok()?);
    let version = cfg.get("version_info").or_else(|| cfg.get("version")).cloned().unwrap_or_default();
    let tool = if cfg.contains_key("uv") {
        "uv"
    } else if cfg.contains_key("virtualenv") {
        "virtualenv"
    } else if cfg.get("prompt").map(|p| p.contains("poetry") || p.contains("py3")).unwrap_or(false) && root.to_string_lossy().contains("pypoetry") {
        "poetry"
    } else {
        "venv"
    };
    Some(Venv {
        root: root.to_path_buf(),
        version: version.split('.').take(3).collect::<Vec<_>>().join("."),
        base: cfg.get("home").cloned().unwrap_or_default(),
        prompt: cfg.get("prompt").cloned().unwrap_or_default(),
        tool: tool.into(),
        packages: packages_in(root),
    })
}

/// The venv for `dir`: the folder itself, a child named like a venv, or one
/// of those in the parents up to the project root (pyproject.toml / .git).
pub fn find(dir: &Path) -> Option<Venv> {
    let mut cur = Some(dir);
    let mut hops = 0;
    while let Some(d) = cur {
        if is_venv_dir(d) {
            return read(d);
        }
        for name in VENV_NAMES {
            let p = d.join(name);
            if is_venv_dir(&p) {
                return read(&p);
            }
        }
        if d.join(".git").exists() || d.join("pyproject.toml").exists() || hops >= 4 {
            break;
        }
        hops += 1;
        cur = d.parent();
    }
    None
}

/// The folder that counts as the Python project for `dir` (where pyproject / requirements live).
pub fn project_root(dir: &Path) -> PathBuf {
    let mut cur = dir;
    for _ in 0..5 {
        if cur.join("pyproject.toml").exists() || cur.join("requirements.txt").exists() || cur.join("setup.py").exists() {
            return cur.to_path_buf();
        }
        match cur.parent() {
            Some(p) if !cur.join(".git").exists() => cur = p,
            _ => break,
        }
    }
    dir.to_path_buf()
}

pub fn is_python_project(dir: &Path) -> bool {
    ["pyproject.toml", "requirements.txt", "setup.py", "Pipfile"].iter().any(|f| dir.join(f).exists()) || find(dir).is_some()
}

/// Requirement names from a requirements.txt (comments, options and markers stripped).
pub fn requirements_txt(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty() && !l.starts_with('-') && !l.starts_with("http") && !l.starts_with("git+") && !l.starts_with('.'))
        .filter_map(requirement_name)
        .collect()
}

/// `requests[security]>=2.31 ; python_version>"3.8"` → `requests`
pub fn requirement_name(spec: &str) -> Option<String> {
    let end = spec.find(|c: char| "[<>=!~; @".contains(c)).unwrap_or(spec.len());
    let name = &spec[..end];
    if name.is_empty() || !name.chars().next().unwrap().is_ascii_alphanumeric() {
        None
    } else {
        Some(normalize(name))
    }
}

/// `dependencies = [...]` from `[project]` in pyproject.toml, plus poetry's `[tool.poetry.dependencies]`.
pub fn pyproject_deps(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut section = String::new();
    let mut in_list = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(['[', ']']).to_string();
            in_list = false;
            continue;
        }
        if section == "project" {
            if line.starts_with("dependencies") && line.contains('[') {
                in_list = !line.contains(']');
                out.extend(quoted(line).into_iter().filter_map(|s| requirement_name(&s)));
                continue;
            }
            if in_list {
                if line.starts_with(']') {
                    in_list = false;
                } else {
                    out.extend(quoted(line).into_iter().filter_map(|s| requirement_name(&s)));
                }
            }
        } else if section == "tool.poetry.dependencies" {
            if let Some((k, _)) = line.split_once('=') {
                let k = k.trim().trim_matches('"');
                if !k.is_empty() && k != "python" {
                    out.push(normalize(k));
                }
            }
        }
    }
    out
}

fn quoted(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find(['"', '\'']) {
        let q = rest.as_bytes()[start] as char;
        let after = &rest[start + 1..];
        let Some(end) = after.find(q) else { break };
        out.push(after[..end].to_string());
        rest = &after[end + 1..];
    }
    out
}

/// (name, installed version or None) for every requirement.
pub fn check(requirements: &[String], venv: Option<&Venv>) -> Vec<(String, Option<String>)> {
    requirements.iter().map(|r| (r.clone(), venv.and_then(|v| v.packages.get(r).cloned()))).collect()
}

pub fn uv_available() -> bool {
    gtk::glib::find_program_in_path("uv").is_some()
}

/// Shell command that creates `.venv` in the current folder.
pub fn create_command() -> String {
    if uv_available() { "uv venv .venv".into() } else { "python3 -m venv .venv".into() }
}

pub fn install_command(venv: &Venv, root: &Path) -> String {
    let py = venv.root.join("bin/python").to_string_lossy().to_string();
    let has_req = root.join("requirements.txt").exists();
    let has_pyproject = root.join("pyproject.toml").exists();
    if uv_available() {
        if has_req {
            format!("uv pip install --python '{py}' -r requirements.txt")
        } else if has_pyproject {
            "uv sync".into()
        } else {
            format!("uv pip install --python '{py}'")
        }
    } else if has_req {
        format!("'{py}' -m pip install -r requirements.txt")
    } else {
        format!("'{py}' -m pip install -e .")
    }
}

pub fn freeze_command(venv: &Venv) -> String {
    let py = venv.root.join("bin/python").to_string_lossy().to_string();
    if uv_available() { format!("uv pip freeze --python '{py}' > requirements.txt") } else { format!("'{py}' -m pip freeze > requirements.txt") }
}

pub fn activate_line(venv: &Venv, shell: &str) -> String {
    let dir = venv.root.join("bin");
    let script = if shell.ends_with("fish") {
        dir.join("activate.fish")
    } else if shell.ends_with("csh") || shell.ends_with("tcsh") {
        dir.join("activate.csh")
    } else {
        dir.join("activate")
    };
    format!(" source '{}'", script.to_string_lossy().replace('\'', "'\\''"))
}

/// Short line for the status bar: `.venv · Python 3.12.4 · 42 packages (uv)`.
pub fn summary(v: &Venv) -> String {
    let name = v.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let n = v.packages.len();
    let mut s = format!("{name} · Python {}", if v.version.is_empty() { "?".into() } else { v.version.clone() });
    s.push_str(&format!(" · {n} {}", if n == 1 { "package" } else { "packages" }));
    if v.tool != "venv" {
        s.push_str(&format!(" ({})", v.tool));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requirements_and_pyproject() {
        let req = requirements_txt("# web\nrequests[security]>=2.31 ; python_version>'3.8'\nFlask==3.0\n-r dev.txt\n--extra-index-url https://x\ngit+https://github.com/a/b\nnumpy\n");
        assert_eq!(req, vec!["requests", "flask", "numpy"]);
        let py = pyproject_deps("[project]\nname = \"x\"\ndependencies = [\n  \"httpx>=0.27\",\n  'rich',\n]\n\n[tool.poetry.dependencies]\npython = \"^3.12\"\ntyper = \"*\"\n");
        assert_eq!(py, vec!["httpx", "rich", "typer"]);
        assert_eq!(normalize("Zope.Interface__x"), "zope-interface-x");
    }

    #[test]
    fn reads_a_fake_venv() {
        let dir = std::env::temp_dir().join(format!("omafile-venv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let site = dir.join("lib/python3.12/site-packages");
        std::fs::create_dir_all(site.join("Requests-2.32.3.dist-info")).unwrap();
        std::fs::create_dir_all(site.join("rich-13.7.1.dist-info")).unwrap();
        std::fs::write(dir.join("pyvenv.cfg"), "home = /usr/bin\nuv = 0.4.0\nversion_info = 3.12.4\nprompt = demo\n").unwrap();
        let v = read(&dir).unwrap();
        assert_eq!(v.version, "3.12.4");
        assert_eq!(v.tool, "uv");
        assert_eq!(v.packages.get("requests").map(String::as_str), Some("2.32.3"));
        let checked = check(&["requests".into(), "numpy".into()], Some(&v));
        assert_eq!(checked[0].1.as_deref(), Some("2.32.3"));
        assert_eq!(checked[1].1, None);
        assert!(summary(&v).contains("2 packages (uv)"));
        // find() from a project folder that holds the venv as .venv
        let project = std::env::temp_dir().join(format!("omafile-pyenv-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&project);
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(project.join("pyproject.toml"), "[project]\nname='p'\n").unwrap();
        std::fs::rename(&dir, project.join(".venv")).unwrap();
        assert!(find(&project.join("src")).is_some());
        let _ = std::fs::remove_dir_all(&project);
    }
}
