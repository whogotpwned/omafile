//! Main window shell: sidebar, header bar with path bar, tabs, preview pane.

use crate::i18n::tr;
use crate::item::FileItem;
use crate::pane::Pane;
use crate::prefs::Prefs;
use crate::preview::PreviewPane;
use crate::palette::{Entry as PaletteEntry, Palette, Target as PaletteTarget};
use crate::projects;
use crate::quicklook::QuickLook;
use crate::remote;
use crate::completion::Completion;
use crate::notes::Notes;
use crate::tags::TagIndex;
use crate::theme::ThemeManager;
use crate::transfer::{Mode, Progress, Queue, Request};
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
    Smart(crate::smart::SmartFolder),
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
    pub palette: RefCell<Option<Rc<Palette>>>,
    pub recent: RefCell<Vec<gio::File>>,
    pub queue: Queue,
    pub progress: Progress,
    pub containers: RefCell<Vec<crate::containers::Volume>>,
    session_saved: Cell<bool>,
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
    pub notes: Rc<Notes>,
    pub shelf: RefCell<Option<Rc<crate::shelf::Shelf>>>,
    pub rule_monitors: RefCell<Vec<gio::FileMonitor>>,
    /// Mounts already warned about for low disk space.
    pub low_space_warned: RefCell<Vec<String>>,
    pub trash_summary: RefCell<Option<(usize, u64)>>,
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
            .title(tr("Files"))
            .build();
        window.add_css_class("omafile");
        let thumbs = ThumbCache::new();

        // ---- header ---------------------------------------------------------
        let back_btn = nav_button("go-previous-symbolic", &tr("Back (Alt+←)"), "win.back");
        let forward_btn = nav_button("go-next-symbolic", &tr("Forward (Alt+→)"), "win.forward");
        let up_btn = nav_button("go-up-symbolic", &tr("Parent folder (Alt+↑)"), "win.up");
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
            .placeholder_text(tr("Type a path, ~/folder or sftp://host/"))
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

        let search_btn = gtk::Button::builder().icon_name("edit-find-symbolic").tooltip_text(tr("Filter (Ctrl+F)")).action_name("win.search").build();
        let view_btn = gtk::Button::builder().icon_name("view-grid-symbolic").tooltip_text(tr("Grid view (Ctrl+2)")).action_name("win.toggle-view").build();
        let preview_btn = gtk::ToggleButton::builder().icon_name("sidebar-show-right-symbolic").tooltip_text(tr("Preview pane (F3)")).action_name("win.show-preview").build();
        let hidden_btn = gtk::ToggleButton::builder().icon_name("view-conceal-symbolic").tooltip_text(tr("Show hidden files (Ctrl+H)")).action_name("win.show-hidden").build();
        hidden_btn.connect_active_notify(|b| {
            b.set_icon_name(if b.is_active() { "view-reveal-symbolic" } else { "view-conceal-symbolic" });
            b.set_tooltip_text(Some(&if b.is_active() { tr("Hide hidden files (Ctrl+H)") } else { tr("Show hidden files (Ctrl+H)") }));
        });
        let menu_button = gtk::MenuButton::builder().icon_name("open-menu-symbolic").tooltip_text(tr("Menu")).menu_model(&main_menu()).primary(true).build();

        let header = adw::HeaderBar::new();
        header.add_css_class("omafile-header");
        header.pack_start(&nav_box);
        header.pack_start(&up_btn);
        header.set_title_widget(Some(&path_stack));
        header.pack_end(&menu_button);
        header.pack_end(&preview_btn);
        header.pack_end(&view_btn);
        header.pack_end(&hidden_btn);
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
        brand.append(&gtk::Label::builder().label(tr("Files")).css_classes(["title-4"]).build());
        sidebar_header.set_title_widget(Some(&brand));
        sidebar_header.set_show_end_title_buttons(false);
        let new_tab_btn = gtk::Button::builder().icon_name("tab-new-symbolic").tooltip_text(tr("New tab (Ctrl+T)")).action_name("win.new-tab").build();
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

        let progress = Progress::new();
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer.append(&split);
        outer.append(&progress.revealer);
        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&outer));
        window.set_content(Some(&toast));
        let quicklook = QuickLook::new(thumbs.clone(), &window);

        let this = Rc::new(Window {
            window,
            shelf: RefCell::new(None),
            rule_monitors: RefCell::new(Vec::new()),
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
            palette: RefCell::new(None),
            recent: RefCell::new(crate::status::load_recent().iter().map(|l| if l.contains("://") { gio::File::for_uri(l) } else { gio::File::for_path(l) }).collect()),
            queue: Queue::default(),
            progress,
            containers: RefCell::new(Vec::new()),
            session_saved: Cell::new(false),
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
            notes: Notes::load(),
            low_space_warned: RefCell::new(Vec::new()),
            trash_summary: RefCell::new(None),
            syncing_sidebar: Cell::new(false),
            sidebar_target: Cell::new(None),
        });

        {
            let weak = Rc::downgrade(&this);
            this.progress.cancel_button().connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() {
                    w.cancel_transfer();
                }
            });
        }
        this.setup_tabs();
        this.setup_path_entry(&pathbar_click);
        this.setup_sidebar();
        this.setup_actions();
        this.open_in_new_tab(start, true);
        this.refresh_containers();
        this.refresh_trash_summary();
        crate::trashauto::run_auto_empty(&this);
        crate::rules::watch(&this);
        {
            let weak = Rc::downgrade(&this);
            glib::spawn_future_local(async move {
                let Some(w) = weak.upgrade() else { return };
                if let Some(summary) = crate::rules::run(&w, None).await {
                    w.toast(&summary);
                    for p in w.panes.borrow().clone() {
                        p.reload();
                    }
                }
            });
        }
        {
            // Close button and Ctrl+Q (app.quit destroys the window without a
            // close-request) both end here; the first one wins.
            let weak = Rc::downgrade(&this);
            this.window.connect_close_request(move |_| {
                if let Some(w) = weak.upgrade() {
                    w.save_session_once();
                    w.cancel_transfer();
                }
                glib::Propagation::Proceed
            });
            let weak = Rc::downgrade(&this);
            this.window.connect_unrealize(move |_| {
                if let Some(w) = weak.upgrade() {
                    w.save_session_once();
                }
            });
        }
        this
    }

    pub fn save_session_once(&self) {
        if self.session_saved.replace(true) {
            return;
        }
        if self.prefs.get_bool("restore-session", true) {
            self.snapshot_session().save();
        }
    }

    /// Count and size of the trash for the sidebar label.
    pub fn refresh_trash_summary(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let summary = crate::trashauto::summary().await;
            let Some(w) = this.upgrade() else { return };
            if *w.trash_summary.borrow() != Some(summary) {
                *w.trash_summary.borrow_mut() = Some(summary);
                w.refresh_sidebar();
            }
        });
    }

    /// Docker / podman volumes for the sidebar, fetched off the main loop.
    pub fn refresh_containers(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let volumes = crate::containers::volumes().await;
            let Some(w) = this.upgrade() else { return };
            if *w.containers.borrow() != volumes {
                *w.containers.borrow_mut() = volumes;
                w.refresh_sidebar();
            }
        });
    }

    // ----- sessions ---------------------------------------------------------------

    pub fn snapshot_session(&self) -> crate::session::Session {
        let mut s = crate::session::Session {
            width: self.window.width(),
            height: self.window.height(),
            maximized: self.window.is_maximized(),
            split: self.is_split(),
            sidebar: self.split.shows_sidebar(),
            active_side: self.active_side.get(),
            ..Default::default()
        };
        let panes = self.panes.borrow();
        for side in 0..2 {
            let view = &self.tab_views[side];
            for i in 0..view.n_pages() {
                let page = view.nth_page(i);
                let Some(pane) = panes.iter().find(|p| p.root.upcast_ref::<gtk::Widget>() == &page.child()) else { continue };
                s.sides[side].push((pane.cwd().uri().to_string(), pane.terminal_visible()));
                if view.selected_page().as_ref() == Some(&page) {
                    s.selected[side] = i as usize;
                }
            }
        }
        s
    }

    /// Rebuild tabs, split and terminal panels from a saved session.
    pub fn restore_session(self: &Rc<Self>, s: &crate::session::Session) {
        if s.width > 400 && s.height > 300 {
            self.window.set_default_size(s.width, s.height);
        }
        if s.maximized {
            self.window.maximize();
        }
        self.split.set_show_sidebar(s.sidebar);
        // Replace the single start tab with the saved ones.
        let initial: Vec<adw::TabPage> = (0..self.tab_views[0].n_pages()).map(|i| self.tab_views[0].nth_page(i)).collect();
        for side in 0..2 {
            if s.sides[side].is_empty() {
                continue;
            }
            if side == 1 {
                self.split_boxes[1].set_visible(true);
            }
            self.active_side.set(side);
            for (uri, _) in &s.sides[side] {
                self.open_in_new_tab(gio::File::for_uri(uri), false);
            }
        }
        for page in initial {
            self.tab_views[0].close_page(&page);
        }
        for side in 0..2 {
            let view = &self.tab_views[side];
            if view.n_pages() == 0 {
                continue;
            }
            let idx = (s.selected[side] as i32).min(view.n_pages() - 1);
            view.set_selected_page(&view.nth_page(idx));
        }
        let side = if self.is_split() { s.active_side.min(1) } else { 0 };
        self.active_side.set(side);
        if self.is_split() {
            self.split_boxes[side].add_css_class("omafile-split-active");
            self.split_boxes[1 - side].remove_css_class("omafile-split-active");
            let this = Rc::downgrade(self);
            glib::idle_add_local_once(move || {
                if let Some(w) = this.upgrade() {
                    let width = w.split_paned.width();
                    if width > 0 {
                        w.split_paned.set_position(width / 2);
                    }
                }
            });
        }
        // Terminal panels once the widgets are realized.
        let wanted: Vec<(usize, usize)> = (0..2).flat_map(|side| s.sides[side].iter().enumerate().filter(|(_, (_, t))| *t).map(move |(i, _)| (side, i))).collect();
        if !wanted.is_empty() {
            let this = Rc::downgrade(self);
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                let Some(w) = this.upgrade() else { return };
                let panes = w.panes.borrow().clone();
                for (side, i) in wanted {
                    let view = &w.tab_views[side];
                    if (i as i32) < view.n_pages() {
                        let child = view.nth_page(i as i32).child();
                        if let Some(pane) = panes.iter().find(|p| p.root.upcast_ref::<gtk::Widget>() == &child) {
                            if !pane.terminal_visible() {
                                pane.toggle_terminal();
                            }
                        }
                    }
                }
            });
        }
        self.sync_active();
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
                self.split_paned.set_position(width);
                crate::motion::slide_paned(&self.split_paned, width / 2, crate::motion::PANEL, None);
            }
            self.active_side.set(1);
            self.split_boxes[1].add_css_class("omafile-split-active");
            self.split_boxes[0].remove_css_class("omafile-split-active");
            self.open_in_new_tab(dir, true);
        }
    }

    /// Fold the right side into the left: its tabs move over, then it hides.
    fn end_split(self: &Rc<Self>) {
        if self.ending_split.get() {
            return;
        }
        // Slide the divider to the right edge first, then fold the tabs over.
        self.ending_split.set(true);
        let width = self.split_paned.width();
        let this = Rc::downgrade(self);
        crate::motion::slide_paned(&self.split_paned, width.max(0), crate::motion::PANEL, Some(Box::new(move || {
            if let Some(w) = this.upgrade() {
                w.ending_split.set(false);
                w.finish_end_split();
            }
        })));
    }

    fn finish_end_split(self: &Rc<Self>) {
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
        if let Some(key) = self.prefs.get("group").filter(|k| k != "none") {
            pane.set_group(&key);
        }
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
        {
            let cwd = pane.cwd();
            let mut recent = self.recent.borrow_mut();
            recent.retain(|f| !f.equal(&cwd));
            recent.insert(0, cwd);
            recent.truncate(25);
            let lines: Vec<String> = recent.iter().filter_map(|f| f.path().map(|p| p.to_string_lossy().to_string()).or_else(|| Some(f.uri().to_string()))).collect();
            crate::status::save_recent(&lines);
        }
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
            [one] => self.preview.show_git(Some(one), 1, pane.git_info_for(one)),
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
                button.set_child(Some(&icon_label("user-home-symbolic", &tr("Home"))));
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
                    PlaceKind::Smart(spec) => {
                        if let Some(p) = w.active_pane() {
                            p.show_smart(&spec);
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

        // Dropping files on a sidebar place copies/moves them there; hovering one opens it.
        let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY | gdk::DragAction::MOVE);
        let spring: Rc<RefCell<Option<(i32, glib::SourceId)>>> = Rc::new(RefCell::new(None));
        {
            let this = Rc::downgrade(self);
            let spring_m = spring.clone();
            target.connect_motion(move |_, _, y| {
                let spring = &spring_m;
                let Some(w) = this.upgrade() else { return gdk::DragAction::COPY };
                let index = w.sidebar_list.row_at_y(y as i32).map(|r| r.index());
                let pending = spring.borrow().as_ref().map(|(i, _)| *i);
                if index != pending {
                    if let Some((_, id)) = spring.borrow_mut().take() {
                        id.remove();
                    }
                    if let Some(i) = index {
                        let this2 = this.clone();
                        let spring2 = spring.clone();
                        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(800), move || {
                            spring2.borrow_mut().take();
                            let Some(w) = this2.upgrade() else { return };
                            let file = match w.state.borrow().places.get(i as usize) {
                                Some(Place::Entry { file, kind, .. }) if !matches!(kind, PlaceKind::Ssh(_) | PlaceKind::Tag(_) | PlaceKind::Smart(_)) => Some(file.clone()),
                                _ => None,
                            };
                            if let Some(f) = file {
                                w.navigate(f);
                            }
                        });
                        *spring.borrow_mut() = Some((i, id));
                    }
                }
                gdk::DragAction::MOVE
            });
            let spring = spring.clone();
            target.connect_leave(move |_| {
                if let Some((_, id)) = spring.borrow_mut().take() {
                    id.remove();
                }
            });
        }
        let this = Rc::downgrade(self);
        target.connect_drop(move |_, value, _, y| {
            if let Some((_, id)) = spring.borrow_mut().take() {
                id.remove();
            }
            let Some(w) = this.upgrade() else { return false };
            let Ok(list) = value.get::<gdk::FileList>() else { return false };
            let Some(row) = w.sidebar_list.row_at_y(y as i32) else { return false };
            let dest = match w.state.borrow().places.get(row.index() as usize) {
                Some(Place::Entry { file, kind, .. }) if !matches!(kind, PlaceKind::Ssh(_) | PlaceKind::Tag(_) | PlaceKind::Smart(_)) => file.clone(),
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
        let mut places = vec![Place::Header(tr("Places"))];
        let home = glib::home_dir();
        let mut add = |name: &str, icon: &str, path: std::path::PathBuf| {
            if path.is_dir() {
                places.push(Place::Entry { name: tr(name), icon: icon.into(), file: gio::File::for_path(path), kind: PlaceKind::Builtin });
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
        if let Some(themes) = crate::omarchy::themes_dir() {
            places.push(Place::Entry { name: tr("Omarchy Themes"), icon: "applications-graphics-symbolic".into(), file: gio::File::for_path(themes), kind: PlaceKind::Builtin });
        }
        places.push(Place::Entry { name: tr("Recent"), icon: "document-open-recent-symbolic".into(), file: gio::File::for_uri("recent:///"), kind: PlaceKind::Builtin });
        let trash_name = match *self.trash_summary.borrow() {
            Some((n, bytes)) if n > 0 => format!("Trash  ·  {n} {}, {}", if n == 1 { tr("item") } else { tr("items") }, crate::ops::human_size(bytes)),
            _ => tr("Trash"),
        };
        places.push(Place::Entry { name: trash_name, icon: if self.trash_summary.borrow().map(|(n, _)| n > 0).unwrap_or(false) { "user-trash-full-symbolic".into() } else { "user-trash-symbolic".into() }, file: gio::File::for_uri("trash:///"), kind: PlaceKind::Builtin });
        let smart = crate::smart::load();
        if !smart.is_empty() {
            places.push(Place::Header(tr("Smart Folders")));
            for s in smart {
                places.push(Place::Entry { name: s.name.clone(), icon: "folder-saved-search-symbolic".into(), file: gio::File::for_uri(&crate::smart::uri(&s)), kind: PlaceKind::Smart(s) });
            }
        }

        let bookmarks = remote::bookmarks();
        if !bookmarks.is_empty() {
            places.push(Place::Header(tr("Bookmarks")));
            for b in bookmarks {
                let icon = if b.file.is_native() { "folder-symbolic" } else { "folder-remote-symbolic" };
                places.push(Place::Entry { name: b.name, icon: icon.into(), file: b.file, kind: PlaceKind::Bookmark });
            }
        }

        let used = self.tags.colors_in_use();
        if !used.is_empty() {
            places.push(Place::Header(tr("Tags")));
            for color in used {
                places.push(Place::Entry {
                    name: crate::tags::label(&color),
                    icon: format!("tag:{color}"),
                    file: gio::File::for_uri(&format!("omafile-tag:///{color}")),
                    kind: PlaceKind::Tag(color),
                });
            }
        }
        places.push(Place::Header(tr("Remote")));
        for host in remote::ssh_hosts() {
            places.push(Place::Entry {
                name: host.alias.clone(),
                icon: "network-server-symbolic".into(),
                file: gio::File::for_uri(&host.uri()),
                kind: PlaceKind::Ssh(host.alias),
            });
        }
        places.push(Place::Action { name: tr("Connect to Server…"), icon: "network-workgroup-symbolic".into(), action: "connect" });

        places.push(Place::Header(tr("Devices")));
        places.push(Place::Entry { name: tr("Computer"), icon: "drive-harddisk-symbolic".into(), file: gio::File::for_path("/"), kind: PlaceKind::Builtin });
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

        let volumes = self.containers.borrow();
        if !volumes.is_empty() {
            places.push(Place::Header(tr("Containers")));
            for v in volumes.iter() {
                places.push(Place::Entry { name: format!("{} ({})", v.name, v.engine), icon: "package-x-generic-symbolic".into(), file: gio::File::for_path(&v.mountpoint), kind: PlaceKind::Builtin });
            }
        }
        drop(volumes);

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
            Some(c) => format!("Tagged {} {} {}", items.len(), if items.len() == 1 { tr("item") } else { tr("items") }, crate::tags::label(c).to_lowercase()),
            None => tr("Tag removed"),
        };
        self.record(Op::Tag(changes), &message);
        for p in self.all_panes() {
            p.reload();
        }
        self.refresh_sidebar();
    }

    // ----- command palette ----------------------------------------------------------

    pub fn open_palette(self: &Rc<Self>) {
        let existing = self.palette.borrow().clone();
        let palette = match existing {
            Some(p) => p,
            None => {
                let p = Palette::new(self);
                *self.palette.borrow_mut() = Some(p.clone());
                p
            }
        };
        let mut entries = Vec::new();
        // Actions with their shortcuts.
        for (name, label, icon) in crate::actions::PALETTE_ACTIONS {
            let accel = crate::actions::ACCELS.iter().find(|(a, _)| *a == format!("win.{name}")).and_then(|(_, keys)| keys.first()).map(|k| pretty_accel(k)).unwrap_or_default();
            entries.push(PaletteEntry { title: tr(label), detail: accel, icon, target: PaletteTarget::Action(name.to_string()) });
        }
        // Project commands of the current folder.
        if let Some(cwd) = self.active_pane().map(|p| p.cwd()) {
            if let Some(project) = cwd.path().and_then(|d| projects::detect(&d)) {
                for action in project.actions {
                    entries.push(PaletteEntry { title: action.label.clone(), detail: format!("{} project · terminal", project.kind), icon: "utilities-terminal-symbolic", target: PaletteTarget::Project(action.command) });
                }
            }
        }
        // Places, bookmarks, recent folders, servers.
        for place in &self.state.borrow().places {
            if let Place::Entry { name, icon, file, kind } = place {
                let (detail, target) = match kind {
                    PlaceKind::Ssh(_) => (tr("SSH server"), PaletteTarget::Connect(file.clone())),
                    PlaceKind::Tag(_) => continue,
                    PlaceKind::Smart(_) => (tr("Smart folder"), PaletteTarget::Action(format!("smart-open::{name}"))),
                    PlaceKind::Bookmark => (tr("Bookmark"), PaletteTarget::Folder(file.clone())),
                    PlaceKind::Mount(_) => (tr("Device"), PaletteTarget::Folder(file.clone())),
                    PlaceKind::Builtin => (location_text(file), PaletteTarget::Folder(file.clone())),
                };
                let icon: &'static str = if icon.starts_with("tag:") { "tag-symbolic" } else { leak_icon(icon) };
                entries.push(PaletteEntry { title: name.clone(), detail, icon, target });
            }
        }
        for file in self.recent.borrow().iter().skip(1) {
            entries.push(PaletteEntry { title: display_name(file), detail: format!("Recent · {}", location_text(file)), icon: "document-open-recent-symbolic", target: PaletteTarget::Folder(file.clone()) });
        }
        if let Some(py) = self.active_pane().and_then(|p| p.cwd().path()).and_then(|d| crate::pyui::menu_state(&d, false)) {
            let rows: Vec<(&str, &str)> = if py.has_venv {
                vec![("py-activate", "Python: Activate venv in Terminal Panel"), ("py-install", "Python: Install Requirements"), ("py-freeze", "Python: Freeze to requirements.txt"), ("py-repl", "Python: REPL in venv"), ("py-recreate", "Python: Recreate venv…")]
            } else {
                vec![("py-create", "Python: Create venv here"), ("py-repl", "Python: REPL")]
            };
            for (action, label) in rows {
                entries.push(PaletteEntry { title: tr(label), detail: tr("terminal panel"), icon: "applications-science-symbolic", target: PaletteTarget::Action(action.into()) });
            }
        }
        for (i, t) in crate::templates::list().iter().enumerate() {
            entries.push(PaletteEntry { title: format!("{} {}", tr("New from Template:"), tr(&crate::templates::label(t))), detail: tr("creates it here"), icon: "folder-templates-symbolic", target: PaletteTarget::Action(format!("new-from-template::{i}")) });
        }
        entries.push(PaletteEntry { title: tr("Export Omarchy Theme…"), detail: tr("selected theme folder → ~/Downloads/<name>.tar.gz"), icon: "applications-graphics-symbolic", target: PaletteTarget::Action("export-theme".into()) });
        entries.push(PaletteEntry { title: tr("Install as Omarchy Theme"), detail: tr("selected archive → ~/.config/omarchy/themes"), icon: "applications-graphics-symbolic", target: PaletteTarget::Action("install-theme".into()) });
        // Notes on files: searchable by their text, open the folder and select the file.
        for (file, note) in self.notes.all() {
            let first = note.lines().next().unwrap_or("").to_string();
            entries.push(PaletteEntry { title: format!("{}  —  {first}", display_name(&file)), detail: format!("Note · {}", location_text(&file)), icon: "user-available-symbolic", target: PaletteTarget::Reveal(file) });
        }
        palette.open(entries);
    }

    pub fn run_palette_target(self: &Rc<Self>, target: PaletteTarget) {
        match target {
            PaletteTarget::Action(name) => match gio::Action::parse_detailed_name(&name) {
                Ok((plain, target)) => gio::prelude::ActionGroupExt::activate_action(&self.window, &plain, target.as_ref()),
                Err(_) => gio::prelude::ActionGroupExt::activate_action(&self.window, &name, None),
            },
            PaletteTarget::Folder(file) => self.navigate(file),
            PaletteTarget::Reveal(file) => self.reveal(file),
            PaletteTarget::Bulk(text) => self.run_bulk(&text),
            PaletteTarget::Connect(file) => self.connect_to(file, true),
            PaletteTarget::Project(command) | PaletteTarget::Command(command) => self.run_in_terminal(&command),
        }
    }

    /// Open the parent folder and select `file`.
    pub fn reveal(self: &Rc<Self>, file: gio::File) {
        let Some(pane) = self.active_pane() else { return };
        match file.parent() {
            Some(parent) => {
                pane.navigate(parent, true);
                pane.select_file_later(file);
            }
            None => pane.navigate(file, true),
        }
    }

    /// The pane on the other side of the split, if any.
    pub fn other_pane(&self) -> Option<Rc<Pane>> {
        if !self.is_split() {
            return None;
        }
        let other = 1 - self.active_side.get();
        let page = self.tab_views[other].selected_page()?;
        let child = page.child();
        self.panes.borrow().iter().find(|p| p.root.upcast_ref::<gtk::Widget>() == &child).cloned()
    }

    /// Run a shell command in a terminal opened in the active folder.
    pub fn run_in_terminal(self: &Rc<Self>, command: &str) {
        let Some(dir) = self.active_pane().and_then(|p| p.cwd().path()) else {
            self.toast_error(&tr("Commands run in local folders only"));
            return;
        };
        let argv = projects::terminal_argv(&dir, command);
        let args: Vec<&std::path::Path> = argv.iter().map(std::path::Path::new).collect();
        match glib::spawn_async(Some(dir.as_path()), &args, &[], glib::SpawnFlags::SEARCH_PATH | glib::SpawnFlags::STDOUT_TO_DEV_NULL | glib::SpawnFlags::STDERR_TO_DEV_NULL, None) {
            Ok(_) => self.toast(&trf!("Running: {command}", command = command)),
            Err(e) => self.toast_error(&format!("Cannot open terminal: {}", e.message())),
        }
    }

    pub fn open_in_editor(self: &Rc<Self>, target: &gio::File) {
        let Some(path) = target.path() else {
            self.toast_error(&tr("The editor opens local files only"));
            return;
        };
        let argv = projects::editor_argv(&path);
        let args: Vec<&std::path::Path> = argv.iter().map(std::path::Path::new).collect();
        let cwd = if path.is_dir() { path.clone() } else { path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| path.clone()) };
        if let Err(e) = glib::spawn_async(Some(cwd.as_path()), &args, &[], glib::SpawnFlags::SEARCH_PATH | glib::SpawnFlags::STDOUT_TO_DEV_NULL | glib::SpawnFlags::STDERR_TO_DEV_NULL, None) {
            self.toast_error(&format!("Cannot open editor: {}", e.message()));
        }
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

    /// The shelf window, created on first use.
    pub fn shelf(self: &Rc<Self>) -> Rc<crate::shelf::Shelf> {
        if let Some(s) = self.shelf.borrow().as_ref() {
            return s.clone();
        }
        let s = crate::shelf::Shelf::new(self);
        self.shelf.replace(Some(s.clone()));
        s
    }

    /// Called by the shelf when its contents change.
    pub fn shelf_changed(&self, _n: usize) {}

    pub fn queue_busy(&self) -> bool {
        self.progress.revealer.reveals_child()
    }

    /// Copy or move `files` into `dest` through the transfer queue.
    pub fn transfer(self: &Rc<Self>, files: Vec<gio::File>, dest: gio::File, mode: Mode, pane: Rc<Pane>, auto_rename: bool) {
        if files.is_empty() {
            return;
        }
        self.enqueue_transfer(Request { files, dest, mode, pane, auto_rename });
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
            self.toast(&tr("Nothing to undo"));
            return;
        };
        self.apply_history(op, true);
    }

    pub fn redo(self: &Rc<Self>) {
        let Some(op) = self.journal.pop_redo() else {
            self.toast(&tr("Nothing to redo"));
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
                    w.toast_error(&format!("Cannot {}: {}", if is_undo { tr("undo") } else { tr("redo") }, e.message()));
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

fn pretty_accel(accel: &str) -> String {
    accel
        .replace("<Control>", "Ctrl+")
        .replace("<Shift>", "Shift+")
        .replace("<Alt>", "Alt+")
        .replace("comma", ",")
        .replace("question", "?")
        .replace("Return", "Enter")
        .replace("Page_Down", "PgDn")
        .replace("Page_Up", "PgUp")
}

fn leak_icon(name: &str) -> &'static str {
    // Icon names are a small fixed set; leaking them keeps the palette entries 'static.
    Box::leak(name.to_string().into_boxed_str())
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
        Some("trash") => ("user-trash-symbolic", tr("Trash")),
        Some("recent") => ("document-open-recent-symbolic", tr("Recent")),
        Some("archive") => ("package-x-generic-symbolic", archive_name(file)),
        Some("file") | None => ("drive-harddisk-symbolic", tr("Computer")),
        Some(_) => {
            // sftp://user@host/ -> host
            let uri = file.uri();
            let host = uri.split("://").nth(1).unwrap_or("").trim_end_matches('/').rsplit('@').next().unwrap_or("").to_string();
            ("network-server-symbolic", if host.is_empty() { uri.to_string() } else { host })
        }
    }
}

/// `archive://file%253A%252F%252F…%252Frelease.tar.gz/` → `release.tar.gz`
pub fn archive_name(file: &gio::File) -> String {
    let uri = file.uri();
    let host = uri.strip_prefix("archive://").unwrap_or("").split('/').next().unwrap_or("");
    let once = glib::uri_unescape_string(host, None::<&str>).map(|s| s.to_string()).unwrap_or_default();
    let twice = glib::uri_unescape_string(&once, None::<&str>).map(|s| s.to_string()).unwrap_or(once);
    twice.rsplit('/').next().unwrap_or("archive").to_string()
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
    s0.append(Some(tr("Undo").as_str()), Some("win.undo"));
    s0.append(Some(tr("Redo").as_str()), Some("win.redo"));
    menu.append_section(None, &s0);
    let s1 = gio::Menu::new();
    s1.append(Some(tr("Command Palette…").as_str()), Some("win.palette"));
    s1.append(Some(tr("New Tab").as_str()), Some("win.new-tab"));
    s1.append(Some(tr("New Folder").as_str()), Some("win.new-folder"));
    s1.append(Some(tr("New File").as_str()), Some("win.new-file"));
    s1.append(Some(tr("New Encrypted Folder…").as_str()), Some("win.vault-new"));
    s1.append(Some(tr("Lock All Encrypted Folders").as_str()), Some("win.vault-lock-all"));
    s1.append(Some(tr("Terminal Panel").as_str()), Some("win.terminal-panel"));
    s1.append(Some(tr("Open Terminal Window Here").as_str()), Some("win.terminal"));
    s1.append(Some(tr("Open in Editor").as_str()), Some("win.open-editor-cwd"));
    s1.append(Some(tr("Connect to Server…").as_str()), Some("win.connect"));
    menu.append_section(None, &s1);
    let s2 = gio::Menu::new();
    s2.append(Some(tr("Add Bookmark").as_str()), Some("win.bookmark"));
    s2.append(Some(tr("Show Hidden Files").as_str()), Some("win.show-hidden"));
    s2.append(Some(tr("Show Sidebar").as_str()), Some("win.show-sidebar"));
    s2.append(Some(tr("Show Preview").as_str()), Some("win.show-preview"));
    s2.append(Some(tr("Split View").as_str()), Some("win.split-view"));
    s2.append(Some(tr("Show Folder Sizes").as_str()), Some("win.folder-sizes"));
    s2.append(Some(tr("Disk Usage Map…").as_str()), Some("win.treemap"));
    s2.append(Some(tr("Find Duplicates…").as_str()), Some("win.find-duplicates"));
    s2.append(Some(tr("Folder Rules…").as_str()), Some("win.rules"));
    s2.append(Some(tr("Shelf").as_str()), Some("win.shelf-show"));
    s2.append(Some(tr("Save Search as Smart Folder…").as_str()), Some("win.save-smart"));
    menu.append_section(None, &s2);
    let view = gio::Menu::new();
    view.append(Some(tr("List").as_str()), Some("win.view::list"));
    view.append(Some(tr("Grid").as_str()), Some("win.view::grid"));
    view.append(Some(tr("Columns").as_str()), Some("win.view::columns"));
    view.append(Some(tr("Gallery").as_str()), Some("win.view::gallery"));
    menu.append_section(Some(tr("View").as_str()), &view);
    let group = gio::Menu::new();
    group.append(Some(tr("None").as_str()), Some("win.group::none"));
    group.append(Some(tr("Kind").as_str()), Some("win.group::kind"));
    group.append(Some(tr("Date Modified").as_str()), Some("win.group::date"));
    group.append(Some(tr("Size").as_str()), Some("win.group::size"));
    group.append(Some(tr("Tag").as_str()), Some("win.group::tag"));
    menu.append_submenu(Some(tr("Group By").as_str()), &group);
    let sort = gio::Menu::new();
    sort.append(Some(tr("Name").as_str()), Some("win.sort::name"));
    sort.append(Some(tr("Size").as_str()), Some("win.sort::size"));
    sort.append(Some(tr("Type").as_str()), Some("win.sort::type"));
    sort.append(Some(tr("Modified").as_str()), Some("win.sort::modified"));
    menu.append_submenu(Some(tr("Sort By").as_str()), &sort);
    let s3 = gio::Menu::new();
    s3.append(Some(tr("Preferences").as_str()), Some("win.preferences"));
    s3.append(Some(tr("Keyboard Shortcuts").as_str()), Some("win.shortcuts"));
    s3.append(Some(tr("About Files").as_str()), Some("app.about"));
    menu.append_section(None, &s3);
    menu
}

#[derive(Clone, Copy, PartialEq)]
pub enum VaultState {
    None,
    /// A ciphertext folder (contains gocryptfs.conf).
    Locked,
    /// A mount point that is currently unlocked.
    Unlocked,
}

impl VaultState {
    pub fn of(path: Option<std::path::PathBuf>) -> VaultState {
        let Some(path) = path else { return VaultState::None };
        if crate::crypt::is_vault(&path) {
            if crate::crypt::is_mounted(&crate::crypt::mount_point_for(&path)) { VaultState::Unlocked } else { VaultState::Locked }
        } else if crate::crypt::vault_for(&path).is_some() && crate::crypt::is_mounted(&path) {
            VaultState::Unlocked
        } else {
            VaultState::None
        }
    }
}

/// What the selection looks like, for building the context menu.
#[derive(Default, Clone, Copy)]
pub struct MenuFlags {
    pub single_dir: bool,
    pub archive: bool,
    pub in_trash: bool,
    pub multiple: bool,
    pub in_repo: bool,
    /// Every selected item is a local image.
    pub images: bool,
    /// The single selected folder is an Omarchy theme.
    pub theme: bool,
    /// Selection is local (share / scp make sense).
    pub local: bool,
    /// A single .py file with a venv around it.
    pub py_script: bool,
    /// Every selected item is a PDF (local).
    pub pdfs: usize,
    /// A single local video / audio file.
    pub video: bool,
    pub audio: bool,
    /// A single symbolic link.
    pub symlink: bool,
    /// A single audio or video file.
    pub media: bool,
    /// Exactly two items selected (compare).
    pub pair: bool,
    /// Split view is on (copy / move to the other pane).
    pub split: bool,
    /// A single archive that looks like an Omarchy theme.
    pub theme_archive: bool,
}

pub fn item_menu_model(single_dir: bool, archive: bool, in_trash: bool, multiple: bool, vault: VaultState, in_repo: bool) -> gio::Menu {
    item_menu_model_ext(MenuFlags { single_dir, archive, in_trash, multiple, in_repo, ..Default::default() }, vault)
}

pub fn item_menu_model_ext(f: MenuFlags, vault: VaultState) -> gio::Menu {
    let (single_dir, archive, in_trash, multiple, in_repo) = (f.single_dir, f.archive, f.in_trash, f.multiple, f.in_repo);
    let menu = gio::Menu::new();
    let open = gio::Menu::new();
    open.append(Some(tr("Open").as_str()), Some("win.open"));
    if f.media {
        open.append(Some(tr("Play").as_str()), Some("win.play"));
    }
    open.append(Some(tr("Quick Look").as_str()), Some("win.quick-look"));
    if single_dir {
        open.append(Some(tr("Open in New Tab").as_str()), Some("win.open-new-tab"));
        open.append(Some(tr("Open in Terminal").as_str()), Some("win.terminal-selected"));
        open.append(Some(tr("Disk Usage Map…").as_str()), Some("win.treemap-selected"));
        open.append(Some(tr("Find Duplicates…").as_str()), Some("win.find-duplicates-selected"));
    } else {
        open.append(Some(tr("Open With…").as_str()), Some("win.open-with"));
    }
    if !multiple && !in_trash {
        open.append(Some(tr("Open in Editor").as_str()), Some("win.open-editor"));
    }
    if in_repo && !in_trash {
        let g = gio::Menu::new();
        g.append(Some(tr("Stage").as_str()), Some("win.git-stage"));
        g.append(Some(tr("Unstage").as_str()), Some("win.git-unstage"));
        g.append(Some(tr("Discard Changes…").as_str()), Some("win.git-discard"));
        g.append(Some(tr("Commit…").as_str()), Some("win.git-commit"));
        if !multiple {
            g.append(Some(tr("Diff in Terminal").as_str()), Some("win.git-diff"));
        }
        open.append_submenu(Some(tr("Git").as_str()), &g);
    }
    match vault {
        VaultState::Locked => open.append(Some(tr("Unlock Encrypted Folder…").as_str()), Some("win.vault-unlock")),
        VaultState::Unlocked => open.append(Some(tr("Lock Encrypted Folder").as_str()), Some("win.vault-lock")),
        VaultState::None => {}
    }
    if vault != VaultState::None {
        open.append(Some(tr("Change Vault Password…").as_str()), Some("win.vault-passwd"));
    }
    if archive {
        open.append(Some(tr("Extract Here").as_str()), Some("win.extract"));
    }
    if f.theme && !in_trash {
        open.append(Some(tr("Apply Omarchy Theme").as_str()), Some("win.apply-theme"));
        open.append(Some(tr("Export Omarchy Theme…").as_str()), Some("win.export-theme"));
    }
    if f.theme_archive && !in_trash {
        open.append(Some(tr("Install as Omarchy Theme").as_str()), Some("win.install-theme"));
    }
    if archive {
        open.append(Some(tr("Browse Archive").as_str()), Some("win.browse-archive"));
    }
    if f.pair && !in_trash {
        open.append(Some(tr("Compare…").as_str()), Some("win.compare"));
    } else if f.split && !multiple && !in_trash {
        open.append(Some(tr("Compare with Other Pane…").as_str()), Some("win.compare"));
    }
    if single_dir && !in_trash && crate::hypr::available() {
        open.append(Some(tr("Open in New Workspace").as_str()), Some("win.open-workspace"));
    }
    if f.py_script && !in_trash {
        open.append(Some(tr("Run in venv (Terminal Panel)").as_str()), Some("win.py-run"));
    }
    if !multiple && !in_trash && !single_dir {
        open.append(Some(tr("Always Open With…").as_str()), Some("win.always-open-with"));
    }
    if f.symlink && !in_trash {
        open.append(Some(tr("Show Original").as_str()), Some("win.show-original"));
    }
    if f.pdfs > 0 && !in_trash {
        let pdf = gio::Menu::new();
        if f.pdfs >= 2 {
            pdf.append(Some(tr("Merge into One PDF").as_str()), Some("win.pdf-merge"));
        }
        if f.pdfs == 1 {
            pdf.append(Some(tr("Split into Pages").as_str()), Some("win.pdf-split"));
            pdf.append(Some(tr("Extract Pages…").as_str()), Some("win.pdf-extract"));
        }
        open.append_submenu(Some(tr("PDF").as_str()), &pdf);
    }
    if f.video && !in_trash {
        let v = gio::Menu::new();
        v.append(Some(tr("Trim…").as_str()), Some("win.video-trim"));
        v.append(Some(tr("Make GIF (first 10 s)").as_str()), Some("win.video-gif"));
        v.append(Some(tr("Extract Audio").as_str()), Some("win.video-audio"));
        v.append(Some(tr("Audio to MP3").as_str()), Some("win.audio-mp3"));
        open.append_submenu(Some(tr("Video").as_str()), &v);
    }
    if f.audio && !in_trash {
        let a = gio::Menu::new();
        a.append(Some(tr("Trim…").as_str()), Some("win.video-trim"));
        a.append(Some(tr("Convert to MP3").as_str()), Some("win.audio-mp3"));
        a.append(Some(tr("Convert to Opus").as_str()), Some("win.audio-opus"));
        a.append(Some(tr("Convert to FLAC").as_str()), Some("win.audio-flac"));
        open.append_submenu(Some(tr("Audio").as_str()), &a);
    }
    if f.images && !in_trash {
        let img = gio::Menu::new();
        img.append(Some(tr("Set as Wallpaper").as_str()), Some("win.set-wallpaper"));
        img.append(Some(tr("Rotate Left").as_str()), Some("win.image-rotate::-90"));
        img.append(Some(tr("Rotate Right").as_str()), Some("win.image-rotate::90"));
        img.append(Some(tr("Resize…").as_str()), Some("win.image-resize"));
        img.append(Some(tr("Create PDF from Images").as_str()), Some("win.pdf-from-images"));
        let conv = gio::Menu::new();
        conv.append(Some("PNG"), Some("win.image-convert::png"));
        conv.append(Some("JPEG"), Some("win.image-convert::jpg"));
        conv.append(Some("WebP"), Some("win.image-convert::webp"));
        img.append_submenu(Some(tr("Convert to").as_str()), &conv);
        open.append_submenu(Some(tr("Image").as_str()), &img);
    }
    menu.append_section(None, &open);
    let edit = gio::Menu::new();
    if in_trash {
        edit.append(Some(tr("Restore").as_str()), Some("win.restore"));
    } else {
        edit.append(Some(tr("Cut").as_str()), Some("win.cut"));
        edit.append(Some(tr("Copy").as_str()), Some("win.copy"));
        edit.append(Some(tr("Copy Path").as_str()), Some("win.copy-path"));
        edit.append(Some(tr("Copy To…").as_str()), Some("win.copy-to"));
        edit.append(Some(tr("Move To…").as_str()), Some("win.move-to"));
        if f.split {
            edit.append(Some(tr("Copy to Other Pane").as_str()), Some("win.copy-other"));
            edit.append(Some(tr("Move to Other Pane").as_str()), Some("win.move-other"));
        }
        edit.append(Some(tr("Duplicate").as_str()), Some("win.duplicate"));
        edit.append(Some(tr("Make Alias").as_str()), Some("win.make-alias"));
        edit.append(Some(&if multiple { tr("New Folder with Selection") } else { tr("New Folder with Item") }), Some("win.new-folder-with-selection"));
        edit.append(Some(&if multiple { tr("Batch Rename…") } else { tr("Rename…") }), Some("win.rename"));
        edit.append(Some(tr("Add to Shelf").as_str()), Some("win.shelf-add"));
        edit.append(Some(tr("Compress…").as_str()), Some("win.compress"));
        if !multiple {
            edit.append(Some(tr("Change Icon…").as_str()), Some("win.change-icon"));
        }
        let tags = gio::Menu::new();
        for (name, _, _) in crate::tags::COLORS {
            tags.append(Some(&crate::tags::label(name)), Some(&format!("win.tag::{name}")));
        }
        tags.append(Some(tr("No Tag").as_str()), Some("win.tag::none"));
        edit.append_submenu(Some(tr("Tag").as_str()), &tags);
        if single_dir {
            edit.append(Some(tr("Add Bookmark").as_str()), Some("win.bookmark-selected"));
        }
        if f.local {
            let share = gio::Menu::new();
            share.append(Some(tr("Copy scp Command").as_str()), Some("win.share-scp"));
            share.append(Some(tr("Share on Local Network…").as_str()), Some("win.share-lan"));
            edit.append_submenu(Some(tr("Share").as_str()), &share);
        }
    }
    menu.append_section(None, &edit);
    let danger = gio::Menu::new();
    if !in_trash {
        danger.append(Some(tr("Move to Trash").as_str()), Some("win.trash"));
    }
    danger.append(Some(tr("Delete Permanently…").as_str()), Some("win.delete"));
    menu.append_section(None, &danger);
    let info = gio::Menu::new();
    if !multiple && !single_dir && !in_trash {
        info.append(Some(tr("Verify Checksum").as_str()), Some("win.verify-checksum"));
    }
    if !multiple && !in_trash {
        info.append(Some(tr("Edit Note…").as_str()), Some("win.edit-note"));
    }
    info.append(Some(tr("Properties").as_str()), Some("win.properties"));
    menu.append_section(None, &info);
    menu
}

pub fn background_menu_model(in_trash: bool, project: Option<&projects::Project>, in_repo: bool) -> gio::Menu {
    background_menu_model_ext(in_trash, project, in_repo, None)
}

pub fn python_menu(py: &crate::pyui::PyMenu) -> gio::Menu {
    let m = gio::Menu::new();
    if py.has_venv {
        m.append(Some(tr("Activate venv in Terminal Panel").as_str()), Some("win.py-activate"));
        if py.has_requirements {
            m.append(Some(&if crate::pyenv::uv_available() { tr("Install Requirements (uv)") } else { tr("Install Requirements (pip)") }), Some("win.py-install"));
        }
        m.append(Some(tr("Freeze to requirements.txt").as_str()), Some("win.py-freeze"));
        m.append(Some(tr("Python REPL in venv").as_str()), Some("win.py-repl"));
        if py.gitignore_missing {
            m.append(Some(tr("Add venv to .gitignore").as_str()), Some("win.py-gitignore"));
        }
        m.append(Some(tr("Recreate venv…").as_str()), Some("win.py-recreate"));
    } else {
        m.append(Some(&if crate::pyenv::uv_available() { tr("Create venv here (uv venv .venv)") } else { tr("Create venv here (python -m venv .venv)") }), Some("win.py-create"));
        m.append(Some(tr("Python REPL").as_str()), Some("win.py-repl"));
    }
    m
}

pub fn background_menu_model_ext(in_trash: bool, project: Option<&projects::Project>, in_repo: bool, python: Option<&crate::pyui::PyMenu>) -> gio::Menu {
    let menu = gio::Menu::new();
    if in_trash {
        let s = gio::Menu::new();
        s.append(Some(tr("Empty Trash…").as_str()), Some("win.empty-trash"));
        s.append(Some(tr("Select All").as_str()), Some("win.select-all"));
        menu.append_section(None, &s);
        return menu;
    }
    let s1 = gio::Menu::new();
    s1.append(Some(tr("New Folder").as_str()), Some("win.new-folder"));
    s1.append(Some(tr("New File").as_str()), Some("win.new-file"));
    s1.append(Some(tr("New Encrypted Folder…").as_str()), Some("win.vault-new"));
    let tpl = gio::Menu::new();
    for (i, t) in crate::templates::list().iter().enumerate() {
        tpl.append(Some(&tr(&crate::templates::label(t))), Some(&format!("win.new-from-template::{i}")));
    }
    tpl.append(Some(tr("Open Templates Folder").as_str()), Some("win.open-templates"));
    s1.append_submenu(Some(tr("New from Template").as_str()), &tpl);
    menu.append_section(None, &s1);
    if let Some(project) = project {
        let sub = gio::Menu::new();
        sub.append(Some(tr("Open in Editor").as_str()), Some("win.open-editor-cwd"));
        for (i, action) in project.actions.iter().enumerate() {
            sub.append(Some(&tr(&action.label)), Some(&format!("win.project-run::{i}")));
        }
        menu.append_submenu(Some(&trf!("{kind} project", kind = tr(project.kind))), &sub);
        if let Some(py) = python {
            menu.append_submenu(Some(tr("Python").as_str()), &python_menu(py));
        }
    } else {
        if let Some(py) = python {
            menu.append_submenu(Some(tr("Python").as_str()), &python_menu(py));
        }
        let s = gio::Menu::new();
        s.append(Some(tr("Open in Editor").as_str()), Some("win.open-editor-cwd"));
        menu.append_section(None, &s);
    }
    if in_repo {
        let g = gio::Menu::new();
        g.append(Some(tr("Commit…").as_str()), Some("win.git-commit"));
        g.append(Some(tr("Switch Branch…").as_str()), Some("win.git-branch"));
        g.append(Some(tr("Git Status in Terminal").as_str()), Some("win.git-status"));
        g.append(Some(tr("Git Diff in Terminal").as_str()), Some("win.git-diff-all"));
        menu.append_submenu(Some(tr("Git").as_str()), &g);
    }
    let s2 = gio::Menu::new();
    s2.append(Some(tr("Paste").as_str()), Some("win.paste"));
    s2.append(Some(tr("Undo").as_str()), Some("win.undo"));
    s2.append(Some(tr("Redo").as_str()), Some("win.redo"));
    s2.append(Some(tr("Select All").as_str()), Some("win.select-all"));
    menu.append_section(None, &s2);
    let s3 = gio::Menu::new();
    s3.append(Some(tr("Show Folder Sizes").as_str()), Some("win.folder-sizes"));
    s3.append(Some(tr("Save Search as Smart Folder…").as_str()), Some("win.save-smart"));
    s3.append(Some(tr("Terminal Panel").as_str()), Some("win.terminal-panel"));
    s3.append(Some(tr("Open Terminal Window Here").as_str()), Some("win.terminal"));
    s3.append(Some(tr("Add Bookmark").as_str()), Some("win.bookmark"));
    s3.append(Some(tr("Copy Path").as_str()), Some("win.copy-cwd-path"));
    s3.append(Some(tr("Properties").as_str()), Some("win.cwd-properties"));
    menu.append_section(None, &s3);
    menu
}
