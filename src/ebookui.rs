//! The eBook menu: open in a reader, rename from metadata, send to an
//! eReader, convert with Calibre.

use crate::i18n::tr;
use crate::ops;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;

impl Window {
    fn ebook_items(&self) -> Vec<crate::item::FileItem> {
        self.selected_items().into_iter().filter(|i| i.is_ebook() && i.target_file().is_native()).collect()
    }

    /// Open the selection in the dedicated reader on this system (foliate, Calibre's viewer, …).
    pub fn ebook_open_reader(self: &Rc<Self>) {
        let Some(reader) = crate::ebook::reader() else {
            self.toast_error(&tr("No eBook reader installed (try foliate)"));
            return;
        };
        let items = self.ebook_items();
        if items.is_empty() {
            return;
        }
        let mut argv = vec!["setsid".to_string(), "uwsm-app".to_string(), "--".to_string(), reader.to_string()];
        if glib::find_program_in_path("uwsm-app").is_none() {
            argv = vec!["setsid".to_string(), reader.to_string()];
        }
        argv.extend(items.iter().filter_map(|i| i.target_file().path()).map(|p| p.to_string_lossy().to_string()));
        let args: Vec<&str> = argv.iter().map(String::as_str).collect();
        if let Err(e) = std::process::Command::new(args[0]).args(&args[1..]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
            self.toast_error(&format!("{reader}: {e}"));
        }
    }

    /// `Author - Title.ext` for every selected eBook with readable metadata.
    pub fn ebook_rename(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let items = self.ebook_items();
        if items.is_empty() {
            return;
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let mut moves: Vec<(gio::File, gio::File)> = Vec::new();
            let mut skipped = 0;
            for item in &items {
                let (Some(path), Some(ct)) = (item.target_file().path(), item.content_type()) else { continue };
                let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
                let (p2, c2) = (path.clone(), ct.clone());
                let Some(name) = gio::spawn_blocking(move || crate::ebook::meta(&p2, &c2).and_then(|m| crate::ebook::suggested_name(&m, &ext))).await.ok().flatten() else {
                    skipped += 1;
                    continue;
                };
                if item.name() == name {
                    continue;
                }
                let file = item.target_file();
                let Some(parent) = file.parent() else { continue };
                let target = ops::unique_child(&parent, &name);
                match ops::rename(file.clone(), &target.basename().map(|b| b.to_string_lossy().to_string()).unwrap_or(name)).await {
                    Ok(renamed) => {
                        if let Some(w) = this.upgrade() {
                            w.tags.moved(&file, &renamed);
                        }
                        moves.push((file, renamed));
                    }
                    Err(_) => skipped += 1,
                }
            }
            let Some(w) = this.upgrade() else { return };
            pane.reload();
            if let Some(last) = moves.last() {
                pane.select_file_later(last.1.clone());
            }
            let n = moves.len();
            if n > 0 {
                let mut msg = trf!("Renamed {n} eBooks from their metadata", n = n);
                if skipped > 0 {
                    msg.push_str(&trf!(", {n} without metadata", n = skipped));
                }
                w.record(Op::Move(moves), &msg);
            } else {
                w.toast(&tr("No metadata to rename from"));
            }
        });
    }

    /// Copy the selection onto a mounted eReader (Kindle: documents/, Kobo and others: the root).
    pub fn ebook_send(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let files: Vec<gio::File> = self.ebook_items().iter().map(|i| i.target_file()).collect();
        if files.is_empty() {
            return;
        }
        let readers = crate::ebook::ereaders();
        match readers.as_slice() {
            [] => self.toast_error(&tr("No eReader connected (Kindle, Kobo, Tolino, PocketBook…)")),
            [(name, dest)] => {
                self.toast(&trf!("Sending {n} to {device}", n = files.len(), device = name.clone()));
                self.transfer(files, dest.clone(), crate::transfer::Mode::Copy, pane, false);
            }
            many => {
                let dialog = adw::AlertDialog::builder().heading(tr("Send to which eReader?")).build();
                for (i, (name, _)) in many.iter().enumerate() {
                    dialog.add_response(&i.to_string(), name);
                }
                dialog.add_response("cancel", &tr("Cancel"));
                dialog.set_default_response(Some("0"));
                let targets: Vec<gio::File> = many.iter().map(|(_, f)| f.clone()).collect();
                let this = Rc::downgrade(self);
                dialog.connect_response(None, move |_, response| {
                    let (Some(w), Ok(i)) = (this.upgrade(), response.parse::<usize>()) else { return };
                    if let Some(dest) = targets.get(i) {
                        w.transfer(files.clone(), dest.clone(), crate::transfer::Mode::Copy, pane.clone(), false);
                    }
                });
                dialog.present(Some(&self.window));
            }
        }
    }

    /// Convert with Calibre's `ebook-convert` into `fmt` next to the originals.
    pub fn ebook_convert(self: &Rc<Self>, fmt: &str) {
        if !crate::ebook::calibre_available() {
            self.toast_error(&tr("Calibre (ebook-convert) is not installed"));
            return;
        }
        let Some(pane) = self.active_pane() else { return };
        let items = self.ebook_items();
        let jobs: Vec<(std::path::PathBuf, std::path::PathBuf)> = items.iter().filter_map(|i| i.target_file().path()).filter(|p| p.extension().map(|e| !e.eq_ignore_ascii_case(fmt)).unwrap_or(true)).map(|p| (p.clone(), crate::ebook::output_path(&p, fmt))).collect();
        if jobs.is_empty() {
            self.toast(&tr("Already in that format"));
            return;
        }
        let fmt = fmt.to_uppercase();
        self.toast_keyed("ebook-convert", &trf!("Converting {n} to {fmt}…", n = jobs.len(), fmt = fmt.clone()));
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let mut made: Vec<gio::File> = Vec::new();
            let mut failed = 0;
            for (input, output) in jobs {
                let argv = ["ebook-convert", &input.to_string_lossy(), &output.to_string_lossy()];
                match ops::run_command(&argv, None).await {
                    Ok(()) if output.exists() => made.push(gio::File::for_path(&output)),
                    _ => failed += 1,
                }
            }
            let Some(w) = this.upgrade() else { return };
            pane.reload();
            if let Some(last) = made.last() {
                pane.select_file_later(last.clone());
            }
            let n = made.len();
            if n > 0 {
                w.record(Op::Derive(made), &trf!("Converted {n} to {fmt}", n = n, fmt = fmt.clone()));
            }
            if failed > 0 {
                w.toast_error(&trf!("{n} conversions failed", n = failed));
            }
        });
    }
}
