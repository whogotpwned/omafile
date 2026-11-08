//! Finder's other two views: Miller columns (`Ctrl+3`) and the gallery
//! (`Ctrl+4`). Both sit on the pane's own selection model for the current
//! folder, so every action (open, rename, tag, …) works unchanged.

use crate::item::FileItem;
use crate::ops;
use crate::pane::Pane;
use crate::preview::PreviewPane;
use adw::prelude::*;
use gtk::{gio, glib, pango};
use std::cell::RefCell;
use std::rc::{Rc, Weak};

const COLUMN_WIDTH: i32 = 230;

struct Column {
    dir: gio::File,
    list: gtk::ListView,
    scroll: gtk::ScrolledWindow,
}

pub struct ColumnsView {
    pub root: gtk::ScrolledWindow,
    hbox: gtk::Box,
    pane: Weak<Pane>,
    columns: RefCell<Vec<Column>>,
    preview: Rc<PreviewPane>,
    preview_box: gtk::Box,
    /// Set while this view changes the pane's folder itself.
    busy: std::cell::Cell<bool>,
}

fn row_factory(pane: Weak<Pane>) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, obj| {
        let li = obj.downcast_ref::<gtk::ListItem>().unwrap();
        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        hbox.set_margin_start(6);
        hbox.set_margin_end(4);
        let icon = gtk::Image::new();
        icon.set_pixel_size(16);
        let label = gtk::Label::builder().xalign(0.0).hexpand(true).ellipsize(pango::EllipsizeMode::Middle).build();
        let chevron = gtk::Image::from_icon_name("go-next-symbolic");
        chevron.add_css_class("omafile-dim");
        hbox.append(&icon);
        hbox.append(&label);
        hbox.append(&chevron);
        li.set_child(Some(&hbox));
        if let Some(p) = pane.upgrade() {
            p.cell_items.borrow_mut().insert(hbox.clone().upcast(), li.clone());
        }
    });
    factory.connect_bind(|_, obj| {
        let li = obj.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(item) = li.item().and_downcast::<FileItem>() else { return };
        let hbox = li.child().and_downcast::<gtk::Box>().unwrap();
        let icon = hbox.first_child().and_downcast::<gtk::Image>().unwrap();
        let label = icon.next_sibling().and_downcast::<gtk::Label>().unwrap();
        let chevron = label.next_sibling().and_downcast::<gtk::Image>().unwrap();
        match item.icon() {
            Some(g) => icon.set_from_gicon(&g),
            None => icon.set_icon_name(Some("text-x-generic")),
        }
        label.set_text(&item.display_name());
        chevron.set_visible(item.is_dir());
        if item.is_hidden() {
            hbox.add_css_class("omafile-hidden");
        } else {
            hbox.remove_css_class("omafile-hidden");
        }
    });
    factory
}

impl ColumnsView {
    pub fn new(pane: &Rc<Pane>) -> Rc<Self> {
        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        hbox.add_css_class("omafile-columns");
        let preview = PreviewPane::new(pane.thumbs.clone());
        let preview_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        preview_box.set_size_request(300, -1);
        preview_box.add_css_class("omafile-column");
        preview_box.append(&preview.root);
        preview_box.set_visible(false);
        hbox.append(&preview_box);
        let root = gtk::ScrolledWindow::builder().child(&hbox).hscrollbar_policy(gtk::PolicyType::Automatic).vscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build();
        let this = Rc::new(ColumnsView { root, hbox, pane: Rc::downgrade(pane), columns: RefCell::new(Vec::new()), preview, preview_box, busy: std::cell::Cell::new(false) });
        this.rebuild(&pane.cwd());
        this
    }

    fn make_column(self: &Rc<Self>, dir: gio::File, model: &impl IsA<gtk::SelectionModel>) -> Column {
        let list = gtk::ListView::new(Some(model.clone()), Some(row_factory(self.pane.clone())));
        list.add_css_class("omafile-files");
        list.set_single_click_activate(false);
        let scroll = gtk::ScrolledWindow::builder().child(&list).hscrollbar_policy(gtk::PolicyType::Never).width_request(COLUMN_WIDTH).build();
        scroll.add_css_class("omafile-column");
        // Enter / double click opens like everywhere else.
        let weak = Rc::downgrade(self);
        list.connect_activate(move |view, pos| {
            let (Some(c), Some(p)) = (weak.upgrade(), weak.upgrade().and_then(|c| c.pane.upgrade())) else { return };
            if let Some(item) = view.model().and_then(|m| m.item(pos)).and_downcast::<FileItem>() {
                if item.is_dir() {
                    c.enter(&p, item.target_file());
                } else {
                    p.open_item(&item);
                }
            }
        });
        // Arrow keys across columns.
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(c) = weak.upgrade() else { return glib::Propagation::Proceed };
            match key {
                gtk::gdk::Key::Right => {
                    c.focus_last_or_enter();
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Left => {
                    if let Some(p) = c.pane.upgrade() {
                        p.go_up();
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        list.add_controller(keys);
        Column { dir, list, scroll }
    }

    /// Start over: the chain from home (or /) down to `cwd`.
    pub fn rebuild(self: &Rc<Self>, cwd: &gio::File) {
        for c in self.columns.borrow_mut().drain(..) {
            self.hbox.remove(&c.scroll);
        }
        let Some(pane) = self.pane.upgrade() else { return };
        let home = gio::File::for_path(glib::home_dir());
        let base = if cwd.equal(&home) || cwd.has_prefix(&home) { home } else { cwd.parent().unwrap_or_else(|| cwd.clone()) };
        // Ancestors between base and cwd, top first.
        let mut chain = Vec::new();
        let mut cur = cwd.clone();
        while !cur.equal(&base) {
            match cur.parent() {
                Some(p) => {
                    chain.push(cur.clone());
                    cur = p;
                }
                None => break,
            }
        }
        chain.reverse();
        // Ancestor columns: own stores with the next element selected.
        let mut dir = base.clone();
        for child in &chain {
            self.push_ancestor(dir.clone(), Some(child.clone()));
            dir = child.clone();
        }
        // The last column is the pane's live model for cwd.
        let col = self.make_column(cwd.clone(), &pane.selection);
        // insert_child_after with no sibling prepends, so this covers the empty case too.
        self.hbox.insert_child_after(&col.scroll, self.columns.borrow().last().map(|c| &c.scroll));
        self.columns.borrow_mut().push(col);
        self.watch_last();
        self.scroll_to_end();
    }

    fn push_ancestor(self: &Rc<Self>, dir: gio::File, selected: Option<gio::File>) {
        let store = gio::ListStore::new::<FileItem>();
        let sel = gtk::SingleSelection::new(Some(store.clone()));
        sel.set_autoselect(false);
        sel.set_can_unselect(true);
        let col = self.make_column(dir.clone(), &sel);
        let after = self.columns.borrow().last().map(|c| c.scroll.clone());
        match after {
            Some(a) => self.hbox.insert_child_after(&col.scroll, Some(&a)),
            None => self.hbox.prepend(&col.scroll),
        }
        self.columns.borrow_mut().push(col);
        // Load it, then select the child that leads on.
        let weak = Rc::downgrade(self);
        let show_hidden = self.pane.upgrade().and_then(|p| p.window.upgrade()).map(|w| w.show_hidden()).unwrap_or(false);
        glib::spawn_future_local(async move {
            let Ok(items) = ops::list_dir(&dir).await else { return };
            let mut items: Vec<FileItem> = items.into_iter().filter(|i| show_hidden || !i.is_hidden()).collect();
            items.sort_by_key(|i| i.sort_key());
            store.extend_from_slice(&items);
            if let Some(target) = &selected {
                if let Some(pos) = items.iter().position(|i| i.file().equal(target)) {
                    sel.set_selected(pos as u32);
                }
            }
            let Some(c) = weak.upgrade() else { return };
            // Clicking an entry in an ancestor column navigates there.
            let weak2 = Rc::downgrade(&c);
            let dir2 = dir.clone();
            sel.connect_selected_item_notify(move |s| {
                let (Some(c), Some(p)) = (weak2.upgrade(), weak2.upgrade().and_then(|c| c.pane.upgrade())) else { return };
                if c.busy.get() {
                    return;
                }
                let Some(item) = s.selected_item().and_downcast::<FileItem>() else { return };
                if item.is_dir() {
                    c.enter(&p, item.target_file());
                } else {
                    c.busy.set(true);
                    p.navigate(dir2.clone(), true);
                    c.busy.set(false);
                    p.select_file_later(item.file());
                }
            });
        });
    }

    /// Open `dir` from the pane: the pane navigates, we add a column.
    fn enter(self: &Rc<Self>, pane: &Rc<Pane>, dir: gio::File) {
        self.busy.set(true);
        pane.navigate(dir, true);
        self.busy.set(false);
    }

    /// Called by the pane after its folder changed.
    pub fn on_navigate(self: &Rc<Self>, cwd: &gio::File) {
        let position = self.columns.borrow().iter().position(|c| c.dir.equal(cwd));
        let parent_pos = cwd.parent().and_then(|p| self.columns.borrow().iter().position(|c| c.dir.equal(&p)));
        let Some(pane) = self.pane.upgrade() else { return };
        if let Some(i) = position {
            // Went back up: that column becomes the live one again.
            self.truncate_after(i);
            let col = &self.columns.borrow()[i];
            col.list.set_model(Some(&pane.selection));
        } else if let Some(i) = parent_pos {
            // Went down one level from column i.
            self.truncate_after(i);
            {
                let cols = self.columns.borrow();
                let col = &cols[i];
                // Freeze the parent column into its own store with cwd selected.
                if col.list.model().map(|m| m == pane.selection.clone().upcast::<gtk::SelectionModel>()).unwrap_or(false) {
                    let store = gio::ListStore::new::<FileItem>();
                    let items: Vec<FileItem> = (0..pane.selection.n_items()).filter_map(|j| pane.item_at(j)).collect();
                    store.extend_from_slice(&items);
                    let sel = gtk::SingleSelection::new(Some(store));
                    sel.set_autoselect(false);
                    sel.set_can_unselect(true);
                    if let Some(pos) = items.iter().position(|it| it.target_file().equal(cwd)) {
                        sel.set_selected(pos as u32);
                    }
                    col.list.set_model(Some(&sel));
                    let weak = Rc::downgrade(self);
                    let dir2 = col.dir.clone();
                    sel.connect_selected_item_notify(move |s| {
                        let (Some(c), Some(p)) = (weak.upgrade(), weak.upgrade().and_then(|c| c.pane.upgrade())) else { return };
                        if c.busy.get() {
                            return;
                        }
                        let Some(item) = s.selected_item().and_downcast::<FileItem>() else { return };
                        if item.is_dir() {
                            c.enter(&p, item.target_file());
                        } else {
                            c.busy.set(true);
                            p.navigate(dir2.clone(), true);
                            c.busy.set(false);
                            p.select_file_later(item.file());
                        }
                    });
                }
            }
            let col = self.make_column(cwd.clone(), &pane.selection);
            let after = self.columns.borrow()[i].scroll.clone();
            self.hbox.insert_child_after(&col.scroll, Some(&after));
            self.columns.borrow_mut().push(col);
        } else {
            self.rebuild(cwd);
            return;
        }
        self.watch_last();
        self.scroll_to_end();
    }

    fn truncate_after(&self, i: usize) {
        let mut cols = self.columns.borrow_mut();
        while cols.len() > i + 1 {
            let c = cols.pop().unwrap();
            self.hbox.remove(&c.scroll);
        }
    }

    /// Selection in the live column drives the preview column.
    fn watch_last(self: &Rc<Self>) {
        self.update_preview();
    }

    pub fn update_preview(&self) {
        let Some(pane) = self.pane.upgrade() else { return };
        let selected = pane.selected_items();
        match selected.as_slice() {
            [one] => {
                self.preview.show_git(Some(one), 1, pane.git_info_for(one));
                self.preview_box.set_visible(true);
            }
            [] => self.preview_box.set_visible(false),
            many => {
                self.preview.show(None, many.len());
                self.preview_box.set_visible(true);
            }
        }
        // Keep the preview at the far right.
        self.hbox.reorder_child_after(&self.preview_box, self.columns.borrow().last().map(|c| &c.scroll));
    }

    fn scroll_to_end(&self) {
        let adj = self.root.hadjustment();
        glib::idle_add_local_once(move || adj.set_value(adj.upper()));
    }

    pub fn focus_last(&self) {
        if let Some(c) = self.columns.borrow().last() {
            c.list.grab_focus();
        }
    }

    fn focus_last_or_enter(self: &Rc<Self>) {
        let Some(pane) = self.pane.upgrade() else { return };
        if let Some(item) = pane.selected_items().into_iter().next().filter(|i| i.is_dir()) {
            self.enter(&pane, item.target_file());
            let weak = Rc::downgrade(self);
            glib::timeout_add_local_once(std::time::Duration::from_millis(250), move || {
                if let Some(c) = weak.upgrade() {
                    c.focus_last();
                }
            });
        }
    }
}

// ----- gallery -----------------------------------------------------------------------

pub struct GalleryView {
    pub root: gtk::Box,
    pub strip: gtk::ListView,
    preview: Rc<PreviewPane>,
    pane: Weak<Pane>,
}

impl GalleryView {
    pub fn new(pane: &Rc<Pane>) -> Rc<Self> {
        let preview = PreviewPane::new_large(pane.thumbs.clone());
        preview.root.set_vexpand(true);
        let factory = gtk::SignalListItemFactory::new();
        let weak = Rc::downgrade(pane);
        factory.connect_setup(move |_, obj| {
            let li = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let thumb = crate::pane::make_thumb(56, weak.clone());
            let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
            frame.add_css_class("omafile-strip-item");
            frame.set_size_request(72, 72);
            frame.set_valign(gtk::Align::Center);
            frame.append(&thumb);
            li.set_child(Some(&frame));
            if let Some(p) = weak.upgrade() {
                p.cell_items.borrow_mut().insert(frame.clone().upcast(), li.clone());
            }
        });
        let weak = Rc::downgrade(pane);
        factory.connect_bind(move |_, obj| {
            let li = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let Some(item) = li.item().and_downcast::<FileItem>() else { return };
            let frame = li.child().and_downcast::<gtk::Box>().unwrap();
            let thumb = frame.first_child().and_downcast::<gtk::Overlay>().unwrap();
            if let Some(p) = weak.upgrade() {
                crate::pane::bind_thumb(&p.thumbs, &thumb, &item, 56, p.thumbnails_enabled());
            }
            frame.set_tooltip_text(Some(&item.display_name()));
        });
        let strip = gtk::ListView::new(Some(pane.selection.clone()), Some(factory));
        strip.set_orientation(gtk::Orientation::Horizontal);
        strip.add_css_class("omafile-files");
        strip.add_css_class("omafile-strip");
        let strip_scroll = gtk::ScrolledWindow::builder().child(&strip).vscrollbar_policy(gtk::PolicyType::Never).height_request(96).build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&preview.root);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&strip_scroll);
        let this = Rc::new(GalleryView { root, strip, preview, pane: Rc::downgrade(pane) });
        let weak = Rc::downgrade(&this);
        this.strip.connect_activate(move |view, pos| {
            if let (Some(g), Some(item)) = (weak.upgrade(), view.model().and_then(|m| m.item(pos)).and_downcast::<FileItem>()) {
                if let Some(p) = g.pane.upgrade() {
                    p.open_item(&item);
                }
            }
        });
        this.update_preview();
        this
    }

    pub fn update_preview(&self) {
        let Some(pane) = self.pane.upgrade() else { return };
        let selected = pane.selected_items();
        match selected.as_slice() {
            [one] => self.preview.show_git(Some(one), 1, pane.git_info_for(one)),
            other => self.preview.show(None, other.len()),
        }
    }
}
