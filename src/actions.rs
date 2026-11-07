//! Window actions: navigation, clipboard, file operations and dialogs.

use crate::item::FileItem;
use crate::ops;
use crate::window::{Clipboard, Window};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::rc::Rc;

type Handler = Box<dyn Fn(&Rc<Window>)>;

pub const ACCELS: &[(&str, &[&str])] = &[
    ("win.back", &["<Alt>Left"]),
    ("win.forward", &["<Alt>Right"]),
    ("win.up", &["<Alt>Up", "BackSpace"]),
    ("win.home", &["<Alt>Home"]),
    ("win.reload", &["F5", "<Control>r"]),
    ("win.show-hidden", &["<Control>h"]),
    ("win.show-sidebar", &["F9"]),
    ("win.search", &["<Control>f"]),
    ("win.location", &["<Control>l"]),
    ("win.view::list", &["<Control>1"]),
    ("win.view::grid", &["<Control>2"]),
    ("win.new-folder", &["<Control><Shift>n"]),
    ("win.new-file", &["<Control><Alt>n"]),
    ("win.rename", &["F2"]),
    ("win.trash", &["Delete"]),
    ("win.delete", &["<Shift>Delete"]),
    ("win.copy", &["<Control>c"]),
    ("win.cut", &["<Control>x"]),
    ("win.paste", &["<Control>v"]),
    ("win.select-all", &["<Control>a"]),
    ("win.copy-path", &["<Control><Shift>c"]),
    ("win.terminal", &["F4", "<Control><Alt>t"]),
    ("win.properties", &["<Control>i", "<Alt>Return"]),
    ("win.open-with", &["<Control><Shift>o"]),
    ("win.shortcuts", &["<Control>question"]),
    ("win.close", &["<Control>w"]),
];

impl Window {
    pub fn setup_actions(self: &Rc<Self>) {
        let simple = |name: &str, f: Handler| {
            let action = gio::SimpleAction::new(name, None);
            let this = Rc::downgrade(self);
            action.connect_activate(move |_, _| {
                if let Some(w) = this.upgrade() {
                    f(&w);
                }
            });
            self.window.add_action(&action);
        };

        simple("back", Box::new(|w| w.go_back()));
        simple("forward", Box::new(|w| w.go_forward()));
        simple("up", Box::new(|w| w.go_up()));
        simple("home", Box::new(|w| w.navigate(gio::File::for_path(glib::home_dir()), true)));
        simple("reload", Box::new(|w| w.reload()));
        simple("search", Box::new(|w| {
            w.search_bar.set_search_mode(true);
            w.search_entry.grab_focus();
        }));
        simple("location", Box::new(|w| {
            w.path_stack.set_visible_child_name("entry");
            w.path_entry.grab_focus();
            w.path_entry.select_region(0, -1);
        }));
        simple("toggle-view", Box::new(|w| {
            let next = if w.is_grid() { "list" } else { "grid" };
            gio::prelude::ActionGroupExt::activate_action(&w.window, "view", Some(&next.to_variant()));
        }));
        simple("open", Box::new(|w| {
            for item in w.selected_items() {
                w.open_item(&item);
            }
        }));
        simple("open-with", Box::new(|w| {
            if let Some(item) = w.selected_items().first() {
                w.launch(&item.file(), true);
            }
        }));
        simple("terminal", Box::new(|w| w.open_terminal(&w.cwd())));
        simple("terminal-selected", Box::new(|w| {
            if let Some(item) = w.selected_items().first() {
                w.open_terminal(&item.file());
            }
        }));
        simple("new-folder", Box::new(|w| w.prompt_create(true)));
        simple("new-file", Box::new(|w| w.prompt_create(false)));
        simple("rename", Box::new(|w| w.prompt_rename()));
        simple("trash", Box::new(|w| w.trash_selected()));
        simple("delete", Box::new(|w| w.delete_selected()));
        simple("copy", Box::new(|w| w.clip_selected(false)));
        simple("cut", Box::new(|w| w.clip_selected(true)));
        simple("paste", Box::new(|w| w.paste()));
        simple("select-all", Box::new(|w| {
            w.selection.select_all();
        }));
        simple("copy-path", Box::new(|w| {
            let paths: Vec<String> = w.selected_items().iter().filter_map(|i| i.file().path()).map(|p| p.display().to_string()).collect();
            if !paths.is_empty() {
                w.window.clipboard().set_text(&paths.join("\n"));
                w.toast(if paths.len() == 1 { "Path copied" } else { "Paths copied" });
            }
        }));
        simple("copy-cwd-path", Box::new(|w| {
            if let Some(p) = w.cwd().path() {
                w.window.clipboard().set_text(&p.display().to_string());
                w.toast("Path copied");
            }
        }));
        simple("properties", Box::new(|w| {
            if let Some(item) = w.selected_items().first() {
                w.show_properties(item);
            }
        }));
        simple("cwd-properties", Box::new(|w| {
            let cwd = w.cwd();
            let this = Rc::downgrade(w);
            glib::spawn_future_local(async move {
                if let (Ok(item), Some(w)) = (ops::query(&cwd).await, this.upgrade()) {
                    w.show_properties(&item);
                }
            });
        }));
        simple("shortcuts", Box::new(|w| w.show_shortcuts()));
        simple("close", Box::new(|w| w.window.close()));

        // Stateful: hidden files, sidebar, view mode.
        let hidden = gio::SimpleAction::new_stateful("show-hidden", None, &false.to_variant());
        let this = Rc::downgrade(self);
        hidden.connect_activate(move |action, _| {
            let Some(w) = this.upgrade() else { return };
            let on = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            action.set_state(&on.to_variant());
            w.state.borrow_mut().show_hidden = on;
            w.filter.changed(gtk::FilterChange::Different);
        });
        self.window.add_action(&hidden);

        let sidebar = gio::SimpleAction::new_stateful("show-sidebar", None, &true.to_variant());
        let this = Rc::downgrade(self);
        sidebar.connect_activate(move |action, _| {
            let Some(w) = this.upgrade() else { return };
            let on = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(true);
            action.set_state(&on.to_variant());
            w.split.set_show_sidebar(on);
        });
        self.window.add_action(&sidebar);

        let view = gio::SimpleAction::new_stateful("view", Some(glib::VariantTy::STRING), &"list".to_variant());
        let this = Rc::downgrade(self);
        view.connect_activate(move |action, param| {
            let Some(w) = this.upgrade() else { return };
            let Some(mode) = param.and_then(|p| p.get::<String>()) else { return };
            action.set_state(&mode.to_variant());
            w.view_stack.set_visible_child_name(&mode);
            w.focus_view();
        });
        self.window.add_action(&view);
    }

    fn open_terminal(&self, dir: &gio::File) {
        match ops::open_terminal(dir) {
            Ok(()) => {}
            Err(e) => self.toast_error(&format!("Cannot open terminal: {}", e.message())),
        }
    }

    // ----- clipboard -----------------------------------------------------------

    fn clip_selected(&self, cut: bool) {
        let files: Vec<gio::File> = self.selected_items().iter().map(|i| i.file()).collect();
        if files.is_empty() {
            return;
        }
        let n = files.len();
        // Share with other applications as a file list and as plain text URIs.
        let list = gdk::FileList::from_array(&files);
        let uris: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
        let provider = gdk::ContentProvider::new_union(&[
            gdk::ContentProvider::for_value(&list.to_value()),
            gdk::ContentProvider::for_value(&uris.join("\n").to_value()),
        ]);
        let _ = self.window.clipboard().set_content(Some(&provider));
        self.state.borrow_mut().clipboard = Some(Clipboard { files, cut });
        self.toast(&format!("{} {n} {}", if cut { "Cut" } else { "Copied" }, if n == 1 { "item" } else { "items" }));
    }

    fn paste(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Some(w) = this.upgrade() else { return };
            // Prefer our own clipboard (knows about cut); fall back to files other apps copied.
            let (files, cut) = match w.state.borrow().clipboard.as_ref() {
                Some(c) => (c.files.clone(), c.cut),
                None => (Vec::new(), false),
            };
            let (files, cut) = if files.is_empty() {
                match w.window.clipboard().read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT).await {
                    Ok(value) => (value.get::<gdk::FileList>().map(|l| l.files()).unwrap_or_default(), false),
                    Err(_) => (Vec::new(), false),
                }
            } else {
                (files, cut)
            };
            if files.is_empty() {
                w.toast("Nothing to paste");
                return;
            }
            let dest_dir = w.cwd();
            let mut done = 0;
            let mut last = None;
            let mut error = None;
            for src in &files {
                let Some(name) = src.basename() else { continue };
                let name = name.to_string_lossy().to_string();
                if cut && src.parent().map(|p| p.equal(&dest_dir)).unwrap_or(false) {
                    continue;
                }
                let dst = ops::unique_child(&dest_dir, &name);
                let result = if cut { ops::move_file(src.clone(), dst.clone()).await } else { ops::copy_recursive(src.clone(), dst.clone()).await };
                match result {
                    Ok(()) => {
                        done += 1;
                        last = Some(dst);
                    }
                    Err(e) => {
                        error = Some(format!("{name}: {}", e.message()));
                        break;
                    }
                }
            }
            if cut {
                w.state.borrow_mut().clipboard = None;
            }
            w.reload();
            if let Some(dst) = last {
                let this = Rc::downgrade(&w);
                glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                    if let Some(w) = this.upgrade() {
                        w.select_file(&dst);
                    }
                });
            }
            match error {
                Some(e) => w.toast_error(&e),
                None => w.toast(&format!("{} {done} {}", if cut { "Moved" } else { "Copied" }, if done == 1 { "item" } else { "items" })),
            }
        });
    }

    // ----- destructive -----------------------------------------------------------

    fn trash_selected(self: &Rc<Self>) {
        let items = self.selected_items();
        if items.is_empty() {
            return;
        }
        // Inside the trash, "trash" means delete for good; ask first.
        if self.cwd().uri_scheme().as_deref() == Some("trash") {
            self.delete_selected();
            return;
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let mut done = 0;
            let mut error = None;
            for item in &items {
                match ops::trash(item.file()).await {
                    Ok(()) => done += 1,
                    Err(e) => {
                        error = Some(format!("{}: {}", item.display_name(), e.message()));
                        break;
                    }
                }
            }
            let Some(w) = this.upgrade() else { return };
            w.reload();
            match error {
                Some(e) => w.toast_error(&e),
                None => w.toast(&format!("Moved {done} {} to trash", if done == 1 { "item" } else { "items" })),
            }
        });
    }

    fn delete_selected(self: &Rc<Self>) {
        let items = self.selected_items();
        if items.is_empty() {
            return;
        }
        let what = if items.len() == 1 {
            format!("“{}”", items[0].display_name())
        } else {
            format!("{} items", items.len())
        };
        let dialog = adw::AlertDialog::new(
            Some(&format!("Permanently delete {what}?")),
            Some("This cannot be undone. The files will not go to the trash."),
        );
        dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            if response != "delete" {
                return;
            }
            let this = this.clone();
            glib::spawn_future_local(async move {
                let mut done = 0;
                let mut error = None;
                for item in &items {
                    match ops::delete_recursive(item.file()).await {
                        Ok(()) => done += 1,
                        Err(e) => {
                            error = Some(format!("{}: {}", item.display_name(), e.message()));
                            break;
                        }
                    }
                }
                let Some(w) = this.upgrade() else { return };
                w.reload();
                match error {
                    Some(e) => w.toast_error(&e),
                    None => w.toast(&format!("Deleted {done} {}", if done == 1 { "item" } else { "items" })),
                }
            });
        });
    }

    // ----- dialogs ---------------------------------------------------------------

    fn prompt_create(self: &Rc<Self>, folder: bool) {
        let heading = if folder { "New Folder" } else { "New File" };
        let entry = gtk::Entry::builder()
            .text(if folder { "Untitled Folder" } else { "Untitled.txt" })
            .activates_default(true)
            .build();
        let dialog = adw::AlertDialog::new(Some(heading), None);
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancel"), ("create", "Create")]);
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("create"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let entry2 = entry.clone();
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "create" {
                return;
            }
            let name = entry2.text().trim().to_string();
            if name.is_empty() || name.contains('/') {
                w.toast_error("Invalid name");
                return;
            }
            let target = w.cwd().child(&name);
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let result = if folder { ops::make_dir(target.clone()).await } else { ops::create_file(target.clone()).await };
                let Some(w) = this.upgrade() else { return };
                match result {
                    Ok(()) => {
                        w.reload();
                        let this = Rc::downgrade(&w);
                        glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                            if let Some(w) = this.upgrade() {
                                w.select_file(&target);
                            }
                        });
                    }
                    Err(e) => w.toast_error(&format!("Cannot create {name}: {}", e.message())),
                }
            });
        });
        entry.grab_focus();
        if !folder {
            if let Some(dot) = entry.text().rfind('.') {
                entry.select_region(0, dot as i32);
            }
        } else {
            entry.select_region(0, -1);
        }
    }

    fn prompt_rename(self: &Rc<Self>) {
        let items = self.selected_items();
        let Some(item) = items.first().cloned() else { return };
        let old = item.display_name();
        let stem_end = if item.is_dir() { None } else { old.rfind('.').filter(|&i| i > 0) };
        let entry = gtk::Entry::builder().text(&old).activates_default(true).build();
        let dialog = adw::AlertDialog::new(Some("Rename"), None);
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancel"), ("rename", "Rename")]);
        dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("rename"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let entry2 = entry.clone();
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "rename" {
                return;
            }
            let name = entry2.text().trim().to_string();
            if name.is_empty() || name.contains('/') || name == old {
                return;
            }
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let result = ops::rename(item.file(), &name).await;
                let Some(w) = this.upgrade() else { return };
                match result {
                    Ok(new_file) => {
                        w.reload();
                        let this = Rc::downgrade(&w);
                        glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                            if let Some(w) = this.upgrade() {
                                w.select_file(&new_file);
                            }
                        });
                    }
                    Err(e) => w.toast_error(&format!("Cannot rename: {}", e.message())),
                }
            });
        });
        entry.grab_focus();
        entry.select_region(0, stem_end.map(|i| i as i32).unwrap_or(-1));
    }

    pub fn show_properties(&self, item: &FileItem) {
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::new();
        let row = |title: &str, subtitle: &str| {
            let r = adw::ActionRow::builder().title(title).subtitle(subtitle).build();
            r.add_css_class("property");
            r.set_subtitle_selectable(true);
            r
        };
        group.add(&row("Name", &item.display_name()));
        group.add(&row("Type", &item.type_description()));
        if !item.is_dir() {
            group.add(&row("Size", &ops::human_size(item.size())));
        }
        if let Some(parent) = item.file().parent().and_then(|p| p.path()) {
            group.add(&row("Location", &crate::window::abbreviate_home(&parent)));
        } else {
            group.add(&row("Location", &item.file().uri()));
        }
        if let Some(m) = item.modified() {
            group.add(&row("Modified", &m.format("%A, %-d %B %Y %H:%M").map(|s| s.to_string()).unwrap_or_default()));
        }
        if let Some(target) = item.symlink_target() {
            group.add(&row("Link target", &target));
        }
        let mode = item.mode();
        if mode != 0 {
            group.add(&row(
                "Permissions",
                &format!("{} ({:o})", ops::perm_string(mode, item.is_dir(), item.is_symlink()), mode & 0o7777),
            ));
        }
        let owner = item.owner();
        if !owner.is_empty() {
            group.add(&row("Owner", &owner));
        }
        page.add(&group);

        let header = adw::HeaderBar::new();
        let icon = gtk::Image::new();
        icon.set_pixel_size(48);
        if let Some(gicon) = item.icon() {
            icon.set_from_gicon(&gicon);
        }
        icon.set_margin_top(12);
        let content = adw::ToolbarView::new();
        content.add_top_bar(&header);
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        vbox.append(&icon);
        vbox.append(&page);
        content.set_content(Some(&vbox));
        let dialog = adw::Dialog::builder().title("Properties").content_width(420).content_height(520).child(&content).build();
        dialog.present(Some(&self.window));
    }

    fn show_shortcuts(&self) {
        let page = adw::PreferencesPage::new();
        let sections: &[(&str, &[(&str, &str)])] = &[
            ("Navigate", &[
                ("Alt+←  /  Alt+→", "Back / forward"),
                ("Alt+↑  /  Backspace", "Parent folder"),
                ("Alt+Home", "Home"),
                ("Ctrl+L", "Type a location"),
                ("Ctrl+F", "Filter this folder"),
                ("F5", "Reload"),
            ]),
            ("View", &[
                ("Ctrl+1  /  Ctrl+2", "List / grid"),
                ("Ctrl+H", "Show hidden files"),
                ("F9", "Toggle sidebar"),
            ]),
            ("Files", &[
                ("Enter", "Open"),
                ("Ctrl+Shift+O", "Open with…"),
                ("F4", "Open terminal here"),
                ("Ctrl+Shift+N", "New folder"),
                ("Ctrl+Alt+N", "New file"),
                ("F2", "Rename"),
                ("Ctrl+C  /  Ctrl+X  /  Ctrl+V", "Copy / cut / paste"),
                ("Ctrl+Shift+C", "Copy path"),
                ("Delete", "Move to trash"),
                ("Shift+Delete", "Delete permanently"),
                ("Ctrl+I", "Properties"),
            ]),
        ];
        for (title, rows) in sections {
            let group = adw::PreferencesGroup::builder().title(*title).build();
            for (keys, desc) in rows.iter() {
                let r = adw::ActionRow::builder().title(*desc).build();
                let label = gtk::Label::new(Some(keys));
                label.add_css_class("omafile-dim");
                label.add_css_class("numeric");
                r.add_suffix(&label);
                group.add(&r);
            }
            page.add(&group);
        }
        let content = adw::ToolbarView::new();
        content.add_top_bar(&adw::HeaderBar::new());
        content.set_content(Some(&page));
        let dialog = adw::Dialog::builder().title("Keyboard Shortcuts").content_width(460).content_height(620).child(&content).build();
        dialog.present(Some(&self.window));
    }
}

pub fn about(app: &adw::Application) {
    let dialog = adw::AboutDialog::builder()
        .application_name("Files")
        .application_icon("system-file-manager")
        .developer_name("omafile")
        .version(env!("CARGO_PKG_VERSION"))
        .website("https://github.com/whogotpwned/omafile")
        .issue_url("https://github.com/whogotpwned/omafile/issues")
        .license_type(gtk::License::MitX11)
        .comments("A file manager that follows your Omarchy theme.")
        .build();
    dialog.present(app.active_window().as_ref());
}
