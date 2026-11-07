//! Asynchronous file operations on top of GIO.

use crate::item::{FileItem, ATTRIBUTES};
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use std::future::Future;
use std::pin::Pin;

pub type OpResult = Result<(), glib::Error>;

const PRIO: glib::Priority = glib::Priority::DEFAULT;

pub async fn list_dir(dir: &gio::File) -> Result<Vec<FileItem>, glib::Error> {
    let enumerator = dir
        .enumerate_children_future(ATTRIBUTES, gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, PRIO)
        .await?;
    let mut items = Vec::new();
    loop {
        let batch = enumerator.next_files_future(200, PRIO).await?;
        if batch.is_empty() {
            break;
        }
        for info in batch {
            let child = enumerator.child(&info);
            items.push(FileItem::new(child, info));
        }
    }
    let _ = enumerator.close_future(PRIO).await;
    Ok(items)
}

pub async fn query(file: &gio::File) -> Result<FileItem, glib::Error> {
    let info = file
        .query_info_future(ATTRIBUTES, gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, PRIO)
        .await?;
    Ok(FileItem::new(file.clone(), info))
}

fn file_type(file: &gio::File) -> gio::FileType {
    file.query_file_type(gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, gio::Cancellable::NONE)
}

/// A name in `dir` that does not exist yet: `name`, `name (1)`, `name (2)`, ...
pub fn unique_child(dir: &gio::File, name: &str) -> gio::File {
    let first = dir.child(name);
    if !first.query_exists(gio::Cancellable::NONE) {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && !e.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for i in 1.. {
        let candidate = dir.child(format!("{stem} ({i}){ext}"));
        if !candidate.query_exists(gio::Cancellable::NONE) {
            return candidate;
        }
    }
    unreachable!()
}

fn copy_flags() -> gio::FileCopyFlags {
    gio::FileCopyFlags::NOFOLLOW_SYMLINKS | gio::FileCopyFlags::ALL_METADATA
}

/// Copy a file or a whole directory tree. `dst` must not exist.
pub fn copy_recursive(src: gio::File, dst: gio::File) -> Pin<Box<dyn Future<Output = OpResult>>> {
    Box::pin(async move {
        if file_type(&src) != gio::FileType::Directory {
            let (fut, _progress) = src.copy_future(&dst, copy_flags(), PRIO);
            return fut.await;
        }
        if dst.has_prefix(&src) {
            return Err(glib::Error::new(gio::IOErrorEnum::InvalidArgument, "Cannot copy a folder into itself"));
        }
        dst.make_directory_future(PRIO).await?;
        let enumerator = src
            .enumerate_children_future("standard::name", gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, PRIO)
            .await?;
        loop {
            let batch = enumerator.next_files_future(200, PRIO).await?;
            if batch.is_empty() {
                break;
            }
            for info in batch {
                let child = enumerator.child(&info);
                copy_recursive(child, dst.child(info.name())).await?;
            }
        }
        Ok(())
    })
}

/// Move; falls back to copy + delete for directories across file systems.
pub async fn move_file(src: gio::File, dst: gio::File) -> OpResult {
    let (fut, _progress) = src.move_future(&dst, copy_flags(), PRIO);
    match fut.await {
        Ok(()) => Ok(()),
        Err(e) if e.matches(gio::IOErrorEnum::WouldRecurse) => {
            copy_recursive(src.clone(), dst).await?;
            delete_recursive(src).await
        }
        Err(e) => Err(e),
    }
}

pub fn delete_recursive(file: gio::File) -> Pin<Box<dyn Future<Output = OpResult>>> {
    Box::pin(async move {
        if file_type(&file) == gio::FileType::Directory {
            let enumerator = file
                .enumerate_children_future("standard::name", gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, PRIO)
                .await?;
            loop {
                let batch = enumerator.next_files_future(200, PRIO).await?;
                if batch.is_empty() {
                    break;
                }
                for info in batch {
                    delete_recursive(enumerator.child(&info)).await?;
                }
            }
        }
        file.delete_future(PRIO).await
    })
}

pub async fn trash(file: gio::File) -> OpResult {
    file.trash_future(PRIO).await
}

pub async fn rename(file: gio::File, new_name: &str) -> Result<gio::File, glib::Error> {
    file.set_display_name_future(new_name, PRIO).await
}

pub async fn make_dir(file: gio::File) -> OpResult {
    file.make_directory_future(PRIO).await
}

pub async fn create_file(file: gio::File) -> OpResult {
    file.create_future(gio::FileCreateFlags::NONE, PRIO).await.map(|_| ())
}

/// Open a terminal in `dir`. Omarchy launches apps through uwsm and ships Alacritty.
pub fn open_terminal(dir: &gio::File) -> Result<(), glib::Error> {
    let term = std::env::var("TERMINAL").unwrap_or_else(|_| "alacritty".into());
    let path = dir.path().unwrap_or_else(glib::home_dir);
    let path = path.to_string_lossy().to_string();
    let mut argv: Vec<String> = Vec::new();
    if glib::find_program_in_path("uwsm").is_some() {
        argv.extend(["uwsm".into(), "app".into(), "--".into()]);
    }
    argv.push(term.clone());
    argv.push("--working-directory".into());
    argv.push(path);
    let argv: Vec<&std::path::Path> = argv.iter().map(std::path::Path::new).collect();
    glib::spawn_async(
        None::<&std::path::Path>,
        &argv,
        &[],
        glib::SpawnFlags::SEARCH_PATH | glib::SpawnFlags::STDOUT_TO_DEV_NULL | glib::SpawnFlags::STDERR_TO_DEV_NULL,
        None,
    )
    .map(|_| ())
}

pub fn human_size(n: u64) -> String {
    glib::format_size(n).to_string()
}

pub fn format_time(dt: &glib::DateTime) -> String {
    let now = glib::DateTime::now_local().ok();
    let local = dt.to_local().unwrap_or_else(|_| dt.clone());
    let same_year = now.as_ref().map(|n| n.year() == local.year()).unwrap_or(false);
    let fmt = if same_year { "%-d %b %H:%M" } else { "%-d %b %Y" };
    local.format(fmt).map(|s| s.to_string()).unwrap_or_default()
}

pub fn perm_string(mode: u32, is_dir: bool, is_symlink: bool) -> String {
    let mut s = String::with_capacity(10);
    s.push(if is_symlink { 'l' } else if is_dir { 'd' } else { '-' });
    for shift in [6u32, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perms() {
        assert_eq!(perm_string(0o755, true, false), "drwxr-xr-x");
        assert_eq!(perm_string(0o644, false, false), "-rw-r--r--");
    }

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir().join(format!("omafile-gio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        let d = gio::File::for_path(&dir);
        assert_eq!(unique_child(&d, "a.txt").basename().unwrap().to_string_lossy(), "a (1).txt");
        assert_eq!(unique_child(&d, "b.txt").basename().unwrap().to_string_lossy(), "b.txt");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copy_move_delete_roundtrip() {
        let base = std::env::temp_dir().join(format!("omafile-ops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("src/inner")).unwrap();
        std::fs::write(base.join("src/inner/f.txt"), b"hello").unwrap();
        let ctx = glib::MainContext::new();
        ctx.block_on(async {
            let b = gio::File::for_path(&base);
            copy_recursive(b.child("src"), b.child("dst")).await.unwrap();
            assert_eq!(std::fs::read(base.join("dst/inner/f.txt")).unwrap(), b"hello");
            assert!(copy_recursive(b.child("src"), b.child("src/inner/loop")).await.is_err());
            move_file(b.child("dst"), b.child("moved")).await.unwrap();
            assert!(base.join("moved/inner/f.txt").exists());
            delete_recursive(b.child("moved")).await.unwrap();
            assert!(!base.join("moved").exists());
            let items = list_dir(&b).await.unwrap();
            assert_eq!(items.len(), 1);
            assert!(items[0].is_dir());
        });
        std::fs::remove_dir_all(&base).unwrap();
    }
}
