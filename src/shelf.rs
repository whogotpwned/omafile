//! The shelf: a small floating window that collects files from anywhere
//! (drag onto it, or Ctrl+Shift+S on the selection) and hands them on in one
//! go: drag them out again, or copy / move them into the folder on show.
//! Its contents survive a restart (`~/.local/state/omafile/shelf`, one URI per line).

use crate::i18n::tr;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

pub fn state_path() -> PathBuf {
    glib::user_state_dir().join("omafile/shelf")
}

pub fn parse(text: &str) -> Vec<gio::File> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(gio::File::for_uri).collect()
}

pub fn serialize(files: &[gio::File]) -> String {
    files.iter().map(|f| f.uri().to_string() + "\n").collect()
}

pub struct Shelf {
    pub window: gtk::Window,
    list: gtk::ListBox,
    count: gtk::Label,
    stack: gtk::Stack,
    files: RefCell<Vec<gio::File>>,
    owner: Weak<Window>,
}

impl Shelf {
    pub fn new(owner: &Rc<Window>) -> Rc<Self> {
        let files = std::fs::read_to_string(state_path()).map(|t| parse(&t)).unwrap_or_default();
        let window = gtk::Window::builder().title(tr("Shelf")).default_width(280).default_height(380).hide_on_close(true).build();
        window.add_css_class("omafile-shelf");
        let header = adw::HeaderBar::new();
        let count = gtk::Label::builder().css_classes(["omafile-dim", "caption"]).build();
        header.set_title_widget(Some(&adw::WindowTitle::new(&tr("Shelf"), "")));
        let clear = gtk::Button::builder().icon_name("edit-clear-all-symbolic").tooltip_text(tr("Clear the shelf")).build();
        clear.add_css_class("flat");
        header.pack_end(&clear);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Multiple);
        list.add_css_class("boxed-list");
        list.set_margin_start(10);
        list.set_margin_end(10);
        list.set_margin_top(10);
        let scroll = gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
        let empty = adw::StatusPage::builder().icon_name("folder-open-symbolic").title(tr("Drop files here")).description(tr("Collect files from any folder, then drag them out together or copy / move them into the folder on show. Ctrl+Shift+S adds the selection.")).build();
        empty.add_css_class("compact");
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&scroll, Some("list"));

        let copy = gtk::Button::with_label(&tr("Copy Here"));
        let mv = gtk::Button::with_label(&tr("Move Here"));
        mv.add_css_class("suggested-action");
        copy.set_tooltip_text(Some(&tr("Copy everything on the shelf into the folder shown in omafile")));
        mv.set_tooltip_text(Some(&tr("Move everything on the shelf into the folder shown in omafile")));
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_homogeneous(true);
        buttons.set_margin_start(10);
        buttons.set_margin_end(10);
        buttons.set_margin_bottom(10);
        buttons.set_margin_top(8);
        buttons.append(&copy);
        buttons.append(&mv);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&stack);
        content.append(&count);
        content.append(&buttons);
        count.set_margin_start(12);
        count.set_xalign(0.0);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&content));
        window.set_child(Some(&view));

        let this = Rc::new(Shelf { window, list, count, stack, files: RefCell::new(files), owner: Rc::downgrade(owner) });

        // Dropping anywhere on the window adds to the shelf.
        let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        let weak = Rc::downgrade(&this);
        target.connect_drop(move |_, value, _, _| {
            let (Some(s), Ok(list)) = (weak.upgrade(), value.get::<gdk::FileList>()) else { return false };
            s.add(list.files());
            true
        });
        this.window.add_controller(target);

        // Dragging from the list hands out the selected rows, or everything.
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::COPY | gdk::DragAction::MOVE);
        let weak = Rc::downgrade(&this);
        source.connect_prepare(move |_, _, _| {
            let s = weak.upgrade()?;
            let files = s.dragged();
            if files.is_empty() {
                return None;
            }
            Some(gdk::ContentProvider::for_value(&gdk::FileList::from_array(&files).to_value()))
        });
        let weak = Rc::downgrade(&this);
        source.connect_drag_end(move |_, _, deleted| {
            // A move elsewhere takes the files off the shelf.
            if let Some(s) = weak.upgrade().filter(|_| deleted) {
                let gone = s.dragged();
                s.files.borrow_mut().retain(|f| !gone.iter().any(|g| g.equal(f)));
                s.rebuild();
            }
        });
        this.list.add_controller(source);

        let weak = Rc::downgrade(&this);
        clear.connect_clicked(move |_| { if let Some(s) = weak.upgrade() { s.files.borrow_mut().clear(); s.rebuild(); } });
        let weak = Rc::downgrade(&this);
        copy.connect_clicked(move |_| { if let Some(s) = weak.upgrade() { s.hand_over(crate::transfer::Mode::Copy); } });
        let weak = Rc::downgrade(&this);
        mv.connect_clicked(move |_| { if let Some(s) = weak.upgrade() { s.hand_over(crate::transfer::Mode::Move); } });
        let weak = Rc::downgrade(&this);
        this.list.connect_row_activated(move |_, row| {
            let (Some(s), Some(w)) = (weak.upgrade(), weak.upgrade().and_then(|s| s.owner.upgrade())) else { return };
            let file = s.files.borrow().get(row.index() as usize).cloned();
            if let Some(file) = file {
                w.reveal(file);
            }
        });
        this.rebuild();
        this
    }

    fn dragged(&self) -> Vec<gio::File> {
        let files = self.files.borrow();
        let selected: Vec<gio::File> = self.list.selected_rows().iter().filter_map(|r| files.get(r.index() as usize).cloned()).collect();
        if selected.is_empty() { files.clone() } else { selected }
    }

    pub fn add(self: &Rc<Self>, new: Vec<gio::File>) {
        let mut files = self.files.borrow_mut();
        let mut added = 0;
        for f in new {
            if !files.iter().any(|e| e.equal(&f)) {
                files.push(f);
                added += 1;
            }
        }
        drop(files);
        self.rebuild();
        if let Some(w) = self.owner.upgrade() {
            if added > 0 {
                w.toast(&trf!("{n} on the shelf", n = self.files.borrow().len()));
            }
        }
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn toggle(&self) {
        if self.window.is_visible() { self.window.set_visible(false) } else { self.window.present() }
    }

    /// Copy or move everything (or the selected rows) into the folder omafile shows.
    fn hand_over(self: &Rc<Self>, mode: crate::transfer::Mode) {
        let Some(w) = self.owner.upgrade() else { return };
        let Some(pane) = w.active_pane() else { return };
        let files = self.dragged();
        if files.is_empty() {
            return;
        }
        let dest = pane.cwd();
        let moving = matches!(mode, crate::transfer::Mode::Move);
        w.transfer(files.clone(), dest, mode, pane, false);
        if moving {
            self.files.borrow_mut().retain(|f| !files.iter().any(|g| g.equal(f)));
            self.rebuild();
        }
        w.window.present();
    }

    fn rebuild(self: &Rc<Self>) {
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        let files = self.files.borrow().clone();
        for file in &files {
            let name = file.basename().map(|b| b.to_string_lossy().to_string()).unwrap_or_else(|| file.uri().to_string());
            let folder = crate::window::location_text(&file.parent().unwrap_or_else(|| file.clone()));
            let row = adw::ActionRow::builder().title(glib::markup_escape_text(&name)).subtitle(glib::markup_escape_text(&folder)).activatable(true).build();
            row.add_prefix(&gtk::Image::from_gicon(&file_icon(file)));
            let remove = gtk::Button::builder().icon_name("window-close-symbolic").valign(gtk::Align::Center).tooltip_text(tr("Take off the shelf")).build();
            remove.add_css_class("flat");
            remove.add_css_class("circular");
            let weak = Rc::downgrade(self);
            let gone = file.clone();
            remove.connect_clicked(move |_| {
                if let Some(s) = weak.upgrade() {
                    s.files.borrow_mut().retain(|f| !f.equal(&gone));
                    s.rebuild();
                }
            });
            row.add_suffix(&remove);
            self.list.append(&row);
        }
        let n = files.len();
        self.stack.set_visible_child_name(if n == 0 { "empty" } else { "list" });
        self.count.set_text(&if n == 0 { String::new() } else if n == 1 { tr("1 item") } else { trf!("{n} items", n = n) });
        self.save();
        if let Some(w) = self.owner.upgrade() {
            w.shelf_changed(n);
        }
    }

    fn save(&self) {
        let path = state_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, serialize(&self.files.borrow()));
    }
}

fn file_icon(file: &gio::File) -> gio::Icon {
    file.query_info("standard::icon", gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)
        .ok()
        .and_then(|i| i.icon())
        .unwrap_or_else(|| gio::ThemedIcon::new("text-x-generic").upcast())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_state_file() {
        let files = vec![gio::File::for_path("/tmp/a b.txt"), gio::File::for_uri("sftp://host/x")];
        let text = serialize(&files);
        let back = parse(&text);
        assert_eq!(back.len(), 2);
        assert!(back[0].equal(&files[0]));
        assert!(back[1].equal(&files[1]));
        assert!(parse("# comment\n\n").is_empty());
    }
}
