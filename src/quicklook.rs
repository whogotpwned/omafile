//! Quick Look: press Space on a file for a large preview of a document (↑ ↓
//! turn its pages),
//! (PDF), the image, or the text; Space or Escape closes it, the arrow keys
//! walk through the folder while it stays open.

use crate::i18n::tr;
use crate::item::FileItem;
use crate::pane::Pane;
use crate::preview::PreviewPane;
use crate::thumbs::ThumbCache;
use adw::prelude::*;
use gtk::{gdk, glib};
use std::cell::RefCell;
use std::rc::{Rc, Weak};

pub struct QuickLook {
    dialog: adw::Dialog,
    preview: Rc<PreviewPane>,
    title: adw::WindowTitle,
    pane: RefCell<Option<Weak<Pane>>>,
}

impl QuickLook {
    pub fn new(thumbs: Rc<ThumbCache>, window: &adw::ApplicationWindow) -> Rc<Self> {
        let preview = PreviewPane::new_large(thumbs);
        let title = adw::WindowTitle::new(&tr("Quick Look"), "");
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&title));
        let hint = gtk::Label::new(Some(tr("Space or Esc closes  ·  ← → previous / next  ·  ↑ ↓ pages").as_str()));
        hint.add_css_class("omafile-dim");
        hint.add_css_class("caption");
        hint.set_margin_bottom(8);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&preview.root));
        view.add_bottom_bar(&hint);
        let dialog = adw::Dialog::builder().title(tr("Quick Look")).child(&view).content_width(960).content_height(720).build();
        dialog.add_css_class("omafile-quicklook");
        let this = Rc::new(QuickLook { dialog, preview, title, pane: RefCell::new(None) });

        let weak = Rc::downgrade(&this);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(q) = weak.upgrade() else { return glib::Propagation::Proceed };
            match key {
                gdk::Key::space | gdk::Key::Escape | gdk::Key::q => {
                    q.dialog.close();
                    glib::Propagation::Stop
                }
                gdk::Key::Right => {
                    q.step(1);
                    glib::Propagation::Stop
                }
                gdk::Key::Left => {
                    q.step(-1);
                    glib::Propagation::Stop
                }
                // Pages of a document first; otherwise the next / previous file.
                gdk::Key::Down | gdk::Key::Page_Down | gdk::Key::j => {
                    if !q.preview.doc_step(1) {
                        q.step(1);
                    }
                    glib::Propagation::Stop
                }
                gdk::Key::Up | gdk::Key::Page_Up | gdk::Key::k => {
                    if !q.preview.doc_step(-1) {
                        q.step(-1);
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        this.dialog.add_controller(keys);
        let weak = Rc::downgrade(&this);
        this.dialog.connect_closed(move |_| {
            if let Some(q) = weak.upgrade() {
                q.preview.stop();
            }
        });
        let _ = window;
        this
    }

    /// Open on the selection and start playing it (context menu "Play").
    pub fn play(self: &Rc<Self>, pane: &Rc<Pane>, window: &adw::ApplicationWindow) {
        if !self.is_open() {
            self.toggle(pane, window);
        }
        self.preview.play();
    }

    pub fn is_open(&self) -> bool {
        self.dialog.is_mapped()
    }

    /// Open for the pane's current selection, or close when already open.
    pub fn toggle(self: &Rc<Self>, pane: &Rc<Pane>, window: &adw::ApplicationWindow) {
        if self.is_open() {
            self.dialog.close();
            return;
        }
        let Some(item) = pane.selected_items().into_iter().next() else { return };
        *self.pane.borrow_mut() = Some(Rc::downgrade(pane));
        self.show_item(&item);
        self.dialog.present(Some(window));
    }

    fn show_item(self: &Rc<Self>, item: &FileItem) {
        self.title.set_title(&item.display_name());
        self.title.set_subtitle(&item.type_description());
        let git = self.pane.borrow().as_ref().and_then(|w| w.upgrade()).and_then(|p| p.git_info_for(item));
        self.preview.show_git(Some(item), 1, git);
        if item.is_audio() {
            // Space on a song: hear it right away, as Finder does.
            self.preview.play();
        }
    }

    /// Move the pane's selection by `delta` and preview the new item.
    fn step(self: &Rc<Self>, delta: i64) {
        let Some(pane) = self.pane.borrow().as_ref().and_then(|w| w.upgrade()) else { return };
        let n = pane.selection.n_items();
        if n == 0 {
            return;
        }
        let current = (0..n).find(|i| pane.selection.is_selected(*i)).unwrap_or(0) as i64;
        let next = (current + delta).clamp(0, n as i64 - 1) as u32;
        pane.selection.select_item(next, true);
        pane.column_view.scroll_to(next, None, gtk::ListScrollFlags::FOCUS, None);
        pane.grid_view.scroll_to(next, gtk::ListScrollFlags::FOCUS, None);
        if let Some(item) = pane.item_at(next) {
            self.show_item(&item);
        }
    }
}
