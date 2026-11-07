//! Main window: sidebar with places, header with navigation and path bar,
//! list/grid file views, search and a status line.

use crate::item::FileItem;
use crate::ops;
use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

pub struct Clipboard {
    pub files: Vec<gio::File>,
    pub cut: bool,
}

pub enum Place {
    Header(String),
    Entry { name: String, icon: String, file: gio::File },
}

pub struct State {
    pub cwd: gio::File,
    pub back: Vec<gio::File>,
    pub forward: Vec<gio::File>,
    pub show_hidden: bool,
    pub search: String,
    pub clipboard: Option<Clipboard>,
    pub monitor: Option<gio::FileMonitor>,
    pub monitor_pending: Option<glib::SourceId>,
    pub generation: u64,
    pub cancel: Option<gio::Cancellable>,
    pub places: Vec<Place>,
}

pub struct Window {
    pub window: adw::ApplicationWindow,
    pub toast: adw::ToastOverlay,
    pub split: adw::OverlaySplitView,
    pub store: gio::ListStore,
    pub filter: gtk::CustomFilter,
    pub selection: gtk::MultiSelection,
    pub column_view: gtk::ColumnView,
    pub grid_view: gtk::GridView,
    pub view_stack: gtk::Stack,
    pub pathbar: gtk::Box,
    pub path_entry: gtk::Entry,
    pub path_stack: gtk::Stack,
    pub search_bar: gtk::SearchBar,
    pub search_entry: gtk::SearchEntry,
    pub sidebar_list: gtk::ListBox,
    pub status: gtk::Label,
    pub back_btn: gtk::Button,
    pub forward_btn: gtk::Button,
    pub up_btn: gtk::Button,
    pub item_menu: gtk::PopoverMenu,
    pub background_menu: gtk::PopoverMenu,
    /// Cell/child widgets created by the factories, mapped to their list items,
    /// so a right-click can be resolved to a model position.
    pub cell_items: RefCell<HashMap<gtk::Widget, gtk::ListItem>>,
    pub state: RefCell<State>,
    pub syncing_sidebar: Cell<bool>,
}

const ICON_LIST: i32 = 20;
const ICON_GRID: i32 = 64;

impl Window {
    pub fn new(app: &adw::Application, start: gio::File) -> Rc<Self> {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .default_width(1080)
            .default_height(680)
            .title("Files")
            .build();
        window.add_css_class("omafile");

        // ---- models ---------------------------------------------------------
        let store = gio::ListStore::new::<FileItem>();
        let filter = gtk::CustomFilter::new(|_| true);
        let filter_model = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
        let column_view = gtk::ColumnView::new(None::<gtk::MultiSelection>);
        column_view.set_show_row_separators(false);
        column_view.set_enable_rubberband(true);
        column_view.add_css_class("omafile-files");
        column_view.add_css_class("data-table");
        let sort_model = gtk::SortListModel::new(Some(filter_model), column_view.sorter());
        let selection = gtk::MultiSelection::new(Some(sort_model));
        column_view.set_model(Some(&selection));

        let grid_factory = gtk::SignalListItemFactory::new();
        let grid_view = gtk::GridView::new(Some(selection.clone()), Some(grid_factory.clone()));
        grid_view.set_min_columns(2);
        grid_view.set_max_columns(24);
        grid_view.set_enable_rubberband(true);
        grid_view.add_css_class("omafile-files");

        // ---- header ---------------------------------------------------------
        let back_btn = nav_button("go-previous-symbolic", "Back", "win.back");
        let forward_btn = nav_button("go-next-symbolic", "Forward", "win.forward");
        let up_btn = nav_button("go-up-symbolic", "Parent folder", "win.up");
        let nav_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        nav_box.add_css_class("linked");
        nav_box.append(&back_btn);
        nav_box.append(&forward_btn);

        let pathbar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        pathbar.add_css_class("omafile-pathbar");
        pathbar.set_valign(gtk::Align::Center);
        let pathbar_scroll = gtk::ScrolledWindow::builder()
            .child(&pathbar)
            .hscrollbar_policy(gtk::PolicyType::External)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_width(true)
            .build();
        let path_entry = gtk::Entry::builder()
            .placeholder_text("Enter a location")
            .primary_icon_name("folder-symbolic")
            .hexpand(true)
            .build();
        let path_stack = gtk::Stack::new();
        path_stack.set_hhomogeneous(false);
        path_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        path_stack.add_named(&pathbar_scroll, Some("bar"));
        path_stack.add_named(&path_entry, Some("entry"));
        path_stack.set_hexpand(true);
        let path_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        path_box.set_hexpand(true);
        path_box.append(&path_stack);

        let search_toggle = gtk::ToggleButton::builder()
            .icon_name("edit-find-symbolic")
            .tooltip_text("Search (Ctrl+F)")
            .build();
        let view_toggle = gtk::Button::builder()
            .icon_name("view-grid-symbolic")
            .tooltip_text("Toggle list / grid (Ctrl+1, Ctrl+2)")
            .action_name("win.toggle-view")
            .build();
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Menu")
            .menu_model(&main_menu())
            .primary(true)
            .build();

        let header = adw::HeaderBar::new();
        header.pack_start(&nav_box);
        header.pack_start(&up_btn);
        header.set_title_widget(Some(&path_box));
        header.pack_end(&menu_button);
        header.pack_end(&view_toggle);
        header.pack_end(&search_toggle);

        // ---- search ---------------------------------------------------------
        let search_entry = gtk::SearchEntry::builder().placeholder_text("Filter this folder").hexpand(true).build();
        let search_bar = gtk::SearchBar::builder().child(&search_entry).build();
        search_bar.connect_entry(&search_entry);
        search_bar.set_key_capture_widget(Some(&window));
        search_toggle
            .bind_property("active", &search_bar, "search-mode-enabled")
            .bidirectional()
            .sync_create()
            .build();

        // ---- content --------------------------------------------------------
        let view_stack = gtk::Stack::new();
        view_stack.set_vexpand(true);
        let list_scroll = gtk::ScrolledWindow::builder().child(&column_view).vexpand(true).build();
        let grid_scroll = gtk::ScrolledWindow::builder().child(&grid_view).vexpand(true).build();
        view_stack.add_named(&list_scroll, Some("list"));
        view_stack.add_named(&grid_scroll, Some("grid"));

        let status = gtk::Label::builder()
            .xalign(0.0)
            .margin_start(12)
            .margin_end(12)
            .margin_top(4)
            .margin_bottom(4)
            .ellipsize(pango::EllipsizeMode::End)
            .css_classes(["caption", "omafile-dim"])
            .build();
        let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        status_bar.append(&status);

        let content_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content_box.append(&search_bar);
        content_box.append(&view_stack);

        let content = adw::ToolbarView::new();
        content.add_top_bar(&header);
        content.set_content(Some(&content_box));
        content.add_bottom_bar(&status_bar);

        // ---- sidebar --------------------------------------------------------
        let sidebar_list = gtk::ListBox::new();
        sidebar_list.add_css_class("navigation-sidebar");
        sidebar_list.set_selection_mode(gtk::SelectionMode::Single);
        let sidebar_scroll = gtk::ScrolledWindow::builder()
            .child(&sidebar_list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.set_title_widget(Some(&adw::WindowTitle::new("Files", "")));
        sidebar_header.set_show_end_title_buttons(false);
        let sidebar = adw::ToolbarView::new();
        sidebar.add_top_bar(&sidebar_header);
        sidebar.set_content(Some(&sidebar_scroll));

        let split = adw::OverlaySplitView::new();
        split.set_sidebar(Some(&sidebar));
        split.set_content(Some(&content));
        split.set_min_sidebar_width(180.0);
        split.set_max_sidebar_width(260.0);
        split.set_sidebar_width_fraction(0.2);

        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&split));
        window.set_content(Some(&toast));

        let item_menu = gtk::PopoverMenu::from_model(Some(&item_menu_model()));
        item_menu.set_has_arrow(false);
        item_menu.set_parent(&view_stack);
        let background_menu = gtk::PopoverMenu::from_model(Some(&background_menu_model()));
        background_menu.set_has_arrow(false);
        background_menu.set_parent(&view_stack);

        let this = Rc::new(Window {
            window,
            toast,
            split,
            store,
            filter,
            selection,
            column_view,
            grid_view,
            view_stack,
            pathbar,
            path_entry,
            path_stack,
            search_bar,
            search_entry,
            sidebar_list,
            status,
            back_btn,
            forward_btn,
            up_btn,
            item_menu,
            background_menu,
            cell_items: RefCell::new(HashMap::new()),
            state: RefCell::new(State {
                cwd: start.clone(),
                back: Vec::new(),
                forward: Vec::new(),
                show_hidden: false,
                search: String::new(),
                clipboard: None,
                monitor: None,
                monitor_pending: None,
                generation: 0,
                cancel: None,
                places: Vec::new(),
            }),
            syncing_sidebar: Cell::new(false),
        });

        this.setup_columns();
        this.setup_grid(&grid_factory);
        this.setup_filter();
        this.setup_views();
        this.setup_search();
        this.setup_path_entry();
        this.setup_sidebar();
        this.setup_actions();
        this.navigate(start, true);
        this
    }

    // ----- views ---------------------------------------------------------------

    fn setup_columns(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        let name_factory = gtk::SignalListItemFactory::new();
        name_factory.connect_setup(move |_, obj| {
                let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
                let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let image = gtk::Image::new();
                image.set_pixel_size(ICON_LIST);
                let label = gtk::Label::builder().xalign(0.0).ellipsize(pango::EllipsizeMode::Middle).hexpand(true).build();
                hbox.append(&image);
                hbox.append(&label);
                list_item.set_child(Some(&hbox));
                if let Some(w) = this.upgrade() {
                    w.cell_items.borrow_mut().insert(hbox.upcast(), list_item.clone());
                }
        });
        name_factory.connect_bind(|_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(item) = list_item.item().and_downcast::<FileItem>() else { return };
            let hbox = list_item.child().and_downcast::<gtk::Box>().unwrap();
            let image = hbox.first_child().and_downcast::<gtk::Image>().unwrap();
            let label = hbox.last_child().and_downcast::<gtk::Label>().unwrap();
            match item.icon() {
                Some(icon) => image.set_from_gicon(&icon),
                None => image.set_icon_name(Some("text-x-generic")),
            }
            label.set_text(&item.display_name());
            apply_item_classes(&hbox, &item);
        });
        let name_col = gtk::ColumnViewColumn::new(Some("Name"), Some(name_factory.clone()));
        name_col.set_expand(true);
        name_col.set_resizable(true);
        name_col.set_sorter(Some(&gtk::CustomSorter::new(|a, b| {
            let a = a.downcast_ref::<FileItem>().unwrap().sort_key();
            let b = b.downcast_ref::<FileItem>().unwrap().sort_key();
            a.cmp(&b).into()
        })));

        let size_col = self.text_column("Size", 1.0, |item| if item.is_dir() { String::new() } else { ops::human_size(item.size()) });
        size_col.set_sorter(Some(&gtk::CustomSorter::new(|a, b| {
            let a = a.downcast_ref::<FileItem>().unwrap();
            let b = b.downcast_ref::<FileItem>().unwrap();
            (!a.is_dir(), a.size()).cmp(&(!b.is_dir(), b.size())).into()
        })));
        let type_col = self.text_column("Type", 0.0, |item| item.type_description());
        type_col.set_sorter(Some(&gtk::CustomSorter::new(|a, b| {
            let a = a.downcast_ref::<FileItem>().unwrap();
            let b = b.downcast_ref::<FileItem>().unwrap();
            (!a.is_dir(), a.type_description()).cmp(&(!b.is_dir(), b.type_description())).into()
        })));
        let modified_col = self.text_column("Modified", 0.0, |item| item.modified().map(|d| ops::format_time(&d)).unwrap_or_default());
        modified_col.set_sorter(Some(&gtk::CustomSorter::new(|a, b| {
            let a = a.downcast_ref::<FileItem>().unwrap();
            let b = b.downcast_ref::<FileItem>().unwrap();
            (!a.is_dir(), b.modified_unix()).cmp(&(!b.is_dir(), a.modified_unix())).into()
        })));

        self.column_view.append_column(&name_col);
        self.column_view.append_column(&size_col);
        self.column_view.append_column(&type_col);
        self.column_view.append_column(&modified_col);
        self.column_view.sort_by_column(Some(&name_col), gtk::SortType::Ascending);
    }

    fn text_column(self: &Rc<Self>, title: &str, xalign: f32, text: fn(&FileItem) -> String) -> gtk::ColumnViewColumn {
        let this = Rc::downgrade(self);
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let label = gtk::Label::builder().xalign(xalign).ellipsize(pango::EllipsizeMode::End).build();
            label.add_css_class("omafile-dim");
            if xalign > 0.5 {
                label.add_css_class("numeric");
            }
            list_item.set_child(Some(&label));
            if let Some(w) = this.upgrade() {
                w.cell_items.borrow_mut().insert(label.upcast(), list_item.clone());
            }
        });
        factory.connect_bind(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(item) = list_item.item().and_downcast::<FileItem>() else { return };
            let label = list_item.child().and_downcast::<gtk::Label>().unwrap();
            label.set_text(&text(&item));
        });
        let col = gtk::ColumnViewColumn::new(Some(title), Some(factory.clone()));
        col.set_resizable(true);
        col
    }

    fn setup_grid(self: &Rc<Self>, factory: &gtk::SignalListItemFactory) {
        let this = Rc::downgrade(self);
        factory.connect_setup(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let vbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
            vbox.set_width_request(96);
            let image = gtk::Image::new();
            image.set_pixel_size(ICON_GRID);
            let label = gtk::Label::builder()
                .justify(gtk::Justification::Center)
                .wrap(true)
                .wrap_mode(pango::WrapMode::WordChar)
                .lines(2)
                .ellipsize(pango::EllipsizeMode::Middle)
                .max_width_chars(14)
                .build();
            vbox.append(&image);
            vbox.append(&label);
            list_item.set_child(Some(&vbox));
            if let Some(w) = this.upgrade() {
                w.cell_items.borrow_mut().insert(vbox.upcast(), list_item.clone());
            }
        });
        factory.connect_bind(|_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(item) = list_item.item().and_downcast::<FileItem>() else { return };
            let vbox = list_item.child().and_downcast::<gtk::Box>().unwrap();
            let image = vbox.first_child().and_downcast::<gtk::Image>().unwrap();
            let label = vbox.last_child().and_downcast::<gtk::Label>().unwrap();
            match item.icon() {
                Some(icon) => image.set_from_gicon(&icon),
                None => image.set_icon_name(Some("text-x-generic")),
            }
            label.set_text(&item.display_name());
            apply_item_classes(&vbox, &item);
        });
    }

    fn setup_filter(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        self.filter.set_filter_func(move |obj| {
            let Some(w) = this.upgrade() else { return true };
            let item = obj.downcast_ref::<FileItem>().unwrap();
            let state = w.state.borrow();
            if !state.show_hidden && item.is_hidden() {
                return false;
            }
            if state.search.is_empty() {
                return true;
            }
            item.display_name().to_lowercase().contains(&state.search)
        });
    }

    fn setup_views(self: &Rc<Self>) {
        for (view, is_grid) in [(self.column_view.clone().upcast::<gtk::Widget>(), false), (self.grid_view.clone().upcast(), true)] {
            let this = Rc::downgrade(self);
            let gesture = gtk::GestureClick::new();
            gesture.set_button(gdk::BUTTON_SECONDARY);
            gesture.connect_pressed(move |g, _, x, y| {
                let Some(w) = this.upgrade() else { return };
                let widget = g.widget().unwrap();
                w.context_menu(&widget, x, y);
            });
            view.add_controller(gesture);
            let this = Rc::downgrade(self);
            if is_grid {
                self.grid_view.connect_activate(move |_, pos| {
                    if let Some(w) = this.upgrade() {
                        w.activate_position(pos);
                    }
                });
            } else {
                self.column_view.connect_activate(move |_, pos| {
                    if let Some(w) = this.upgrade() {
                        w.activate_position(pos);
                    }
                });
            }
        }
        let this = Rc::downgrade(self);
        self.selection.connect_selection_changed(move |_, _, _| {
            if let Some(w) = this.upgrade() {
                w.update_status();
            }
        });
        let this = Rc::downgrade(self);
        self.selection.connect_items_changed(move |_, _, _, _| {
            if let Some(w) = this.upgrade() {
                w.update_status();
            }
        });
    }

    fn setup_search(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        self.search_entry.connect_search_changed(move |entry| {
            let Some(w) = this.upgrade() else { return };
            w.state.borrow_mut().search = entry.text().to_lowercase();
            w.filter.changed(gtk::FilterChange::Different);
        });
        let this = Rc::downgrade(self);
        self.search_entry.connect_stop_search(move |_| {
            if let Some(w) = this.upgrade() {
                w.search_bar.set_search_mode(false);
            }
        });
        let this = Rc::downgrade(self);
        self.search_entry.connect_activate(move |_| {
            let Some(w) = this.upgrade() else { return };
            // Enter in the search box opens the first match.
            if w.selection.n_items() > 0 {
                w.activate_position(0);
            }
        });
    }

    fn setup_path_entry(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        self.path_entry.connect_activate(move |entry| {
            let Some(w) = this.upgrade() else { return };
            let text = entry.text().to_string();
            let text = text.trim();
            let expanded = if let Some(rest) = text.strip_prefix('~') {
                format!("{}{}", glib::home_dir().display(), rest)
            } else {
                text.to_string()
            };
            let file = if expanded.contains("://") {
                gio::File::for_uri(&expanded)
            } else if expanded.starts_with('/') {
                gio::File::for_path(&expanded)
            } else {
                w.state.borrow().cwd.resolve_relative_path(&expanded)
            };
            w.path_stack.set_visible_child_name("bar");
            w.navigate(file, true);
        });
        let this = Rc::downgrade(self);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(w) = this.upgrade() {
                    w.path_stack.set_visible_child_name("bar");
                    w.focus_view();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.path_entry.add_controller(keys);
    }

    // ----- sidebar -----------------------------------------------------------

    fn setup_sidebar(self: &Rc<Self>) {
        self.rebuild_places();
        let this = Rc::downgrade(self);
        self.sidebar_list.connect_row_activated(move |_, row| {
            let Some(w) = this.upgrade() else { return };
            if w.syncing_sidebar.get() {
                return;
            }
            let file = {
                let state = w.state.borrow();
                match state.places.get(row.index() as usize) {
                    Some(Place::Entry { file, .. }) => Some(file.clone()),
                    _ => None,
                }
            };
            if let Some(file) = file {
                w.navigate(file, true);
            }
        });
        let monitor = gio::VolumeMonitor::get();
        for signal in ["mount-added", "mount-removed", "mount-changed"] {
            let this = Rc::downgrade(self);
            monitor.connect_local(signal, false, move |_| {
                if let Some(w) = this.upgrade() {
                    w.rebuild_places();
                    w.sync_sidebar();
                }
                None
            });
        }
        // Keep the monitor alive for the window's lifetime.
        unsafe { self.window.set_data("volume-monitor", monitor) };
    }

    fn rebuild_places(&self) {
        let mut places = vec![Place::Header("Places".into())];
        let home = glib::home_dir();
        let mut add = |name: &str, icon: &str, path: std::path::PathBuf| {
            if path.is_dir() {
                places.push(Place::Entry { name: name.into(), icon: icon.into(), file: gio::File::for_path(path) });
            }
        };
        add("Home", "user-home-symbolic", home.clone());
        for (dir, name, icon) in [
            (glib::UserDirectory::Desktop, "Desktop", "user-desktop-symbolic"),
            (glib::UserDirectory::Documents, "Documents", "folder-documents-symbolic"),
            (glib::UserDirectory::Downloads, "Downloads", "folder-download-symbolic"),
            (glib::UserDirectory::Music, "Music", "folder-music-symbolic"),
            (glib::UserDirectory::Pictures, "Pictures", "folder-pictures-symbolic"),
            (glib::UserDirectory::Videos, "Videos", "folder-videos-symbolic"),
        ] {
            if let Some(path) = glib::user_special_dir(dir) {
                if path != home {
                    add(name, icon, path);
                }
            }
        }
        add("Projects", "folder-symbolic", home.join("Projects"));
        add("Code", "folder-symbolic", home.join("Code"));
        places.push(Place::Entry { name: "Trash".into(), icon: "user-trash-symbolic".into(), file: gio::File::for_uri("trash:///") });
        places.push(Place::Header("Devices".into()));
        places.push(Place::Entry { name: "Computer".into(), icon: "drive-harddisk-symbolic".into(), file: gio::File::for_path("/") });
        for mount in gio::VolumeMonitor::get().mounts() {
            let icon = mount
                .symbolic_icon()
                .downcast::<gio::ThemedIcon>()
                .ok()
                .and_then(|i| i.names().first().map(|n| n.to_string()))
                .unwrap_or_else(|| "drive-removable-media-symbolic".into());
            places.push(Place::Entry { name: mount.name().to_string(), icon, file: mount.default_location() });
        }

        while let Some(child) = self.sidebar_list.first_child() {
            self.sidebar_list.remove(&child);
        }
        for place in &places {
            let row = gtk::ListBoxRow::new();
            match place {
                Place::Header(title) => {
                    let label = gtk::Label::builder().label(title).xalign(0.0).margin_start(6).margin_top(8).margin_bottom(2).build();
                    label.add_css_class("caption-heading");
                    label.add_css_class("omafile-dim");
                    row.set_child(Some(&label));
                    row.set_selectable(false);
                    row.set_activatable(false);
                }
                Place::Entry { name, icon, .. } => {
                    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                    hbox.append(&gtk::Image::from_icon_name(icon));
                    hbox.append(&gtk::Label::builder().label(name).xalign(0.0).ellipsize(pango::EllipsizeMode::End).build());
                    row.set_child(Some(&hbox));
                }
            }
            self.sidebar_list.append(&row);
        }
        self.state.borrow_mut().places = places;
    }

    fn sync_sidebar(&self) {
        let state = self.state.borrow();
        let index = state.places.iter().position(|p| matches!(p, Place::Entry { file, .. } if file.equal(&state.cwd)));
        drop(state);
        self.syncing_sidebar.set(true);
        match index.and_then(|i| self.sidebar_list.row_at_index(i as i32)) {
            Some(row) => self.sidebar_list.select_row(Some(&row)),
            None => self.sidebar_list.unselect_all(),
        }
        self.syncing_sidebar.set(false);
    }

    // ----- navigation --------------------------------------------------------

    pub fn navigate(self: &Rc<Self>, file: gio::File, record: bool) {
        {
            let mut state = self.state.borrow_mut();
            if record && !state.cwd.equal(&file) {
                let prev = state.cwd.clone();
                state.back.push(prev);
                state.back.truncate(100);
                state.forward.clear();
            }
            state.cwd = file.clone();
            state.search.clear();
            if let Some(c) = state.cancel.take() {
                c.cancel();
            }
            state.generation += 1;
        }
        self.search_bar.set_search_mode(false);
        self.search_entry.set_text("");
        self.update_pathbar();
        self.update_nav_buttons();
        self.sync_sidebar();
        self.watch_cwd();
        self.window.set_title(Some(&format!("{} - Files", display_name(&file))));
        self.reload();
    }

    pub fn reload(self: &Rc<Self>) {
        let (file, generation) = {
            let state = self.state.borrow();
            (state.cwd.clone(), state.generation)
        };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = ops::list_dir(&file).await;
            let Some(w) = this.upgrade() else { return };
            if w.state.borrow().generation != generation {
                return;
            }
            match result {
                Ok(items) => {
                    w.store.remove_all();
                    w.store.extend_from_slice(&items);
                    w.update_status();
                    if !w.search_bar.is_search_mode() && !w.path_entry.has_focus() {
                        w.focus_view();
                    }
                }
                Err(e) => {
                    w.store.remove_all();
                    w.update_status();
                    w.toast_error(&format!("Cannot open {}: {}", display_name(&file), e.message()));
                }
            }
        });
    }

    fn watch_cwd(self: &Rc<Self>) {
        let cwd = self.state.borrow().cwd.clone();
        let monitor = cwd.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE).ok();
        if let Some(m) = &monitor {
            let this = Rc::downgrade(self);
            m.connect_changed(move |_, _, _, _| {
                let Some(w) = this.upgrade() else { return };
                let mut state = w.state.borrow_mut();
                if let Some(id) = state.monitor_pending.take() {
                    id.remove();
                }
                let this2 = this.clone();
                state.monitor_pending = Some(glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                    if let Some(w) = this2.upgrade() {
                        w.state.borrow_mut().monitor_pending = None;
                        w.reload();
                    }
                }));
            });
        }
        let mut state = self.state.borrow_mut();
        if let Some(id) = state.monitor_pending.take() {
            id.remove();
        }
        state.monitor = monitor;
    }

    pub fn go_back(self: &Rc<Self>) {
        let target = {
            let mut state = self.state.borrow_mut();
            let Some(target) = state.back.pop() else { return };
            let cur = state.cwd.clone();
            state.forward.push(cur);
            target
        };
        self.navigate(target, false);
    }

    pub fn go_forward(self: &Rc<Self>) {
        let target = {
            let mut state = self.state.borrow_mut();
            let Some(target) = state.forward.pop() else { return };
            let cur = state.cwd.clone();
            state.back.push(cur);
            target
        };
        self.navigate(target, false);
    }

    pub fn go_up(self: &Rc<Self>) {
        let parent = self.state.borrow().cwd.parent();
        if let Some(parent) = parent {
            self.navigate(parent, true);
        }
    }

    fn update_nav_buttons(&self) {
        let state = self.state.borrow();
        self.back_btn.set_sensitive(!state.back.is_empty());
        self.forward_btn.set_sensitive(!state.forward.is_empty());
        self.up_btn.set_sensitive(state.cwd.parent().is_some());
    }

    fn update_pathbar(self: &Rc<Self>) {
        while let Some(child) = self.pathbar.first_child() {
            self.pathbar.remove(&child);
        }
        let cwd = self.state.borrow().cwd.clone();
        self.path_entry.set_text(&cwd.parse_name());

        // Collect ancestors up to the root, or up to $HOME which gets its own button.
        let home = gio::File::for_path(glib::home_dir());
        let mut chain = vec![cwd.clone()];
        let mut cursor = cwd.clone();
        while let Some(parent) = cursor.parent() {
            if cursor.equal(&home) {
                break;
            }
            chain.push(parent.clone());
            cursor = parent;
        }
        chain.reverse();
        let last = chain.len() - 1;
        for (i, file) in chain.into_iter().enumerate() {
            if i > 0 {
                let sep = gtk::Label::new(Some("/"));
                sep.add_css_class("separator");
                self.pathbar.append(&sep);
            }
            let button = gtk::Button::new();
            button.add_css_class("flat");
            if file.equal(&home) {
                let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                b.append(&gtk::Image::from_icon_name("user-home-symbolic"));
                b.append(&gtk::Label::new(Some("Home")));
                button.set_child(Some(&b));
            } else if file.parent().is_none() {
                let (icon, text) = match file.uri_scheme().as_deref() {
                    Some("trash") => ("user-trash-symbolic", "Trash".to_string()),
                    Some("file") | None => ("drive-harddisk-symbolic", "Computer".to_string()),
                    Some(scheme) => ("network-server-symbolic", scheme.to_string()),
                };
                let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                b.append(&gtk::Image::from_icon_name(icon));
                b.append(&gtk::Label::new(Some(&text)));
                button.set_child(Some(&b));
            } else {
                button.set_label(&display_name(&file));
            }
            if i == last {
                button.add_css_class("current");
            }
            let this = Rc::downgrade(self);
            let target = file.clone();
            button.connect_clicked(move |_| {
                if let Some(w) = this.upgrade() {
                    w.navigate(target.clone(), true);
                }
            });
            self.pathbar.append(&button);
        }
    }

    // ----- helpers ---------------------------------------------------------------

    pub fn cwd(&self) -> gio::File {
        self.state.borrow().cwd.clone()
    }

    pub fn is_grid(&self) -> bool {
        self.view_stack.visible_child_name().as_deref() == Some("grid")
    }

    pub fn focus_view(&self) {
        if self.is_grid() {
            self.grid_view.grab_focus();
        } else {
            self.column_view.grab_focus();
        }
    }

    pub fn item_at(&self, position: u32) -> Option<FileItem> {
        self.selection.item(position).and_downcast::<FileItem>()
    }

    pub fn selected_items(&self) -> Vec<FileItem> {
        let n = self.selection.n_items();
        (0..n).filter(|i| self.selection.is_selected(*i)).filter_map(|i| self.item_at(i)).collect()
    }

    pub fn select_file(&self, file: &gio::File) {
        for i in 0..self.selection.n_items() {
            if let Some(item) = self.item_at(i) {
                if item.file().equal(file) {
                    self.selection.select_item(i, true);
                    self.column_view.scroll_to(i, None, gtk::ListScrollFlags::NONE, None);
                    self.grid_view.scroll_to(i, gtk::ListScrollFlags::NONE, None);
                    return;
                }
            }
        }
    }

    pub fn activate_position(self: &Rc<Self>, position: u32) {
        let Some(item) = self.item_at(position) else { return };
        self.open_item(&item);
    }

    pub fn open_item(self: &Rc<Self>, item: &FileItem) {
        if item.is_dir() {
            self.navigate(item.file(), true);
        } else {
            self.launch(&item.file(), false);
        }
    }

    pub fn launch(self: &Rc<Self>, file: &gio::File, ask: bool) {
        let launcher = gtk::FileLauncher::new(Some(file));
        launcher.set_always_ask(ask);
        let this = Rc::downgrade(self);
        let name = display_name(file);
        launcher.launch(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if let Err(e) = result {
                if !e.matches(gtk::DialogError::Dismissed) && !e.matches(gtk::DialogError::Cancelled) {
                    if let Some(w) = this.upgrade() {
                        w.toast_error(&format!("Cannot open {name}: {}", e.message()));
                    }
                }
            }
        });
    }

    pub fn update_status(&self) {
        let total = self.selection.n_items();
        let selected = self.selected_items();
        let mut text = format!("{total} {}", if total == 1 { "item" } else { "items" });
        if !selected.is_empty() {
            let size: u64 = selected.iter().filter(|i| !i.is_dir()).map(|i| i.size()).sum();
            text.push_str(&format!(", {} selected", selected.len()));
            if size > 0 {
                text.push_str(&format!(" ({})", ops::human_size(size)));
            }
        } else {
            let state = self.state.borrow();
            if let Some(path) = state.cwd.path() {
                text.push_str(&format!("  ·  {}", abbreviate_home(&path)));
            }
        }
        self.status.set_text(&text);
    }

    pub fn toast(&self, text: &str) {
        self.toast.add_toast(adw::Toast::builder().title(text).timeout(3).build());
    }

    pub fn toast_error(&self, text: &str) {
        self.toast.add_toast(adw::Toast::builder().title(text).timeout(6).build());
    }

    /// Resolve the model position of the widget under (x, y) in `view`.
    pub fn position_at(&self, view: &gtk::Widget, x: f64, y: f64) -> Option<u32> {
        let mut widget = view.pick(x, y, gtk::PickFlags::DEFAULT)?;
        let map = self.cell_items.borrow();
        loop {
            if let Some(list_item) = map.get(&widget) {
                return Some(list_item.position());
            }
            widget = widget.parent()?;
            if &widget == view {
                return None;
            }
        }
    }

    fn context_menu(self: &Rc<Self>, view: &gtk::Widget, x: f64, y: f64) {
        let position = self.position_at(view, x, y);
        if let Some(pos) = position {
            if !self.selection.is_selected(pos) {
                self.selection.select_item(pos, true);
            }
        } else {
            self.selection.unselect_all();
        }
        // Translate the click into the coordinate space of the menus' parent.
        let point = view
            .compute_point(&self.view_stack, &gtk::graphene::Point::new(x as f32, y as f32))
            .unwrap_or_else(|| gtk::graphene::Point::new(x as f32, y as f32));
        let rect = gdk::Rectangle::new(point.x() as i32, point.y() as i32, 1, 1);
        let menu = if position.is_some() { &self.item_menu } else { &self.background_menu };
        if position.is_some() {
            let single_dir = self.selected_items().iter().all(|i| i.is_dir()) && self.selected_items().len() == 1;
            self.item_menu.set_menu_model(Some(&item_menu_model_for(single_dir)));
        }
        menu.set_pointing_to(Some(&rect));
        menu.popup();
    }
}

fn nav_button(icon: &str, tooltip: &str, action: &str) -> gtk::Button {
    gtk::Button::builder().icon_name(icon).tooltip_text(tooltip).action_name(action).build()
}

fn apply_item_classes(widget: &impl IsA<gtk::Widget>, item: &FileItem) {
    let w = widget.as_ref();
    for (class, on) in [
        ("omafile-hidden", item.is_hidden()),
        ("omafile-symlink", item.is_symlink()),
        ("omafile-exec", item.is_executable()),
    ] {
        if on {
            w.add_css_class(class);
        } else {
            w.remove_css_class(class);
        }
    }
}

pub fn display_name(file: &gio::File) -> String {
    if let Some(name) = file.basename() {
        let name = name.to_string_lossy().to_string();
        if name != "/" {
            return name;
        }
    }
    match file.uri_scheme().as_deref() {
        Some("trash") => "Trash".into(),
        _ => "Computer".into(),
    }
}

pub fn abbreviate_home(path: &std::path::Path) -> String {
    let home = glib::home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn main_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let s1 = gio::Menu::new();
    s1.append(Some("New Folder"), Some("win.new-folder"));
    s1.append(Some("New File"), Some("win.new-file"));
    s1.append(Some("Open Terminal Here"), Some("win.terminal"));
    menu.append_section(None, &s1);
    let s2 = gio::Menu::new();
    s2.append(Some("Show Hidden Files"), Some("win.show-hidden"));
    s2.append(Some("Show Sidebar"), Some("win.show-sidebar"));
    menu.append_section(None, &s2);
    let view = gio::Menu::new();
    view.append(Some("List"), Some("win.view::list"));
    view.append(Some("Grid"), Some("win.view::grid"));
    menu.append_section(Some("View"), &view);
    let s3 = gio::Menu::new();
    s3.append(Some("Keyboard Shortcuts"), Some("win.shortcuts"));
    s3.append(Some("About Files"), Some("app.about"));
    menu.append_section(None, &s3);
    menu
}

fn item_menu_model() -> gio::Menu {
    item_menu_model_for(false)
}

fn item_menu_model_for(single_dir: bool) -> gio::Menu {
    let menu = gio::Menu::new();
    let open = gio::Menu::new();
    open.append(Some("Open"), Some("win.open"));
    if single_dir {
        open.append(Some("Open in Terminal"), Some("win.terminal-selected"));
    } else {
        open.append(Some("Open With…"), Some("win.open-with"));
    }
    menu.append_section(None, &open);
    let edit = gio::Menu::new();
    edit.append(Some("Cut"), Some("win.cut"));
    edit.append(Some("Copy"), Some("win.copy"));
    edit.append(Some("Copy Path"), Some("win.copy-path"));
    edit.append(Some("Rename…"), Some("win.rename"));
    menu.append_section(None, &edit);
    let danger = gio::Menu::new();
    danger.append(Some("Move to Trash"), Some("win.trash"));
    danger.append(Some("Delete Permanently…"), Some("win.delete"));
    menu.append_section(None, &danger);
    let info = gio::Menu::new();
    info.append(Some("Properties"), Some("win.properties"));
    menu.append_section(None, &info);
    menu
}

fn background_menu_model() -> gio::Menu {
    let menu = gio::Menu::new();
    let s1 = gio::Menu::new();
    s1.append(Some("New Folder"), Some("win.new-folder"));
    s1.append(Some("New File"), Some("win.new-file"));
    menu.append_section(None, &s1);
    let s2 = gio::Menu::new();
    s2.append(Some("Paste"), Some("win.paste"));
    s2.append(Some("Select All"), Some("win.select-all"));
    menu.append_section(None, &s2);
    let s3 = gio::Menu::new();
    s3.append(Some("Open Terminal Here"), Some("win.terminal"));
    s3.append(Some("Copy Path"), Some("win.copy-cwd-path"));
    s3.append(Some("Properties"), Some("win.cwd-properties"));
    menu.append_section(None, &s3);
    menu
}
