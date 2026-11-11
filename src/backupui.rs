//! The Backup page and its row in the sidebar: when the last backup ran, the ones before
//! it, where they go, what is included and how often. The work itself is done by
//! `omafile --backup` in a process of its own (see `backup`); this page starts it, watches
//! its status file and shows what it says.

use crate::backup::{self, Config, Destination, Schedule, Status};
use crate::i18n::tr;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

fn now() -> i64 {
    glib::DateTime::now_utc().map(|d| d.to_unix()).unwrap_or(0)
}

/// "just now", "12 min ago", "3 h ago", "yesterday", "5 days ago".
pub fn ago(then: i64) -> String {
    let seconds = (now() - then).max(0);
    if seconds < 90 {
        tr("just now")
    } else if seconds < 3600 {
        trf!("{n} min ago", n = seconds / 60)
    } else if seconds < 86400 {
        trf!("{n} h ago", n = seconds / 3600)
    } else if seconds < 2 * 86400 {
        tr("yesterday")
    } else {
        trf!("{n} days ago", n = seconds / 86400)
    }
}

fn clock_time(unix: i64) -> String {
    let now = glib::DateTime::now_local().ok();
    crate::feeds::friendly_date(unix, now.as_ref().map(|n| n.to_unix()).unwrap_or(0), now.as_ref().map(|n| n.utc_offset().as_seconds()).unwrap_or(0))
}

/// What the sidebar row and the page's headline say, and how urgent it is.
#[derive(Clone, Copy, PartialEq)]
pub enum Mood {
    Good,
    Busy,
    Late,
    Bad,
    Unset,
}

pub fn summary(config: &Config, status: &Status) -> (String, Mood) {
    if !config.is_set_up() {
        return (tr("Set Up Backup…"), Mood::Unset);
    }
    if status.state == "running" {
        let percent = (status.files_done * 100).checked_div(status.files_total).unwrap_or(0);
        return (trf!("Backing up… {n} %", n = percent.min(100)), Mood::Busy);
    }
    let last = backup::last_success();
    if last == 0 {
        return (tr("Backup  ·  never"), if status.state == "failed" { Mood::Bad } else { Mood::Late });
    }
    // Late: three intervals have passed, or a week when backups are started by hand.
    let patience = config.schedule.interval().map(|i| i * 3).unwrap_or(7 * 86400);
    let mood = if status.state == "failed" && status.finished > last {
        Mood::Bad
    } else if now() - last > patience {
        Mood::Late
    } else {
        Mood::Good
    };
    // Rounded to ten minutes: this text is in the sidebar, which should not change every minute.
    let age = now() - last;
    let when = if (600..3600).contains(&age) { trf!("{n} min ago", n = age / 600 * 10) } else { ago(last) };
    (format!("{}  ·  {}", tr("Backup"), when), mood)
}

/// Start `omafile --backup` as a child that is reaped when it ends.
fn spawn_backup(extra: &[&str]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut argv: Vec<&std::ffi::OsStr> = vec![exe.as_os_str(), std::ffi::OsStr::new("--backup")];
    argv.extend(extra.iter().map(std::ffi::OsStr::new));
    gio::Subprocess::newv(&argv, gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE).map(|_| ()).map_err(|e| e.message().to_string())
}

pub struct BackupPage {
    pub root: gtk::Box,
    column: gtk::Box,
    icon: gtk::DrawingArea,
    headline: gtk::Label,
    detail: gtk::Label,
    progress: gtk::ProgressBar,
    current: gtk::Label,
    start: gtk::Button,
    stop: gtk::Button,
    browse: gtk::Button,
    /// Everything below the headline, rebuilt when something changes.
    groups: gtk::Box,
    mood: Cell<Mood>,
    was_running: Cell<bool>,
    spinning_since: Cell<Instant>,
    ticking: Cell<bool>,
    accent: RefCell<[(f64, f64, f64); 4]>,
}

impl BackupPage {
    fn new(window: &Rc<Window>) -> Rc<Self> {
        let back = gtk::Button::builder()
            .child(&{
                let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                b.append(&gtk::Image::from_icon_name("go-previous-symbolic"));
                b.append(&gtk::Label::new(Some(&tr("Files"))));
                b
            })
            .tooltip_text(tr("Back to the files (Esc)"))
            .halign(gtk::Align::Start)
            .margin_top(10)
            .margin_start(12)
            .build();
        back.add_css_class("flat");

        let icon = gtk::DrawingArea::new();
        icon.set_content_width(150);
        icon.set_content_height(150);
        icon.set_halign(gtk::Align::Center);
        let headline = gtk::Label::builder().css_classes(["omafile-vault-title"]).wrap(true).justify(gtk::Justification::Center).build();
        let detail = gtk::Label::builder().css_classes(["omafile-dim"]).wrap(true).justify(gtk::Justification::Center).build();
        let progress = gtk::ProgressBar::new();
        progress.set_size_request(360, -1);
        progress.set_halign(gtk::Align::Center);
        progress.set_margin_top(8);
        let current = gtk::Label::builder().css_classes(["omafile-dim", "caption"]).ellipsize(gtk::pango::EllipsizeMode::Middle).max_width_chars(70).build();
        let start = gtk::Button::builder().label(tr("Back Up Now")).build();
        start.add_css_class("suggested-action");
        start.add_css_class("pill");
        let stop = gtk::Button::builder().label(tr("Stop")).build();
        stop.add_css_class("pill");
        let browse = gtk::Button::builder().label(tr("Browse Backups")).tooltip_text(tr("Open the backups on the drive; every one is a complete folder tree")).build();
        browse.add_css_class("pill");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::Center);
        buttons.set_margin_top(12);
        buttons.append(&start);
        buttons.append(&stop);
        buttons.append(&browse);
        let groups = gtk::Box::new(gtk::Orientation::Vertical, 22);
        groups.set_margin_top(18);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
        column.set_margin_top(4);
        column.set_margin_bottom(28);
        column.set_margin_start(16);
        column.set_margin_end(16);
        for w in [icon.upcast_ref::<gtk::Widget>(), headline.upcast_ref(), detail.upcast_ref(), progress.upcast_ref(), current.upcast_ref(), buttons.upcast_ref(), groups.upcast_ref()] {
            column.append(w);
        }
        let clamp = adw::Clamp::builder().maximum_size(700).child(&column).build();
        let scroll = gtk::ScrolledWindow::builder().child(&clamp).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("omafile-vault-page");
        root.append(&back);
        root.append(&scroll);

        let page = Rc::new(BackupPage { root, column, icon, headline, detail, progress, current, start, stop, browse, groups, mood: Cell::new(Mood::Unset), was_running: Cell::new(false), spinning_since: Cell::new(Instant::now()), ticking: Cell::new(false), accent: RefCell::new([(0.5, 0.6, 1.0); 4]) });

        let weak = Rc::downgrade(&page);
        page.icon.set_draw_func(move |_, cr, w, h| {
            if let Some(p) = weak.upgrade() {
                p.draw_icon(cr, w as f64, h as f64);
            }
        });
        let wweak = Rc::downgrade(window);
        back.connect_clicked(move |_| {
            if let Some(w) = wweak.upgrade() {
                w.show_files();
                w.refocus();
            }
        });
        let wweak = Rc::downgrade(window);
        page.start.connect_clicked(move |_| {
            if let Some(w) = wweak.upgrade() {
                w.backup_now();
            }
        });
        page.stop.connect_clicked(|_| backup::request_stop());
        let wweak = Rc::downgrade(window);
        page.browse.connect_clicked(move |_| {
            let Some(w) = wweak.upgrade() else { return };
            if let Destination::Ready(dir) = backup::destination(&Config::load()) {
                w.navigate(gio::File::for_path(backup::store_dir(&dir).join(".snapshots")));
                w.toast(&tr("Every numbered folder is one backup; look into its “snapshot” folder. Change nothing here."));
            }
        });
        let keys = gtk::EventControllerKey::new();
        let wweak = Rc::downgrade(window);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(w) = wweak.upgrade() {
                    w.show_files();
                    w.refocus();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        page.root.add_controller(keys);
        page.root.set_focusable(true);
        let _ = &page.column;
        page
    }

    /// The clock with the arrow running backwards around it; it turns while a backup runs.
    fn draw_icon(&self, cr: &gtk::cairo::Context, w: f64, h: f64) {
        let [accent, green, orange, red] = *self.accent.borrow();
        let (r, g, b) = match self.mood.get() {
            Mood::Good => green,
            Mood::Busy | Mood::Unset => accent,
            Mood::Late => orange,
            Mood::Bad => red,
        };
        let pi = std::f64::consts::PI;
        let (cx, cy, radius) = (w / 2.0, h / 2.0, w.min(h) * 0.34);
        let turn = if self.mood.get() == Mood::Busy { -self.spinning_since.get().elapsed().as_secs_f64() * 1.6 } else { 0.0 };
        // A soft halo.
        for i in (1..=8).rev() {
            cr.arc(cx, cy, radius + 8.0 + i as f64 * 2.5, 0.0, 2.0 * pi);
            cr.set_source_rgba(r, g, b, 0.02);
            let _ = cr.fill();
        }
        // The arrow: most of a circle, with a head at its end.
        let (from, to) = (turn - 0.25 * pi, turn - 1.75 * pi);
        cr.set_line_width(radius * 0.17);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_source_rgb(r, g, b);
        cr.arc_negative(cx, cy, radius, from, to);
        let _ = cr.stroke();
        let (hx, hy) = (cx + radius * to.cos(), cy + radius * to.sin());
        let dir = to - pi / 2.0; // the way the arrow travels there
        let size = radius * 0.34;
        cr.move_to(hx + size * dir.cos(), hy + size * dir.sin());
        cr.line_to(hx + size * 0.75 * (dir + 2.2).cos(), hy + size * 0.75 * (dir + 2.2).sin());
        cr.line_to(hx + size * 0.75 * (dir - 2.2).cos(), hy + size * 0.75 * (dir - 2.2).sin());
        cr.close_path();
        let _ = cr.fill();
        // The hands.
        cr.set_line_width(radius * 0.13);
        cr.move_to(cx, cy - radius * 0.52);
        cr.line_to(cx, cy);
        cr.line_to(cx + radius * 0.36, cy + radius * 0.2);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        let _ = cr.stroke();
    }

    fn spin(self: &Rc<Self>) {
        if self.ticking.replace(true) {
            return;
        }
        self.spinning_since.set(Instant::now());
        let weak = Rc::downgrade(self);
        self.icon.add_tick_callback(move |area, _| {
            let Some(p) = weak.upgrade() else { return glib::ControlFlow::Break };
            area.queue_draw();
            if p.mood.get() == Mood::Busy {
                glib::ControlFlow::Continue
            } else {
                p.ticking.set(false);
                glib::ControlFlow::Break
            }
        });
    }

    /// The headline, the progress and the buttons, from the status file.
    fn update_status(self: &Rc<Self>, window: &Rc<Window>) -> bool {
        let config = Config::load();
        let status = Status::load();
        let (_, mood) = summary(&config, &status);
        self.mood.set(mood);
        let p = window.theme.palette();
        let c = |c: crate::theme::Rgb| (c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0);
        self.accent.replace([c(p.accent), c(p.green), c(p.orange), c(p.red)]);
        let running = status.state == "running";
        let last = backup::last_success();
        let connected = matches!(backup::destination(&config), Destination::Ready(_));
        let headline = if !config.is_set_up() {
            tr("No backup yet")
        } else if running {
            tr("Backing up…")
        } else if last == 0 {
            tr("No backup has been made yet")
        } else {
            format!("{} {}", tr("Last backup"), ago(last))
        };
        self.headline.set_text(&headline);
        let detail = if !config.is_set_up() {
            tr("Choose a drive below. omafile then keeps complete copies of your folders there: every backup can be opened like a normal folder, and unchanged files take no extra space.")
        } else if running {
            format!("{} / {}  ·  {}", status.files_done, status.files_total.max(status.files_done), glib::format_size(status.bytes_copied))
        } else {
            let mut parts: Vec<String> = Vec::new();
            if last > 0 {
                parts.push(clock_time(last));
            }
            if status.state == "failed" && status.finished > last {
                parts.push(format!("{}: {}", tr("the last attempt failed"), status.message));
            }
            if !connected {
                parts.push(tr("the drive is not connected"));
            } else if let Some(interval) = config.schedule.interval() {
                let next = (last + interval - now()).max(0);
                parts.push(if next < 120 { tr("next backup: soon") } else if next < 7200 { trf!("next backup in {n} min", n = next / 60) } else { trf!("next backup in {n} h", n = next / 3600) });
            } else {
                parts.push(tr("automatic backups are off"));
            }
            parts.join("  ·  ")
        };
        self.detail.set_text(&detail);
        self.progress.set_visible(running);
        self.current.set_visible(running);
        if running {
            self.progress.set_fraction(if status.files_total > 0 { (status.files_done as f64 / status.files_total as f64).min(1.0) } else { 0.0 });
            self.current.set_text(&status.message);
            self.spin();
        }
        self.start.set_visible(!running);
        self.start.set_sensitive(config.is_set_up() && connected);
        self.stop.set_visible(running);
        self.browse.set_visible(!running && connected && config.is_set_up());
        self.icon.queue_draw();
        let finished_now = self.was_running.replace(running) && !running;
        if finished_now {
            window.toast(&if status.state == "ok" { tr("Backup finished") } else if status.message == "stopped" { tr("Backup stopped; the next run continues where it left off") } else { format!("{}: {}", tr("Backup failed"), status.message) });
        }
        finished_now
    }

    /// The groups below: destination, recent backups, what is included, how often.
    fn rebuild(self: &Rc<Self>, window: &Rc<Window>) {
        while let Some(child) = self.groups.first_child() {
            self.groups.remove(&child);
        }
        let config = Config::load();
        let dest = backup::destination(&config);

        // ----- where
        let place = adw::PreferencesGroup::builder().title(tr("Backup drive")).build();
        let row = adw::ActionRow::new();
        match &dest {
            Destination::NotSetUp => {
                row.set_title(&tr("No drive chosen"));
                row.set_subtitle(&tr("An external disk or a USB stick is best: a backup on the same disk is lost with it"));
            }
            Destination::NotConnected => {
                row.set_title(&glib::markup_escape_text(&config.dest.to_string_lossy()));
                row.set_subtitle(&tr("Not connected. Plug the drive in and the next backup starts by itself."));
            }
            Destination::Ready(dir) => {
                row.set_title(&glib::markup_escape_text(&crate::window::abbreviate_home(dir)));
                let mut facts: Vec<String> = Vec::new();
                if let Some((_, _, fstype, _)) = backup::mount_info(dir) {
                    facts.push(fstype);
                }
                if let Some((free, total)) = backup::space(dir) {
                    facts.push(format!("{} {} {}", glib::format_size(free), tr("free of"), glib::format_size(total)));
                }
                row.set_subtitle(&glib::markup_escape_text(&facts.join("  ·  ")));
            }
        }
        row.add_prefix(&gtk::Image::from_icon_name("drive-harddisk-symbolic"));
        let choose = gtk::Button::builder().label(tr("Choose…")).valign(gtk::Align::Center).build();
        let (weak, wweak) = (Rc::downgrade(self), Rc::downgrade(window));
        choose.connect_clicked(move |_| {
            let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) else { return };
            let chooser = gtk::FileDialog::builder().title(tr("Where to keep the backups")).modal(true).build();
            let (weak, wweak) = (Rc::downgrade(&p), Rc::downgrade(&w));
            chooser.select_folder(Some(&w.window), gio::Cancellable::NONE, move |result| {
                let (Some(p), Some(w), Some(dir)) = (weak.upgrade(), wweak.upgrade(), result.ok().and_then(|f| f.path())) else { return };
                w.backup_set_destination(&dir);
                p.update_status(&w);
                p.rebuild(&w);
            });
        });
        row.add_suffix(&choose);
        place.add(&row);
        if let Destination::Ready(dir) = &dest {
            // Two things worth knowing about a destination before trusting it.
            let home_fs = backup::mount_info(&glib::home_dir()).map(|m| m.3);
            let dest_fs = backup::mount_info(dir).map(|m| m.3);
            if home_fs.is_some() && home_fs == dest_fs {
                let warn = adw::ActionRow::builder().title(tr("This folder is on the same disk as your files")).subtitle(tr("It protects against deleting and overwriting, not against the disk failing or the laptop being lost")).build();
                warn.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
                place.add(&warn);
            }
            if !backup::supports_hard_links(dir) {
                let warn = adw::ActionRow::builder().title(tr("This drive cannot share unchanged files between backups")).subtitle(tr("It is formatted as FAT or exFAT: every backup is a full copy. Formatting it as ext4 or Btrfs makes backups small and many")).build();
                warn.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
                place.add(&warn);
            }
        }
        self.groups.append(&place);

        // ----- the last ones
        let recent = adw::PreferencesGroup::builder().title(tr("Recent backups")).build();
        let store = match &dest {
            Destination::Ready(dir) => Some(backup::store_dir(dir)),
            _ => None,
        };
        let on_drive = store.as_ref().map(|s| backup::entries(s)).unwrap_or_default();
        let journal = backup::journal();
        if on_drive.is_empty() && journal.is_empty() {
            recent.add(&adw::ActionRow::builder().title(tr("None yet")).build());
        }
        // Attempts that failed since the newest backup: worth seeing first.
        let newest = on_drive.first().map(|e| e.date).unwrap_or(0);
        for record in journal.iter().filter(|r| !r.ok && r.time > newest && r.message != "stopped").take(3) {
            let row = adw::ActionRow::builder().title(glib::markup_escape_text(&format!("{}  ·  {}", clock_time(record.time), tr("failed")))).subtitle(glib::markup_escape_text(&record.message)).build();
            let mark = gtk::Image::from_icon_name("dialog-warning-symbolic");
            mark.add_css_class("error");
            row.add_prefix(&mark);
            recent.add(&row);
        }
        let facts = |files: u64, changed: u64, bytes: u64, seconds: u64| format!("{} {}  ·  {} {}  ·  {} {}  ·  {} s", files, tr("files"), changed, tr("new or changed"), glib::format_size(bytes), tr("copied"), seconds);
        if store.is_some() {
            for entry in on_drive.iter().take(15) {
                let row = adw::ActionRow::builder().title(glib::markup_escape_text(&format!("{}  ·  {}", clock_time(entry.date), ago(entry.date)))).subtitle(glib::markup_escape_text(&facts(entry.files, entry.changed, entry.bytes, entry.seconds))).build();
                let mark = gtk::Image::from_icon_name("emblem-ok-symbolic");
                mark.add_css_class("omafile-help-found");
                row.add_prefix(&mark);
                let open = gtk::Button::builder().icon_name("folder-open-symbolic").tooltip_text(tr("Look into this backup")).valign(gtk::Align::Center).build();
                open.add_css_class("flat");
                let (wweak, dir) = (Rc::downgrade(window), entry.dir.clone());
                open.connect_clicked(move |_| {
                    if let Some(w) = wweak.upgrade() {
                        // Straight to the home folder inside the backup when it is there.
                        let home_inside = dir.join(glib::home_dir().strip_prefix("/").unwrap_or(Path::new("")));
                        w.navigate(gio::File::for_path(if home_inside.is_dir() { home_inside } else { dir.clone() }));
                        w.toast(&tr("You are inside a backup. Copy out what you need; change nothing here."));
                    }
                });
                row.add_suffix(&open);
                let delete = gtk::Button::builder().icon_name("user-trash-symbolic").tooltip_text(tr("Delete this backup")).valign(gtk::Align::Center).build();
                delete.add_css_class("flat");
                let (weak, wweak, dir) = (Rc::downgrade(self), Rc::downgrade(window), entry.dir.clone());
                delete.connect_clicked(move |_| {
                    let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) else { return };
                    w.backup_delete(&p, dir.clone());
                });
                row.add_suffix(&delete);
                recent.add(&row);
            }
        } else {
            for record in journal.iter().filter(|r| r.ok).take(15) {
                let row = adw::ActionRow::builder().title(glib::markup_escape_text(&format!("{}  ·  {}", clock_time(record.time), ago(record.time)))).subtitle(glib::markup_escape_text(&facts(record.files, record.changed, record.bytes, record.seconds))).build();
                let mark = gtk::Image::from_icon_name("emblem-ok-symbolic");
                mark.add_css_class("omafile-help-found");
                row.add_prefix(&mark);
                recent.add(&row);
            }
        }
        self.groups.append(&recent);

        // ----- what
        let what = adw::PreferencesGroup::builder().title(tr("What is backed up")).build();
        for source in &config.sources {
            let row = adw::ActionRow::builder().title(glib::markup_escape_text(&crate::window::abbreviate_home(source))).build();
            row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
            let remove = gtk::Button::builder().icon_name("list-remove-symbolic").tooltip_text(tr("Leave this folder out from now on")).valign(gtk::Align::Center).build();
            remove.add_css_class("flat");
            let (weak, wweak, source) = (Rc::downgrade(self), Rc::downgrade(window), source.clone());
            remove.connect_clicked(move |_| {
                let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) else { return };
                let mut config = Config::load();
                config.sources.retain(|s| *s != source);
                config.save();
                p.update_status(&w);
                p.rebuild(&w);
                w.refresh_sidebar();
            });
            row.add_suffix(&remove);
            what.add(&row);
        }
        let add = adw::ActionRow::builder().title(tr("Add Folder…")).activatable(true).build();
        add.add_prefix(&gtk::Image::from_icon_name("list-add-symbolic"));
        let (weak, wweak) = (Rc::downgrade(self), Rc::downgrade(window));
        add.connect_activated(move |_| {
            let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) else { return };
            let chooser = gtk::FileDialog::builder().title(tr("Folder to back up")).modal(true).build();
            chooser.set_initial_folder(Some(&gio::File::for_path(glib::home_dir())));
            let (weak, wweak) = (Rc::downgrade(&p), Rc::downgrade(&w));
            chooser.select_folder(Some(&w.window), gio::Cancellable::NONE, move |result| {
                let (Some(p), Some(w), Some(dir)) = (weak.upgrade(), wweak.upgrade(), result.ok().and_then(|f| f.path())) else { return };
                let mut config = Config::load();
                if !config.sources.contains(&dir) {
                    config.sources.push(dir);
                    config.save();
                }
                p.update_status(&w);
                p.rebuild(&w);
                w.refresh_sidebar();
            });
        });
        what.add(&add);
        let caches = adw::SwitchRow::builder().title(tr("Leave out caches, the trash and build folders")).subtitle(tr(".cache, node_modules, the trash, and folders a program marked as a cache (like Rust's target)")).active(config.skip_caches).build();
        caches.connect_active_notify(|row| {
            let mut config = Config::load();
            config.skip_caches = row.is_active();
            config.save();
        });
        what.add(&caches);
        let excludes = adw::EntryRow::builder().title(tr("Also leave out (names with *, or folders; separated by commas)")).text(config.excludes.join(", ")).build();
        excludes.connect_changed(|row| {
            let mut config = Config::load();
            config.excludes = row.text().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            config.save();
        });
        what.add(&excludes);
        self.groups.append(&what);

        // ----- how often
        let often = adw::PreferencesGroup::builder().title(tr("Automatic backups")).description(tr("A systemd timer starts the backup, also when omafile is closed. If the drive is away, the backup is made up for when it comes back. Old backups thin out: all of the last day, one a day for a month, one a week beyond.")).build();
        let choices = [Schedule::Manual, Schedule::Hourly, Schedule::Daily, Schedule::Weekly];
        let labels = [tr("Off (only by hand)"), tr("Every hour"), tr("Every day"), tr("Every week")];
        let model = gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
        let schedule = adw::ComboRow::builder().title(tr("Back up")).model(&model).selected(choices.iter().position(|s| *s == config.schedule).unwrap_or(0) as u32).build();
        let (weak, wweak) = (Rc::downgrade(self), Rc::downgrade(window));
        schedule.connect_selected_notify(move |row| {
            let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) else { return };
            let chosen = choices.get(row.selected() as usize).copied().unwrap_or(Schedule::Manual);
            let mut config = Config::load();
            if config.schedule == chosen {
                return;
            }
            config.schedule = chosen;
            config.save();
            w.backup_apply_schedule(&config, row);
            p.update_status(&w);
            w.refresh_sidebar();
        });
        often.add(&schedule);
        self.groups.append(&often);
    }
}

impl Window {
    fn backup_page(self: &Rc<Self>) -> Rc<BackupPage> {
        // Cloned out first: the borrow must not live through the arm that fills the slot.
        let existing = self.backup_page.borrow().clone();
        match existing {
            Some(p) => p,
            None => {
                let p = BackupPage::new(self);
                self.content_stack.add_named(&p.root, Some("backup"));
                self.backup_page.replace(Some(p.clone()));
                p
            }
        }
    }

    /// The sidebar's Backup row: the page with the status, the list and the settings.
    pub fn open_backup(self: &Rc<Self>) {
        let page = self.backup_page();
        if self.content_stack.visible_child_name().as_deref() == Some("nowplaying") {
            self.show_now_playing(false);
        }
        page.update_status(self);
        page.rebuild(self);
        self.content_stack.set_visible_child_name("backup");
        page.root.grab_focus();
    }

    /// Start a backup now, in a process of its own.
    pub fn backup_now(self: &Rc<Self>) {
        let config = Config::load();
        if !config.is_set_up() {
            self.open_backup();
            return;
        }
        if !matches!(backup::destination(&config), Destination::Ready(_)) {
            self.toast_error(&tr("The backup drive is not connected"));
            return;
        }
        match spawn_backup(&[]) {
            Ok(()) => {
                // The page shows it at once, without waiting for the first status write.
                if let Some(page) = self.backup_page.borrow().as_ref() {
                    page.was_running.set(true);
                    page.mood.set(Mood::Busy);
                    page.spin();
                }
            }
            Err(e) => self.toast_error(&format!("{}: {e}", tr("Cannot start the backup"))),
        }
    }

    /// Remember a destination: the folder, the file system it is on (to find the drive again
    /// under another mount point), and sensible defaults on first use.
    pub fn backup_set_destination(self: &Rc<Self>, dir: &Path) {
        // A name typed into the dialog that is not there yet is a folder to make.
        if !dir.is_dir() && std::fs::create_dir_all(dir).is_err() {
            self.toast_error(&tr("Cannot create a folder there"));
            return;
        }
        let mut config = Config::load();
        let first_time = !config.is_set_up();
        config.dest = dir.to_path_buf();
        match backup::mount_info(dir) {
            Some((uuid, target, _, _)) if !uuid.is_empty() => {
                config.subdir = dir.strip_prefix(&target).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                config.uuid = uuid;
            }
            _ => {
                config.uuid.clear();
                config.subdir.clear();
            }
        }
        if config.sources.is_empty() {
            config.sources.push(glib::home_dir());
        }
        config.save();
        if first_time {
            if let Err(e) = backup::apply_schedule(config.schedule) {
                self.toast(&format!("{} ({e})", tr("Backups will run while omafile is open: the systemd timer could not be set up")));
            }
        }
        self.refresh_sidebar();
    }

    fn backup_apply_schedule(self: &Rc<Self>, config: &Config, row: &adw::ComboRow) {
        match backup::apply_schedule(config.schedule) {
            Ok(()) => row.set_subtitle(""),
            // Said in the row: it is the row that was just changed.
            Err(e) => row.set_subtitle(&format!("{} ({e})", tr("Backups will run while omafile is open: the systemd timer could not be set up"))),
        }
    }

    fn backup_delete(self: &Rc<Self>, page: &Rc<BackupPage>, dir: PathBuf) {
        let dialog = adw::AlertDialog::new(Some(tr("Delete this backup?").as_str()), Some(tr("Files that other backups share with it stay; only what this one alone holds is freed.").as_str()));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("delete", tr("Delete").as_str())]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_close_response("cancel");
        let (weak, wweak) = (Rc::downgrade(page), Rc::downgrade(self));
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) else { return };
            if response != "delete" {
                return;
            }
            let (weak, wweak) = (Rc::downgrade(&p), Rc::downgrade(&w));
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || backup::remove(&dir)).await;
                let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) else { return };
                match result {
                    Ok(Ok(())) => w.toast(&tr("Backup deleted")),
                    _ => w.toast_error(&tr("Cannot delete the backup")),
                }
                p.rebuild(&w);
            });
        });
    }

    /// Rebuild the sidebar only when the Backup row would say something else.
    pub fn backup_sidebar_sync(self: &Rc<Self>) {
        let (label, _) = summary(&Config::load(), &Status::load());
        if *self.backup_label.borrow() != label {
            self.refresh_sidebar();
        }
    }

    /// Watch the status file: move the page along while a backup runs, keep the sidebar's
    /// "… ago" fresh, and start a backup that is due when the drive is there.
    pub fn start_backup_watch(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        let seen = Rc::new(Cell::new(std::time::UNIX_EPOCH));
        let ticks = Rc::new(Cell::new(0u32));
        glib::timeout_add_local(Duration::from_millis(500), move || {
            let Some(w) = this.upgrade() else { return glib::ControlFlow::Break };
            ticks.set(ticks.get().wrapping_add(1));
            let changed = std::fs::metadata(backup::status_path()).and_then(|m| m.modified()).ok().filter(|t| *t != seen.get());
            let page = w.backup_page.borrow().clone();
            let showing = w.content_stack.visible_child_name().as_deref() == Some("backup");
            if let Some(time) = changed {
                seen.set(time);
                let finished = page.as_ref().map(|p| p.update_status(&w)).unwrap_or(false);
                if let (true, Some(p)) = (finished && showing, page.as_ref()) {
                    p.rebuild(&w);
                }
                let running = Status::load().state == "running";
                // While it runs the percentage moves a few times a second: the sidebar follows slowly.
                if !running || ticks.get().is_multiple_of(6) {
                    w.backup_sidebar_sync();
                }
            } else if showing && ticks.get().is_multiple_of(20) {
                // Nothing happened: the "next backup in …" line still counts down.
                if let Some(p) = page.as_ref() {
                    p.update_status(&w);
                }
            }
            // Once a minute: is a backup due, and is the drive there? (Also covers computers
            // where the timer could not be installed, and a drive that was just plugged in.)
            if ticks.get() % 120 == 60 {
                let config = Config::load();
                if config.is_set_up() && backup::due(config.schedule, backup::last_success(), now()) && Status::load().state != "running" && matches!(backup::destination(&config), Destination::Ready(_)) {
                    let _ = spawn_backup(&["--scheduled"]);
                }
                w.backup_sidebar_sync();
            }
            glib::ControlFlow::Continue
        });
    }
}
