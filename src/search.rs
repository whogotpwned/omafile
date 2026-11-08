//! Recursive search below a directory: `fd` when available, a GIO walk otherwise.

use gtk::{gio, glib};
use gtk::prelude::*;

fn fd_binary() -> Option<String> {
    ["fd", "fdfind"].iter().find(|b| glib::find_program_in_path(b).is_some()).map(|s| s.to_string())
}

pub async fn deep_search(root: &gio::File, query: &str, show_hidden: bool, limit: usize) -> Vec<gio::File> {
    if let (Some(fd), Some(path)) = (fd_binary(), root.path()) {
        if let Some(files) = fd_search(&fd, &path, query, show_hidden, limit).await {
            return files;
        }
    }
    walk_search(root, &query.to_lowercase(), show_hidden, limit).await
}

async fn fd_search(fd: &str, root: &std::path::Path, query: &str, show_hidden: bool, limit: usize) -> Option<Vec<gio::File>> {
    let mut argv: Vec<String> = vec![
        fd.into(),
        "--ignore-case".into(),
        "--absolute-path".into(),
        "--max-results".into(),
        limit.to_string(),
        "--max-depth".into(),
        "12".into(),
        "--fixed-strings".into(),
    ];
    if show_hidden {
        argv.push("--hidden".into());
    }
    argv.push("--".into());
    argv.push(query.into());
    argv.push(root.to_string_lossy().to_string());
    let argv: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE);
    let process = launcher.spawn(&argv).ok()?;
    let (stdout, _) = process.communicate_utf8_future(None).await.ok()?;
    let text = stdout?.to_string();
    Some(text.lines().filter(|l| !l.is_empty()).map(gio::File::for_path).collect())
}

/// Breadth-first walk with GIO, so remote locations work too.
async fn walk_search(root: &gio::File, needle: &str, show_hidden: bool, limit: usize) -> Vec<gio::File> {
    let mut found = Vec::new();
    let mut queue = std::collections::VecDeque::from([(root.clone(), 0u32)]);
    let mut visited = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        if found.len() >= limit || visited > 20_000 {
            break;
        }
        let Ok(enumerator) = dir
            .enumerate_children_future("standard::name,standard::display-name,standard::type,standard::is-hidden", gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, glib::Priority::DEFAULT)
            .await
        else {
            continue;
        };
        loop {
            let Ok(batch) = enumerator.next_files_future(200, glib::Priority::DEFAULT).await else { break };
            if batch.is_empty() {
                break;
            }
            for info in batch {
                visited += 1;
                let name = info.display_name().to_string();
                let hidden = name.starts_with('.');
                if hidden && !show_hidden {
                    continue;
                }
                let child = enumerator.child(&info);
                if name.to_lowercase().contains(needle) {
                    found.push(child.clone());
                    if found.len() >= limit {
                        break;
                    }
                }
                if info.file_type() == gio::FileType::Directory && depth < 8 {
                    queue.push_back((child, depth + 1));
                }
            }
        }
    }
    found
}
