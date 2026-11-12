//! Git actions without a terminal: stage/unstage, commit, branch switch, discard.

use crate::i18n::tr;
use crate::git;
use crate::item::FileItem;
use crate::pane::Pane;
use crate::window::Window;
use adw::prelude::*;
use gtk::glib;
use std::path::PathBuf;
use std::rc::Rc;

impl Window {
    fn repo_root(&self, pane: &Pane) -> Option<PathBuf> {
        pane.git.borrow().as_ref().map(|r| r.root.clone())
    }

    fn selected_paths(pane: &Pane) -> Vec<PathBuf> {
        pane.selected_items().iter().filter_map(|i| i.target_file().path()).collect()
    }

    fn after_git(self: &Rc<Self>, pane: Rc<Pane>, result: Result<(), String>, ok: &str) {
        match result {
            Ok(()) => {
                self.toast(ok);
                pane.reload();
            }
            Err(e) => self.toast_error(&trf!("git: {e}", e = e)),
        }
    }

    /// Badge click: staged → unstage, otherwise stage.
    pub fn git_toggle_stage(self: &Rc<Self>, pane: &Rc<Pane>, item: &FileItem) {
        let Some(root) = self.repo_root(pane) else { return };
        let Some(path) = item.target_file().path() else { return };
        let status = pane.git_status_of(item);
        let staged = matches!(status, Some(git::Status::Added) | Some(git::Status::Renamed)) || pane.git.borrow().as_ref().map(|r| r.staged_paths.contains(&path)).unwrap_or(false);
        let this = Rc::downgrade(self);
        let pane = pane.clone();
        glib::spawn_future_local(async move {
            let result = if staged { git::unstage(&root, &[path]).await } else { git::stage(&root, &[path]).await };
            if let Some(w) = this.upgrade() {
                w.after_git(pane, result, &if staged { tr("Unstaged") } else { tr("Staged") });
            }
        });
    }

    pub fn git_stage_selected(self: &Rc<Self>, unstage: bool) {
        let Some(pane) = self.active_pane() else { return };
        let Some(root) = self.repo_root(&pane) else { return };
        let paths = Self::selected_paths(&pane);
        if paths.is_empty() {
            return;
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = if unstage { git::unstage(&root, &paths).await } else { git::stage(&root, &paths).await };
            if let Some(w) = this.upgrade() {
                w.after_git(pane, result, &if unstage { tr("Unstaged") } else { tr("Staged") });
            }
        });
    }

    pub fn git_discard_selected(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let Some(root) = self.repo_root(&pane) else { return };
        let paths = Self::selected_paths(&pane);
        if paths.is_empty() {
            return;
        }
        let n = paths.len();
        let body = format!("Local changes in {n} {} go back to the last commit. Untracked files are deleted. This cannot be undone.", if n == 1 { tr("item") } else { tr("items") });
        let dialog = adw::AlertDialog::new(Some(tr("Discard changes?").as_str()), Some(&body));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("discard", tr("Discard").as_str())]);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        dialog.choose(Some(&self.window), gtk::gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            w.refocus();
            if response != "discard" {
                return;
            }
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let result = git::discard(&root, &paths).await;
                if let Some(w) = this.upgrade() {
                    w.after_git(pane, result, &tr("Changes discarded"));
                }
            });
        });
    }

    pub fn git_commit_dialog(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let Some(repo) = pane.git.borrow().clone() else { return };
        let root = repo.root.clone();
        let subject = gtk::Entry::builder().placeholder_text(tr("Summary (required)")).activates_default(true).build();
        let body = gtk::TextView::builder().wrap_mode(gtk::WrapMode::WordChar).top_margin(6).bottom_margin(6).left_margin(8).right_margin(8).build();
        body.add_css_class("omafile-commit-body");
        let body_scroll = gtk::ScrolledWindow::builder().child(&body).min_content_height(90).has_frame(true).build();
        let stage_all = gtk::CheckButton::with_label(&tr("Stage all changes first (git add -A)"));
        stage_all.set_active(repo.staged == 0);
        let summary = gtk::Label::builder().xalign(0.0).wrap(true).build();
        summary.add_css_class("omafile-dim");
        summary.add_css_class("caption");
        summary.set_text(&format!("{}  ·  {} staged, {} modified, {} untracked", repo.branch, repo.staged, repo.modified.saturating_sub(repo.staged), repo.untracked));
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 10);
        vbox.set_width_request(420);
        vbox.append(&summary);
        vbox.append(&subject);
        vbox.append(&body_scroll);
        vbox.append(&stage_all);
        let dialog = adw::AlertDialog::new(Some(tr("Commit").as_str()), None);
        dialog.set_extra_child(Some(&vbox));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("commit", tr("Commit").as_str())]);
        dialog.set_response_appearance("commit", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("commit"));
        dialog.set_close_response("cancel");
        dialog.set_response_enabled("commit", false);
        let d2 = dialog.clone();
        subject.connect_changed(move |e| d2.set_response_enabled("commit", !e.text().trim().is_empty()));
        let this = Rc::downgrade(self);
        let subject2 = subject.clone();
        dialog.choose(Some(&self.window), gtk::gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            w.refocus();
            if response != "commit" {
                return;
            }
            let mut message = subject2.text().trim().to_string();
            let buffer = body.buffer();
            let extra = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false).trim().to_string();
            if !extra.is_empty() {
                message.push_str("\n\n");
                message.push_str(&extra);
            }
            let all = stage_all.is_active();
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let result = git::commit(&root, &message, all).await;
                let Some(w) = this.upgrade() else { return };
                match result {
                    Ok(line) => {
                        w.toast(&trf!("Committed {line}", line = line));
                        pane.reload();
                    }
                    Err(e) => w.toast_error(&trf!("git commit: {e}", e = e)),
                }
            });
        });
        subject.grab_focus();
    }

    /// Branch switcher: list of local branches plus "new branch".
    pub fn git_branch_dialog(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let Some(root) = self.repo_root(&pane) else { return };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let branches = git::branches(&root).await;
            let Some(w) = this.upgrade() else { return };
            let list = gtk::ListBox::new();
            list.add_css_class("boxed-list");
            list.set_selection_mode(gtk::SelectionMode::None);
            let dialog = adw::Dialog::builder().title(tr("Switch Branch")).content_width(380).build();
            for (name, current) in &branches {
                let row = adw::ActionRow::builder().title(glib::markup_escape_text(name)).activatable(!current).build();
                if *current {
                    let check = gtk::Image::from_icon_name("object-select-symbolic");
                    row.add_suffix(&check);
                    row.set_subtitle("current");
                } else {
                    let root = root.clone();
                    let name = name.clone();
                    let this = Rc::downgrade(&w);
                    let pane = pane.clone();
                    let dialog = dialog.clone();
                    row.connect_activated(move |_| {
                        dialog.close();
                        let (root, name, pane) = (root.clone(), name.clone(), pane.clone());
                        let this = this.clone();
                        glib::spawn_future_local(async move {
                            let result = git::switch(&root, &name, false).await;
                            if let Some(w) = this.upgrade() {
                                w.after_git(pane, result, &trf!("Switched to {name}", name = name));
                            }
                        });
                    });
                }
                list.append(&row);
            }
            let new_entry = gtk::Entry::builder().placeholder_text(tr("New branch from HEAD…")).activates_default(false).hexpand(true).build();
            let new_row = adw::ActionRow::new();
            new_row.add_prefix(&gtk::Image::from_icon_name("list-add-symbolic"));
            new_row.add_suffix(&new_entry);
            list.append(&new_row);
            {
                let root = root.clone();
                let this = Rc::downgrade(&w);
                let pane = pane.clone();
                let dialog = dialog.clone();
                new_entry.connect_activate(move |e| {
                    let name = e.text().trim().replace(' ', "-");
                    if name.is_empty() {
                        return;
                    }
                    dialog.close();
                    let (root, pane) = (root.clone(), pane.clone());
                    let this = this.clone();
                    glib::spawn_future_local(async move {
                        let result = git::switch(&root, &name, true).await;
                        if let Some(w) = this.upgrade() {
                            w.after_git(pane, result, &trf!("Created and switched to {name}", name = name));
                        }
                    });
                });
            }
            let content = adw::ToolbarView::new();
            content.add_top_bar(&adw::HeaderBar::new());
            let scroll = gtk::ScrolledWindow::builder().child(&list).propagate_natural_height(true).max_content_height(420).build();
            scroll.set_margin_start(12);
            scroll.set_margin_end(12);
            scroll.set_margin_bottom(12);
            content.set_content(Some(&scroll));
            dialog.set_child(Some(&content));
            w.present_dialog(&dialog);
        });
    }
}
