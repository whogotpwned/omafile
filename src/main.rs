mod actions;
mod completion;
mod containers;
mod crypt;
mod extras;
mod git;
mod gitui;
mod imagetools;
mod item;
mod motion;
mod omarchy;
mod ops;
mod palette;
mod pane;
mod prefs;
mod projects;
mod preview;
mod quicklook;
mod remote;
mod search;
mod session;
mod share;
mod status;
mod tags;
mod termpanel;
mod theme;
mod thumbs;
mod transfer;
mod undo;
mod vaultui;
mod window;

use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

const APP_ID: &str = "io.github.whogotpwned.Omafile";

const USAGE: &str = "usage: omafile [FOLDER…]
       omafile --print-recent   list recent folders, newest first (for launchers)
       omafile --lock-all       unmount every unlocked encrypted folder
       omafile --version";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--print-recent") => {
            status::print_recent();
            return glib::ExitCode::SUCCESS;
        }
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

    let prefs = prefs::Prefs::load();
    let theme: Rc<RefCell<Option<Rc<theme::ThemeManager>>>> = Rc::new(RefCell::new(None));
    let theme2 = theme.clone();
    let prefs2 = prefs.clone();
    app.connect_startup(move |app| {
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

fn open_window(app: &adw::Application, start: gio::File, prefs: &Rc<prefs::Prefs>, theme: &Rc<RefCell<Option<Rc<theme::ThemeManager>>>>) {
    let theme = theme.borrow().clone().expect("theme manager is created at startup");
    let win = window::Window::new(app, start, prefs.clone(), theme);
    // The Rc graph is owned by the GTK window's closures; keep it alive explicitly too.
    unsafe { win.window.set_data("omafile-window", win.clone()) };
    win.window.present();
}
