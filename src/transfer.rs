//! Copy / move engine with a progress banner, cancel, a queue and a conflict
//! dialog (replace, skip, rename, apply to all), used by paste, drag and drop,
//! Copy To, Move To and Duplicate.

use crate::ops;
use crate::pane::Pane;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Copy,
    Move,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Conflict {
    Replace,
    Skip,
    Rename,
    Cancel,
}

pub struct Request {
    pub files: Vec<gio::File>,
    pub dest: gio::File,
    pub mode: Mode,
    pub pane: Rc<Pane>,
    /// Never ask: always pick a free name (Duplicate).
    pub auto_rename: bool,
}

/// Banner at the bottom of the window with the running operation.
pub struct Progress {
    pub revealer: gtk::Revealer,
    label: gtk::Label,
    bar: gtk::ProgressBar,
    cancel: gtk::Button,
}

impl Progress {
    pub fn cancel_button(&self) -> &gtk::Button {
        &self.cancel
    }

    pub fn new() -> Progress {
        let label = gtk::Label::builder().xalign(0.0).hexpand(true).ellipsize(gtk::pango::EllipsizeMode::Middle).build();
        let bar = gtk::ProgressBar::builder().hexpand(true).valign(gtk::Align::Center).build();
        bar.add_css_class("omafile-progress-bar");
        let cancel = gtk::Button::builder().label("Cancel").valign(gtk::Align::Center).tooltip_text("Stop the running operation").build();
        cancel.add_css_class("flat");
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 4);
        texts.set_hexpand(true);
        texts.append(&label);
        texts.append(&bar);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.add_css_class("omafile-progress");
        row.append(&gtk::Image::from_icon_name("folder-copy-symbolic"));
        row.append(&texts);
        row.append(&cancel);
        let revealer = gtk::Revealer::builder().child(&row).transition_type(gtk::RevealerTransitionType::SlideUp).reveal_child(false).build();
        Progress { revealer, label, bar, cancel }
    }
}

#[derive(Default)]
pub struct Queue {
    pending: RefCell<VecDeque<Request>>,
    running: Cell<bool>,
    cancel: RefCell<Option<gio::Cancellable>>,
}

impl Window {
    pub fn enqueue_transfer(self: &Rc<Self>, req: Request) {
        self.queue.pending.borrow_mut().push_back(req);
        self.pump_queue();
    }

    fn pump_queue(self: &Rc<Self>) {
        if self.queue.running.get() {
            return;
        }
        let Some(req) = self.queue.pending.borrow_mut().pop_front() else {
            self.progress.revealer.set_reveal_child(false);
            crate::status::write(self);
            return;
        };
        self.queue.running.set(true);
        let cancel = gio::Cancellable::new();
        *self.queue.cancel.borrow_mut() = Some(cancel.clone());
        crate::status::write(self);
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Some(w) = this.upgrade() else { return };
            w.run_transfer(req, cancel).await;
            w.queue.running.set(false);
            *w.queue.cancel.borrow_mut() = None;
            w.pump_queue();
        });
    }

    pub fn cancel_transfer(&self) {
        if let Some(c) = self.queue.cancel.borrow().as_ref() {
            c.cancel();
        }
        self.queue.pending.borrow_mut().clear();
    }

    fn show_progress(&self, text: &str, fraction: Option<f64>) {
        self.progress.label.set_text(text);
        match fraction {
            Some(f) => self.progress.bar.set_fraction(f.clamp(0.0, 1.0)),
            None => self.progress.bar.pulse(),
        }
        self.progress.revealer.set_reveal_child(true);
    }

    async fn run_transfer(self: &Rc<Self>, req: Request, cancel: gio::Cancellable) {
        let verb = match req.mode {
            Mode::Copy => "Copying",
            Mode::Move => "Moving",
        };
        let total = req.files.len();
        let mut pairs: Vec<(gio::File, gio::File)> = Vec::new();
        let mut remembered: Option<Conflict> = None;
        let mut error: Option<String> = None;
        let mut skipped = 0;
        let dest_name = crate::window::display_name(&req.dest);
        self.show_progress(&format!("{verb} {total} {} to {dest_name}", if total == 1 { "item" } else { "items" }), Some(0.0));

        for (index, src) in req.files.iter().enumerate() {
            if cancel.is_cancelled() {
                break;
            }
            let Some(name) = src.basename() else { continue };
            let name = name.to_string_lossy().to_string();
            if src.equal(&req.dest) || req.dest.has_prefix(src) {
                error = Some(format!("{name}: cannot copy a folder into itself"));
                break;
            }
            if req.mode == Mode::Move && src.parent().map(|p| p.equal(&req.dest)).unwrap_or(false) {
                continue;
            }
            let mut dst = req.dest.child(&name);
            if req.auto_rename {
                dst = ops::unique_child(&req.dest, &name);
            } else if dst.query_exists(gio::Cancellable::NONE) {
                let choice = match remembered {
                    Some(c) => c,
                    None => {
                        let (choice, all) = self.ask_conflict(src, &dst).await;
                        if all {
                            remembered = Some(choice);
                        }
                        choice
                    }
                };
                match choice {
                    Conflict::Cancel => break,
                    Conflict::Skip => {
                        skipped += 1;
                        continue;
                    }
                    Conflict::Rename => dst = ops::unique_child(&req.dest, &name),
                    Conflict::Replace => {
                        if let Err(e) = ops::delete_recursive(dst.clone()).await {
                            error = Some(format!("{name}: cannot replace: {}", e.message()));
                            break;
                        }
                    }
                }
            }
            let base = index as f64 / total as f64;
            let share = 1.0 / total as f64;
            self.show_progress(&format!("{verb} {name} ({} of {total})", index + 1), Some(base));
            let this = Rc::downgrade(self);
            let progress_cb: Box<dyn FnMut(i64, i64)> = Box::new(move |done, all| {
                if let (Some(w), true) = (this.upgrade(), all > 0) {
                    w.progress.bar.set_fraction((base + share * done as f64 / all as f64).clamp(0.0, 1.0));
                }
            });
            let result = match req.mode {
                Mode::Copy => copy_cancellable(src.clone(), dst.clone(), cancel.clone(), progress_cb).await,
                Mode::Move => move_cancellable(src.clone(), dst.clone(), cancel.clone(), progress_cb).await,
            };
            match result {
                Ok(()) => {
                    if req.mode == Mode::Move {
                        self.tags.moved(src, &dst);
                    }
                    pairs.push((src.clone(), dst));
                }
                Err(e) if e.matches(gio::IOErrorEnum::Cancelled) => {
                    let _ = ops::delete_recursive(dst).await; // half-written copy
                    break;
                }
                Err(e) => {
                    error = Some(format!("{name}: {}", e.message()));
                    break;
                }
            }
        }

        req.pane.reload();
        if let Some(last) = pairs.last() {
            req.pane.select_file_later(last.1.clone());
        }
        let n = pairs.len();
        let done_verb = match req.mode {
            Mode::Copy => "Copied",
            Mode::Move => "Moved",
        };
        let mut message = format!("{done_verb} {n} {}", if n == 1 { "item" } else { "items" });
        if skipped > 0 {
            message.push_str(&format!(", {skipped} skipped"));
        }
        if cancel.is_cancelled() {
            message.push_str(", cancelled");
        }
        let moved_venv = req.mode == Mode::Move && pairs.iter().any(|(src, _)| src.path().map(|p| crate::pyenv::is_venv_dir(&p) || crate::pyenv::VENV_NAMES.iter().any(|n| crate::pyenv::is_venv_dir(&p.join(n)))).unwrap_or(false));
        if let Some(e) = error {
            self.toast_error(&e);
        } else if moved_venv {
            self.record(match req.mode { Mode::Copy => Op::Copy(pairs), Mode::Move => Op::Move(pairs) }, &format!("{message}  ·  a Python venv moved: recreate it (Python › Recreate venv)"));
        } else if n > 0 {
            let op = match req.mode {
                Mode::Copy => Op::Copy(pairs),
                Mode::Move => Op::Move(pairs),
            };
            self.record(op, &message);
        } else if skipped > 0 || cancel.is_cancelled() {
            self.toast(&message);
        }
    }

    /// Ask what to do about an existing target. Returns (choice, apply to all).
    async fn ask_conflict(self: &Rc<Self>, src: &gio::File, dst: &gio::File) -> (Conflict, bool) {
        let describe = |f: &gio::File| -> String {
            match f.query_info("standard::size,standard::type,time::modified", gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE) {
                Ok(info) => {
                    let when = info.modification_date_time().map(|d| ops::format_time(&d)).unwrap_or_default();
                    if info.file_type() == gio::FileType::Directory {
                        format!("folder · {when}")
                    } else {
                        format!("{} · {when}", ops::human_size(info.size().max(0) as u64))
                    }
                }
                Err(_) => String::new(),
            }
        };
        let name = crate::window::display_name(dst);
        let body = format!("“{name}” already exists in {}.\n\nExisting: {}\nNew: {}", crate::window::display_name(&dst.parent().unwrap_or_else(|| dst.clone())), describe(dst), describe(src));
        let apply_all = gtk::CheckButton::with_label("Apply to all remaining conflicts");
        let dialog = adw::AlertDialog::new(Some("Replace file?"), Some(&body));
        dialog.set_extra_child(Some(&apply_all));
        dialog.add_responses(&[("cancel", "Cancel"), ("skip", "Skip"), ("rename", "Keep Both"), ("replace", "Replace")]);
        dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("rename"));
        dialog.set_close_response("cancel");
        let response = dialog.choose_future(Some(&self.window)).await;
        self.refocus();
        let choice = match response.as_str() {
            "replace" => Conflict::Replace,
            "skip" => Conflict::Skip,
            "rename" => Conflict::Rename,
            _ => Conflict::Cancel,
        };
        (choice, apply_all.is_active())
    }
}

fn flags() -> gio::FileCopyFlags {
    gio::FileCopyFlags::NOFOLLOW_SYMLINKS | gio::FileCopyFlags::ALL_METADATA
}

async fn copy_one(src: gio::File, dst: gio::File, cancel: gio::Cancellable, progress: Box<dyn FnMut(i64, i64)>) -> Result<(), glib::Error> {
    let (tx, rx) = futures_channel::oneshot::channel();
    src.copy_async(&dst, flags(), glib::Priority::DEFAULT, Some(&cancel), Some(progress), move |r| {
        let _ = tx.send(r);
    });
    rx.await.unwrap_or_else(|_| Err(glib::Error::new(gio::IOErrorEnum::Cancelled, "cancelled")))
}

/// Recursive, cancellable copy with per-file progress.
pub fn copy_cancellable(src: gio::File, dst: gio::File, cancel: gio::Cancellable, progress: Box<dyn FnMut(i64, i64)>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), glib::Error>>>> {
    Box::pin(async move {
        let ftype = src.query_file_type(gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, gio::Cancellable::NONE);
        if ftype != gio::FileType::Directory {
            return copy_one(src, dst, cancel, progress).await;
        }
        if cancel.is_cancelled() {
            return Err(glib::Error::new(gio::IOErrorEnum::Cancelled, "cancelled"));
        }
        dst.make_directory_future(glib::Priority::DEFAULT).await?;
        let enumerator = src.enumerate_children_future("standard::name", gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, glib::Priority::DEFAULT).await?;
        let progress = Rc::new(RefCell::new(progress));
        loop {
            let batch = enumerator.next_files_future(200, glib::Priority::DEFAULT).await?;
            if batch.is_empty() {
                break;
            }
            for info in batch {
                if cancel.is_cancelled() {
                    return Err(glib::Error::new(gio::IOErrorEnum::Cancelled, "cancelled"));
                }
                let child = enumerator.child(&info);
                let p = progress.clone();
                let cb: Box<dyn FnMut(i64, i64)> = Box::new(move |a, b| (p.borrow_mut())(a, b));
                copy_cancellable(child, dst.child(info.name()), cancel.clone(), cb).await?;
            }
        }
        Ok(())
    })
}

async fn move_cancellable(src: gio::File, dst: gio::File, cancel: gio::Cancellable, progress: Box<dyn FnMut(i64, i64)>) -> Result<(), glib::Error> {
    let (tx, rx) = futures_channel::oneshot::channel();
    let cancel2 = cancel.clone();
    src.move_async(&dst, flags(), glib::Priority::DEFAULT, Some(&cancel2), None, move |r| {
        let _ = tx.send(r);
    });
    match rx.await.unwrap_or_else(|_| Err(glib::Error::new(gio::IOErrorEnum::Cancelled, "cancelled"))) {
        Ok(()) => Ok(()),
        Err(e) if e.matches(gio::IOErrorEnum::WouldRecurse) => {
            copy_cancellable(src.clone(), dst, cancel, progress).await?;
            ops::delete_recursive(src).await
        }
        Err(e) => Err(e),
    }
}
