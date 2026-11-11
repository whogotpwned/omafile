//! Earlier Versions: the dialog that lists what a file or folder looked like in the
//! snapshots, brings an older state back (as a copy, or in place with the current one
//! moved to the trash), restores what was deleted from a folder, and turns snapshots on.

use crate::i18n::tr;
use crate::versions::{self, Readiness, Store};
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn when(date: i64) -> String {
    let now = glib::DateTime::now_local().ok();
    crate::feeds::friendly_date(date, now.as_ref().map(|n| n.to_unix()).unwrap_or(0), now.as_ref().map(|n| n.utc_offset().as_seconds()).unwrap_or(0))
}

fn small_button(icon: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::builder().icon_name(icon).tooltip_text(tooltip).valign(gtk::Align::Center).build();
    b.add_css_class("flat");
    b
}

impl Window {
    /// Ctrl+Alt+H: earlier versions of the selected item, or of the folder on show.
    pub fn versions_selected(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let target = match pane.selected_items().as_slice() {
            [one] => one.file().path(),
            [] => pane.cwd().path(),
            _ => {
                self.toast(&tr("Select one file or folder to see its earlier versions"));
                return;
            }
        };
        match target {
            Some(path) => self.show_versions(&path),
            None => self.toast_error(&tr("Earlier versions exist for local files only")),
        }
    }

    pub fn show_versions(self: &Rc<Self>, path: &Path) {
        match versions::readiness(path) {
            Readiness::Ready => {
                if let Some(store) = versions::store_for(path) {
                    self.versions_dialog(&store, path);
                }
            }
            Readiness::Locked(root) => self.versions_setup(&root, true),
            Readiness::CanSetUp(root) => self.versions_setup(&root, false),
            Readiness::Unavailable(why) => self.toast_error(&match why.as_str() {
                "not-btrfs" => tr("Earlier versions need a Btrfs file system; this folder is on another one"),
                "no-snapper" => tr("Earlier versions need snapper (sudo pacman -S snapper)"),
                _ => tr("Earlier versions are not available for this folder"),
            }),
        }
    }

    /// Explain what turning versions on means, show the commands, and run them in a terminal.
    fn versions_setup(self: &Rc<Self>, root: &Path, locked: bool) {
        let user = glib::user_name().to_string_lossy().to_string();
        let script = versions::setup_script(root, &user, locked);
        let text = gtk::Label::builder().label(&script).selectable(true).xalign(0.0).wrap(true).wrap_mode(gtk::pango::WrapMode::WordChar).css_classes(["monospace", "caption"]).build();
        let scroll = gtk::ScrolledWindow::builder().child(&text).min_content_height(170).max_content_height(240).hscrollbar_policy(gtk::PolicyType::Never).build();
        scroll.add_css_class("card");
        let body = if locked {
            tr("This folder has snapshots, but your user may not read them yet. The commands below give you access; they need your password once.")
        } else {
            tr("Omarchy snapshots the system, not your files. Turn on versions and a snapshot of this folder is taken every hour: any file can then be brought back as it was, and deleted files too. Old snapshots are pruned automatically; they take space only for what has changed. The commands below need your password once.")
        };
        let dialog = adw::AlertDialog::new(Some(&format!("{} {}", tr("Turn on versions for"), crate::window::abbreviate_home(root))), Some(&body));
        dialog.set_extra_child(Some(&scroll));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("run", tr("Open Terminal and Turn On").as_str())]);
        dialog.set_response_appearance("run", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("run"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "run" {
                w.refocus();
                return;
            }
            // The script goes to a private file; the terminal shows every command as it runs.
            let path = glib::user_runtime_dir().join("omafile-versions-setup.sh");
            if std::fs::write(&path, &script).is_err() {
                w.toast_error(&tr("Cannot write the setup script"));
                return;
            }
            w.run_in_terminal(&format!("bash -x '{}'", path.display()));
        });
    }

    fn versions_dialog(self: &Rc<Self>, store: &Store, path: &Path) {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string());
        let is_dir = path.is_dir();
        let page = adw::PreferencesPage::new();
        let all = versions::snapshots(store);
        let dialog = adw::Dialog::builder().title(tr("Earlier Versions")).content_width(720).content_height(660).build();

        // ----- the item's own history
        let mut found = versions::versions_of(store, path);
        // A folder has one entry per snapshot: the newest thirty are plenty to pick from.
        found.truncate(30);
        let history = adw::PreferencesGroup::builder().title(if is_dir { tr("This folder at earlier times") } else { tr("Versions of this file") }).build();
        if found.is_empty() {
            history.add(&adw::ActionRow::builder().title(tr("Not in any snapshot yet")).subtitle(tr("It was created after the newest snapshot, or lives outside the folder that is snapshotted")).build());
        }
        for version in &found {
            let mut facts: Vec<String> = Vec::new();
            if !is_dir {
                facts.push(glib::format_size(version.size).to_string());
            }
            if is_dir {
                // The title already says when: add which snapshot it is.
                facts.push(format!("#{}", version.snapshot.number));
                if !version.snapshot.description.is_empty() {
                    facts.push(version.snapshot.description.clone());
                }
            } else {
                facts.push(format!("{} {}", tr("snapshot of"), when(version.snapshot.date)));
            }
            if version.kept_in > 1 {
                facts.push(format!("{} {}", version.kept_in, tr("snapshots")));
            }
            if version.current {
                facts.push(tr("same as now"));
            }
            let title = if is_dir { format!("{} {}", tr("As of"), when(version.snapshot.date)) } else { format!("{} {}", tr("Changed"), when(version.modified)) };
            let row = adw::ActionRow::builder().title(glib::markup_escape_text(&title)).subtitle(glib::markup_escape_text(&facts.join("  ·  "))).build();
            row.add_prefix(&gtk::Image::from_icon_name(if version.current { "emblem-ok-symbolic" } else { "document-open-recent-symbolic" }));
            let (snap_path, original) = (version.path.clone(), path.to_path_buf());
            // Look at it where it is: the snapshot is read-only, nothing can be damaged there.
            let show = small_button("folder-open-symbolic", &tr("Show this version in its snapshot (read-only)"));
            let (weak, d2, p2) = (Rc::downgrade(self), dialog.clone(), snap_path.clone());
            show.connect_clicked(move |_| {
                let Some(w) = weak.upgrade() else { return };
                d2.close();
                w.versions_show(&p2);
            });
            row.add_suffix(&show);
            if !version.current {
                let compare = small_button("edit-copy-symbolic", &tr("Compare with what is there now"));
                let (weak, a, b) = (Rc::downgrade(self), snap_path.clone(), original.clone());
                compare.connect_clicked(move |_| {
                    if let Some(w) = weak.upgrade() {
                        w.versions_compare(&a, &b);
                    }
                });
                row.add_suffix(&compare);
                if !is_dir {
                    // In a file name a full date reads better later than "Sat 16:20".
                    let stamp = version.modified;
                    let label = glib::DateTime::from_unix_local(stamp).ok().and_then(|d| d.format("%Y-%m-%d %H.%M").ok()).map(|g| g.to_string()).unwrap_or_else(|| when(stamp));
                    let copy = gtk::Button::builder().label(tr("Restore a Copy")).valign(gtk::Align::Center).tooltip_text(tr("Put this version next to the current one")).build();
                    let (weak, d2, a, b, l) = (Rc::downgrade(self), dialog.clone(), snap_path.clone(), original.clone(), label.clone());
                    copy.connect_clicked(move |_| {
                        if let Some(w) = weak.upgrade() {
                            d2.close();
                            w.versions_restore(a.clone(), versions::copy_name(&b, &l), None);
                        }
                    });
                    row.add_suffix(&copy);
                    let replace = gtk::Button::builder().label(tr("Restore")).valign(gtk::Align::Center).tooltip_text(tr("Put this version in place; the current one goes to the trash")).build();
                    replace.add_css_class("suggested-action");
                    let (weak, d2, a, b) = (Rc::downgrade(self), dialog.clone(), snap_path.clone(), original.clone());
                    replace.connect_clicked(move |_| {
                        if let Some(w) = weak.upgrade() {
                            d2.close();
                            w.versions_restore(a.clone(), b.clone(), Some(b.clone()));
                        }
                    });
                    row.add_suffix(&replace);
                }
            }
            history.add(&row);
        }
        page.add(&history);

        // ----- what is gone from a folder
        if is_dir {
            let gone = versions::deleted_in(store, path);
            let deleted = adw::PreferencesGroup::builder().title(tr("Deleted from this folder")).description(if gone.is_empty() { tr("Nothing that was here in a snapshot is missing now.") } else { tr("These were here when a snapshot was taken and are gone now.") }).build();
            for item in gone.iter().take(200) {
                let mut facts = vec![format!("{} {}", tr("last seen in the snapshot of"), when(item.snapshot.date))];
                if !item.is_dir {
                    facts.push(glib::format_size(item.size).to_string());
                }
                let row = adw::ActionRow::builder().title(glib::markup_escape_text(&item.name)).subtitle(glib::markup_escape_text(&facts.join("  ·  "))).build();
                row.add_prefix(&gtk::Image::from_icon_name(if item.is_dir { "folder-symbolic" } else { "text-x-generic-symbolic" }));
                let show = small_button("folder-open-symbolic", &tr("Show this version in its snapshot (read-only)"));
                let (weak, d2, p2) = (Rc::downgrade(self), dialog.clone(), item.path.clone());
                show.connect_clicked(move |_| {
                    let Some(w) = weak.upgrade() else { return };
                    d2.close();
                    w.versions_show(&p2);
                });
                row.add_suffix(&show);
                let restore = gtk::Button::builder().label(tr("Restore")).valign(gtk::Align::Center).tooltip_text(tr("Bring it back into this folder")).build();
                restore.add_css_class("suggested-action");
                let (weak, d2, a, b) = (Rc::downgrade(self), dialog.clone(), item.path.clone(), path.join(&item.name));
                restore.connect_clicked(move |_| {
                    if let Some(w) = weak.upgrade() {
                        d2.close();
                        w.versions_restore(a.clone(), b.clone(), None);
                    }
                });
                row.add_suffix(&restore);
                deleted.add(&row);
            }
            page.add(&deleted);
        }

        // ----- the snapshots themselves
        let about = adw::PreferencesGroup::builder().title(tr("Snapshots")).build();
        let span = match (all.last(), all.first()) {
            (Some(oldest), Some(newest)) => format!("{} {}  ·  {} {}  ·  {} {}", all.len(), tr("snapshots"), tr("oldest"), when(oldest.date), tr("newest"), when(newest.date)),
            _ => tr("No snapshot has been taken yet; the first one comes within the hour"),
        };
        let row = adw::ActionRow::builder().title(glib::markup_escape_text(&span)).subtitle(glib::markup_escape_text(&format!("{}  ·  {}", crate::window::abbreviate_home(&store.root), tr("taken hourly by snapper, pruned automatically")))).build();
        let now_btn = gtk::Button::builder().label(tr("Snapshot Now")).valign(gtk::Align::Center).tooltip_text(tr("Take a snapshot right away, for instance before a risky change")).build();
        let (weak, d2, root) = (Rc::downgrade(self), dialog.clone(), store.root.clone());
        now_btn.connect_clicked(move |_| {
            if let Some(w) = weak.upgrade() {
                d2.close();
                w.versions_snapshot_now(&root);
            }
        });
        row.add_suffix(&now_btn);
        about.add(&row);
        page.add(&about);

        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&adw::WindowTitle::new(&tr("Earlier Versions"), &name)));
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&page));
        dialog.set_child(Some(&view));
        self.present_dialog(&dialog);
    }

    /// Go to a file inside a snapshot and select it.
    fn versions_show(self: &Rc<Self>, inside: &Path) {
        if inside.is_dir() {
            self.navigate(gio::File::for_path(inside));
        } else if let Some(parent) = inside.parent() {
            self.navigate(gio::File::for_path(parent));
            if let Some(pane) = self.active_pane() {
                pane.select_file_later(gio::File::for_path(inside));
            }
        }
        self.toast(&tr("You are inside a snapshot: everything here is read-only"));
    }

    fn versions_compare(self: &Rc<Self>, then: &Path, now: &Path) {
        if !crate::compare::available() {
            self.toast_error(&tr("diff (diffutils) is not installed"));
            return;
        }
        if !now.exists() {
            self.toast(&tr("There is nothing to compare with: it does not exist any more"));
            return;
        }
        let (this, a, b) = (Rc::downgrade(self), then.to_path_buf(), now.to_path_buf());
        glib::spawn_future_local(async move {
            let Some(w) = this.upgrade() else { return };
            if a.is_dir() && b.is_dir() {
                if let Some(mut d) = crate::compare::dirs(&a, &b).await {
                    // The snapshots' own folder is not part of the comparison.
                    d.only_left.retain(|e| !e.starts_with(".snapshots"));
                    d.only_right.retain(|e| !e.starts_with(".snapshots"));
                    w.show_dir_compare(&a, &b, d);
                }
            } else if let Some(diff) = crate::compare::files(&a, &b).await {
                w.show_file_compare(&a, &b, &diff);
            }
        });
    }

    /// Copy `from` (inside a snapshot) to `to`. `replace`: what is at that place now goes to
    /// the trash first, so the step can be taken back.
    fn versions_restore(self: &Rc<Self>, from: PathBuf, to: PathBuf, replace: Option<PathBuf>) {
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            if let Some(current) = replace.as_ref().filter(|c| c.exists()) {
                if let Err(e) = crate::ops::trash(gio::File::for_path(current)).await {
                    if let Some(w) = this.upgrade() {
                        w.toast_error(&format!("{}: {}", tr("Cannot move the current version to the trash"), e.message()));
                    }
                    return;
                }
            }
            let result = crate::ops::copy_recursive(gio::File::for_path(&from), gio::File::for_path(&to)).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(()) => {
                    if let (Some(pane), Some(parent)) = (w.active_pane(), to.parent()) {
                        if pane.cwd().path().as_deref() == Some(parent) {
                            pane.reload();
                            pane.select_file_later(gio::File::for_path(&to));
                        }
                    }
                    w.toast(&if replace.is_some() { tr("Restored; what was there before is in the trash") } else { format!("{} {}", tr("Restored as"), to.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()) });
                }
                Err(e) => w.toast_error(&format!("{}: {}", tr("Cannot restore"), e.message())),
            }
        });
    }

    fn versions_snapshot_now(self: &Rc<Self>, root: &Path) {
        let config = versions::config_name(root);
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let done = gio::spawn_blocking(move || std::process::Command::new("snapper").args(["-c", &config, "create", "--description", "omafile"]).stdin(std::process::Stdio::null()).output().map(|o| o.status.success()).unwrap_or(false)).await.unwrap_or(false);
            if let Some(w) = this.upgrade() {
                if done {
                    w.toast(&tr("Snapshot taken"));
                } else {
                    w.toast_error(&tr("Could not take a snapshot (is snapper set up for this folder?)"));
                }
            }
        });
    }
}
