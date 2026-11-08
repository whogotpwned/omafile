//! Image tools on the context menu: rotate, convert, resize (ImageMagick),
//! and "Set as Wallpaper" through omarchy-theme-bg-set.

use crate::item::FileItem;
use crate::ops;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// `magick` (IM 7) or `convert` (IM 6).
pub fn magick() -> Option<&'static str> {
    if glib::find_program_in_path("magick").is_some() {
        Some("magick")
    } else if glib::find_program_in_path("convert").is_some() {
        Some("convert")
    } else {
        None
    }
}

fn with_suffix(path: &Path, suffix: &str, ext: Option<&str>) -> PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let ext = ext.map(str::to_string).or_else(|| path.extension().and_then(|e| e.to_str()).map(str::to_string)).unwrap_or_default();
    let name = if ext.is_empty() { format!("{stem}{suffix}") } else { format!("{stem}{suffix}.{ext}") };
    path.with_file_name(name)
}

impl Window {
    fn image_targets(&self) -> Vec<PathBuf> {
        self.selected_items().iter().filter(|i| i.is_image()).filter_map(|i| i.target_file().path()).collect()
    }

    /// Run one ImageMagick job per file; new files are recorded for Undo.
    fn run_magick(self: &Rc<Self>, jobs: Vec<(Vec<String>, PathBuf)>, verb: &'static str, in_place: bool) {
        let Some(pane) = self.active_pane() else { return };
        let Some(tool) = magick() else {
            self.toast_error("ImageMagick (magick) is not installed");
            return;
        };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let mut created = Vec::new();
            let mut error = None;
            for (args, output) in jobs {
                let mut argv: Vec<&str> = vec![tool];
                argv.extend(args.iter().map(String::as_str));
                if let Err(e) = ops::run_command(&argv, None).await {
                    error = Some(e.message().to_string());
                    break;
                }
                if !in_place {
                    created.push(gio::File::for_path(&output));
                }
            }
            let Some(w) = this.upgrade() else { return };
            pane.reload();
            if let Some(e) = error {
                w.toast_error(&format!("{verb} failed: {e}"));
            } else if in_place {
                w.toast(&format!("{verb} done"));
            } else {
                if let Some(last) = created.last() {
                    pane.select_file_later(last.clone());
                }
                let n = created.len();
                w.record(Op::Derive(created), &format!("{verb}: {n} {}", if n == 1 { "file" } else { "files" }));
            }
        });
    }

    pub fn image_rotate(self: &Rc<Self>, degrees: i32) {
        let jobs: Vec<(Vec<String>, PathBuf)> = self
            .image_targets()
            .into_iter()
            .map(|p| {
                let s = p.to_string_lossy().to_string();
                (vec![s.clone(), "-auto-orient".into(), "-rotate".into(), degrees.to_string(), s], p)
            })
            .collect();
        if jobs.is_empty() {
            return;
        }
        self.run_magick(jobs, "Rotate", true);
    }

    pub fn image_convert(self: &Rc<Self>, format: &str) {
        let ext = format.to_ascii_lowercase();
        let jobs: Vec<(Vec<String>, PathBuf)> = self
            .image_targets()
            .into_iter()
            .filter(|p| p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) != Some(ext.clone()))
            .map(|p| {
                let out = ops::unique_child(&gio::File::for_path(p.parent().unwrap_or(Path::new("."))), &with_suffix(&p, "", Some(&ext)).file_name().unwrap().to_string_lossy()).path().unwrap();
                let mut args = vec![p.to_string_lossy().to_string(), "-auto-orient".into()];
                if ext == "jpg" || ext == "jpeg" {
                    args.extend(["-background".into(), "white".into(), "-flatten".into(), "-quality".into(), "90".into()]);
                }
                if ext == "webp" {
                    args.extend(["-quality".into(), "85".into()]);
                }
                args.push(out.to_string_lossy().to_string());
                (args, out)
            })
            .collect();
        if jobs.is_empty() {
            self.toast("Already in that format");
            return;
        }
        self.run_magick(jobs, "Convert", false);
    }

    /// Ask for a size, then write `name@WIDTH.ext` next to each image.
    pub fn image_resize_dialog(self: &Rc<Self>) {
        let targets = self.image_targets();
        if targets.is_empty() {
            return;
        }
        let entry = gtk::Entry::builder().text("1920").activates_default(true).build();
        let mode = gtk::DropDown::from_strings(&["Width in pixels", "Height in pixels", "Percent"]);
        let hint = gtk::Label::builder().label("Aspect ratio is kept. Originals stay untouched; a copy is written next to each one.").wrap(true).xalign(0.0).build();
        hint.add_css_class("omafile-dim");
        hint.add_css_class("caption");
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&entry);
        row.append(&mode);
        vbox.append(&row);
        vbox.append(&hint);
        let dialog = adw::AlertDialog::new(Some("Resize images"), None);
        dialog.set_extra_child(Some(&vbox));
        dialog.add_responses(&[("cancel", "Cancel"), ("resize", "Resize")]);
        dialog.set_response_appearance("resize", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("resize"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let entry2 = entry.clone();
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            w.refocus();
            if response != "resize" {
                return;
            }
            let Ok(value) = entry2.text().trim().parse::<u32>() else {
                w.toast_error("Enter a whole number");
                return;
            };
            let (geometry, suffix) = match mode.selected() {
                0 => (format!("{value}x"), format!("@{value}w")),
                1 => (format!("x{value}"), format!("@{value}h")),
                _ => (format!("{value}%"), format!("@{value}pct")),
            };
            let jobs: Vec<(Vec<String>, PathBuf)> = targets
                .iter()
                .map(|p| {
                    let out = ops::unique_child(&gio::File::for_path(p.parent().unwrap_or(Path::new("."))), &with_suffix(p, &suffix, None).file_name().unwrap().to_string_lossy()).path().unwrap();
                    (vec![p.to_string_lossy().to_string(), "-auto-orient".into(), "-resize".into(), geometry.clone(), out.to_string_lossy().to_string()], out)
                })
                .collect();
            w.run_magick(jobs, "Resize", false);
        });
        entry.grab_focus();
        entry.select_region(0, -1);
    }

    pub fn set_wallpaper(self: &Rc<Self>) {
        let Some(path) = self.image_targets().into_iter().next() else { return };
        if !crate::omarchy::bg_set_available() {
            self.toast_error("omarchy-theme-bg-set not found (not on Omarchy?)");
            return;
        }
        let argv = crate::omarchy::set_wallpaper_argv(&path);
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let args: Vec<&str> = argv.iter().map(String::as_str).collect();
            let result = ops::run_command(&args, None).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(()) => w.toast(&format!("Wallpaper set: {}", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())),
                Err(e) => w.toast_error(&format!("Cannot set wallpaper: {}", e.message())),
            }
        });
    }

    pub fn apply_omarchy_theme(self: &Rc<Self>) {
        let Some(item) = self.selected_items().into_iter().find(|i| i.is_dir()) else { return };
        let Some(dir) = item.target_file().path() else { return };
        if !crate::omarchy::is_theme_dir(&dir) {
            self.toast_error("Not an Omarchy theme folder");
            return;
        }
        if !crate::omarchy::theme_set_available() {
            self.toast_error("omarchy-theme-set not found (not on Omarchy?)");
            return;
        }
        let Some(argv) = crate::omarchy::apply_theme_argv(&dir) else { return };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let args: Vec<&str> = argv.iter().map(String::as_str).collect();
            let result = ops::run_command(&args, None).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(()) => w.toast(&format!("Theme applied: {}", argv[1])),
                Err(e) => w.toast_error(&format!("Cannot apply theme: {}", e.message())),
            }
        });
    }

    pub fn image_menu_applies(items: &[FileItem]) -> bool {
        !items.is_empty() && items.iter().all(|i| i.is_image() && i.target_file().is_native())
    }
}
