//! Telling the user about a newer omafile, and showing how to get it.
//!
//! A few seconds after the first window opens, at most once a day, omafile asks which
//! version is the newest (see `update`). When there is a newer one a notification stays
//! until it is answered; *Update…* shows what would be done. *Update Now* (also in the main
//! menu as *Update omafile…*) then does it: for an installation from source omafile fetches
//! the newest source, builds and installs it in a window of its own and offers to restart;
//! a package is updated in a terminal, where the package manager asks for the password.
//! Nothing is ever installed without that click.

use crate::help::{VERSION, WEBSITE};
use crate::i18n::tr;
use crate::update::{self, Install, Plan, Step};
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

thread_local! {
    /// One window asks, however many are opened.
    static ASKED: Cell<bool> = const { Cell::new(false) };
    /// One update at a time.
    static UPDATING: Cell<bool> = const { Cell::new(false) };
    /// The notification about a newer version, to take it away once the update runs.
    static NOTICE: std::cell::RefCell<Option<adw::Toast>> = const { std::cell::RefCell::new(None) };
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
                    NOTICE.with(|notice| notice.replace(Some(toast.clone())));
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
        let plan = update::plan(&install, update::aur_helper(), &update::own_checkout());
        let command = match &plan {
            Plan::Here(command) | Plan::Terminal(command) => Some(command.clone()),
            Plan::ByHand => None,
        };
        let how = match (&plan, &install) {
            (Plan::Here(_), _) => tr("Update Now fetches the newest source, builds and installs it, here in omafile; that takes a few minutes. This is what runs:"),
            (Plan::Terminal(_), _) => tr("This command updates this installation. Update Now runs it in a terminal; restart omafile afterwards."),
            (Plan::ByHand, Install::Package(_)) => tr("Update it with your package manager, then restart omafile."),
            (Plan::ByHand, _) => tr("Update it the way it was installed: git pull and ./install.sh in the checkout, or the package. Then restart omafile."),
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
                "update" => w.update_carry_out(&plan),
                _ => {}
            }
            w.refocus();
        });
    }

    /// Main menu › Update omafile…: ask which version is the newest and offer to install it;
    /// when this one is the newest already, offer to fetch and install the latest state all
    /// the same (a fix can arrive without a new number).
    pub fn update_now(self: &Rc<Self>) {
        self.toast(&tr("Looking for a newer version…"));
        let this = self.clone();
        glib::spawn_future_local(async move {
            match ask_now().await {
                Ok(Some(_)) => this.update_dialog(),
                Ok(None) => this.update_offer_again(),
                Err(e) => this.toast_error(&crate::trf!("Could not ask for the newest version: {error}", error = e)),
            }
        });
    }

    fn update_offer_again(self: &Rc<Self>) {
        let plan = update::plan(&update::install(), update::aur_helper(), &update::own_checkout());
        let heading = crate::trf!("omafile {version} is the newest version", version = VERSION);
        let (body, can) = match &plan {
            Plan::Here(_) => (tr("Update Anyway fetches the latest state of the source, builds and installs it again; a fix can arrive without a new version number."), true),
            Plan::Terminal(_) => (tr("Update Anyway runs the package manager in a terminal once more."), true),
            Plan::ByHand => (String::new(), false),
        };
        let dialog = adw::AlertDialog::new(Some(heading.as_str()), Some(body.as_str()));
        dialog.add_response("close", tr("Close").as_str());
        if can {
            dialog.add_response("update", tr("Update Anyway").as_str());
        }
        dialog.set_close_response("close");
        dialog.set_default_response(Some("close"));
        let this = Rc::downgrade(self);
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response == "update" {
                w.update_carry_out(&plan);
            } else {
                w.refocus();
            }
        });
    }

    fn update_carry_out(self: &Rc<Self>, plan: &Plan) {
        match plan {
            Plan::Here(script) => self.update_here(script.clone()),
            Plan::Terminal(command) => self.update_run(command),
            Plan::ByHand => {}
        }
    }

    /// Fetch, build and install, here: the output runs through a window, and when it is
    /// done omafile offers to start again as the new version.
    fn update_here(self: &Rc<Self>, script: String) {
        if UPDATING.with(|u| u.replace(true)) {
            self.toast(&tr("An update is running already"));
            return;
        }
        if let Some(notice) = NOTICE.with(|notice| notice.take()) {
            notice.dismiss();
        }
        let status = gtk::Label::builder().label(tr("Fetching the newest source…")).xalign(0.0).hexpand(true).wrap(true).css_classes(["title-4"]).build();
        let spinner = gtk::Spinner::new();
        spinner.start();
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        head.append(&spinner);
        head.append(&status);
        let log = gtk::TextView::builder().editable(false).cursor_visible(false).monospace(true).wrap_mode(gtk::WrapMode::WordChar).top_margin(8).bottom_margin(8).left_margin(10).right_margin(10).build();
        log.add_css_class("omafile-update-log");
        let scroll = gtk::ScrolledWindow::builder().child(&log).vexpand(true).min_content_height(220).build();
        scroll.add_css_class("card");
        let stop = gtk::Button::builder().label(tr("Stop")).build();
        let terminal = gtk::Button::builder().label(tr("Run in a Terminal")).visible(false).build();
        let restart = gtk::Button::builder().label(tr("Restart omafile")).visible(false).build();
        restart.add_css_class("suggested-action");
        for b in [&stop, &terminal, &restart] {
            b.add_css_class("pill");
        }
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        buttons.set_halign(gtk::Align::End);
        buttons.append(&stop);
        buttons.append(&terminal);
        buttons.append(&restart);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.set_margin_top(6);
        column.set_margin_bottom(18);
        column.set_margin_start(18);
        column.set_margin_end(18);
        column.append(&head);
        column.append(&scroll);
        column.append(&buttons);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        view.set_content(Some(&column));
        let dialog = adw::Dialog::builder().title(tr("Updating omafile")).content_width(680).content_height(460).child(&view).build();

        // A login shell, so that cargo is found the way it is in a terminal; git must not
        // stop to ask for a name or a password.
        let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE);
        launcher.setenv("GIT_TERMINAL_PROMPT", "0", true);
        // cargo from rustup lives in ~/.cargo/bin, which a shell started this way may not have.
        let path = std::env::var("PATH").unwrap_or_default();
        launcher.setenv("PATH", format!("{path}:{}", glib::home_dir().join(".cargo/bin").display()), true);
        launcher.set_cwd(glib::home_dir());
        let process = match launcher.spawn(&[std::ffi::OsStr::new("bash"), std::ffi::OsStr::new("-lc"), std::ffi::OsStr::new(&script)]) {
            Ok(process) => process,
            Err(e) => {
                UPDATING.with(|u| u.set(false));
                self.toast_error(e.message());
                return;
            }
        };
        let running = process.clone();
        stop.connect_clicked(move |_| running.force_exit());
        let this = Rc::downgrade(self);
        restart.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.update_restart();
            }
        });
        let (this, again, weak) = (Rc::downgrade(self), script.clone(), dialog.downgrade());
        terminal.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                if let Some(dialog) = weak.upgrade() {
                    dialog.close();
                }
                w.update_run(&again);
            }
        });
        dialog.present(Some(&self.window));

        let this = Rc::downgrade(self);
        let (status, spinner, log, scroll, stop, terminal, restart, dialog) = (status.downgrade(), spinner.downgrade(), log.downgrade(), scroll.downgrade(), stop.downgrade(), terminal.downgrade(), restart.downgrade(), dialog.downgrade());
        glib::spawn_future_local(async move {
            let started = std::time::Instant::now();
            if let Some(out) = process.stdout_pipe() {
                let lines = gio::DataInputStream::new(&out);
                while let Ok(Some(line)) = lines.read_line_utf8_future(glib::Priority::DEFAULT).await {
                    if let Some(log) = log.upgrade() {
                        let buffer = log.buffer();
                        buffer.insert(&mut buffer.end_iter(), &format!("{line}\n"));
                        // Keep the newest line in view.
                        if let Some(scroll) = scroll.upgrade() {
                            let adjustment = scroll.vadjustment();
                            glib::idle_add_local_once(move || adjustment.set_value(adjustment.upper()));
                        }
                    }
                    if let (Some(status), Some(step)) = (status.upgrade(), update::step_of(&line)) {
                        status.set_text(&match step {
                            Step::Fetching => tr("Fetching the newest source…"),
                            Step::Building(part) if part.is_empty() => tr("Building; this takes a few minutes…"),
                            Step::Building(part) => crate::trf!("Building {part}; this takes a few minutes…", part = part),
                            Step::Installing => tr("Installing…"),
                        });
                    }
                }
            }
            let fine = process.wait_future().await.is_ok() && process.is_successful();
            UPDATING.with(|u| u.set(false));
            if let Some(spinner) = spinner.upgrade() {
                spinner.stop();
                spinner.set_visible(false);
            }
            if let Some(stop) = stop.upgrade() {
                stop.set_visible(false);
            }
            let shown = dialog.upgrade().is_some_and(|d| d.is_mapped());
            if fine {
                if let Some(status) = status.upgrade() {
                    status.set_text(&crate::trf!("omafile is updated (it took {seconds} s). Restart it to use the new version.", seconds = started.elapsed().as_secs()));
                }
                if let Some(restart) = restart.upgrade() {
                    restart.set_visible(true);
                    restart.grab_focus();
                }
                // The window was closed meanwhile: say it where it is seen.
                if let (false, Some(w)) = (shown, this.upgrade()) {
                    let toast = adw::Toast::builder().use_markup(false).title(tr("omafile is updated. Restart it to use the new version.")).button_label(tr("Restart")).action_name("win.update-restart").timeout(0).build();
                    w.toast.add_toast(toast);
                }
            } else {
                if let Some(status) = status.upgrade() {
                    status.set_text(&tr("The update did not go through. The lines below say why; omafile keeps running as it is."));
                    status.add_css_class("error");
                }
                if let Some(terminal) = terminal.upgrade() {
                    terminal.set_visible(true);
                }
                if let (false, Some(w)) = (shown, this.upgrade()) {
                    w.toast_error(&tr("The update did not go through"));
                }
            }
        });
    }

    /// Start omafile again as the version that was just installed: a small helper waits
    /// until this one is gone (there is only ever one omafile), then starts the new one.
    pub fn update_restart(self: &Rc<Self>) {
        let target = glib::home_dir().join(".local/bin/omafile");
        let target = if target.is_file() { target } else { std::env::current_exe().unwrap_or(target) };
        let start = if glib::find_program_in_path("uwsm-app").is_some() { "exec uwsm-app -- \"$1\"" } else { "exec \"$1\"" };
        let script = format!("while kill -0 \"$0\" 2>/dev/null; do sleep 0.2; done; {start}");
        let pid = std::process::id().to_string();
        let args = [std::ffi::OsStr::new("setsid"), std::ffi::OsStr::new("sh"), std::ffi::OsStr::new("-c"), std::ffi::OsStr::new(&script), std::ffi::OsStr::new(&pid), target.as_os_str()];
        match gio::Subprocess::newv(&args, gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE) {
            Ok(_) => {
                if let Some(app) = self.window.application() {
                    app.quit();
                }
            }
            Err(e) => self.toast_error(e.message()),
        }
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
