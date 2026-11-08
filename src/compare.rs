//! Compare two files (unified diff, coloured) or two folders (only left,
//! only right, differ) with diffutils.

use gtk::{gio, glib};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, PartialEq)]
pub struct DirDiff {
    pub only_left: Vec<String>,
    pub only_right: Vec<String>,
    pub differ: Vec<String>,
}

pub fn available() -> bool {
    glib::find_program_in_path("diff").is_some()
}

async fn run_diff(args: &[&str]) -> Option<String> {
    let mut argv: Vec<&std::ffi::OsStr> = vec![std::ffi::OsStr::new("diff")];
    argv.extend(args.iter().map(std::ffi::OsStr::new));
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE);
    launcher.setenv("LC_ALL", "C", true);
    let process = launcher.spawn(&argv).ok()?;
    let (out, _) = process.communicate_future(None).await.ok()?;
    // Exit status 1 means "differences": still output we want.
    out.map(|b| String::from_utf8_lossy(&b).to_string())
}

pub async fn files(a: &Path, b: &Path) -> Option<String> {
    run_diff(&["-u", "--label", &a.to_string_lossy(), "--label", &b.to_string_lossy(), &a.to_string_lossy(), &b.to_string_lossy()]).await
}

pub async fn dirs(a: &Path, b: &Path) -> Option<DirDiff> {
    let out = run_diff(&["-rq", &a.to_string_lossy(), &b.to_string_lossy()]).await?;
    Some(parse_dirs(&out, a, b))
}

pub fn parse_dirs(out: &str, a: &Path, b: &Path) -> DirDiff {
    let mut d = DirDiff::default();
    let rel = |p: &str, base: &Path| -> String {
        Path::new(p).strip_prefix(base).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|_| p.to_string())
    };
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("Only in ") {
            if let Some((dir, name)) = rest.split_once(": ") {
                let full = PathBuf::from(dir).join(name);
                if full.starts_with(a) {
                    d.only_left.push(rel(&full.to_string_lossy(), a));
                } else {
                    d.only_right.push(rel(&full.to_string_lossy(), b));
                }
            }
        } else if let Some(rest) = line.strip_prefix("Files ") {
            if let Some((left, _)) = rest.split_once(" and ") {
                d.differ.push(rel(left, a));
            }
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recursive_brief_output() {
        let out = "Only in /a/sub: x.txt\nFiles /a/same.txt and /b/same.txt differ\nOnly in /b: y.txt\n";
        let d = parse_dirs(out, Path::new("/a"), Path::new("/b"));
        assert_eq!(d.only_left, vec!["sub/x.txt"]);
        assert_eq!(d.only_right, vec!["y.txt"]);
        assert_eq!(d.differ, vec!["same.txt"]);
    }
}
