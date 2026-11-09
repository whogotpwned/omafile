//! iPhone and iPad over USB: Omarchy mounts them through GVFS (`gvfs-afc`),
//! omafile shows them under "iPhone & iPad" and imports the camera roll into
//! dated folders, converting HEIC to JPEG when a converter is around.

use crate::i18n::tr;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Mounted Apple devices: (name, root).
pub fn devices() -> Vec<(String, gio::File)> {
    gio::VolumeMonitor::get()
        .mounts()
        .into_iter()
        .filter(|m| {
            let root = m.root();
            let name = m.name().to_lowercase();
            root.uri_scheme().as_deref() == Some("afc") || name.contains("iphone") || name.contains("ipad") || name.contains("ipod") || root.path().map(|p| p.join("DCIM").is_dir() && p.join("PhotoData").is_dir()).unwrap_or(false)
        })
        .map(|m| (m.name().to_string(), m.root()))
        .collect()
}

pub fn afc_available() -> bool {
    // gvfs-afc installs this backend binary.
    ["/usr/lib/gvfs/gvfsd-afc", "/usr/libexec/gvfsd-afc"].iter().any(|p| Path::new(p).exists())
}

pub fn is_photo_name(name: &str) -> bool {
    let l = name.to_lowercase();
    [".heic", ".heif", ".jpg", ".jpeg", ".png", ".mov", ".mp4", ".dng", ".gif", ".webp", ".aae"].iter().any(|e| l.ends_with(e)) && !l.ends_with(".aae")
}

pub fn heic_converter() -> Option<Vec<&'static str>> {
    if glib::find_program_in_path("heif-convert").is_some() {
        Some(vec!["heif-convert", "-q", "92"])
    } else if glib::find_program_in_path("magick").is_some() {
        Some(vec!["magick"])
    } else {
        None
    }
}

fn log_path() -> PathBuf {
    glib::user_state_dir().join("omafile/iphone-imported")
}

fn imported() -> std::collections::HashSet<String> {
    std::fs::read_to_string(log_path()).map(|t| t.lines().map(String::from).collect()).unwrap_or_default()
}

/// Month folder for a shot: from the modification time (the camera roll keeps the capture time there).
pub fn month_folder(base: &Path, modified_unix: i64) -> PathBuf {
    match glib::DateTime::from_unix_local(modified_unix) {
        Ok(dt) => base.join(format!("{:04}-{:02}", dt.year(), dt.month())),
        Err(_) => base.join("undated"),
    }
}

impl Window {
    /// Import new photos and videos from every connected device into Pictures/iPhone/YYYY-MM.
    pub fn import_photos(self: &Rc<Self>) {
        let devices = devices();
        if devices.is_empty() {
            let hint = if afc_available() { tr("No iPhone or iPad connected. Plug it in, unlock it and tap “Trust”.") } else { tr("No iPhone or iPad connected. For USB access install gvfs-afc and libimobiledevice, then plug the device in and tap “Trust”.") };
            self.toast_error(&hint);
            return;
        }
        let base = self.prefs.get("iphone-folder").map(|p| crate::rules::expand(&p)).unwrap_or_else(|| glib::user_special_dir(glib::UserDirectory::Pictures).unwrap_or_else(|| glib::home_dir().join("Pictures")).join("iPhone"));
        let convert = self.prefs.get_bool("iphone-convert-heic", true);
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Some(w) = this.upgrade() else { return };
            let mut done = imported();
            let mut created: Vec<gio::File> = Vec::new();
            let mut skipped = 0;
            let mut failed = 0;
            let converter = if convert { heic_converter() } else { None };
            for (name, root) in devices {
                let dcim = root.child("DCIM");
                let shots = list_photos(&dcim).await;
                let total = shots.len();
                for (i, (file, size, modified)) in shots.into_iter().enumerate() {
                    let fname = file.basename().map(|b| b.to_string_lossy().to_string()).unwrap_or_default();
                    let key = format!("{fname}\t{size}");
                    if done.contains(&key) {
                        skipped += 1;
                        continue;
                    }
                    w.show_import_progress(&trf!("Importing from {device}: {name} ({i} of {total})", device = name.clone(), name = fname.clone(), i = i + 1, total = total), i as f64 / total.max(1) as f64);
                    let folder = month_folder(&base, modified);
                    let _ = std::fs::create_dir_all(&folder);
                    let target = gio::File::for_path(crate::localsend::unique_path(&folder, &fname));
                    let (fut, _) = file.copy_future(&target, gio::FileCopyFlags::NOFOLLOW_SYMLINKS, glib::Priority::DEFAULT);
                    match fut.await {
                        Ok(()) => {
                            let mut result = target.clone();
                            if let (Some(conv), Some(path)) = (&converter, target.path().filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case("heic") || e.eq_ignore_ascii_case("heif")).unwrap_or(false))) {
                                let jpg = path.with_extension("jpg");
                                let mut argv: Vec<String> = conv.iter().map(|s| s.to_string()).collect();
                                argv.push(path.to_string_lossy().to_string());
                                argv.push(jpg.to_string_lossy().to_string());
                                let args: Vec<&str> = argv.iter().map(String::as_str).collect();
                                if crate::ops::run_command(&args, None).await.is_ok() && jpg.exists() {
                                    let _ = std::fs::remove_file(&path);
                                    result = gio::File::for_path(&jpg);
                                }
                            }
                            created.push(result);
                            done.insert(key);
                        }
                        Err(_) => failed += 1,
                    }
                }
            }
            w.hide_import_progress();
            if let Some(dir) = log_path().parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let mut lines: Vec<&String> = done.iter().collect();
            lines.sort();
            let _ = std::fs::write(log_path(), lines.iter().map(|l| format!("{l}\n")).collect::<String>());
            let n = created.len();
            let mut msg = if n == 0 { tr("Nothing new on the device") } else { trf!("Imported {n} photos and videos into {folder}", n = n, folder = crate::rules::shorten(&base)) };
            if skipped > 0 {
                msg.push_str(&trf!(", {n} already imported", n = skipped));
            }
            if failed > 0 {
                msg.push_str(&trf!(", {n} failed", n = failed));
            }
            if n > 0 {
                w.record(Op::Derive(created), &msg);
                for p in w.panes.borrow().clone() {
                    if p.cwd().path().map(|c| c.starts_with(&base)).unwrap_or(false) {
                        p.reload();
                    }
                }
            } else {
                w.toast(&msg);
            }
        });
    }
}

/// Every photo / video below DCIM: (file, size, modified unix).
async fn list_photos(dcim: &gio::File) -> Vec<(gio::File, u64, i64)> {
    let mut out = Vec::new();
    let mut stack = vec![dcim.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(items) = crate::ops::list_dir(&dir).await else { continue };
        for item in items {
            if item.is_dir() {
                stack.push(item.file());
            } else if is_photo_name(&item.name()) {
                out.push((item.file(), item.size(), item.modified_unix()));
            }
        }
    }
    out.sort_by_key(|(_, _, m)| *m);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_names_and_month_folders() {
        assert!(is_photo_name("IMG_0001.HEIC"));
        assert!(is_photo_name("IMG_0002.MOV"));
        assert!(!is_photo_name("IMG_0001.AAE"));
        assert!(!is_photo_name("notes.txt"));
        let f = month_folder(Path::new("/p"), 1_700_000_000);
        assert!(f.starts_with("/p"));
        assert_eq!(f.file_name().unwrap().to_string_lossy().len(), 7);
    }
}
