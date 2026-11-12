#[macro_use]
mod i18n;
mod actions;
mod atelier;
mod captures;
mod capturesui;
mod backup;
mod backupui;
mod bindings;
mod bulk;
mod checksum;
mod compare;
mod completion;
mod containers;
mod connections;
mod connectui;
mod crypt;
mod extras;
mod features;
mod feeds;
mod feedsui;
mod fm1;
mod finder;
mod git;
mod help;
mod gitui;
mod groups;
mod hypr;
mod inhibit;
mod imagetools;
mod item;
mod mediatools;
mod motion;
mod notes;
mod omarchy;
mod ops;
mod palette;
mod pane;
mod perms;
mod prefs;
mod pyenv;
mod pyui;
mod projects;
mod preview;
mod quicklook;
mod render;
mod remote;
mod scheme;
mod search;
mod selftest;
mod session;
mod share;
mod smart;
mod status;
mod tags;
mod templates;
mod termpanel;
mod theme;
mod thumbs;
mod transfer;
mod tour;
mod trashauto;
mod undo;
mod vaults;
mod vaultui;
mod views;
mod versions;
mod versionsui;
mod vimkeys;
mod viewmem;
mod shelf;
mod treemap;
mod rules;
mod rulesui;
mod dupes;
mod ebook;
mod mpris;
mod nowplaying;
mod visualizer;
mod json;
mod httpd;
mod localsend;
mod nearby;
mod iphone;
mod ebookui;
mod window;

use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

const APP_ID: &str = "io.github.whogotpwned.Omafile";

/// What a start option does in the window it reaches.
type InWindow = fn(&Rc<window::Window>);

/// Start options that act in a window: the flag, the application action a running omafile
/// performs for it, and the line `--help` shows.
const WINDOW_OPTIONS: &[(&str, &str, &str)] = &[
    ("captures", "show-captures", "Open on the screenshots and screen recordings"),
    ("import-photos", "run-import-photos", "Import the camera roll of a connected iPhone / iPad"),
];

const USAGE: &str = "usage: omafile [FOLDER…]
       omafile --print-recent   list recent folders, newest first (for launchers)
       omafile --print-bindings list every shortcut (keys, action, group; tab separated)
       omafile --toggle         Hyprland: show / hide a drop-down omafile on the special workspace files
       omafile --lock-all       unmount every unlocked encrypted folder
       omafile --backup         back up now to the drive chosen under Backup (what the timer runs)
       omafile --import-photos  open omafile and import the camera roll of a connected iPhone / iPad
       omafile --theme-from-picture <picture> [--name <name>] [--dark | --light]
                                make an Omarchy theme out of a picture, as the theme atelier does
       omafile --captures       open omafile on the screenshots and screen recordings
       omafile --copy-capture   put the newest screenshot or recording on the clipboard
       omafile --version";

fn main() -> glib::ExitCode {
    i18n::export_language();
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--toggle") => return glib::ExitCode::from(hypr::toggle().min(1) as u8),
        Some("--print-bindings") => {
            bindings::print();
            return glib::ExitCode::SUCCESS;
        }
        Some("--print-recent") => {
            status::print_recent();
            return glib::ExitCode::SUCCESS;
        }
        Some("--backup") => return glib::ExitCode::from(backup::main_headless(args.iter().any(|a| a == "--scheduled"))),
        Some("--describe-theme") => return glib::ExitCode::from(omarchy::describe_theme(args.get(2))),
        Some("--theme-from-picture") => return glib::ExitCode::from(atelier::create_headless(&args[2..])),
        Some("--copy-capture") => return glib::ExitCode::from(captures::copy_latest()),
        Some("--lock-all") => return glib::ExitCode::from(status::lock_all().min(1) as u8),
        Some("--version") | Some("-V") => {
            println!("omafile {}", env!("CARGO_PKG_VERSION"));
            return glib::ExitCode::SUCCESS;
        }
        Some("--help") | Some("-h") => {
            println!("{USAGE}");
            return glib::ExitCode::SUCCESS;
        }
        _ => {}
    }
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    // Options that start something in a window. They are declared so that GApplication
    // takes them, and handed to the omafile that is already running when there is one.
    for (flag, _, text) in WINDOW_OPTIONS {
        app.add_main_option(flag, glib::Char(0), glib::OptionFlags::NONE, glib::OptionArg::None, text, None);
    }
    app.connect_handle_local_options(|app, options| {
        for (flag, action, _) in WINDOW_OPTIONS {
            if options.contains(flag) && app.register(gio::Cancellable::NONE).is_ok() && app.is_remote() {
                app.activate_action(action, None);
                return std::ops::ControlFlow::Break(glib::ExitCode::SUCCESS);
            }
        }
        std::ops::ControlFlow::Continue(())
    });

    let prefs = prefs::Prefs::load();
    let theme: Rc<RefCell<Option<Rc<theme::ThemeManager>>>> = Rc::new(RefCell::new(None));
    let theme2 = theme.clone();
    let prefs2 = prefs.clone();
    app.connect_startup(move |app| {
        if std::env::var_os("OMAFILE_TRACE").is_some() {
            eprintln!("language: {}", i18n::language());
        }
        gtk::Window::set_default_icon_name("io.github.whogotpwned.Omafile");
        *theme2.borrow_mut() = Some(theme::ThemeManager::install(prefs2.get("icon-theme")));
        let quit = gio::SimpleAction::new("quit", None);
        let app2 = app.clone();
        quit.connect_activate(move |_, _| {
            // app.quit() skips close-request, so save the layout here.
            for w in app2.windows() {
                if let Some(win) = unsafe { w.data::<Rc<window::Window>>("omafile-window") } {
                    unsafe { win.as_ref() }.save_session_once();
                }
            }
            app2.quit()
        });
        app.add_action(&quit);
        // "Show in folder" from browsers and other programs ends up here.
        if prefs2.get_bool("file-manager-dbus", true) {
            let (app3, prefs3, theme3) = (app.clone(), prefs2.clone(), theme2.clone());
            fm1::serve(move |request| show_for_other_apps(&app3, request, &prefs3, &theme3));
        }
        // What `omafile --captures` and `--import-photos` ask of a running omafile.
        let in_window: [(&str, InWindow); 2] = [("show-captures", |w| w.open_captures()), ("run-import-photos", |w| w.import_photos())];
        for (name, run) in in_window {
            let action = gio::SimpleAction::new(name, None);
            let (app3, prefs3, theme3) = (app.clone(), prefs2.clone(), theme2.clone());
            action.connect_activate(move |_, _| {
                let existing = app3.active_window().or_else(|| app3.windows().into_iter().next()).and_then(|w| unsafe { w.data::<Rc<window::Window>>("omafile-window").map(|p| p.as_ref().clone()) });
                let win = existing.unwrap_or_else(|| open_window(&app3, gio::File::for_path(glib::home_dir()), &prefs3, &theme3));
                win.window.present();
                run(&win);
            });
            app.add_action(&action);
        }
        let about = gio::SimpleAction::new("about", None);
        let app2 = app.clone();
        about.connect_activate(move |_, _| actions::about(&app2));
        app.add_action(&about);
        app.set_accels_for_action("app.quit", &["<Control>q"]);
        for (action, accels) in actions::ACCELS {
            app.set_accels_for_action(action, accels);
        }
    });

    let (prefs_a, theme_a) = (prefs.clone(), theme.clone());
    app.connect_activate(move |app| {
        // A saved session wins over the current directory when restoring is on
        // and this is the first window.
        if app.windows().is_empty() && prefs_a.get_bool("restore-session", true) {
            if let Some(session) = session::Session::load() {
                let start = gio::File::for_path(glib::home_dir());
                let theme_m = theme_a.borrow().clone().expect("theme manager is created at startup");
                let win = window::Window::new(app, start, prefs_a.clone(), theme_m);
                unsafe { win.window.set_data("omafile-window", win.clone()) };
                win.restore_session(&session);
                win.window.present();
                return;
            }
        }
        let start = std::env::current_dir().map(gio::File::for_path).unwrap_or_else(|_| gio::File::for_path(glib::home_dir()));
        open_window(app, start, &prefs_a, &theme_a);
    });

    let (prefs_o, theme_o) = (prefs.clone(), theme.clone());
    app.connect_open(move |app, files, _hint| {
        for file in files {
            let target = match file.query_file_type(gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE) {
                gio::FileType::Directory => file.clone(),
                gio::FileType::Unknown => file.clone(),
                _ => file.parent().unwrap_or_else(|| file.clone()),
            };
            open_window(app, target, &prefs_o, &theme_o);
        }
    });

    app.run()
}

fn open_window(app: &adw::Application, start: gio::File, prefs: &Rc<prefs::Prefs>, theme: &Rc<RefCell<Option<Rc<theme::ThemeManager>>>>) -> Rc<window::Window> {
    let theme = theme.borrow().clone().expect("theme manager is created at startup");
    let win = window::Window::new(app, start, prefs.clone(), theme);
    // The Rc graph is owned by the GTK window's closures; keep it alive explicitly too.
    unsafe { win.window.set_data("omafile-window", win.clone()) };
    win.window.present();
    win
}

/// "Show in folder" from another program (org.freedesktop.FileManager1): a new tab in the
/// window that is there, or a new window; the item selected, its properties if asked for.
fn show_for_other_apps(app: &adw::Application, request: fm1::Request, prefs: &Rc<prefs::Prefs>, theme: &Rc<RefCell<Option<Rc<theme::ThemeManager>>>>) {
    let (uris, select, properties) = match request {
        fm1::Request::Folders(uris) => (uris, false, false),
        fm1::Request::Items(uris) => (uris, true, false),
        fm1::Request::Properties(uris) => (uris, true, true),
    };
    for uri in uris {
        let file = gio::File::for_uri(&uri);
        let folder = if select { file.parent().unwrap_or_else(|| file.clone()) } else { file.clone() };
        let existing = app.active_window().or_else(|| app.windows().into_iter().next()).and_then(|w| unsafe { w.data::<Rc<window::Window>>("omafile-window").map(|p| p.as_ref().clone()) });
        let win = match existing {
            Some(win) => {
                win.show_files();
                win.open_in_new_tab(folder, true);
                win
            }
            None => open_window(app, folder, prefs, theme),
        };
        if select {
            if let Some(pane) = win.active_pane() {
                pane.select_file_later(file.clone());
            }
        }
        if properties {
            // Once the folder has been read and the item is selected.
            let weak = Rc::downgrade(&win);
            glib::timeout_add_local_once(std::time::Duration::from_millis(900), move || {
                if let Some(win) = weak.upgrade() {
                    gio::prelude::ActionGroupExt::activate_action(&win.window, "properties", None);
                }
            });
        }
        win.window.present();
    }
}
