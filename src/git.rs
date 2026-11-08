//! Git awareness: branch and per-file status for the folder being shown.

use gtk::{gio, glib};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Modified,
    Added,
    Untracked,
    Deleted,
    Renamed,
    Conflict,
    Ignored,
}

impl Status {
    pub fn badge(&self) -> &'static str {
        match self {
            Status::Modified => "M",
            Status::Added => "A",
            Status::Untracked => "?",
            Status::Deleted => "D",
            Status::Renamed => "R",
            Status::Conflict => "!",
            Status::Ignored => "·",
        }
    }

    pub fn css(&self) -> &'static str {
        match self {
            Status::Modified | Status::Renamed => "omafile-git-modified",
            Status::Added | Status::Untracked => "omafile-git-added",
            Status::Deleted | Status::Conflict => "omafile-git-deleted",
            Status::Ignored => "omafile-git-ignored",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Status::Modified => "modified",
            Status::Added => "added",
            Status::Untracked => "untracked",
            Status::Deleted => "deleted",
            Status::Renamed => "renamed",
            Status::Conflict => "conflict",
            Status::Ignored => "ignored",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RepoStatus {
    pub root: PathBuf,
    pub branch: String,
    /// Absolute path → status. Directories get the "strongest" status of their contents.
    pub files: HashMap<PathBuf, Status>,
    pub modified: usize,
    pub untracked: usize,
    pub staged: usize,
    /// Files with something in the index (for the badge click: unstage).
    pub staged_paths: std::collections::HashSet<PathBuf>,
}

async fn run_any_exit(root: &Path, args: &[&str]) -> Option<String> {
    let mut argv: Vec<&std::ffi::OsStr> = vec![std::ffi::OsStr::new("git"), std::ffi::OsStr::new("-C"), root.as_os_str()];
    argv.extend(args.iter().map(std::ffi::OsStr::new));
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE);
    let process = launcher.spawn(&argv).ok()?;
    let (stdout, _) = process.communicate_future(None).await.ok()?;
    stdout.map(|b| String::from_utf8_lossy(&b).to_string())
}

async fn run(root: &Path, args: &[&str]) -> Option<String> {
    let mut argv: Vec<&std::ffi::OsStr> = vec![std::ffi::OsStr::new("git"), std::ffi::OsStr::new("-C"), root.as_os_str()];
    argv.extend(args.iter().map(std::ffi::OsStr::new));
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE);
    launcher.setenv("GIT_OPTIONAL_LOCKS", "0", true);
    let process = launcher.spawn(&argv).ok()?;
    // Bytes, not UTF-8 strings: `-z` output is NUL separated and a GString stops at the first NUL.
    let (stdout, _) = process.communicate_future(None).await.ok()?;
    if !process.is_successful() {
        return None;
    }
    stdout.map(|b| String::from_utf8_lossy(&b).to_string())
}

/// Run git and return stdout, or the trimmed stderr as the error message.
pub async fn run_checked(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut argv: Vec<&std::ffi::OsStr> = vec![std::ffi::OsStr::new("git"), std::ffi::OsStr::new("-C"), root.as_os_str()];
    argv.extend(args.iter().map(std::ffi::OsStr::new));
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_PIPE);
    launcher.setenv("GIT_OPTIONAL_LOCKS", "0", true);
    launcher.setenv("GIT_TERMINAL_PROMPT", "0", true);
    let process = launcher.spawn(&argv).map_err(|e| e.message().to_string())?;
    let (stdout, stderr) = process.communicate_future(None).await.map_err(|e| e.message().to_string())?;
    let text = |b: Option<glib::Bytes>| b.map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
    if process.is_successful() {
        Ok(text(stdout))
    } else {
        let err = text(stderr);
        Err(summarize_error(&err, args.first().unwrap_or(&"")))
    }
}

/// Git errors are multi-line essays; keep what a toast can show.
pub fn summarize_error(stderr: &str, verb: &str) -> String {
    let text = stderr.trim();
    if text.contains("Author identity unknown") || text.contains("Please tell me who you are") {
        return "set your identity first: git config --global user.name / user.email".into();
    }
    let line = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('*') && !l.starts_with("hint:"));
    match line {
        Some(l) => l.trim_start_matches("error: ").trim_start_matches("fatal: ").to_string(),
        None => format!("git {verb} failed"),
    }
}

fn with_paths<'a>(base: &[&'a str], paths: &'a [PathBuf]) -> Vec<&'a str> {
    let mut v: Vec<&str> = base.to_vec();
    v.push("--");
    v.extend(paths.iter().filter_map(|p| p.to_str()));
    v
}

pub async fn stage(root: &Path, paths: &[PathBuf]) -> Result<(), String> {
    run_checked(root, &with_paths(&["add", "-A"], paths)).await.map(|_| ())
}

pub async fn unstage(root: &Path, paths: &[PathBuf]) -> Result<(), String> {
    run_checked(root, &with_paths(&["reset", "-q", "HEAD"], paths)).await.map(|_| ())
}

/// Throw away local changes: tracked files go back to HEAD, untracked ones are removed.
pub async fn discard(root: &Path, paths: &[PathBuf]) -> Result<(), String> {
    // `checkout` fails for paths unknown to the index; try both and only report
    // when neither could do anything.
    let a = run_checked(root, &with_paths(&["checkout", "-q", "HEAD"], paths)).await;
    let b = run_checked(root, &with_paths(&["clean", "-fdq"], paths)).await;
    match (a, b) {
        (Err(ea), Err(_)) => Err(ea),
        _ => Ok(()),
    }
}

pub async fn commit(root: &Path, message: &str, stage_all: bool) -> Result<String, String> {
    if stage_all {
        run_checked(root, &["add", "-A"]).await?;
    }
    run_checked(root, &["commit", "-q", "-m", message]).await?;
    run_checked(root, &["log", "-1", "--format=%h %s"]).await.map(|s| s.trim().to_string())
}

/// Local branches, current one first; (name, is_current).
pub async fn branches(root: &Path) -> Vec<(String, bool)> {
    let out = run_checked(root, &["branch", "--format=%(HEAD) %(refname:short)", "--sort=-committerdate"]).await.unwrap_or_default();
    out.lines()
        .filter_map(|l| {
            let (head, name) = l.split_at(1);
            let name = name.trim();
            if name.is_empty() { None } else { Some((name.to_string(), head == "*")) }
        })
        .collect()
}

pub async fn switch(root: &Path, branch: &str, create: bool) -> Result<(), String> {
    let args: Vec<&str> = if create { vec!["switch", "-q", "-c", branch] } else { vec!["switch", "-q", branch] };
    run_checked(root, &args).await.map(|_| ())
}

/// Unified diff for one path: work tree vs. index, plus the index vs. HEAD, and
/// untracked files against nothing.
pub async fn diff(root: &Path, path: &Path, status: &Status) -> String {
    let Some(p) = path.to_str() else { return String::new() };
    let rel = path.strip_prefix(root).ok().and_then(|r| r.to_str()).unwrap_or(p);
    if matches!(status, Status::Untracked | Status::Added) {
        let tracked = run_checked(root, &["ls-files", "--error-unmatch", "--", rel]).await.is_ok();
        if !tracked {
            // Exit status 1 means "differences", so do not use run_checked here.
            return run_any_exit(root, &["diff", "--no-index", "--", "/dev/null", p]).await.unwrap_or_default();
        }
    }
    let mut out = run(root, &["diff", "--", rel]).await.unwrap_or_default();
    let staged = run(root, &["diff", "--cached", "--", rel]).await.unwrap_or_default();
    if !staged.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("# staged\n");
        out.push_str(&staged);
    }
    out
}

pub fn available() -> bool {
    glib::find_program_in_path("git").is_some()
}

/// Status for the repository containing `dir`, or None when it is not in one.
pub async fn status(dir: &Path) -> Option<RepoStatus> {
    if !available() {
        return None;
    }
    let root = run(dir, &["rev-parse", "--show-toplevel"]).await?;
    let root = PathBuf::from(root.trim());
    if root.as_os_str().is_empty() {
        return None;
    }
    let branch = run(&root, &["branch", "--show-current"]).await.map(|b| b.trim().to_string()).unwrap_or_default();
    let branch = if branch.is_empty() {
        run(&root, &["rev-parse", "--short", "HEAD"]).await.map(|h| format!("detached {}", h.trim())).unwrap_or_else(|| "HEAD".into())
    } else {
        branch
    };
    let out = run(&root, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--ignored=matching"]).await?;
    let mut repo = RepoStatus { root: root.clone(), branch, ..Default::default() };
    let mut entries = out.split('\0').filter(|e| !e.is_empty()).peekable();
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(3);
        let (x, y) = (code.as_bytes()[0] as char, code.as_bytes()[1] as char);
        if x == 'R' || x == 'C' {
            // Renames carry the old path in the next record.
            entries.next();
        }
        let status = match (x, y) {
            ('?', '?') => Status::Untracked,
            ('!', '!') => Status::Ignored,
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => Status::Conflict,
            ('R', _) => Status::Renamed,
            ('A', _) => Status::Added,
            ('D', _) | (_, 'D') => Status::Deleted,
            _ => Status::Modified,
        };
        match status {
            Status::Untracked => repo.untracked += 1,
            Status::Ignored => {}
            _ => {
                repo.modified += 1;
                if x != ' ' && x != '?' {
                    repo.staged += 1;
                }
            }
        }
        let abs = root.join(path.trim_end_matches('/'));
        if x != ' ' && x != '?' && x != '!' {
            repo.staged_paths.insert(abs.clone());
        }
        // Propagate to parent folders so a folder shows the state of its contents.
        if status != Status::Ignored {
            let mut parent = abs.parent();
            while let Some(p) = parent {
                if !p.starts_with(&root) || p == root {
                    break;
                }
                let current = repo.files.get(p).cloned();
                let next = match (current, &status) {
                    (Some(Status::Conflict), _) => Status::Conflict,
                    (_, Status::Conflict) => Status::Conflict,
                    (Some(Status::Modified), _) | (_, Status::Modified) | (_, Status::Deleted) | (_, Status::Renamed) => Status::Modified,
                    (Some(Status::Added), _) | (_, Status::Added) => Status::Added,
                    _ => Status::Untracked,
                };
                repo.files.insert(p.to_path_buf(), next);
                parent = p.parent();
            }
        }
        repo.files.insert(abs, status);
    }
    Some(repo)
}

impl RepoStatus {
    pub fn summary(&self) -> String {
        let repo = self.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut parts = vec![format!("{repo} on {}", self.branch)];
        if self.modified > 0 {
            parts.push(format!("{} modified", self.modified));
        }
        if self.staged > 0 {
            parts.push(format!("{} staged", self.staged));
        }
        if self.untracked > 0 {
            parts.push(format!("{} untracked", self.untracked));
        }
        if self.modified == 0 && self.untracked == 0 {
            parts.push("clean".into());
        }
        parts.join("  ·  ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_summary_is_one_line() {
        assert!(summarize_error("\n*** Please tell me who you are.\n\nRun\n\n  git config --global user.email", "commit").contains("identity"));
        assert_eq!(summarize_error("error: pathspec 'x' did not match any file(s) known to git\nhint: foo", "switch"), "pathspec 'x' did not match any file(s) known to git");
        assert_eq!(summarize_error("", "add"), "git add failed");
    }

    #[test]
    fn parses_status_of_a_real_repo() {
        if !available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("omafile-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git").arg("-C").arg(&dir).args(args).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@example.org").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@example.org").output().unwrap()
        };
        git(&["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "1").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "init"]);
        std::fs::write(dir.join("a.txt"), "2").unwrap();
        std::fs::write(dir.join("sub/new.txt"), "x").unwrap();
        let ctx = glib::MainContext::new();
        let repo = ctx.block_on(status(&dir)).expect("repo status");
        assert_eq!(repo.files.get(&dir.join("a.txt")), Some(&Status::Modified));
        assert_eq!(repo.files.get(&dir.join("sub/new.txt")), Some(&Status::Untracked));
        assert_eq!(repo.files.get(&dir.join("sub")), Some(&Status::Untracked));
        assert_eq!(repo.modified, 1);
        assert_eq!(repo.untracked, 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
