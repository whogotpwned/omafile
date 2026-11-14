//! Telling the user about a newer omafile, and showing how to get it.
//!
//! A few seconds after the first window opens, at most once a day, omafile asks which
//! version is the newest (see `update`). When there is a newer one a notification stays
//! until it is answered; *Update…* shows the command for this installation and starts it
//! in a terminal on request. Nothing is ever installed without that click.

use crate::help::{VERSION, WEBSITE};
use crate::i18n::tr;
use crate::update::{self, Install};
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

thread_local! {
    /// One window asks, however many are opened.
    static ASKED: Cell<bool> = const { Cell::new(false) };
}

/// Ask GitHub now and remember the answer. The newer version, if there is one.
pub async fn ask_now() -> Result<Option<String>, String> {
    let answer = gio::spawn_blocking(|| update::ask(&update::install())).await.map_err(|_| tr("The question could not be asked"))??;
    let mut state = update::load_state();
    state.checked = update::now();
    if let Some(latest) = answer {
        state.latest = latest;
    }
    update::save_state(&state);
    Ok(state.available(VERSION))
}

impl Window {
    /// The daily look for a newer version: quietly, and only if Preferences allows it.
    pub fn start_update_check(self: &Rc<Self>) {
        if std::env::var_os("OMAFILE_SELFTEST").is_some() || !self.prefs.get_bool("check-updates", true) || ASKED.with(|asked| asked.replace(true)) {
            return;
        }
        let this = Rc::downgrade(self);
        // Not in the first seconds: the window comes first.
        glib::timeout_add_local_once(Duration::from_secs(6), move || {
            glib::spawn_future_local(async move {
                if update::load_state().due(update::now()) {
                    // No network, no answer: nothing is said, and the next start asks again.
                    let _ = ask_now().await;
                }
                let mut state = update::load_state();
                let now = update::now();
                if let (Some(w), Some(latest)) = (this.upgrade(), state.to_tell(VERSION, now)) {
                    state.told = now;
                    update::save_state(&state);
                    let toast = adw::Toast::builder().use_markup(false).title(crate::trf!("omafile {version} is available", version = latest)).button_label(tr("Update…")).action_name("win.update-show").timeout(0).build();
                    w.toast.add_toast(toast);
                }
            });
        });
    }

    /// Palette › Check for Updates: ask now, whatever Preferences says, and answer either way.
    pub fn check_update_now(self: &Rc<Self>) {
        self.toast(&tr("Looking for a newer version…"));
        let this = self.clone();
        glib::spawn_future_local(async move {
            match ask_now().await {
                Ok(Some(_)) => this.update_dialog(),
                Ok(None) => this.toast(&crate::trf!("omafile {version} is the newest version", version = VERSION)),
                Err(e) => this.toast_error(&crate::trf!("Could not ask for the newest version: {error}", error = e)),
            }
        });
    }

    /// What the newer version is and how this installation gets it.
    pub fn update_dialog(self: &Rc<Self>) {
        let Some(latest) = update::load_state().available(VERSION) else {
            self.toast(&crate::trf!("omafile {version} is the newest version", version = VERSION));
            return;
        };
        let install = update::install();
        let command = update::update_command(&install, update::aur_helper());
        let how = match (&command, &install) {
            (Some(_), _) => tr("This command updates this installation. Update Now runs it in a terminal; restart omafile afterwards."),
            (None, Install::Package(_)) => tr("Update it with your package manager, then restart omafile."),
            (None, _) => tr("Update it the way it was installed: git pull and ./install.sh in the checkout, or the package. Then restart omafile."),
        };
        let body = format!("{} {how}", crate::trf!("You have {version}.", version = VERSION));
        let dialog = adw::AlertDialog::new(Some(crate::trf!("omafile {version} is available", version = latest).as_str()), Some(body.as_str()));
        if let Some(command) = &command {
            let line = gtk::Label::builder().label(command).selectable(true).wrap(true).wrap_mode(gtk::pango::WrapMode::WordChar).xalign(0.0).hexpand(true).css_classes(["monospace"]).build();
            let copy = gtk::Button::builder().icon_name("edit-copy-symbolic").tooltip_text(tr("Copy the command")).valign(gtk::Align::Center).build();
            copy.add_css_class("flat");
            let (this, text) = (Rc::downgrade(self), command.clone());
            copy.connect_clicked(move |_| {
                if let Some(w) = this.upgrade() {
                    w.window.clipboard().set_text(&text);
                }
            });
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.add_css_class("card");
            row.add_css_class("omafile-update-command");
            // The dialog takes its width from this: wide enough for the command to read well.
            // (Setting a content width on an alert dialog crashes libadwaita 1.5 when it is shown.)
            row.set_width_request(440);
            row.append(&line);
            row.append(&copy);
            dialog.set_extra_child(Some(&row));
        }
        dialog.add_responses(&[("later", tr("Later").as_str()), ("skip", tr("Skip This Version").as_str()), ("news", tr("What Is New").as_str())]);
        if command.is_some() {
            dialog.add_response("update", tr("Update Now").as_str());
            dialog.set_response_appearance("update", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("update"));
        }
        dialog.set_close_response("later");
        let this = Rc::downgrade(self);
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            match response.as_str() {
                "skip" => {
                    let mut state = update::load_state();
                    state.skipped = latest.clone();
                    update::save_state(&state);
                    w.toast(&tr("Skipped. The version after it will be announced again."));
                }
                "news" => gtk::UriLauncher::new(&format!("{WEBSITE}/blob/main/CHANGELOG.md")).launch(Some(&w.window), gio::Cancellable::NONE, |_| {}),
                "update" => {
                    if let Some(command) = &command {
                        w.update_run(command);
                    }
                }
                _ => {}
            }
            w.refocus();
        });
    }

    /// Start the update command in a terminal, in the home folder: the user sees what
    /// happens and types a password there if one is asked for.
    fn update_run(self: &Rc<Self>, command: &str) {
        let home = glib::home_dir();
        let argv = crate::projects::terminal_argv(&home, command);
        let args: Vec<&std::path::Path> = argv.iter().map(std::path::Path::new).collect();
        match glib::spawn_async(Some(home.as_path()), &args, &[], glib::SpawnFlags::SEARCH_PATH | glib::SpawnFlags::STDOUT_TO_DEV_NULL | glib::SpawnFlags::STDERR_TO_DEV_NULL, None) {
            Ok(_) => self.toast(&tr("The update runs in a terminal. Restart omafile when it is done.")),
            Err(e) => self.toast_error(e.message()),
        }
    }
}
