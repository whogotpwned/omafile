//! Recursive search below a directory: streamed from `fd` when available,
//! a breadth-first GIO walk otherwise (which also covers remote locations).
//!
//! Results arrive in batches through a callback so the view fills while the
//! search is still running; `Cancellable` stops both the process and the walk.

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

pub const MAX_RESULTS: usize = 600;
const BATCH: usize = 24;

fn fd_binary() -> Option<String> {
    ["fd", "fdfind"].iter().find(|b| glib::find_program_in_path(b).is_some()).map(|s| s.to_string())
}

/// Lower is better: name starts with the query, then name contains it, then
/// only the path matched; shorter paths first within a group.
pub fn rank(path: &str, query: &str) -> (u8, usize) {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    let group = if name.starts_with(query) {
        0
    } else if name.contains(query) {
        1
    } else {
        2
    };
    (group, path.len())
}

/// Run the search, delivering batches of files to `emit` until done or cancelled.
pub async fn deep_search(
    root: &gio::File,
    query: &str,
    show_hidden: bool,
    cancel: gio::Cancellable,
    emit: Rc<dyn Fn(Vec<gio::File>, bool)>,
) {
    let query = query.to_lowercase();
    if let (Some(fd), Some(path)) = (fd_binary(), root.path()) {
        if fd_search(&fd, &path, &query, show_hidden, &cancel, emit.clone()).await {
            return;
        }
    }
    walk_search(root, &query, show_hidden, &cancel, emit).await;
}

async fn fd_search(fd: &str, root: &std::path::Path, query: &str, show_hidden: bool, cancel: &gio::Cancellable, emit: Rc<dyn Fn(Vec<gio::File>, bool)>) -> bool {
    let mut argv: Vec<String> = vec![
        fd.into(),
        "--ignore-case".into(),
        "--fixed-strings".into(),
        "--absolute-path".into(),
        "--color".into(),
        "never".into(),
        "--no-ignore".into(),
        "--exclude".into(),
        ".git".into(),
        "--max-results".into(),
        MAX_RESULTS.to_string(),
        "--max-depth".into(),
        "16".into(),
    ];
    if show_hidden {
        argv.push("--hidden".into());
    }
    argv.push("--".into());
    argv.push(query.into());
    argv.push(root.to_string_lossy().to_string());
    let argv: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE);
    let Ok(process) = launcher.spawn(&argv) else { return false };
    let Some(stdout) = process.stdout_pipe() else { return false };
    let reader = gio::DataInputStream::new(&stdout);
    let mut batch = Vec::new();
    let mut total = 0usize;
    loop {
        if cancel.is_cancelled() {
            process.force_exit();
            return true;
        }
        match reader.read_line_utf8_future(glib::Priority::DEFAULT).await {
            Ok(Some(line)) => {
                let line = line.trim_end_matches('\n');
                if line.is_empty() {
                    continue;
                }
                batch.push(gio::File::for_path(line));
                total += 1;
                if batch.len() >= BATCH {
                    emit(std::mem::take(&mut batch), false);
                }
                if total >= MAX_RESULTS {
                    process.force_exit();
                    break;
                }
            }
            _ => break,
        }
    }
    if !cancel.is_cancelled() {
        emit(batch, true);
    }
    true
}

pub fn rg_available() -> bool {
    glib::find_program_in_path("rg").is_some()
}

/// Full-text search with ripgrep: files below `root` whose contents contain
/// `query` (case-insensitive, literal), streamed like the name search.
pub async fn content_search(root: &gio::File, query: &str, show_hidden: bool, cancel: gio::Cancellable, emit: Rc<dyn Fn(Vec<gio::File>, bool)>) {
    let Some(path) = root.path() else {
        emit(Vec::new(), true);
        return;
    };
    let mut argv: Vec<String> = vec![
        "rg".into(),
        "--files-with-matches".into(),
        "--ignore-case".into(),
        "--fixed-strings".into(),
        "--no-messages".into(),
        "--max-count".into(),
        "1".into(),
        "--max-filesize".into(),
        "8M".into(),
        "--max-depth".into(),
        "16".into(),
        "--glob".into(),
        "!.git".into(),
        "--color".into(),
        "never".into(),
    ];
    if show_hidden {
        argv.push("--hidden".into());
    }
    argv.push("--".into());
    argv.push(query.into());
    argv.push(path.to_string_lossy().to_string());
    let args: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
    if std::env::var_os("OMAFILE_TRACE").is_some() {
        eprintln!("content search: {}", argv.join(" "));
    }
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE);
    let process = match launcher.spawn(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("content search: cannot start rg: {}", e.message());
            emit(Vec::new(), true);
            return;
        }
    };
    let Some(stdout) = process.stdout_pipe() else {
        emit(Vec::new(), true);
        return;
    };
    let reader = gio::DataInputStream::new(&stdout);
    let mut batch = Vec::new();
    let mut total = 0usize;
    loop {
        if cancel.is_cancelled() {
            process.force_exit();
            return;
        }
        match reader.read_line_utf8_future(glib::Priority::DEFAULT).await {
            Ok(Some(line)) => {
                let line = line.trim_end_matches('\n');
                if line.is_empty() {
                    continue;
                }
                batch.push(gio::File::for_path(line));
                total += 1;
                if batch.len() >= BATCH {
                    emit(std::mem::take(&mut batch), false);
                }
                if total >= MAX_RESULTS {
                    process.force_exit();
                    break;
                }
            }
            _ => break,
        }
    }
    if !cancel.is_cancelled() {
        emit(batch, true);
    }
}

/// Breadth-first walk with GIO, so remote locations work too.
async fn walk_search(root: &gio::File, needle: &str, show_hidden: bool, cancel: &gio::Cancellable, emit: Rc<dyn Fn(Vec<gio::File>, bool)>) {
    let mut queue = std::collections::VecDeque::from([(root.clone(), 0u32)]);
    let mut visited = 0usize;
    let mut total = 0usize;
    let batch = RefCell::new(Vec::new());
    'outer: while let Some((dir, depth)) = queue.pop_front() {
        if cancel.is_cancelled() {
            return;
        }
        if total >= MAX_RESULTS || visited > 60_000 {
            break;
        }
        let Ok(enumerator) = dir
            .enumerate_children_future("standard::name,standard::display-name,standard::type,standard::is-hidden", gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, glib::Priority::DEFAULT)
            .await
        else {
            continue;
        };
        loop {
            let Ok(files) = enumerator.next_files_future(300, glib::Priority::DEFAULT).await else { break };
            if files.is_empty() {
                break;
            }
            for info in files {
                visited += 1;
                let name = info.display_name().to_string();
                if name.starts_with('.') && (!show_hidden || name == ".git") {
                    continue;
                }
                let child = enumerator.child(&info);
                if name.to_lowercase().contains(needle) {
                    batch.borrow_mut().push(child.clone());
                    total += 1;
                    if batch.borrow().len() >= BATCH {
                        emit(std::mem::take(&mut *batch.borrow_mut()), false);
                    }
                    if total >= MAX_RESULTS {
                        break 'outer;
                    }
                }
                if info.file_type() == gio::FileType::Directory && depth < 10 {
                    queue.push_back((child, depth + 1));
                }
            }
        }
    }
    if !cancel.is_cancelled() {
        emit(batch.take(), true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_prefers_name_matches() {
        assert!(rank("/a/notes.md", "notes") < rank("/a/my-notes.md", "notes"));
        assert!(rank("/a/my-notes.md", "notes") < rank("/notes/other.txt", "notes"));
        assert!(rank("/a/notes.md", "notes") < rank("/a/b/notes.md", "notes"));
    }
}
