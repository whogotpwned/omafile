//! Main window shell: sidebar, header bar with path bar, tabs, preview pane.

use crate::item::FileItem;
use crate::pane::Pane;
use crate::prefs::Prefs;
use crate::preview::PreviewPane;
use crate::quicklook::QuickLook;
use crate::remote;
use crate::completion::Completion;
use crate::tags::TagIndex;
use crate::theme::ThemeManager;
use crate::undo::{Journal, Op};
use crate::thumbs::ThumbCache;
use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct Clipboard {
    pub files: Vec<gio::File>,
    pub cut: bool,
}

pub enum Place {
    Header(String),
    Entry { name: String, icon: String, file: gio::File, kind: PlaceKind },
    Action { name: String, icon: String, action: &'static str },
}

#[derive(Clone, PartialEq)]
pub enum PlaceKind {
    Builtin,
    Bookmark,
    Ssh(String),
    Mount(gio::Mount),
    Tag(String),
}

pub struct WinState {
    pub show_hidden: bool,
    pub view_mode: String,
    pub clipboard: Option<Clipboard>,
    pub places: Vec<Place>,
}

pub struct Window {
    pub window: adw::ApplicationWindow,
    pub toast: adw::ToastOverlay,
    pub split: adw::OverlaySplitView,
    pub preview_split: adw::OverlaySplitView,
    pub preview: Rc<PreviewPane>,
    pub tab_views: [adw::TabView; 2],
    pub split_paned: gtk::Paned,
    pub split_boxes: [gtk::Box; 2],
    pub active_side: Cell<usize>,
    pub ending_split: Cell<bool>,
    pub pathbar: gtk::Box,
    pub path_entry: gtk::Entry,
    pub path_stack: gtk::Stack,
    pub completion: Rc<Completion>,
    pub journal: Journal,
    pub quicklook: Rc<QuickLook>,
    pub sidebar_list: gtk::ListBox,
    pub sidebar_menu: gtk::PopoverMenu,
    pub back_btn: gtk::Button,
    pub forward_btn: gtk::Button,
    pub up_btn: gtk::Button,
    pub view_btn: gtk::Button,
    pub panes: RefCell<Vec<Rc<Pane>>>,
    pub thumbs: Rc<ThumbCache>,
    pub prefs: Rc<Prefs>,
    pub theme: Rc<ThemeManager>,
    pub tags: Rc<TagIndex>,
    pub thumbnails_enabled: Cell<bool>,
    pub state: RefCell<WinState>,
    pub syncing_sidebar: Cell<bool>,
    pub sidebar_target: Cell<Option<usize>>,
}

impl Window {
    pub fn new(app: &adw::Application, start: gio::File, prefs: Rc<Prefs>, theme: Rc<ThemeManager>) -> Rc<Self> {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .default_width(1120)
            .default_height(700)
            .title("Files")
            .build();
        window.add_css_class("omafile");
        let thumbs = ThumbCache::new();

        // ---- header ---------------------------------------------------------
        let back_btn = nav_button("go-previous-symbolic", "Back (Alt+←)", "win.back");
        let forward_btn = nav_button("go-next-symbolic", "Forward (Alt+→)", "win.forward");
        let up_btn = nav_button("go-up-symbolic", "Parent folder (Alt+↑)", "win.up");
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
        let pathbar_click = gtk::GestureClick::new();
        pathbar_scroll.add_controller(pathbar_click.clone());
        let path_entry = gtk::Entry::builder()
            .placeholder_text("Type a path, ~/folder or sftp://host/")
            .primary_icon_name("folder-symbolic")
            .hexpand(true)
            .build();
        path_entry.add_css_class("omafile-path-entry");
        let completion = Completion::new(&path_entry);
        let path_stack = gtk::Stack::new();
        path_stack.set_hhomogeneous(false);
        path_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        path_stack.add_named(&pathbar_scroll, Some("bar"));
        path_stack.add_named(&path_entry, Some("entry"));
        path_stack.set_hexpand(true);
        path_stack.set_halign(gtk::Align::Fill);

        let search_btn = gtk::Button::builder().icon_name("edit-find-symbolic").tooltip_text("Filter (Ctrl+F)").action_name("win.search").build();
        let view_btn = gtk::Button::builder().icon_name("view-grid-symbolic").tooltip_text("Grid view (Ctrl+2)").action_name("win.toggle-view").build();
        let preview_btn = gtk::ToggleButton::builder().icon_name("sidebar-show-right-symbolic").tooltip_text("Preview pane (F3)").action_name("win.show-preview").build();
        let menu_button = gtk::MenuButton::builder().icon_name("open-menu-symbolic").tooltip_text("Menu").menu_model(&main_menu()).primary(true).build();

        let header = adw::HeaderBar::new();
        header.add_css_class("omafile-header");
        header.pack_start(&nav_box);
        header.pack_start(&up_btn);
        header.set_title_widget(Some(&path_stack));
        header.pack_end(&menu_button);
        header.pack_end(&preview_btn);
        header.pack_end(&view_btn);
        header.pack_end(&search_btn);

        // ---- tabs -----------------------------------------------------------
        let mut tab_views = Vec::new();
        let mut split_boxes = Vec::new();
        for _ in 0..2 {
            let tab_view = adw::TabView::new();
            tab_view.set_vexpand(true);
            let tab_bar = adw::TabBar::builder().view(&tab_view).autohide(true).expand_tabs(false).build();
            tab_bar.add_css_class("omafile-tabs");
            let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
            b.append(&tab_bar);
            b.append(&tab_view);
            b.set_hexpand(true);
            tab_views.push(tab_view);
            split_boxes.push(b);
        }
        let split_paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        split_paned.set_start_child(Some(&split_boxes[0]));
        split_paned.set_end_child(Some(&split_boxes[1]));
        split_paned.set_resize_start_child(true);
        split_paned.set_resize_end_child(true);
        split_paned.set_shrink_start_child(false);
        split_paned.set_shrink_end_child(false);
        split_boxes[1].set_visible(false);

        // ---- preview --------------------------------------------------------
        let preview = PreviewPane::new(thumbs.clone());
        let preview_split = adw::OverlaySplitView::new();
        preview_split.set_sidebar_position(gtk::PackType::End);
        preview_split.set_content(Some(&split_paned));
        preview_split.set_sidebar(Some(&preview.root));
        preview_split.set_min_sidebar_width(240.0);
        preview_split.set_max_sidebar_width(420.0);
        preview_split.set_sidebar_width_fraction(0.3);
        preview_split.set_show_sidebar(false);
        preview_split.set_collapsed(false);

        let content = adw::ToolbarView::new();
        content.set_top_bar_style(adw::ToolbarStyle::Flat);
        content.add_top_bar(&header);
        content.set_content(Some(&preview_split));

        // ---- sidebar --------------------------------------------------------
        let sidebar_list = gtk::ListBox::new();
        sidebar_list.add_css_class("navigation-sidebar");
        sidebar_list.add_css_class("omafile-sidebar");
        sidebar_list.set_selection_mode(gtk::SelectionMode::Single);
        let sidebar_scroll = gtk::ScrolledWindow::builder().child(&sidebar_list).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build();
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.add_css_class("omafile-header");
        let brand = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        brand.append(&gtk::Image::from_icon_name("folder-symbolic"));
        brand.append(&gtk::Label::builder().label("Files").css_classes(["title-4"]).build());
        sidebar_header.set_title_widget(Some(&brand));
        sidebar_header.set_show_end_title_buttons(false);
        let new_tab_btn = gtk::Button::builder().icon_name("tab-new-symbolic").tooltip_text("New tab (Ctrl+T)").action_name("win.new-tab").build();
        sidebar_header.pack_end(&new_tab_btn);
        let sidebar = adw::ToolbarView::new();
        sidebar.set_top_bar_style(adw::ToolbarStyle::Flat);
        sidebar.add_top_bar(&sidebar_header);
        sidebar.set_content(Some(&sidebar_scroll));
        let sidebar_menu = gtk::PopoverMenu::from_model(None::<&gio::MenuModel>);
        sidebar_menu.set_has_arrow(false);
        sidebar_menu.set_parent(&sidebar_list);

        let split = adw::OverlaySplitView::new();
        split.set_sidebar(Some(&sidebar));
        split.set_content(Some(&content));
        split.set_min_sidebar_width(190.0);
        split.set_max_sidebar_width(280.0);
        split.set_sidebar_width_fraction(0.2);

        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&split));
        window.set_content(Some(&toast));
        let quicklook = QuickLook::new(thumbs.clone(), &window);

        let this = Rc::new(Window {
            window,
            toast,
            split,
            preview_split,
            preview,
            tab_views: [tab_views[0].clone(), tab_views[1].clone()],
            split_paned,
            split_boxes: [split_boxes[0].clone(), split_boxes[1].clone()],
            active_side: Cell::new(0),
            ending_split: Cell::new(false),
            pathbar,
            path_entry,
            path_stack,
            completion,
            journal: Journal::default(),
            quicklook,
            sidebar_list,
            sidebar_menu,
            back_btn,
            forward_btn,
            up_btn,
            view_btn,
            panes: RefCell::new(Vec::new()),
            thumbs,
            thumbnails_enabled: Cell::new(prefs.get_bool("thumbnails", true)),
            state: RefCell::new(WinState {
                show_hidden: prefs.get_bool("show-hidden", false),
                view_mode: prefs.get("view").unwrap_or_else(|| "list".into()),
                clipboard: None,
                places: Vec::new(),
            }),
            prefs,
            theme,
            tags: TagIndex::load(),
            syncing_sidebar: Cell::new(false),
            sidebar_target: Cell::new(None),
        });

        this.setup_tabs();
        this.setup_path_entry(&pathbar_click);
        this.setup_sidebar();
        this.setup_actions();
        this.open_in_new_tab(start, true);
        this
    }

    // ----- tabs ----------------------------------------------------------------

    fn setup_tabs(self: &Rc<Self>) {
        for side in 0..2 {
            let this = Rc::downgrade(self);
            self.tab_views[side].connect_selected_page_notify(move |_| {
                if let Some(w) = this.upgrade() {
                    if w.active_side.get() == side {
                        w.sync_active();
                    }
                }
            });
            // Pages close through the tab view itself; this only cleans up afterwards.
            let this = Rc::downgrade(self);
            self.tab_views[side].connect_page_detached(move |view, page, _| {
                let Some(w) = this.upgrade() else { return };
                if w.ending_split.get() {
                    return; // pages are being moved between sides, not closed
                }
                let child = page.child();
                w.panes.borrow_mut().retain(|p| p.root.upcast_ref::<gtk::Widget>() != &child);
                if view.n_pages() == 0 {
                    if w.is_split() {
                        w.end_split();
                    } else {
                        w.window.close();
                    }
                } else {
                    w.refocus();
                }
            });
        }
    }

    pub fn tab_view(&self) -> &adw::TabView {
        &self.tab_views[self.active_side.get()]
    }

    pub fn all_panes(&self) -> Vec<Rc<Pane>> {
        self.panes.borrow().clone()
    }

    pub fn set_active_side(self: &Rc<Self>, side: usize) {
        if self.active_side.get() == side {
            return;
        }
        self.active_side.set(side);
        for (i, b) in self.split_boxes.iter().enumerate() {
            if i == side {
                b.add_css_class("omafile-split-active");
            } else {
                b.remove_css_class("omafile-split-active");
            }
        }
        self.sync_active();
    }

    pub fn is_split(&self) -> bool {
        self.split_boxes[1].is_visible()
    }

    /// F6: show a second set of tabs beside the first, or hide it again.
    pub fn toggle_split(self: &Rc<Self>) {
        if self.is_split() {
            self.end_split();
        } else {
            let dir = self.active_pane().map(|p| p.cwd()).unwrap_or_else(|| gio::File::for_path(glib::home_dir()));
            self.split_boxes[1].set_visible(true);
            let width = self.split_paned.width();
            if width > 0 {
                self.split_paned.set_position(width / 2);
            }
            self.active_side.set(1);
            self.split_boxes[1].add_css_class("omafile-split-active");
            self.split_boxes[0].remove_css_class("omafile-split-active");
            self.open_in_new_tab(dir, true);
        }
    }

    /// Fold the right side into the left: its tabs move over, then it hides.
    fn end_split(self: &Rc<Self>) {
        self.ending_split.set(true);
        let (from, to) = (&self.tab_views[1], &self.tab_views[0]);
        while from.n_pages() > 0 {
            let page = from.nth_page(0);
            let child = page.child();
            if let Some(pane) = self.panes.borrow().iter().find(|p| p.root.upcast_ref::<gtk::Widget>() == &child) {
                pane.side.set(0);
            }
            from.transfer_page(&page, to, to.n_pages());
        }
        self.ending_split.set(false);
        self.split_boxes[1].set_visible(false);
        for b in &self.split_boxes {
            b.remove_css_class("omafile-split-active");
        }
        self.active_side.set(0);
        if to.n_pages() == 0 {
            self.window.close();
        } else {
            self.sync_active();
            self.refocus();
        }
    }

    pub fn open_in_new_tab(self: &Rc<Self>, file: gio::File, select: bool) {
        let side = self.active_side.get();
        let pane = Pane::new(self, file, side);
        // Register before appending so the selection notify can find the pane.
        self.panes.borrow_mut().push(pane.clone());
        let page = self.tab_views[side].append(&pane.root);
        page.set_title(&display_name(&pane.cwd()));
        page.set_icon(Some(&gio::ThemedIcon::new("folder-symbolic")));
        if select {
            self.tab_views[side].set_selected_page(&page);
        }
        pane.apply_view_mode();
        self.pane_navigated(&pane);
    }

    pub fn active_pane(&self) -> Option<Rc<Pane>> {
        let page = self.tab_view().selected_page()?;
        let child = page.child();
        self.panes.borrow().iter().find(|p| p.root.upcast_ref::<gtk::Widget>() == &child).cloned()
    }

    pub fn is_active_pane(&self, pane: &Rc<Pane>) -> bool {
        self.active_pane().map(|p| Rc::ptr_eq(&p, pane)).unwrap_or(false)
    }

    fn page_for(&self, pane: &Rc<Pane>) -> Option<adw::TabPage> {
        let view = &self.tab_views[pane.side.get()];
        (0..view.n_pages()).map(|i| view.nth_page(i)).find(|pg| pg.child() == *pane.root.upcast_ref::<gtk::Widget>())
    }

    pub fn close_active_tab(self: &Rc<Self>) {
        if let Some(page) = self.tab_view().selected_page() {
            self.tab_view().close_page(&page);
        }
    }

    /// Called by a pane whenever its directory changes.
    pub fn pane_navigated(self: &Rc<Self>, pane: &Rc<Pane>) {
        if let Some(page) = self.page_for(pane) {
            let cwd = pane.cwd();
            page.set_title(&display_name(&cwd));
            let icon = match cwd.uri_scheme().as_deref() {
                Some("trash") => "user-trash-symbolic",
                Some("sftp") | Some("ssh") | Some("smb") | Some("ftp") => "network-server-symbolic",
                Some("recent") => "document-open-recent-symbolic",
                _ if cwd.equal(&gio::File::for_path(glib::home_dir())) => "user-home-symbolic",
                _ => "folder-symbolic",
            };
            page.set_icon(Some(&gio::ThemedIcon::new(icon)));
        }
        if self.is_active_pane(pane) {
            self.sync_active();
        }
    }

    pub fn pane_selection_changed(self: &Rc<Self>, pane: &Rc<Pane>) {
        if self.is_active_pane(pane) {
            self.update_preview(pane);
        }
    }

    fn update_preview(&self, pane: &Rc<Pane>) {
        if !self.preview_split.shows_sidebar() {
            return;
        }
        let selected = pane.selected_items();
        match selected.as_slice() {
            [one] => self.preview.show(Some(one), 1),
            _ => self.preview.show(None, selected.len()),
        }
    }

    /// Refresh header, sidebar and preview for the active tab.
    pub fn sync_active(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let cwd = pane.cwd();
        self.back_btn.set_sensitive(pane.can_go_back());
        self.forward_btn.set_sensitive(pane.can_go_forward());
        self.up_btn.set_sensitive(cwd.parent().is_some());
        self.update_pathbar(&cwd);
        self.sync_sidebar(&cwd);
        self.window.set_title(Some(&format!("{} - Files", display_name(&cwd))));
        self.update_preview(&pane);
        self.path_stack.set_visible_child_name("bar");
    }

    pub fn navigate(self: &Rc<Self>, file: gio::File) {
        if let Some(pane) = self.active_pane() {
            pane.navigate(file, true);
        }
    }

    // ----- path bar ------------------------------------------------------------

    fn setup_path_entry(self: &Rc<Self>, pathbar_click: &gtk::GestureClick) {
        let this = Rc::downgrade(self);
        pathbar_click.connect_released(move |g, _, x, y| {
            // A click on the empty part of the path bar opens the location entry.
            let (Some(w), Some(widget)) = (this.upgrade(), g.widget()) else { return };
            let hit_button = widget.pick(x, y, gtk::PickFlags::DEFAULT).map(|p| p.is::<gtk::Button>() || p.ancestor(gtk::Button::static_type()).is_some()).unwrap_or(false);
            if !hit_button {
                w.show_location_entry();
            }
        });
        let this = Rc::downgrade(self);
        self.path_entry.connect_changed(move |_| {
            let Some(w) = this.upgrade() else { return };
            // Only while the entry is shown; programmatic updates of the hidden entry stay silent.
            if w.path_stack.visible_child_name().as_deref() != Some("entry") {
                return;
            }
            let cwd = w.active_pane().map(|p| p.cwd()).unwrap_or_else(|| gio::File::for_path(glib::home_dir()));
            w.completion.refresh(cwd);
        });
        let this = Rc::downgrade(self);
        self.path_entry.connect_activate(move |entry| {
            let Some(w) = this.upgrade() else { return };
            if std::env::var_os("OMAFILE_TRACE").is_some() {
                eprintln!("path activate {:?}", entry.text());
            }
            w.completion.hide();
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
            } else if let Some(uri) = remote::parse_server(&expanded).filter(|_| expanded.contains('@') || expanded.contains(':')) {
                gio::File::for_uri(&uri)
            } else {
                w.active_pane().map(|p| p.cwd().resolve_relative_path(&expanded)).unwrap_or_else(|| gio::File::for_path(&expanded))
            };
            w.path_stack.set_visible_child_name("bar");
            if file.is_native() {
                w.navigate(file);
            } else {
                w.connect_to(file, false);
            }
        });
        let this = Rc::downgrade(self);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(w) = this.upgrade() else { return glib::Propagation::Proceed };
            match key {
                gdk::Key::Escape => {
                    if w.completion.is_open() {
                        w.completion.hide();
                    } else {
                        w.path_stack.set_visible_child_name("bar");
                        if let Some(p) = w.active_pane() {
                            p.focus_view();
                        }
                    }
                    glib::Propagation::Stop
                }
                gdk::Key::Tab | gdk::Key::ISO_Left_Tab => {
                    // Tab completes; it never moves focus away from the path.
                    if w.completion.is_open() {
                        w.completion.accept_selected(false);
                    }
                    glib::Propagation::Stop
                }
                gdk::Key::Down if w.completion.is_open() => {
                    w.completion.step(1);
                    glib::Propagation::Stop
                }
                gdk::Key::Up if w.completion.is_open() => {
                    w.completion.step(-1);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.path_entry.add_controller(keys);
        let this = Rc::downgrade(self);
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(move |_| {
            if let Some(w) = this.upgrade() {
                w.completion.hide();
            }
        });
        self.path_entry.add_controller(focus);
    }

    pub fn show_location_entry(&self) {
        if let Some(p) = self.active_pane() {
            self.path_entry.set_text(&location_text(&p.cwd()));
        }
        self.path_stack.set_visible_child_name("entry");
        self.path_entry.grab_focus();
        // Cursor at the end so the path can be edited in place; Ctrl+A still selects it all.
        self.path_entry.select_region(-1, -1);
        self.path_entry.set_position(-1);
    }

    fn update_pathbar(self: &Rc<Self>, cwd: &gio::File) {
        while let Some(child) = self.pathbar.first_child() {
            self.pathbar.remove(&child);
        }
        if let Some(title) = self.active_pane().and_then(|p| p.virtual_title()) {
            let button = gtk::Button::new();
            button.add_css_class("flat");
            button.add_css_class("current");
            button.set_child(Some(&icon_label("tag-symbolic", &title)));
            let this = Rc::downgrade(self);
            let back = cwd.clone();
            button.connect_clicked(move |_| {
                if let Some(w) = this.upgrade() {
                    w.navigate(back.clone());
                }
            });
            self.pathbar.append(&button);
            return;
        }
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
                let sep = gtk::Label::new(Some("›"));
                sep.add_css_class("separator");
                self.pathbar.append(&sep);
            }
            let button = gtk::Button::new();
            button.add_css_class("flat");
            if file.equal(&home) {
                button.set_child(Some(&icon_label("user-home-symbolic", "Home")));
            } else if file.parent().is_none() {
                let (icon, text) = root_label(&file);
                button.set_child(Some(&icon_label(icon, &text)));
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
                    w.navigate(target.clone());
                }
            });
            self.pathbar.append(&button);
        }
    }

    // ----- sidebar ---------------------------------------------------------------

    fn setup_sidebar(self: &Rc<Self>) {
        self.rebuild_places();
        let this = Rc::downgrade(self);
        self.sidebar_list.connect_row_activated(move |_, row| {
            let Some(w) = this.upgrade() else { return };
            if w.syncing_sidebar.get() {
                return;
            }
            let place = {
                let state = w.state.borrow();
                match state.places.get(row.index() as usize) {
                    Some(Place::Entry { file, kind, .. }) => Some((file.clone(), kind.clone())),
                    Some(Place::Action { action, .. }) => {
                        gio::prelude::ActionGroupExt::activate_action(&w.window, action, None);
                        None
                    }
                    _ => None,
                }
            };
            if let Some((file, kind)) = place {
                match kind {
                    PlaceKind::Ssh(_) => w.connect_to(file, true),
                    PlaceKind::Tag(color) => {
                        if let Some(p) = w.active_pane() {
                            p.show_tag(&color);
                        }
                    }
                    _ => w.navigate(file),
                }
            }
        });
        let this = Rc::downgrade(self);
        let right = gtk::GestureClick::new();
        right.set_button(gdk::BUTTON_SECONDARY);
        right.connect_pressed(move |_, _, x, y| {
            let Some(w) = this.upgrade() else { return };
            let Some(row) = w.sidebar_list.row_at_y(y as i32) else { return };
            let index = row.index() as usize;
            let model = {
                let state = w.state.borrow();
                match state.places.get(index) {
                    Some(Place::Entry { kind: PlaceKind::Bookmark, .. }) => menu_of(&[("Open in New Tab", "win.sidebar-new-tab"), ("Remove Bookmark", "win.sidebar-remove-bookmark")]),
                    Some(Place::Entry { kind: PlaceKind::Tag(_), .. }) => return,
                    Some(Place::Entry { kind: PlaceKind::Mount(m), .. }) if m.can_unmount() => menu_of(&[("Open in New Tab", "win.sidebar-new-tab"), ("Unmount", "win.sidebar-unmount")]),
                    Some(Place::Entry { .. }) => menu_of(&[("Open in New Tab", "win.sidebar-new-tab")]),
                    _ => return,
                }
            };
            w.sidebar_target.set(Some(index));
            w.sidebar_menu.set_menu_model(Some(&model));
            w.sidebar_menu.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            w.sidebar_menu.popup();
        });
        self.sidebar_list.add_controller(right);

        // Dropping files on a sidebar place copies/moves them there.
        let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY | gdk::DragAction::MOVE);
        let this = Rc::downgrade(self);
        target.connect_drop(move |_, value, _, y| {
            let Some(w) = this.upgrade() else { return false };
            let Ok(list) = value.get::<gdk::FileList>() else { return false };
            let Some(row) = w.sidebar_list.row_at_y(y as i32) else { return false };
            let dest = match w.state.borrow().places.get(row.index() as usize) {
                Some(Place::Entry { file, kind, .. }) if !matches!(kind, PlaceKind::Ssh(_) | PlaceKind::Tag(_)) => file.clone(),
                _ => return false,
            };
            if let Some(p) = w.active_pane() {
                p.receive_files(list.files(), dest);
            }
            true
        });
        self.sidebar_list.add_controller(target);

        let monitor = gio::VolumeMonitor::get();
        for signal in ["mount-added", "mount-removed", "mount-changed"] {
            let this = Rc::downgrade(self);
            monitor.connect_local(signal, false, move |_| {
                if let Some(w) = this.upgrade() {
                    w.refresh_sidebar();
                }
                None
            });
        }
        unsafe { self.window.set_data("volume-monitor", monitor) };
    }

    pub fn refresh_sidebar(self: &Rc<Self>) {
        self.rebuild_places();
        if let Some(p) = self.active_pane() {
            self.sync_sidebar(&p.cwd());
        }
    }

    pub fn sidebar_target_place(&self) -> Option<(gio::File, PlaceKind)> {
        let index = self.sidebar_target.get()?;
        match self.state.borrow().places.get(index) {
            Some(Place::Entry { file, kind, .. }) => Some((file.clone(), kind.clone())),
            _ => None,
        }
    }

    fn rebuild_places(&self) {
        let mut places = vec![Place::Header("Places".into())];
        let home = glib::home_dir();
        let mut add = |name: &str, icon: &str, path: std::path::PathBuf| {
            if path.is_dir() {
                places.push(Place::Entry { name: name.into(), icon: icon.into(), file: gio::File::for_path(path), kind: PlaceKind::Builtin });
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
        places.push(Place::Entry { name: "Recent".into(), icon: "document-open-recent-symbolic".into(), file: gio::File::for_uri("recent:///"), kind: PlaceKind::Builtin });
        places.push(Place::Entry { name: "Trash".into(), icon: "user-trash-symbolic".into(), file: gio::File::for_uri("trash:///"), kind: PlaceKind::Builtin });

        let bookmarks = remote::bookmarks();
        if !bookmarks.is_empty() {
            places.push(Place::Header("Bookmarks".into()));
            for b in bookmarks {
                let icon = if b.file.is_native() { "folder-symbolic" } else { "folder-remote-symbolic" };
                places.push(Place::Entry { name: b.name, icon: icon.into(), file: b.file, kind: PlaceKind::Bookmark });
            }
        }

        let used = self.tags.colors_in_use();
        if !used.is_empty() {
            places.push(Place::Header("Tags".into()));
            for color in used {
                places.push(Place::Entry {
                    name: crate::tags::label(&color),
                    icon: format!("tag:{color}"),
                    file: gio::File::for_uri(&format!("omafile-tag:///{color}")),
                    kind: PlaceKind::Tag(color),
                });
            }
        }
        places.push(Place::Header("Remote".into()));
        for host in remote::ssh_hosts() {
            places.push(Place::Entry {
                name: host.alias.clone(),
                icon: "network-server-symbolic".into(),
                file: gio::File::for_uri(&host.uri()),
                kind: PlaceKind::Ssh(host.alias),
            });
        }
        places.push(Place::Action { name: "Connect to Server…".into(), icon: "network-workgroup-symbolic".into(), action: "connect" });

        places.push(Place::Header("Devices".into()));
        places.push(Place::Entry { name: "Computer".into(), icon: "drive-harddisk-symbolic".into(), file: gio::File::for_path("/"), kind: PlaceKind::Builtin });
        for mount in gio::VolumeMonitor::get().mounts() {
            if mount.is_shadowed() {
                continue;
            }
            let icon = mount
                .symbolic_icon()
                .downcast::<gio::ThemedIcon>()
                .ok()
                .and_then(|i| i.names().first().map(|n| n.to_string()))
                .unwrap_or_else(|| "drive-removable-media-symbolic".into());
            places.push(Place::Entry { name: mount.name().to_string(), icon, file: mount.default_location(), kind: PlaceKind::Mount(mount) });
        }

        // The context popover is also a child of the list box, so remove rows by index.
        while let Some(row) = self.sidebar_list.row_at_index(0) {
            self.sidebar_list.remove(&row);
        }
        for place in &places {
            let row = gtk::ListBoxRow::new();
            match place {
                Place::Header(title) => {
                    let label = gtk::Label::builder().label(title).xalign(0.0).margin_start(8).margin_top(10).margin_bottom(2).build();
                    label.add_css_class("caption-heading");
                    label.add_css_class("omafile-dim");
                    row.set_child(Some(&label));
                    row.set_selectable(false);
                    row.set_activatable(false);
                }
                Place::Entry { name, icon, kind, .. } => {
                    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                    if let Some(color) = icon.strip_prefix("tag:") {
                        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        dot.add_css_class("omafile-tag-dot");
                        dot.add_css_class("omafile-tag-sidebar");
                        dot.add_css_class(&format!("omafile-tag-{color}"));
                        dot.set_valign(gtk::Align::Center);
                        hbox.append(&dot);
                    } else {
                        hbox.append(&gtk::Image::from_icon_name(icon));
                    }
                    hbox.append(&gtk::Label::builder().label(name).xalign(0.0).hexpand(true).ellipsize(pango::EllipsizeMode::End).build());
                    if let PlaceKind::Mount(m) = kind {
                        if m.can_unmount() {
                            let eject = gtk::Image::from_icon_name("media-eject-symbolic");
                            eject.add_css_class("omafile-dim");
                            hbox.append(&eject);
                        }
                    }
                    row.set_child(Some(&hbox));
                }
                Place::Action { name, icon, .. } => {
                    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                    hbox.append(&gtk::Image::from_icon_name(icon));
                    hbox.append(&gtk::Label::builder().label(name).xalign(0.0).ellipsize(pango::EllipsizeMode::End).build());
                    hbox.add_css_class("omafile-dim");
                    row.set_child(Some(&hbox));
                    row.set_selectable(false);
                }
            }
            self.sidebar_list.append(&row);
        }
        self.state.borrow_mut().places = places;
    }

    fn sync_sidebar(&self, cwd: &gio::File) {
        let tag = self.active_pane().and_then(|p| p.virtual_title()).and_then(|t| t.strip_prefix("Tag: ").map(|c| c.to_lowercase()));
        let index = {
            let state = self.state.borrow();
            match tag {
                Some(color) => state.places.iter().position(|p| matches!(p, Place::Entry { kind: PlaceKind::Tag(c), .. } if *c == color)),
                None => state.places.iter().position(|p| matches!(p, Place::Entry { file, kind, .. } if file.equal(cwd) && !matches!(kind, PlaceKind::Tag(_)))),
            }
        };
        self.syncing_sidebar.set(true);
        match index.and_then(|i| self.sidebar_list.row_at_index(i as i32)) {
            Some(row) => self.sidebar_list.select_row(Some(&row)),
            None => self.sidebar_list.unselect_all(),
        }
        self.syncing_sidebar.set(false);
    }

    // ----- remote --------------------------------------------------------------------

    /// Mount a remote location (asking for credentials through a GTK dialog) and open it.
    pub fn connect_to(self: &Rc<Self>, file: gio::File, new_tab: bool) {
        let op = gtk::MountOperation::new(Some(&self.window));
        let this = Rc::downgrade(self);
        let label = file.uri();
        glib::spawn_future_local(async move {
            let result = remote::ensure_mounted(&file, &op).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(()) => {
                    // SFTP: prefer the remote home directory when the root was given.
                    let target = if file.uri_scheme().as_deref() == Some("sftp") && file.parent().is_none() {
                        let home = file.resolve_relative_path("~");
                        if home.query_exists(gio::Cancellable::NONE) { home } else { file }
                    } else {
                        file
                    };
                    if new_tab { w.open_in_new_tab(target, true) } else { w.navigate(target) }
                    w.refresh_sidebar();
                }
                Err(e) if e.matches(gio::IOErrorEnum::FailedHandled) => {}
                Err(e) => w.toast_error(&format!("Cannot connect to {label}: {}", e.message())),
            }
        });
    }

    pub fn unmount(self: &Rc<Self>, mount: gio::Mount) {
        let op = gtk::MountOperation::new(Some(&self.window));
        let this = Rc::downgrade(self);
        let name = mount.name().to_string();
        let root = mount.root();
        glib::spawn_future_local(async move {
            let result = mount.unmount_with_operation_future(gio::MountUnmountFlags::NONE, Some(&op)).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(()) => {
                    // Tabs that were inside the mount go home.
                    let panes: Vec<Rc<Pane>> = w.panes.borrow().clone();
                    for p in panes {
                        if p.cwd().has_prefix(&root) || p.cwd().equal(&root) {
                            p.navigate(gio::File::for_path(glib::home_dir()), true);
                        }
                    }
                    w.toast(&format!("{name} unmounted"));
                    w.refresh_sidebar();
                }
                Err(e) => w.toast_error(&format!("Cannot unmount {name}: {}", e.message())),
            }
        });
    }

    pub fn tag_selected(self: &Rc<Self>, color: Option<String>) {
        let items = self.selected_items();
        if items.is_empty() {
            return;
        }
        let mut changes = Vec::new();
        for item in &items {
            let old = self.tags.get(&item.file());
            if let Err(e) = self.tags.set(&item.file(), color.as_deref()) {
                self.toast_error(&format!("Cannot tag {}: {}", item.display_name(), e.message()));
                return;
            }
            changes.push((item.file(), old, color.clone()));
        }
        let message = match &color {
            Some(c) => format!("Tagged {} {} {}", items.len(), if items.len() == 1 { "item" } else { "items" }, crate::tags::label(c).to_lowercase()),
            None => "Tag removed".to_string(),
        };
        self.record(Op::Tag(changes), &message);
        for p in self.all_panes() {
            p.reload();
        }
        self.refresh_sidebar();
    }

    /// Space: large preview of the selected file, like Quick Look.
    pub fn toggle_quick_look(self: &Rc<Self>) {
        if std::env::var_os("OMAFILE_TRACE").is_some() {
            eprintln!("quick-look toggle, selected={}", self.selected_items().len());
        }
        if let Some(pane) = self.active_pane() {
            self.quicklook.toggle(&pane, &self.window);
        }
    }

    // ----- undo / redo ----------------------------------------------------------------

    /// Record an operation and show a toast with an Undo button.
    pub fn record(&self, op: Op, message: &str) {
        self.journal.record(op);
        let toast = adw::Toast::builder().title(message).timeout(5).button_label("Undo").action_name("win.undo").build();
        self.toast.add_toast(toast);
    }

    pub fn undo(self: &Rc<Self>) {
        let Some(op) = self.journal.pop_undo() else {
            self.toast("Nothing to undo");
            return;
        };
        self.apply_history(op, true);
    }

    pub fn redo(self: &Rc<Self>) {
        let Some(op) = self.journal.pop_redo() else {
            self.toast("Nothing to redo");
            return;
        };
        self.apply_history(op, false);
    }

    fn apply_history(self: &Rc<Self>, op: Op, is_undo: bool) {
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Some(w) = this.upgrade() else { return };
            let to_apply = if is_undo { op.inverse() } else { op.clone() };
            let result = crate::undo::apply(&to_apply, &w.tags).await;
            match result {
                Ok(_) => {
                    if is_undo {
                        w.journal.push_redo(op.clone());
                        w.toast(&format!("Undid {}", op.describe()));
                    } else {
                        w.journal.push_undo_only(op.clone());
                        w.toast(&format!("Redid {}", op.describe()));
                    }
                }
                Err(e) => {
                    // Put it back so the user can retry after fixing the cause.
                    if is_undo { w.journal.push_undo_only(op) } else { w.journal.push_redo(op) }
                    w.toast_error(&format!("Cannot {}: {}", if is_undo { "undo" } else { "redo" }, e.message()));
                }
            }
            for p in w.all_panes() {
                p.reload();
            }
            w.refresh_sidebar();
        });
    }

    // ----- misc helpers -------------------------------------------------------------

    pub fn show_hidden(&self) -> bool {
        self.state.borrow().show_hidden
    }

    pub fn view_mode(&self) -> String {
        self.state.borrow().view_mode.clone()
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

    pub fn toast(&self, text: &str) {
        self.toast.add_toast(adw::Toast::builder().title(text).timeout(3).build());
    }

    pub fn toast_error(&self, text: &str) {
        self.toast.add_toast(adw::Toast::builder().title(text).timeout(6).build());
    }

    pub fn selected_items(&self) -> Vec<FileItem> {
        self.active_pane().map(|p| p.selected_items()).unwrap_or_default()
    }
}

fn nav_button(icon: &str, tooltip: &str, action: &str) -> gtk::Button {
    gtk::Button::builder().icon_name(icon).tooltip_text(tooltip).action_name(action).build()
}

fn icon_label(icon: &str, text: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    b.append(&gtk::Image::from_icon_name(icon));
    b.append(&gtk::Label::new(Some(text)));
    b
}

fn menu_of(entries: &[(&str, &str)]) -> gio::Menu {
    let menu = gio::Menu::new();
    for (label, action) in entries {
        menu.append(Some(label), Some(action));
    }
    menu
}

fn root_label(file: &gio::File) -> (&'static str, String) {
    match file.uri_scheme().as_deref() {
        Some("trash") => ("user-trash-symbolic", "Trash".into()),
        Some("recent") => ("document-open-recent-symbolic", "Recent".into()),
        Some("file") | None => ("drive-harddisk-symbolic", "Computer".into()),
        Some(_) => {
            // sftp://user@host/ -> host
            let uri = file.uri();
            let host = uri.split("://").nth(1).unwrap_or("").trim_end_matches('/').rsplit('@').next().unwrap_or("").to_string();
            ("network-server-symbolic", if host.is_empty() { uri.to_string() } else { host })
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
    root_label(file).1
}

pub fn location_text(file: &gio::File) -> String {
    match file.path() {
        Some(p) => abbreviate_home(&p),
        None => file.uri().to_string(),
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
    let s0 = gio::Menu::new();
    s0.append(Some("Undo"), Some("win.undo"));
    s0.append(Some("Redo"), Some("win.redo"));
    menu.append_section(None, &s0);
    let s1 = gio::Menu::new();
    s1.append(Some("New Tab"), Some("win.new-tab"));
    s1.append(Some("New Folder"), Some("win.new-folder"));
    s1.append(Some("New File"), Some("win.new-file"));
    s1.append(Some("Open Terminal Here"), Some("win.terminal"));
    s1.append(Some("Connect to Server…"), Some("win.connect"));
    menu.append_section(None, &s1);
    let s2 = gio::Menu::new();
    s2.append(Some("Add Bookmark"), Some("win.bookmark"));
    s2.append(Some("Show Hidden Files"), Some("win.show-hidden"));
    s2.append(Some("Show Sidebar"), Some("win.show-sidebar"));
    s2.append(Some("Show Preview"), Some("win.show-preview"));
    s2.append(Some("Split View"), Some("win.split-view"));
    menu.append_section(None, &s2);
    let view = gio::Menu::new();
    view.append(Some("List"), Some("win.view::list"));
    view.append(Some("Grid"), Some("win.view::grid"));
    menu.append_section(Some("View"), &view);
    let sort = gio::Menu::new();
    sort.append(Some("Name"), Some("win.sort::name"));
    sort.append(Some("Size"), Some("win.sort::size"));
    sort.append(Some("Type"), Some("win.sort::type"));
    sort.append(Some("Modified"), Some("win.sort::modified"));
    menu.append_submenu(Some("Sort By"), &sort);
    let s3 = gio::Menu::new();
    s3.append(Some("Preferences"), Some("win.preferences"));
    s3.append(Some("Keyboard Shortcuts"), Some("win.shortcuts"));
    s3.append(Some("About Files"), Some("app.about"));
    menu.append_section(None, &s3);
    menu
}

pub fn item_menu_model(single_dir: bool, archive: bool, in_trash: bool, multiple: bool) -> gio::Menu {
    let menu = gio::Menu::new();
    let open = gio::Menu::new();
    open.append(Some("Open"), Some("win.open"));
    open.append(Some("Quick Look"), Some("win.quick-look"));
    if single_dir {
        open.append(Some("Open in New Tab"), Some("win.open-new-tab"));
        open.append(Some("Open in Terminal"), Some("win.terminal-selected"));
    } else {
        open.append(Some("Open With…"), Some("win.open-with"));
    }
    if archive {
        open.append(Some("Extract Here"), Some("win.extract"));
    }
    menu.append_section(None, &open);
    let edit = gio::Menu::new();
    if in_trash {
        edit.append(Some("Restore"), Some("win.restore"));
    } else {
        edit.append(Some("Cut"), Some("win.cut"));
        edit.append(Some("Copy"), Some("win.copy"));
        edit.append(Some("Copy Path"), Some("win.copy-path"));
        edit.append(Some("Copy To…"), Some("win.copy-to"));
        edit.append(Some("Move To…"), Some("win.move-to"));
        edit.append(Some("Duplicate"), Some("win.duplicate"));
        edit.append(Some(if multiple { "Batch Rename…" } else { "Rename…" }), Some("win.rename"));
        edit.append(Some("Compress…"), Some("win.compress"));
        if !multiple {
            edit.append(Some("Change Icon…"), Some("win.change-icon"));
        }
        let tags = gio::Menu::new();
        for (name, _, _) in crate::tags::COLORS {
            tags.append(Some(&crate::tags::label(name)), Some(&format!("win.tag::{name}")));
        }
        tags.append(Some("No Tag"), Some("win.tag::none"));
        edit.append_submenu(Some("Tag"), &tags);
        if single_dir {
            edit.append(Some("Add Bookmark"), Some("win.bookmark-selected"));
        }
    }
    menu.append_section(None, &edit);
    let danger = gio::Menu::new();
    if !in_trash {
        danger.append(Some("Move to Trash"), Some("win.trash"));
    }
    danger.append(Some("Delete Permanently…"), Some("win.delete"));
    menu.append_section(None, &danger);
    let info = gio::Menu::new();
    info.append(Some("Properties"), Some("win.properties"));
    menu.append_section(None, &info);
    menu
}

pub fn background_menu_model(in_trash: bool) -> gio::Menu {
    let menu = gio::Menu::new();
    if in_trash {
        let s = gio::Menu::new();
        s.append(Some("Empty Trash…"), Some("win.empty-trash"));
        s.append(Some("Select All"), Some("win.select-all"));
        menu.append_section(None, &s);
        return menu;
    }
    let s1 = gio::Menu::new();
    s1.append(Some("New Folder"), Some("win.new-folder"));
    s1.append(Some("New File"), Some("win.new-file"));
    menu.append_section(None, &s1);
    let s2 = gio::Menu::new();
    s2.append(Some("Paste"), Some("win.paste"));
    s2.append(Some("Undo"), Some("win.undo"));
    s2.append(Some("Redo"), Some("win.redo"));
    s2.append(Some("Select All"), Some("win.select-all"));
    menu.append_section(None, &s2);
    let s3 = gio::Menu::new();
    s3.append(Some("Open Terminal Here"), Some("win.terminal"));
    s3.append(Some("Add Bookmark"), Some("win.bookmark"));
    s3.append(Some("Copy Path"), Some("win.copy-cwd-path"));
    s3.append(Some("Properties"), Some("win.cwd-properties"));
    menu.append_section(None, &s3);
    menu
}
