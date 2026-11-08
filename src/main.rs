mod actions;
mod completion;
mod crypt;
mod extras;
mod git;
mod item;
mod ops;
mod palette;
mod pane;
mod prefs;
mod projects;
mod preview;
mod quicklook;
mod remote;
mod search;
mod tags;
mod theme;
mod thumbs;
mod undo;
mod vaultui;
mod window;

use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

const APP_ID: &str = "io.github.whogotpwned.Omafile";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    let prefs = prefs::Prefs::load();
    let theme: Rc<RefCell<Option<Rc<theme::ThemeManager>>>> = Rc::new(RefCell::new(None));
    let theme2 = theme.clone();
    let prefs2 = prefs.clone();
    app.connect_startup(move |app| {
        gtk::Window::set_default_icon_name("system-file-manager");
        *theme2.borrow_mut() = Some(theme::ThemeManager::install(prefs2.get("icon-theme")));
        let quit = gio::SimpleAction::new("quit", None);
        let app2 = app.clone();
        quit.connect_activate(move |_, _| app2.quit());
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
