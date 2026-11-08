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
    pub side: Cell<usize>,
    pub item_menu: gtk::PopoverMenu,
    pub background_menu: gtk::PopoverMenu,
    pub cell_items: RefCell<HashMap<gtk::Widget, gtk::ListItem>>,
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

        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("omafile-card");
        card.append(&view_stack);
        card.append(&status);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&search_bar);
        root.append(&card);

        let item_menu = gtk::PopoverMenu::from_model(Some(&window::item_menu_model(false, false, false, false)));
        item_menu.set_has_arrow(false);
        item_menu.set_parent(&view_stack);
        let background_menu = gtk::PopoverMenu::from_model(Some(&window::background_menu_model(false)));
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
            side: Cell::new(side),
            item_menu,
            background_menu,
            cell_items: RefCell::new(HashMap::new()),
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
            list_item.set_child(Some(&hbox));
            if let Some(p) = this.upgrade() {
                p.cell_items.borrow_mut().insert(hbox.clone().upcast(), list_item.clone());
                p.add_drag_source(&hbox);
            }
        });
        let this = Rc::downgrade(&self);
        name_factory.connect_bind(move |_, obj| {
            let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(item) = list_item.item().and_downcast::<FileItem>() else { return };
            let hbox = list_item.child().and_downcast::<gtk::Box>().unwrap();
            let thumb = hbox.first_child().and_downcast::<gtk::Stack>().unwrap();
            let dot = hbox.last_child().and_downcast::<gtk::Box>().unwrap();
            let label = dot.prev_sibling().and_downcast::<gtk::Label>().unwrap();
            if let Some(p) = this.upgrade() {
                bind_thumb(&p.thumbs, &thumb, &item, ICON_LIST, p.thumbnails_enabled());
                bind_tag_dot(&dot, p.tag_of(&item));
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

        let size_col = self.text_column("Size", 1.0, |item| if item.is_dir() { String::new() } else { ops::human_size(item.size()) });
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
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let mut moved = 0;
            let mut copied = 0;
            let mut error = None;
            for src in files {
                if src.parent().map(|p| p.equal(&dest_dir)).unwrap_or(false) || src.equal(&dest_dir) || dest_dir.has_prefix(&src) {
                    continue;
                }
                let Some(name) = src.basename() else { continue };
                let dst = ops::unique_child(&dest_dir, &name.to_string_lossy());
                let same_fs = src.is_native() && dest_dir.is_native();
                let result = if same_fs { ops::move_file(src.clone(), dst).await } else { ops::copy_recursive(src.clone(), dst).await };
                match result {
                    Ok(()) if same_fs => moved += 1,
                    Ok(()) => copied += 1,
                    Err(e) => {
                        error = Some(format!("{}: {}", name.to_string_lossy(), e.message()));
                        break;
                    }
                }
            }
            if let Some(p) = this.upgrade() {
                p.reload();
            }
            match error {
                Some(e) => w.toast_error(&e),
                None if moved + copied == 0 => {}
                None => w.toast(&format!(
                    "{} {} {}",
                    if moved > 0 { "Moved" } else { "Copied" },
                    moved + copied,
                    if moved + copied == 1 { "item" } else { "items" }
                )),
            }
        });
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
        self.watch_cwd();
        self.reload();
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
            let result = ops::list_dir(&file).await;
            let Some(p) = this.upgrade() else { return };
            if p.state.borrow().generation != generation {
                return;
            }
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
        } else if let Some(path) = self.state.borrow().cwd.path() {
            text.push_str(&format!("  ·  {}", window::abbreviate_home(&path)));
        } else {
            text.push_str(&format!("  ·  {}", self.state.borrow().cwd.uri()));
        }
        self.status.set_text(&text);
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
            self.item_menu.set_menu_model(Some(&window::item_menu_model(single_dir, archive, in_trash, items.len() > 1)));
            self.item_menu.set_pointing_to(Some(&rect));
            self.item_menu.popup();
        } else {
            self.background_menu.set_menu_model(Some(&window::background_menu_model(in_trash)));
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
