//! One tab: a directory with its list/grid views, filter, selection and monitor.

use crate::item::FileItem;
use crate::ops;
use crate::thumbs::ThumbCache;
use crate::window::{self, Window};
use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

pub const ICON_LIST: i32 = 22;
pub const ICON_GRID: i32 = 72;

pub struct PaneState {
    pub cwd: gio::File,
    pub back: Vec<gio::File>,
    pub forward: Vec<gio::File>,
    pub search: String,
    pub monitor: Option<gio::FileMonitor>,
    pub monitor_pending: Option<glib::SourceId>,
    pub generation: u64,
    pub loading: bool,
    /// Showing results of a recursive search instead of the directory.
    pub deep_results: bool,
    pub deep_pending: Option<glib::SourceId>,
    pub deep_cancel: Option<gio::Cancellable>,
    /// Title shown in the path bar for virtual listings such as a tag.
    pub virtual_title: Option<String>,
}

pub struct Pane {
    pub root: gtk::Box,
    pub store: gio::ListStore,
    pub filter: gtk::CustomFilter,
    pub selection: gtk::MultiSelection,
    pub column_view: gtk::ColumnView,
    pub columns: RefCell<Vec<(&'static str, gtk::ColumnViewColumn)>>,
    pub grid_view: gtk::GridView,
    pub view_stack: gtk::Stack,
    pub empty: adw::StatusPage,
    pub search_bar: gtk::SearchBar,
    pub search_entry: gtk::SearchEntry,
    pub deep_toggle: gtk::ToggleButton,
    pub status: gtk::Label,
    pub branch_btn: gtk::Button,
    pub side: Cell<usize>,
    pub item_menu: gtk::PopoverMenu,
    pub background_menu: gtk::PopoverMenu,
    pub cell_items: RefCell<HashMap<gtk::Widget, gtk::ListItem>>,
    pub git: RefCell<Option<crate::git::RepoStatus>>,
    pub term_paned: gtk::Paned,
    pub term: RefCell<Option<Rc<crate::termpanel::TermPanel>>>,
    pub state: RefCell<PaneState>,
    pub window: Weak<Window>,
    pub thumbs: Rc<ThumbCache>,
}

impl Pane {
    pub fn new(window: &Rc<Window>, start: gio::File, side: usize) -> Rc<Self> {
        let store = gio::ListStore::new::<FileItem>();
        let filter = gtk::CustomFilter::new(|_| true);
        let filter_model = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
        let column_view = gtk::ColumnView::new(None::<gtk::MultiSelection>);
        column_view.set_show_row_separators(false);
        column_view.set_enable_rubberband(true);
        column_view.add_css_class("omafile-files");
        let sort_model = gtk::SortListModel::new(Some(filter_model), column_view.sorter());
        let selection = gtk::MultiSelection::new(Some(sort_model));
        column_view.set_model(Some(&selection));

        let grid_factory = gtk::SignalListItemFactory::new();
        let grid_view = gtk::GridView::new(Some(selection.clone()), Some(grid_factory.clone()));
        grid_view.set_min_columns(2);
        grid_view.set_max_columns(32);
        grid_view.set_enable_rubberband(true);
        grid_view.add_css_class("omafile-files");

        let search_entry = gtk::SearchEntry::builder().placeholder_text("Filter this folder").hexpand(true).build();
        let deep_toggle = gtk::ToggleButton::builder()
            .icon_name("folder-saved-search-symbolic")
            .tooltip_text("Search subfolders too (Ctrl+Shift+F)")
            .build();
        let search_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        search_box.append(&search_entry);
        search_box.append(&deep_toggle);
        let search_bar = gtk::SearchBar::builder().child(&search_box).build();
        search_bar.connect_entry(&search_entry);
        search_bar.set_key_capture_widget(Some(&window.window));

        let view_stack = gtk::Stack::new();
        view_stack.set_vexpand(true);
        view_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        let list_scroll = gtk::ScrolledWindow::builder().child(&column_view).vexpand(true).build();
        let grid_scroll = gtk::ScrolledWindow::builder().child(&grid_view).vexpand(true).build();
        let empty = adw::StatusPage::builder().icon_name("folder-open-symbolic").title("Empty Folder").build();
        empty.add_css_class("compact");
        view_stack.add_named(&list_scroll, Some("list"));
        view_stack.add_named(&grid_scroll, Some("grid"));
        view_stack.add_named(&empty, Some("empty"));

        let status = gtk::Label::builder()
            .xalign(0.0)
            .margin_start(14)
            .margin_end(14)
            .margin_top(5)
            .margin_bottom(5)
            .ellipsize(pango::EllipsizeMode::End)
            .css_classes(["caption", "omafile-dim"])
            .build();

        let branch_btn = gtk::Button::builder().valign(gtk::Align::Center).margin_end(8).tooltip_text("Switch branch (Ctrl+Alt+B)").visible(false).build();
        branch_btn.add_css_class("flat");
        branch_btn.add_css_class("omafile-branch");
        let status_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        status_row.append(&status);
        status_row.append(&branch_btn);
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("omafile-card");
        card.append(&view_stack);
        card.append(&status_row);

        // The terminal panel docks below the file card inside a vertical paned.
        let term_paned = gtk::Paned::new(gtk::Orientation::Vertical);
        term_paned.set_start_child(Some(&card));
        term_paned.set_shrink_start_child(false);
        term_paned.set_shrink_end_child(false);
        term_paned.set_resize_start_child(true);
        term_paned.set_resize_end_child(false);
        term_paned.set_vexpand(true);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&search_bar);
        root.append(&term_paned);

        branch_btn.set_action_name(Some("win.git-branch"));
        let item_menu = gtk::PopoverMenu::from_model(Some(&window::item_menu_model(false, false, false, false, window::VaultState::None, false)));
        item_menu.set_has_arrow(false);
        item_menu.set_parent(&view_stack);
        let background_menu = gtk::PopoverMenu::from_model(Some(&window::background_menu_model(false, None, false)));
        background_menu.set_has_arrow(false);
        background_menu.set_parent(&view_stack);

        let pane = Rc::new(Pane {
            root,
            store,
            filter,
            selection,
            column_view,
            columns: RefCell::new(Vec::new()),
            grid_view,
            view_stack,
            empty,
            search_bar,
            search_entry,
            deep_toggle,
            status,
            branch_btn,
            side: Cell::new(side),
            item_menu,
            background_menu,
            cell_items: RefCell::new(HashMap::new()),
            git: RefCell::new(None),
            term_paned,
            term: RefCell::new(None),
            state: RefCell::new(PaneState {
                cwd: start.clone(),
                back: Vec::new(),
                forward: Vec::new(),
                search: String::new(),
                monitor: None,
                monitor_pending: None,
                generation: 0,
                loading: false,
                deep_results: false,
                deep_pending: None,
                deep_cancel: None,
                virtual_title: None,
            }),
            window: Rc::downgrade(window),
            thumbs: window.thumbs.clone(),
        });
        let pane = pane.with_columns();
        pane.setup_grid(&grid_factory);
        pane.setup_filter();
        pane.setup_views();
        pane.setup_search();
        pane.setup_drop();
        pane.setup_focus();
        pane.navigate(start, false);
        pane
    }

    /// Clicking or focusing anything inside the pane makes it the active side.
    fn setup_focus(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(move |_, _, _, _| {
            if let Some(p) = this.upgrade() {
                if let Some(w) = p.window.upgrade() {
                    w.set_active_side(p.side.get());
                }
            }
        });
        self.root.add_controller(click);
        let this = Rc::downgrade(self);
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| {
            if let Some(p) = this.upgrade() {
                if let Some(w) = p.window.upgrade() {
                    w.set_active_side(p.side.get());
                }
            }
        });
        self.root.add_controller(focus);
    }

    fn with_columns(self: Rc<Self>) -> Rc<Self> {
        // Columns need a weak reference to the pane, so they are built after Rc::new.
        let mut columns = Vec::new();

        let this = Rc::downgrade(&self);
        let name_factory = gtk::SignalListItemFactory::new();
        name_factory.connect_setup(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            let thumb = make_thumb(ICON_LIST);
            let label = gtk::Label::builder().xalign(0.0).ellipsize(pango::EllipsizeMode::Middle).hexpand(true).build();
            hbox.append(&thumb);
            hbox.append(&label);
            hbox.append(&make_tag_dot());
            let lock = gtk::Image::from_icon_name("changes-prevent-symbolic");
            lock.add_css_class("omafile-dim");
            lock.set_visible(false);
            hbox.append(&lock);
            let badge = gtk::Label::new(None);
            badge.add_css_class("omafile-git-badge");
            badge.set_visible(false);
            hbox.append(&badge);
            list_item.set_child(Some(&hbox));
            if let Some(p) = this.upgrade() {
                p.cell_items.borrow_mut().insert(hbox.clone().upcast(), list_item.clone());
                p.add_drag_source(&hbox);
                // Click on the git badge toggles staging for that file.
                let click = gtk::GestureClick::new();
                click.set_button(1);
                click.set_propagation_phase(gtk::PropagationPhase::Capture);
                let weak = this.clone();
                let li = list_item.clone();
                click.connect_pressed(move |g, _, _, _| {
                    let (Some(p), Some(item)) = (weak.upgrade(), li.item().and_downcast::<FileItem>()) else { return };
                    g.set_state(gtk::EventSequenceState::Claimed);
                    if let Some(w) = p.window.upgrade() {
                        w.git_toggle_stage(&p, &item);
                    }
                });
                badge.add_controller(click);
            }
        });
        let this = Rc::downgrade(&self);
        name_factory.connect_bind(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(item) = list_item.item().and_downcast::<FileItem>() else { return };
            let hbox = list_item.child().and_downcast::<gtk::Box>().unwrap();
            let thumb = hbox.first_child().and_downcast::<gtk::Stack>().unwrap();
            let badge = hbox.last_child().and_downcast::<gtk::Label>().unwrap();
            let lock = badge.prev_sibling().and_downcast::<gtk::Image>().unwrap();
            let dot = lock.prev_sibling().and_downcast::<gtk::Box>().unwrap();
            let label = dot.prev_sibling().and_downcast::<gtk::Label>().unwrap();
            if let Some(p) = this.upgrade() {
                bind_thumb(&p.thumbs, &thumb, &item, ICON_LIST, p.thumbnails_enabled());
                bind_tag_dot(&dot, p.tag_of(&item));
                let staged = item.file().path().map(|path| p.git.borrow().as_ref().map(|r| r.staged_paths.contains(&path)).unwrap_or(false)).unwrap_or(false);
                bind_git_badge(&badge, p.git_status_of(&item), staged);
                let path = item.file().path();
                let is_vault = item.is_dir() && path.as_ref().map(|d| crate::crypt::is_vault(d)).unwrap_or(false);
                let is_open_mount = item.is_dir() && !is_vault && path.as_ref().map(|d| crate::crypt::vault_for(d).is_some() && crate::crypt::is_mounted(d)).unwrap_or(false);
                let secret = item.is_secret();
                lock.set_visible(is_vault || is_open_mount || secret);
                if secret {
                    lock.set_icon_name(Some("dialog-password-symbolic"));
                    lock.add_css_class("omafile-secret");
                    lock.set_tooltip_text(Some("Looks like a secret: preview is masked, copying asks first"));
                } else {
                    lock.remove_css_class("omafile-secret");
                    lock.set_icon_name(Some(if is_open_mount { "changes-allow-symbolic" } else { "changes-prevent-symbolic" }));
                    lock.set_tooltip_text(Some(if is_open_mount { "Encrypted folder, unlocked" } else { "Encrypted folder (gocryptfs), locked" }));
                }
            }
            label.set_text(&item.display_label());
            apply_item_classes(&hbox, &item);
        });
        let name_col = gtk::ColumnViewColumn::new(Some("Name"), Some(name_factory));
        name_col.set_expand(true);
        name_col.set_resizable(true);
        name_col.set_sorter(Some(&gtk::CustomSorter::new(|a, b| {
            let a = a.downcast_ref::<FileItem>().unwrap().sort_key();
            let b = b.downcast_ref::<FileItem>().unwrap().sort_key();
            a.cmp(&b).into()
        })));
        columns.push(("name", name_col));

        let size_col = self.text_column("Size", 1.0, |item| match (item.is_dir(), item.dir_size()) {
            (true, None) => String::new(),
            (_, _) => ops::human_size(item.size()),
        });
        size_col.set_sorter(Some(&gtk::CustomSorter::new(|a, b| {
            let a = a.downcast_ref::<FileItem>().unwrap();
            let b = b.downcast_ref::<FileItem>().unwrap();
            (!a.is_dir(), a.size()).cmp(&(!b.is_dir(), b.size())).into()
        })));
        columns.push(("size", size_col));

        let type_col = self.text_column("Type", 0.0, |item| item.type_description());
        type_col.set_sorter(Some(&gtk::CustomSorter::new(|a, b| {
            let a = a.downcast_ref::<FileItem>().unwrap();
            let b = b.downcast_ref::<FileItem>().unwrap();
            (!a.is_dir(), a.type_description()).cmp(&(!b.is_dir(), b.type_description())).into()
        })));
        columns.push(("type", type_col));

        let modified_col = self.text_column("Modified", 0.0, |item| item.modified().map(|d| ops::format_time(&d)).unwrap_or_default());
        modified_col.set_sorter(Some(&gtk::CustomSorter::new(|a, b| {
            let a = a.downcast_ref::<FileItem>().unwrap();
            let b = b.downcast_ref::<FileItem>().unwrap();
            (!a.is_dir(), b.modified_unix()).cmp(&(!b.is_dir(), a.modified_unix())).into()
        })));
        columns.push(("modified", modified_col));

        // Trash only: when it was deleted and where it came from.
        let deleted_col = self.text_column("Deleted", 0.0, |item| item.trash_deleted().map(|d| ops::format_time(&d)).unwrap_or_default());
        deleted_col.set_sorter(Some(&gtk::CustomSorter::new(|a, b| {
            let a = a.downcast_ref::<FileItem>().unwrap().trash_deleted().map(|d| d.to_unix()).unwrap_or(0);
            let b = b.downcast_ref::<FileItem>().unwrap().trash_deleted().map(|d| d.to_unix()).unwrap_or(0);
            b.cmp(&a).into()
        })));
        deleted_col.set_visible(false);
        columns.push(("deleted", deleted_col));
        let origin_col = self.text_column("Original Location", 0.0, |item| {
            item.trash_origin().map(|p| {
                let path = std::path::Path::new(&p);
                window::abbreviate_home(path.parent().unwrap_or(path))
            }).unwrap_or_default()
        });
        origin_col.set_visible(false);
        columns.push(("origin", origin_col));

        for (_, col) in &columns {
            self.column_view.append_column(col);
        }
        self.column_view.sort_by_column(Some(&columns[0].1), gtk::SortType::Ascending);

        *self.columns.borrow_mut() = columns;
        self
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
            if let Some(p) = this.upgrade() {
                p.cell_items.borrow_mut().insert(label.upcast(), list_item.clone());
            }
        });
        factory.connect_bind(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(item) = list_item.item().and_downcast::<FileItem>() else { return };
            let label = list_item.child().and_downcast::<gtk::Label>().unwrap();
            label.set_text(&text(&item));
        });
        let col = gtk::ColumnViewColumn::new(Some(title), Some(factory));
        col.set_resizable(true);
        col
    }

    fn setup_grid(self: &Rc<Self>, factory: &gtk::SignalListItemFactory) {
        let this = Rc::downgrade(self);
        factory.connect_setup(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
            vbox.set_width_request(112);
            vbox.add_css_class("omafile-grid-item");
            let thumb = make_thumb(ICON_GRID);
            let label = gtk::Label::builder()
                .justify(gtk::Justification::Center)
                .wrap(true)
                .wrap_mode(pango::WrapMode::WordChar)
                .lines(2)
                .ellipsize(pango::EllipsizeMode::Middle)
                .max_width_chars(14)
                .build();
            let caption = gtk::Box::new(gtk::Orientation::Horizontal, 5);
            caption.set_halign(gtk::Align::Center);
            caption.append(&label);
            caption.append(&make_tag_dot());
            vbox.append(&thumb);
            vbox.append(&caption);
            list_item.set_child(Some(&vbox));
            if let Some(p) = this.upgrade() {
                p.cell_items.borrow_mut().insert(vbox.clone().upcast(), list_item.clone());
                p.add_drag_source(&vbox);
            }
        });
        let this = Rc::downgrade(self);
        factory.connect_bind(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(item) = list_item.item().and_downcast::<FileItem>() else { return };
            let vbox = list_item.child().and_downcast::<gtk::Box>().unwrap();
            let thumb = vbox.first_child().and_downcast::<gtk::Stack>().unwrap();
            let caption = vbox.last_child().and_downcast::<gtk::Box>().unwrap();
            let label = caption.first_child().and_downcast::<gtk::Label>().unwrap();
            let dot = caption.last_child().and_downcast::<gtk::Box>().unwrap();
            if let Some(p) = this.upgrade() {
                bind_thumb(&p.thumbs, &thumb, &item, ICON_GRID, p.thumbnails_enabled());
                bind_tag_dot(&dot, p.tag_of(&item));
            }
            label.set_text(&item.display_label());
            apply_item_classes(&vbox, &item);
        });
    }

    fn setup_filter(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        self.filter.set_filter_func(move |obj| {
            let Some(p) = this.upgrade() else { return true };
            let item = obj.downcast_ref::<FileItem>().unwrap();
            let show_hidden = p.window.upgrade().map(|w| w.show_hidden()).unwrap_or(false);
            if !show_hidden && item.is_hidden() {
                return false;
            }
            let state = p.state.borrow();
            if state.search.is_empty() || state.deep_results {
                return true;
            }
            item.display_name().to_lowercase().contains(&state.search)
        });
    }

    fn setup_views(self: &Rc<Self>) {
        for view in [self.column_view.clone().upcast::<gtk::Widget>(), self.grid_view.clone().upcast()] {
            // Keys that mean something else in text fields are handled on the views only.
            let this = Rc::downgrade(self);
            let keys = gtk::EventControllerKey::new();
            // Capture phase: the focused row would otherwise swallow Space for selection.
            keys.set_propagation_phase(gtk::PropagationPhase::Capture);
            keys.connect_key_pressed(move |_, key, _, state| {
                let Some(p) = this.upgrade() else { return glib::Propagation::Proceed };
                let Some(w) = p.window.upgrade() else { return glib::Propagation::Proceed };
                let plain = !state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK);
                match key {
                    gdk::Key::BackSpace if plain && !state.contains(gdk::ModifierType::SHIFT_MASK) => {
                        p.go_up();
                        glib::Propagation::Stop
                    }
                    gdk::Key::Delete if plain => {
                        let action = if state.contains(gdk::ModifierType::SHIFT_MASK) { "delete" } else { "trash" };
                        gio::prelude::ActionGroupExt::activate_action(&w.window, action, None);
                        glib::Propagation::Stop
                    }
                    gdk::Key::space if plain && !state.contains(gdk::ModifierType::SHIFT_MASK) => {
                        w.toggle_quick_look();
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            });
            view.add_controller(keys);
            let this = Rc::downgrade(self);
            let right = gtk::GestureClick::new();
            right.set_button(gdk::BUTTON_SECONDARY);
            right.connect_pressed(move |g, _, x, y| {
                if let (Some(p), Some(widget)) = (this.upgrade(), g.widget()) {
                    p.context_menu(&widget, x, y);
                }
            });
            view.add_controller(right);

            let this = Rc::downgrade(self);
            let middle = gtk::GestureClick::new();
            middle.set_button(gdk::BUTTON_MIDDLE);
            middle.connect_pressed(move |g, _, x, y| {
                let (Some(p), Some(widget)) = (this.upgrade(), g.widget()) else { return };
                if let Some(item) = p.position_at(&widget, x, y).and_then(|pos| p.item_at(pos)) {
                    if item.is_dir() {
                        if let Some(w) = p.window.upgrade() {
                            w.open_in_new_tab(item.target_file(), false);
                        }
                    }
                }
            });
            view.add_controller(middle);
        }
        let this = Rc::downgrade(self);
        self.column_view.connect_activate(move |_, pos| {
            if let Some(p) = this.upgrade() {
                p.activate_position(pos);
            }
        });
        let this = Rc::downgrade(self);
        self.grid_view.connect_activate(move |_, pos| {
            if let Some(p) = this.upgrade() {
                p.activate_position(pos);
            }
        });
        let this = Rc::downgrade(self);
        self.selection.connect_selection_changed(move |_, _, _| {
            if let Some(p) = this.upgrade() {
                p.selection_changed();
            }
        });
        let this = Rc::downgrade(self);
        self.selection.connect_items_changed(move |_, _, _, _| {
            if let Some(p) = this.upgrade() {
                p.selection_changed();
            }
        });
    }

    fn setup_search(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        self.search_entry.connect_search_changed(move |entry| {
            let Some(p) = this.upgrade() else { return };
            p.state.borrow_mut().search = entry.text().to_lowercase();
            if p.deep_toggle.is_active() {
                p.schedule_deep_search();
            } else {
                p.filter.changed(gtk::FilterChange::Different);
                p.update_empty();
            }
        });
        let this = Rc::downgrade(self);
        self.deep_toggle.connect_toggled(move |toggle| {
            let Some(p) = this.upgrade() else { return };
            p.search_entry.set_placeholder_text(Some(if toggle.is_active() { "Search this folder and everything below" } else { "Filter this folder" }));
            if toggle.is_active() {
                p.schedule_deep_search();
            } else if p.state.borrow().deep_results {
                p.leave_deep_search();
            }
        });
        let this = Rc::downgrade(self);
        self.search_bar.connect_search_mode_enabled_notify(move |bar| {
            let Some(p) = this.upgrade() else { return };
            if !bar.is_search_mode() && p.state.borrow().deep_results {
                p.leave_deep_search();
            }
        });
        let this = Rc::downgrade(self);
        self.search_entry.connect_stop_search(move |_| {
            if let Some(p) = this.upgrade() {
                p.search_bar.set_search_mode(false);
                p.focus_view();
            }
        });
        let this = Rc::downgrade(self);
        self.search_entry.connect_activate(move |_| {
            let Some(p) = this.upgrade() else { return };
            if p.selection.n_items() > 0 {
                p.activate_position(0);
            }
        });
    }

    // ----- drag and drop --------------------------------------------------------

    fn add_drag_source(self: &Rc<Self>, widget: &gtk::Box) {
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::COPY | gdk::DragAction::MOVE);
        let this = Rc::downgrade(self);
        let widget2 = widget.clone();
        source.connect_prepare(move |_, _, _| {
            let p = this.upgrade()?;
            let pos = p.cell_items.borrow().get(widget2.upcast_ref::<gtk::Widget>()).map(|li| li.position())?;
            if !p.selection.is_selected(pos) {
                p.selection.select_item(pos, true);
            }
            let files: Vec<gio::File> = p.selected_items().iter().map(|i| i.target_file()).collect();
            if files.is_empty() {
                return None;
            }
            Some(gdk::ContentProvider::for_value(&gdk::FileList::from_array(&files).to_value()))
        });
        let widget2 = widget.clone();
        source.connect_drag_begin(move |source, _| {
            let paintable = gtk::WidgetPaintable::new(Some(&widget2));
            source.set_icon(Some(&paintable), 16, 16);
        });
        widget.add_controller(source);
    }

    fn setup_drop(self: &Rc<Self>) {
        for view in [self.column_view.clone().upcast::<gtk::Widget>(), self.grid_view.clone().upcast()] {
            let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY | gdk::DragAction::MOVE);
            let this = Rc::downgrade(self);
            target.connect_drop(move |t, value, x, y| {
                let (Some(p), Some(view)) = (this.upgrade(), t.widget()) else { return false };
                let Ok(list) = value.get::<gdk::FileList>() else { return false };
                let dest = p
                    .position_at(&view, x, y)
                    .and_then(|pos| p.item_at(pos))
                    .filter(|item| item.is_dir())
                    .map(|item| item.target_file())
                    .unwrap_or_else(|| p.cwd());
                p.receive_files(list.files(), dest);
                true
            });
            view.add_controller(target);
        }
    }

    /// Files dropped onto this pane: move within the local file system, copy otherwise.
    pub fn receive_files(self: &Rc<Self>, files: Vec<gio::File>, dest_dir: gio::File) {
        let Some(w) = self.window.upgrade() else { return };
        let files: Vec<gio::File> = files
            .into_iter()
            .filter(|src| !(src.parent().map(|p| p.equal(&dest_dir)).unwrap_or(false) || src.equal(&dest_dir) || dest_dir.has_prefix(src)))
            .collect();
        if files.is_empty() {
            return;
        }
        let same_fs = files.iter().all(|f| f.is_native()) && dest_dir.is_native();
        w.transfer(files, dest_dir, if same_fs { crate::transfer::Mode::Move } else { crate::transfer::Mode::Copy }, self.clone(), false);
    }

    // ----- navigation -------------------------------------------------------------

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
            state.generation += 1;
            state.deep_results = false;
            state.virtual_title = None;
            if let Some(id) = state.deep_pending.take() {
                id.remove();
            }
            if let Some(c) = state.deep_cancel.take() {
                c.cancel();
            }
        }
        self.deep_toggle.set_active(false);
        self.search_bar.set_search_mode(false);
        self.search_entry.set_text("");
        let in_trash = file.uri_scheme().as_deref() == Some("trash");
        for (id, col) in self.columns.borrow().iter() {
            if *id == "deleted" || *id == "origin" {
                col.set_visible(in_trash);
            }
        }
        self.watch_cwd();
        self.reload();
        if self.terminal_visible() {
            if let (Some(t), Some(dir)) = (self.term.borrow().clone(), file.path()) {
                t.follow_to(&dir);
            }
        }
        if let Some(w) = self.window.upgrade() {
            w.pane_navigated(self);
        }
    }

    pub fn reload(self: &Rc<Self>) {
        if let Some(title) = self.virtual_title() {
            if let Some(color) = title.strip_prefix("Tag: ") {
                let color = color.to_lowercase();
                self.show_tag(&color);
            }
            return;
        }
        if self.state.borrow().deep_results {
            self.schedule_deep_search();
            return;
        }
        let (file, generation) = {
            let mut state = self.state.borrow_mut();
            state.loading = true;
            (state.cwd.clone(), state.generation)
        };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let git_wanted = file.is_native() && crate::git::available();
            let (result, git) = futures_util::future::join(ops::list_dir(&file), async {
                if git_wanted {
                    crate::git::status(&file.path().unwrap_or_default()).await
                } else {
                    None
                }
            })
            .await;
            let Some(p) = this.upgrade() else { return };
            if p.state.borrow().generation != generation {
                return;
            }
            *p.git.borrow_mut() = git;
            p.state.borrow_mut().loading = false;
            let selected: Vec<gio::File> = p.selected_items().iter().map(|i| i.file()).collect();
            match result {
                Ok(items) => {
                    p.store.remove_all();
                    p.store.extend_from_slice(&items);
                    for f in &selected {
                        p.select_file(f, false);
                    }
                    // Keep the keyboard cursor on the selection instead of jumping to the top.
                    if let Some(first) = (0..p.selection.n_items()).find(|i| p.selection.is_selected(*i)) {
                        let flags = gtk::ListScrollFlags::FOCUS;
                        if p.is_grid() {
                            p.grid_view.scroll_to(first, flags, None);
                        } else {
                            p.column_view.scroll_to(first, None, flags, None);
                        }
                    }
                }
                Err(e) => {
                    p.store.remove_all();
                    if let Some(w) = p.window.upgrade() {
                        w.toast_error(&format!("Cannot open {}: {}", window::display_name(&file), e.message()));
                    }
                }
            }
            p.update_empty();
            p.selection_changed();
            if !p.search_bar.is_search_mode() {
                if let Some(w) = p.window.upgrade() {
                    if w.is_active_pane(&p) && !w.path_entry.has_focus() {
                        p.focus_view();
                    }
                }
            }
        });
    }

    // ----- embedded terminal --------------------------------------------------------

    pub fn terminal_visible(&self) -> bool {
        self.term.borrow().as_ref().map(|t| t.root.is_visible()).unwrap_or(false)
    }

    /// Ctrl+`: show the terminal below the files, or hide it again.
    pub fn toggle_terminal(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else { return };
        let existing = self.term.borrow().clone();
        let (panel, created) = match existing {
            Some(t) => (t, false),
            None => {
                let t = crate::termpanel::TermPanel::new(&w, self);
                self.term_paned.set_end_child(Some(&t.root));
                *self.term.borrow_mut() = Some(t.clone());
                (t, true)
            }
        };
        if !created && panel.root.is_visible() {
            panel.root.set_visible(false);
            self.focus_view();
        } else {
            panel.root.set_visible(true);
            let height = self.term_paned.height();
            if height > 0 {
                self.term_paned.set_position((height - 280).max(height / 3));
            }
            if let Some(dir) = self.cwd().path() {
                panel.follow_to(&dir);
            }
            panel.focus();
            if std::env::var_os("OMAFILE_TRACE").is_some() {
                let paned = self.term_paned.clone();
                let root = panel.root.clone();
                let term = panel.terminal.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                    eprintln!(
                        "terminal panel: paned {}x{} pos {} end-child visible={} mapped={} alloc {}x{} term mapped={} has_focus={}",
                        paned.width(), paned.height(), paned.position(), root.is_visible(), root.is_mapped(), root.width(), root.height(), term.is_mapped(), term.has_focus()
                    );
                });
            }
        }
    }

    /// Repository root and status for `item`, when it has local changes.
    pub fn git_info_for(&self, item: &FileItem) -> Option<(std::path::PathBuf, crate::git::Status)> {
        let status = self.git_status_of(item)?;
        if status == crate::git::Status::Ignored {
            return None;
        }
        let root = self.git.borrow().as_ref()?.root.clone();
        Some((root, status))
    }

    pub fn git_status_of(&self, item: &FileItem) -> Option<crate::git::Status> {
        let git = self.git.borrow();
        let repo = git.as_ref()?;
        let path = item.file().path()?;
        repo.files.get(&path).cloned()
    }

    pub fn tag_of(&self, item: &FileItem) -> Option<String> {
        self.window.upgrade().and_then(|w| w.tags.get(&item.file()))
    }

    /// Show every file carrying `color` as a virtual listing.
    pub fn show_tag(self: &Rc<Self>, color: &str) {
        let Some(w) = self.window.upgrade() else { return };
        let files = w.tags.files_with(color);
        {
            let mut state = self.state.borrow_mut();
            state.generation += 1;
            state.deep_results = true;
            state.virtual_title = Some(format!("Tag: {}", crate::tags::label(color)));
            if let Some(c) = state.deep_cancel.take() {
                c.cancel();
            }
        }
        self.search_bar.set_search_mode(false);
        let generation = self.state.borrow().generation;
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let infos = futures_util::future::join_all(files.iter().map(ops::query)).await;
            let Some(p) = this.upgrade() else { return };
            if p.state.borrow().generation != generation {
                return;
            }
            let mut items = Vec::new();
            for (file, info) in files.iter().zip(infos) {
                if let Ok(item) = info {
                    item.set_label(Some(window::location_text(file)));
                    items.push(item);
                }
            }
            p.store.remove_all();
            p.store.extend_from_slice(&items);
            p.filter.changed(gtk::FilterChange::Different);
            p.update_empty();
            p.selection_changed();
        });
        w.pane_navigated(self);
    }

    pub fn virtual_title(&self) -> Option<String> {
        self.state.borrow().virtual_title.clone()
    }

    pub fn thumbnails_enabled(&self) -> bool {
        self.window.upgrade().map(|w| w.thumbnails_enabled.get()).unwrap_or(true)
    }

    // ----- recursive search ---------------------------------------------------------

    fn schedule_deep_search(self: &Rc<Self>) {
        let mut state = self.state.borrow_mut();
        if let Some(id) = state.deep_pending.take() {
            id.remove();
        }
        let this = Rc::downgrade(self);
        state.deep_pending = Some(glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
            if let Some(p) = this.upgrade() {
                p.state.borrow_mut().deep_pending = None;
                p.run_deep_search();
            }
        }));
    }

    fn leave_deep_search(self: &Rc<Self>) {
        {
            let mut state = self.state.borrow_mut();
            state.deep_results = false;
            state.virtual_title = None;
            state.generation += 1;
            if let Some(c) = state.deep_cancel.take() {
                c.cancel();
            }
        }
        self.filter.changed(gtk::FilterChange::Different);
        self.reload();
    }

    fn run_deep_search(self: &Rc<Self>) {
        let query = self.state.borrow().search.clone();
        if query.chars().count() < 2 {
            if self.state.borrow().deep_results {
                self.leave_deep_search();
            }
            return;
        }
        let cancel = gio::Cancellable::new();
        let generation = {
            let mut state = self.state.borrow_mut();
            if let Some(c) = state.deep_cancel.take() {
                c.cancel();
            }
            state.deep_cancel = Some(cancel.clone());
            state.generation += 1;
            state.deep_results = true;
            state.generation
        };
        let cwd = self.cwd();
        self.store.remove_all();
        self.status.set_text("Searching…");
        self.update_empty();
        let started = std::time::Instant::now();
        let this = Rc::downgrade(self);
        let show_hidden = self.window.upgrade().map(|w| w.show_hidden()).unwrap_or(false);
        // Every batch of paths is turned into items concurrently and appended
        // at once, so the view fills while fd is still walking the tree.
        let emit: Rc<dyn Fn(Vec<gio::File>, bool)> = Rc::new({
            let this = this.clone();
            let cwd = cwd.clone();
            let query = query.clone();
            move |files, done| {
                let Some(p) = this.upgrade() else { return };
                let this = this.clone();
                let cwd = cwd.clone();
                let query = query.clone();
                let _ = p;
                glib::spawn_future_local(async move {
                    let infos = futures_util::future::join_all(files.iter().map(ops::query)).await;
                    let Some(p) = this.upgrade() else { return };
                    if p.state.borrow().generation != generation {
                        return;
                    }
                    let mut items = Vec::new();
                    for (file, info) in files.iter().zip(infos) {
                        if let Ok(item) = info {
                            let rel = cwd.relative_path(file).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|| item.display_name());
                            item.set_label(Some(rel));
                            items.push(item);
                        }
                    }
                    p.store.extend_from_slice(&items);
                    if done {
                        p.store.sort(|a, b| {
                            let a = a.downcast_ref::<FileItem>().unwrap();
                            let b = b.downcast_ref::<FileItem>().unwrap();
                            crate::search::rank(&a.display_label(), &query).cmp(&crate::search::rank(&b.display_label(), &query))
                        });
                        p.state.borrow_mut().deep_cancel = None;
                        if std::env::var_os("OMAFILE_TRACE").is_some() {
                            eprintln!("search \"{query}\": {} results in {} ms", p.store.n_items(), started.elapsed().as_millis());
                        }
                    }
                    p.update_empty();
                    p.selection_changed();
                });
            }
        });
        glib::spawn_future_local(async move {
            crate::search::deep_search(&cwd, &query, show_hidden, cancel, emit).await;
        });
    }

    fn update_empty(&self) {
        let has_items = self.selection.n_items() > 0;
        let mode = self.view_mode();
        if has_items {
            self.view_stack.set_visible_child_name(if mode == "grid" { "grid" } else { "list" });
        } else {
            let filtering = !self.state.borrow().search.is_empty() || self.state.borrow().deep_results;
            self.empty.set_title(if filtering { "No Matches" } else { "Empty Folder" });
            self.empty.set_icon_name(Some(if filtering { "edit-find-symbolic" } else { "folder-open-symbolic" }));
            self.view_stack.set_visible_child_name("empty");
        }
    }

    fn watch_cwd(self: &Rc<Self>) {
        let cwd = self.state.borrow().cwd.clone();
        let monitor = cwd.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE).ok();
        if let Some(m) = &monitor {
            let this = Rc::downgrade(self);
            m.connect_changed(move |_, _, _, _| {
                let Some(p) = this.upgrade() else { return };
                let mut state = p.state.borrow_mut();
                if let Some(id) = state.monitor_pending.take() {
                    id.remove();
                }
                let this2 = this.clone();
                state.monitor_pending = Some(glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                    if let Some(p) = this2.upgrade() {
                        p.state.borrow_mut().monitor_pending = None;
                        p.reload();
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
        let (parent, from) = {
            let state = self.state.borrow();
            (state.cwd.parent(), state.cwd.clone())
        };
        if let Some(parent) = parent {
            self.navigate(parent, true);
            self.select_file_later(from);
        }
    }

    // ----- helpers --------------------------------------------------------------------

    pub fn cwd(&self) -> gio::File {
        self.state.borrow().cwd.clone()
    }

    pub fn can_go_back(&self) -> bool {
        !self.state.borrow().back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.state.borrow().forward.is_empty()
    }

    pub fn view_mode(&self) -> String {
        self.window.upgrade().map(|w| w.view_mode()).unwrap_or_else(|| "list".into())
    }

    pub fn apply_view_mode(&self) {
        self.update_empty();
    }

    pub fn is_grid(&self) -> bool {
        self.view_mode() == "grid"
    }

    pub fn focus_view(&self) {
        if self.is_grid() {
            self.grid_view.grab_focus();
        } else {
            self.column_view.grab_focus();
        }
    }

    pub fn sort_by(&self, key: &str) {
        let col = self.columns.borrow().iter().find(|(k, _)| *k == key).map(|(_, c)| c.clone());
        if let Some(col) = col {
            self.column_view.sort_by_column(Some(&col), gtk::SortType::Ascending);
        }
    }

    pub fn item_at(&self, position: u32) -> Option<FileItem> {
        self.selection.item(position).and_downcast::<FileItem>()
    }

    /// The bound cell widget currently showing `item`, if it is on screen.
    pub fn widget_for(&self, item: &FileItem) -> Option<gtk::Widget> {
        let map = self.cell_items.borrow();
        let grid = self.view_stack.visible_child_name().as_deref() == Some("grid");
        map.iter()
            .filter(|(w, li)| {
                let right_view = if grid { w.is::<gtk::Box>() && w.downcast_ref::<gtk::Box>().map(|b| b.orientation() == gtk::Orientation::Vertical).unwrap_or(false) } else { w.is::<gtk::Box>() && w.downcast_ref::<gtk::Box>().map(|b| b.orientation() == gtk::Orientation::Horizontal).unwrap_or(false) };
                right_view && w.is_mapped() && li.item().and_downcast::<FileItem>().map(|i| i.file().equal(&item.file())).unwrap_or(false)
            })
            .map(|(w, _)| w.clone())
            .next()
    }

    /// Rename in place: a small entry anchored to the row (F2). Falls back to a
    /// dialog when the row is not on screen.
    pub fn inline_rename(self: &Rc<Self>, item: &FileItem) -> bool {
        let Some(anchor) = self.widget_for(item) else { return false };
        let old = item.display_name();
        let entry = gtk::Entry::builder().text(&old).width_chars((old.chars().count() as i32 + 4).clamp(16, 60)).build();
        entry.add_css_class("omafile-inline-rename");
        let popover = gtk::Popover::builder().child(&entry).has_arrow(false).position(gtk::PositionType::Bottom).autohide(true).build();
        popover.add_css_class("omafile-rename-popover");
        popover.set_parent(&anchor);
        let w = anchor.width();
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(0, 0, w.max(1), anchor.height().max(1))));
        popover.set_offset(0, -anchor.height());
        let this = Rc::downgrade(self);
        let item2 = item.clone();
        let pop2 = popover.clone();
        entry.connect_activate(move |e| {
            let name = e.text().trim().to_string();
            pop2.popdown();
            let Some(p) = this.upgrade() else { return };
            if let Some(win) = p.window.upgrade() {
                win.do_rename(p.clone(), item2.clone(), name);
            }
        });
        let esc = gtk::EventControllerKey::new();
        let pop3 = popover.clone();
        esc.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                pop3.popdown();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        entry.add_controller(esc);
        let this = Rc::downgrade(self);
        popover.connect_closed(move |pop| {
            let pop = pop.clone();
            let this = this.clone();
            glib::idle_add_local_once(move || {
                pop.unparent();
                if let Some(p) = this.upgrade() {
                    if let Some(w) = p.window.upgrade() {
                        w.refocus();
                    }
                }
            });
        });
        popover.popup();
        let stem_end = if item.is_dir() { None } else { old.rfind('.').filter(|&i| i > 0) };
        entry.grab_focus();
        entry.select_region(0, stem_end.map(|i| i as i32).unwrap_or(-1));
        true
    }

    /// Compute recursive sizes for the folders in view, then sort by size (disk usage).
    pub fn compute_folder_sizes(self: &Rc<Self>) {
        let generation = self.state.borrow().generation;
        let dirs: Vec<FileItem> = (0..self.store.n_items()).filter_map(|i| self.store.item(i).and_downcast::<FileItem>()).filter(|i| i.is_dir() && i.dir_size().is_none()).collect();
        if dirs.is_empty() {
            self.sort_by_size();
            return;
        }
        self.status.set_text(&format!("Measuring {} folders…", dirs.len()));
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            for (n, item) in dirs.iter().enumerate() {
                let file = item.target_file();
                let size = ops::dir_size(&file).await;
                let Some(p) = this.upgrade() else { return };
                if p.state.borrow().generation != generation {
                    return;
                }
                item.set_dir_size(Some(size));
                if let Some(pos) = (0..p.store.n_items()).find(|i| p.store.item(*i).and_downcast::<FileItem>().map(|x| x == *item).unwrap_or(false)) {
                    p.store.items_changed(pos, 1, 1);
                }
                p.status.set_text(&format!("Measuring folders… {} of {}", n + 1, dirs.len()));
            }
            if let Some(p) = this.upgrade() {
                p.sort_by_size();
                p.selection_changed();
            }
        });
    }

    fn sort_by_size(&self) {
        if let Some((_, col)) = self.columns.borrow().iter().find(|(id, _)| *id == "size") {
            self.column_view.sort_by_column(Some(col), gtk::SortType::Descending);
        }
    }

    pub fn selected_items(&self) -> Vec<FileItem> {
        let n = self.selection.n_items();
        (0..n).filter(|i| self.selection.is_selected(*i)).filter_map(|i| self.item_at(i)).collect()
    }

    pub fn select_file(&self, file: &gio::File, exclusive: bool) {
        for i in 0..self.selection.n_items() {
            if let Some(item) = self.item_at(i) {
                if item.file().equal(file) {
                    self.selection.select_item(i, exclusive);
                    self.column_view.scroll_to(i, None, gtk::ListScrollFlags::NONE, None);
                    self.grid_view.scroll_to(i, gtk::ListScrollFlags::NONE, None);
                    return;
                }
            }
        }
    }

    /// Select `file` once the next reload has landed.
    pub fn select_file_later(self: &Rc<Self>, file: gio::File) {
        let this = Rc::downgrade(self);
        glib::timeout_add_local_once(std::time::Duration::from_millis(350), move || {
            if let Some(p) = this.upgrade() {
                p.select_file(&file, true);
            }
        });
    }

    pub fn activate_position(self: &Rc<Self>, position: u32) {
        if let Some(item) = self.item_at(position) {
            self.open_item(&item);
        }
    }

    pub fn open_item(self: &Rc<Self>, item: &FileItem) {
        if item.is_dir() {
            self.navigate(item.target_file(), true);
        } else if let Some(w) = self.window.upgrade() {
            w.launch(&item.target_file(), false);
        }
    }

    fn selection_changed(self: &Rc<Self>) {
        let total = self.selection.n_items();
        let selected = self.selected_items();
        let deep = self.state.borrow().deep_results;
        let virtual_view = self.state.borrow().virtual_title.is_some();
        let mut text = if virtual_view {
            format!("{total} tagged {}", if total == 1 { "item" } else { "items" })
        } else if deep {
            format!("{total} {} found", if total == 1 { "match" } else { "matches" })
        } else {
            format!("{total} {}", if total == 1 { "item" } else { "items" })
        };
        if !selected.is_empty() {
            let size: u64 = selected.iter().filter(|i| !i.is_dir()).map(|i| i.size()).sum();
            text.push_str(&format!(", {} selected", selected.len()));
            if size > 0 {
                text.push_str(&format!(" ({})", ops::human_size(size)));
            }
        } else if deep {
        } else if let Some(repo) = self.git.borrow().as_ref() {
            text.push_str(&format!("  ·  {}", repo.summary()));
        } else if let Some(path) = self.state.borrow().cwd.path() {
            text.push_str(&format!("  ·  {}", window::abbreviate_home(&path)));
        } else {
            text.push_str(&format!("  ·  {}", self.state.borrow().cwd.uri()));
        }
        self.status.set_text(&text);
        match self.git.borrow().as_ref() {
            Some(repo) if !deep && !virtual_view => {
                self.branch_btn.set_label(&format!("\u{2387} {}", repo.branch));
                self.branch_btn.set_visible(true);
            }
            _ => self.branch_btn.set_visible(false),
        }
        if let Some(w) = self.window.upgrade() {
            w.pane_selection_changed(self);
        }
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
        let point = view
            .compute_point(&self.view_stack, &gtk::graphene::Point::new(x as f32, y as f32))
            .unwrap_or_else(|| gtk::graphene::Point::new(x as f32, y as f32));
        let rect = gdk::Rectangle::new(point.x() as i32, point.y() as i32, 1, 1);
        let in_trash = self.cwd().uri_scheme().as_deref() == Some("trash");
        if position.is_some() {
            let items = self.selected_items();
            let single_dir = items.len() == 1 && items[0].is_dir();
            let archive = items.len() == 1 && items[0].is_archive() && items[0].target_file().is_native();
            let vault = window::VaultState::of(items.first().filter(|_| single_dir).and_then(|i| i.file().path()));
            let in_repo = self.git.borrow().is_some();
            let theme = single_dir && items[0].target_file().path().map(|p| crate::omarchy::is_theme_dir(&p)).unwrap_or(false);
            let flags = window::MenuFlags {
                single_dir,
                archive,
                in_trash,
                multiple: items.len() > 1,
                in_repo,
                images: Window::image_menu_applies(&items),
                theme,
                local: items.iter().all(|i| i.target_file().is_native()),
            };
            self.item_menu.set_menu_model(Some(&window::item_menu_model_ext(flags, vault)));
            self.item_menu.set_pointing_to(Some(&rect));
            self.item_menu.popup();
        } else {
            let project = self.cwd().path().and_then(|d| crate::projects::detect(&d));
            let in_repo = self.git.borrow().is_some();
            self.background_menu.set_menu_model(Some(&window::background_menu_model(in_trash, project.as_ref(), in_repo)));
            self.background_menu.set_pointing_to(Some(&rect));
            self.background_menu.popup();
        }
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        self.item_menu.unparent();
        self.background_menu.unparent();
        if let Some(c) = self.state.borrow_mut().deep_cancel.take() {
            c.cancel();
        }
    }
}

/// Icon-or-thumbnail widget used in both views.
fn make_thumb(size: i32) -> gtk::Stack {
    let stack = gtk::Stack::new();
    stack.set_hhomogeneous(true);
    stack.set_vhomogeneous(true);
    let image = gtk::Image::new();
    image.set_pixel_size(size);
    let picture = gtk::Picture::new();
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_size_request(size, size);
    picture.set_can_shrink(true);
    picture.add_css_class("omafile-thumb");
    stack.add_named(&image, Some("icon"));
    stack.add_named(&picture, Some("thumb"));
    // Switch to the thumbnail whenever one arrives, back to the icon when cleared.
    let stack2 = stack.clone();
    picture.connect_paintable_notify(move |p| {
        stack2.set_visible_child_name(if p.paintable().is_some() { "thumb" } else { "icon" });
    });
    stack.set_valign(gtk::Align::Center);
    stack.set_halign(gtk::Align::Center);
    stack
}

fn bind_thumb(thumbs: &Rc<ThumbCache>, stack: &gtk::Stack, item: &FileItem, size: i32, enabled: bool) {
    let image = stack.child_by_name("icon").and_downcast::<gtk::Image>().unwrap();
    let picture = stack.child_by_name("thumb").and_downcast::<gtk::Picture>().unwrap();
    match item.icon() {
        Some(icon) => image.set_from_gicon(&icon),
        None => image.set_icon_name(Some("text-x-generic")),
    }
    if enabled && ThumbCache::can_thumbnail(item) {
        thumbs.assign(item, &picture, size * 2);
    } else {
        picture.set_widget_name("");
        picture.set_paintable(None::<&gdk::Paintable>);
    }
    stack.set_visible_child_name(if picture.paintable().is_some() { "thumb" } else { "icon" });
}

fn make_tag_dot() -> gtk::Box {
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("omafile-tag-dot");
    dot.set_valign(gtk::Align::Center);
    dot.set_visible(false);
    dot
}

fn bind_tag_dot(dot: &gtk::Box, color: Option<String>) {
    for (name, _, _) in crate::tags::COLORS {
        dot.remove_css_class(&format!("omafile-tag-{name}"));
    }
    match color {
        Some(c) => {
            dot.add_css_class(&format!("omafile-tag-{c}"));
            dot.set_tooltip_text(Some(&format!("Tag: {}", crate::tags::label(&c))));
            dot.set_visible(true);
        }
        None => dot.set_visible(false),
    }
}

fn bind_git_badge(badge: &gtk::Label, status: Option<crate::git::Status>, staged: bool) {
    for cls in ["omafile-git-modified", "omafile-git-added", "omafile-git-deleted", "omafile-git-ignored", "omafile-git-staged"] {
        badge.remove_css_class(cls);
    }
    match status {
        Some(s) => {
            badge.set_text(s.badge());
            badge.add_css_class(s.css());
            if staged {
                badge.add_css_class("omafile-git-staged");
            }
            badge.set_tooltip_text(Some(&format!("git: {}{}  ·  click to {}", s.label(), if staged { " (staged)" } else { "" }, if staged { "unstage" } else { "stage" })));
            badge.set_visible(true);
        }
        None => badge.set_visible(false),
    }
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
