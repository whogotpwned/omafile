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
    /// Shown, by this view's own account: asked for and not yet closed.
    open: std::cell::Cell<bool>,
    /// Asked to close and not gone yet: a second Space in that moment is swallowed.
    closing: std::cell::Cell<bool>,
    shown_at: std::cell::Cell<Option<std::time::Instant>>,
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
        let this = Rc::new(QuickLook { dialog, preview, title, pane: RefCell::new(None), open: std::cell::Cell::new(false), closing: std::cell::Cell::new(false), shown_at: std::cell::Cell::new(None) });

        // The keys of the preview. They are listened for twice: on the preview itself, and on
        // the window it lies over, because the keyboard focus is not always inside it (it
        // can stay in the file list behind). Space then has to close the preview all the
        // same and must not reach the list, where it would act on some other file.
        let handle: Rc<dyn Fn(gdk::Key) -> glib::Propagation> = {
            let weak = Rc::downgrade(&this);
            Rc::new(move |key| {
                let Some(q) = weak.upgrade() else { return glib::Propagation::Proceed };
                match key {
                    gdk::Key::space | gdk::Key::Escape | gdk::Key::q => {
                        q.request_close();
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
            })
        };
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let h = handle.clone();
        keys.connect_key_pressed(move |_, key, _, _| h(key));
        this.dialog.add_controller(keys);
        let over = gtk::EventControllerKey::new();
        over.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (weak, h) = (Rc::downgrade(&this), handle);
        over.connect_key_pressed(move |_, key, _, state| {
            let plain = !state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK);
            match weak.upgrade() {
                Some(q) if plain && q.is_open() => h(key),
                _ => glib::Propagation::Proceed,
            }
        });
        window.add_controller(over);
        // A video that plays in the video player's window needs no second view of it here.
        let dialog = this.dialog.downgrade();
        this.preview.connect_video_left(move || {
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        });
        let weak = Rc::downgrade(&this);
        this.dialog.connect_closed(move |_| {
            if let Some(q) = weak.upgrade() {
                q.open.set(false);
                q.closing.set(false);
                q.preview.stop();
            }
        });
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
        self.open.get()
    }

    /// Close the preview, once. A dialog asked to close while it still slides in does not
    /// always hear it, and one asked twice can trip the toolkit, so the request waits for
    /// the end of the opening and is not repeated while it is being carried out.
    fn request_close(self: &Rc<Self>) {
        if !self.open.get() || self.closing.replace(true) {
            return;
        }
        let arrived = std::time::Duration::from_millis(320);
        let wait = self.shown_at.get().map(|at| arrived.saturating_sub(at.elapsed())).unwrap_or_default();
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(wait, move || {
            let Some(q) = weak.upgrade() else { return };
            q.dialog.close();
            // Should it not have gone after all, Space works again rather than never.
            let weak = Rc::downgrade(&q);
            glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
                if let Some(q) = weak.upgrade() {
                    if q.closing.replace(false) {
                        q.open.set(q.dialog.is_mapped());
                    }
                }
            });
        });
    }

    /// Open for the pane's current selection, or close when already open.
    pub fn toggle(self: &Rc<Self>, pane: &Rc<Pane>, window: &adw::ApplicationWindow) {
        if self.is_open() {
            self.request_close();
            return;
        }
        let Some(item) = pane.selected_items().into_iter().next() else { return };
        *self.pane.borrow_mut() = Some(Rc::downgrade(pane));
        self.show_item(&item);
        self.open.set(true);
        self.closing.set(false);
        self.shown_at.set(Some(std::time::Instant::now()));
        self.dialog.present(Some(window));
        // The keyboard belongs to the preview while it is shown.
        self.dialog.grab_focus();
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
