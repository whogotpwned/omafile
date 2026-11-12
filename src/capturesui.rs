//! The Captures page: screenshots and screen recordings as a film strip by day.
//!
//! Every card can be dragged straight into another program, copied to the clipboard,
//! drawn on (Satty, the way Omarchy's own screenshot script starts it), turned into a
//! GIF, shared or thrown away. A recording flips through its stills while the pointer
//! is over it. The folders are watched: a new capture slides in by itself.

use crate::captures::{self, Capture, Day, Kind};
use crate::i18n::tr;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// What a button or menu entry does with a capture.
type Run = Box<dyn Fn(&Rc<Window>, &Capture)>;

const CARD_W: i32 = 264;
const CARD_H: i32 = 165;
const FRAMES: u32 = 12;

fn cache_dir() -> PathBuf {
    glib::user_cache_dir().join("omafile")
}

fn day_title(day: Day) -> String {
    match day {
        Day::Today => tr("Today"),
        Day::Yesterday => tr("Yesterday"),
        Day::Weekday(n) => tr(["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][n.min(6) as usize]),
        Day::Earlier(time) => glib::DateTime::from_unix_local(time).ok().and_then(|d| d.format("%-d %B %Y").ok()).map(|s| s.to_string()).unwrap_or_default(),
    }
}

fn time_of_day(time: i64) -> String {
    glib::DateTime::from_unix_local(time).ok().and_then(|d| d.format("%H:%M").ok()).map(|s| s.to_string()).unwrap_or_default()
}

/// The stills of a recording that are flipped through while the pointer is over its card.
struct Flip {
    frames: RefCell<Vec<gdk::Paintable>>,
    poster: RefCell<Option<gdk::Paintable>>,
    timer: RefCell<Option<glib::SourceId>>,
    hovering: Cell<bool>,
    loading: Cell<bool>,
    index: Cell<usize>,
}

pub struct CapturesPage {
    pub root: gtk::Box,
    days: gtk::Box,
    stack: gtk::Stack,
    status: gtk::Label,
    known: RefCell<HashSet<PathBuf>>,
    first: Cell<bool>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
    pending: Cell<bool>,
}

impl CapturesPage {
    fn new(window: &Rc<Window>) -> Rc<Self> {
        let back = gtk::Button::builder()
            .child(&{
                let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                b.append(&gtk::Image::from_icon_name("go-previous-symbolic"));
                b.append(&gtk::Label::new(Some(&tr("Files"))));
                b
            })
            .tooltip_text(tr("Back to the files (Esc)"))
            .build();
        back.add_css_class("flat");
        let title = gtk::Label::builder().label(tr("Captures")).css_classes(["title-4"]).xalign(0.0).margin_start(6).build();
        let status = gtk::Label::builder().css_classes(["omafile-dim"]).xalign(0.0).hexpand(true).margin_start(8).ellipsize(gtk::pango::EllipsizeMode::End).build();
        let shots = gtk::Button::builder().icon_name("folder-pictures-symbolic").tooltip_text(tr("Open the folder with the screenshots")).build();
        let videos = gtk::Button::builder().icon_name("folder-videos-symbolic").tooltip_text(tr("Open the folder with the recordings")).build();
        for b in [&shots, &videos] {
            b.add_css_class("flat");
        }
        let take = gtk::Button::builder().label(tr("Take Screenshot")).tooltip_text(tr("Omarchy's screenshot: select an area (also on the Print key)")).build();
        take.add_css_class("pill");
        take.add_css_class("suggested-action");
        take.set_visible(glib::find_program_in_path("omarchy-capture-screenshot").is_some());
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        top.set_margin_top(8);
        top.set_margin_bottom(6);
        top.set_margin_start(10);
        top.set_margin_end(10);
        top.append(&back);
        top.append(&title);
        top.append(&status);
        top.append(&shots);
        top.append(&videos);
        top.append(&take);

        let days = gtk::Box::new(gtk::Orientation::Vertical, 6);
        days.set_margin_top(8);
        days.set_margin_bottom(28);
        days.set_margin_start(18);
        days.set_margin_end(18);
        days.set_halign(gtk::Align::Center);
        let scroll = gtk::ScrolledWindow::builder().child(&days).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build();
        let nothing = adw::StatusPage::builder()
            .icon_name("camera-photo-symbolic")
            .title(tr("No captures yet"))
            .description(tr("Screenshots and screen recordings made with Omarchy appear here: Print takes a screenshot, Alt + Print records the screen."))
            .build();
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.add_named(&nothing, Some("empty"));
        stack.add_named(&scroll, Some("captures"));

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("omafile-theme-page");
        root.set_focusable(true);
        root.append(&top);
        root.append(&stack);
        let page = Rc::new(CapturesPage { root, days, stack, status, known: RefCell::new(HashSet::new()), first: Cell::new(true), monitors: RefCell::new(Vec::new()), pending: Cell::new(false) });

        let this = Rc::downgrade(window);
        back.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.show_files();
            }
        });
        for (button, which) in [(&shots, 0), (&videos, 1)] {
            let this = Rc::downgrade(window);
            button.connect_clicked(move |_| {
                if let Some(w) = this.upgrade() {
                    let (shots, videos) = captures::dirs();
                    w.navigate(gio::File::for_path(if which == 0 { shots } else { videos }));
                }
            });
        }
        let this = Rc::downgrade(window);
        take.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                // Out of the way first: the screenshot is of what is behind this window.
                if let Err(e) = gio::Subprocess::newv(&[std::ffi::OsStr::new("omarchy-capture-screenshot")], gio::SubprocessFlags::NONE) {
                    w.toast_error(&e.to_string());
                }
            }
        });
        let this = Rc::downgrade(window);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(w) = this.upgrade() {
                    w.show_files();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        page.root.add_controller(keys);

        // Watch both folders: a capture taken while the page is open appears by itself.
        let (shots_dir, videos_dir) = captures::dirs();
        for dir in if shots_dir == videos_dir { vec![shots_dir] } else { vec![shots_dir, videos_dir] } {
            let Ok(monitor) = gio::File::for_path(&dir).monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) else { continue };
            let (weak, this) = (Rc::downgrade(&page), Rc::downgrade(window));
            monitor.connect_changed(move |_, file, _, _| {
                let (Some(p), Some(w)) = (weak.upgrade(), this.upgrade()) else { return };
                let relevant = file.basename().map(|n| n.to_string_lossy().to_lowercase()).is_some_and(|n| n.starts_with("screenshot") || n.starts_with("screenrecording"));
                if !relevant || p.pending.replace(true) {
                    return;
                }
                // A capture is written in steps: wait until it has settled.
                let (weak, this) = (Rc::downgrade(&p), Rc::downgrade(&w));
                glib::timeout_add_local_once(Duration::from_millis(700), move || {
                    if let (Some(p), Some(w)) = (weak.upgrade(), this.upgrade()) {
                        p.pending.set(false);
                        p.reload(&w);
                    }
                });
            });
            page.monitors.borrow_mut().push(monitor);
        }
        page
    }

    pub fn reload(self: &Rc<Self>, window: &Rc<Window>) {
        while let Some(child) = self.days.first_child() {
            self.days.remove(&child);
        }
        let all = captures::list();
        let now = glib::DateTime::now_local().map(|d| d.to_unix()).unwrap_or(0);
        let offset = crate::backup::utc_offset();
        self.stack.set_visible_child_name(if all.is_empty() { "empty" } else { "captures" });
        let shots = all.iter().filter(|c| c.kind == Kind::Screenshot).count();
        let recordings = all.len() - shots;
        let mut parts = Vec::new();
        if shots > 0 {
            parts.push(if shots == 1 { tr("1 screenshot") } else { crate::trf!("{n} screenshots", n = shots) });
        }
        if recordings > 0 {
            parts.push(if recordings == 1 { tr("1 recording") } else { crate::trf!("{n} recordings", n = recordings) });
        }
        self.status.set_text(&parts.join("  ·  "));
        let first = self.first.replace(false);
        let known = self.known.replace(all.iter().map(|c| c.path.clone()).collect());
        let mut index = 0u32;
        // The newest two hundred: a folder with years of screenshots stays quick.
        let shown: Vec<Capture> = all.into_iter().take(200).collect();
        for (day, list) in captures::by_day(&shown, now, offset) {
            let heading = gtk::Label::builder().label(day_title(day)).xalign(0.0).css_classes(["omafile-capture-day"]).build();
            self.days.append(&heading);
            let flow = gtk::FlowBox::new();
            flow.set_selection_mode(gtk::SelectionMode::None);
            flow.set_activate_on_single_click(true);
            flow.set_homogeneous(true);
            flow.set_halign(gtk::Align::Start);
            flow.set_min_children_per_line(3);
            flow.set_max_children_per_line(3);
            flow.set_column_spacing(14);
            flow.set_row_spacing(14);
            flow.set_margin_bottom(10);
            flow.add_css_class("omafile-theme-flow");
            for (i, capture) in list.iter().enumerate() {
                let card = self.card(window, capture);
                flow.insert(&card, -1);
                if let Some(child) = flow.child_at_index(i as i32) {
                    if first {
                        crate::motion::stagger_in(child.upcast_ref::<gtk::Widget>(), index);
                    } else if !known.contains(&capture.path) {
                        // It pops in with a ring in the accent colour, which then fades.
                        child.add_css_class("omafile-capture-new");
                        let weak = child.downgrade();
                        glib::timeout_add_local_once(Duration::from_millis(2600), move || {
                            if let Some(child) = weak.upgrade() {
                                child.remove_css_class("omafile-capture-new");
                            }
                        });
                    }
                }
                index += 1;
            }
            let (this, list) = (Rc::downgrade(window), list.clone());
            flow.connect_child_activated(move |_, child| {
                if let (Some(w), Some(capture)) = (this.upgrade(), list.get(child.index() as usize)) {
                    w.capture_open(&capture.path);
                }
            });
            self.days.append(&flow);
        }
    }

    fn card(self: &Rc<Self>, window: &Rc<Window>, capture: &Capture) -> gtk::Widget {
        let overlay = gtk::Overlay::new();
        overlay.set_size_request(CARD_W, CARD_H);
        overlay.set_overflow(gtk::Overflow::Hidden);
        overlay.add_css_class("omafile-theme-card");
        overlay.add_css_class("omafile-capture-card");
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_can_shrink(true);
        picture.add_css_class("omafile-theme-picture");
        overlay.set_child(Some(&picture));
        let flip = Rc::new(Flip { frames: RefCell::new(Vec::new()), poster: RefCell::new(None), timer: RefCell::new(None), hovering: Cell::new(false), loading: Cell::new(false), index: Cell::new(0) });

        // The still: the screenshot itself, or the recording's poster.
        let (weak, flip2, source, video) = (picture.downgrade(), flip.clone(), capture.poster.clone().unwrap_or_else(|| capture.path.clone()), capture.kind == Kind::Recording && capture.poster.is_none());
        glib::spawn_future_local(async move {
            let decoded = gio::spawn_blocking(move || if video { crate::thumbs::decode_video(&source, 640) } else { crate::thumbs::decode_image(&source, 640) }).await.ok().flatten();
            if let (Some(picture), Some(decoded)) = (weak.upgrade(), decoded) {
                let paintable = decoded.to_paintable();
                picture.set_paintable(Some(&paintable));
                picture.add_css_class("omafile-theme-loaded");
                flip2.poster.replace(Some(paintable));
            }
        });

        // Caption: when, and how long or how big.
        let when = gtk::Label::builder().label(time_of_day(capture.time)).xalign(0.0).hexpand(true).css_classes(["omafile-theme-name"]).build();
        let detail = gtk::Label::builder().label(crate::ops::human_size(capture.size)).css_classes(["caption"]).build();
        let caption = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        caption.set_valign(gtk::Align::End);
        caption.add_css_class("omafile-theme-caption");
        caption.append(&when);
        caption.append(&detail);
        overlay.add_overlay(&caption);

        let play = gtk::Image::from_icon_name("media-playback-start-symbolic");
        if capture.kind == Kind::Recording {
            play.set_halign(gtk::Align::Center);
            play.set_valign(gtk::Align::Center);
            play.set_pixel_size(22);
            play.add_css_class("omafile-capture-play");
            overlay.add_overlay(&play);
            let (weak, path) = (detail.downgrade(), capture.path.clone());
            glib::spawn_future_local(async move {
                let seconds = gio::spawn_blocking(move || captures::duration(&path)).await.ok().flatten();
                if let (Some(detail), Some(seconds)) = (weak.upgrade(), seconds) {
                    detail.set_text(&captures::clock(seconds));
                }
            });
        }

        // What can be done with it, shown while the pointer is over the card.
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        actions.set_halign(gtk::Align::End);
        actions.set_valign(gtk::Align::Start);
        actions.add_css_class("omafile-capture-actions");
        let action = |icon: &str, tip: String, run: Run| {
            let button = gtk::Button::builder().icon_name(icon).tooltip_text(tip).build();
            button.add_css_class("circular");
            button.add_css_class("osd");
            let (this, capture) = (Rc::downgrade(window), capture.clone());
            button.connect_clicked(move |_| {
                if let Some(w) = this.upgrade() {
                    run(&w, &capture);
                }
            });
            actions.append(&button);
        };
        action("edit-copy-symbolic", tr("Copy to the clipboard"), Box::new(|w, c| w.capture_copy(c)));
        match capture.kind {
            Kind::Screenshot => action("document-edit-symbolic", tr("Draw on it"), Box::new(|w, c| w.capture_annotate(c))),
            Kind::Recording => action("image-x-generic-symbolic", tr("Save as GIF"), Box::new(|w, c| w.video_to_gif_for(c.path.clone()))),
        }
        action("send-to-symbolic", tr("Share on the local network"), Box::new(|w, c| w.share_lan_dialog_for(vec![c.path.clone()])));
        action("user-trash-symbolic", tr("Move to the trash"), Box::new(|w, c| w.capture_trash(c)));
        overlay.add_overlay(&actions);

        // Drag it out: into a chat, a mail, a document, another folder.
        let drag = gtk::DragSource::new();
        drag.set_actions(gdk::DragAction::COPY);
        let (path, weak) = (capture.path.clone(), picture.downgrade());
        drag.connect_prepare(move |source, _, _| {
            if let Some(paintable) = weak.upgrade().and_then(|p| p.paintable()) {
                source.set_icon(Some(&paintable), 24, 24);
            }
            let file = gio::File::for_path(&path);
            let list = gdk::FileList::from_array(std::slice::from_ref(&file));
            Some(gdk::ContentProvider::new_union(&[gdk::ContentProvider::for_value(&list.to_value()), gdk::ContentProvider::for_value(&file.uri().to_string().to_value())]))
        });
        overlay.add_controller(drag);

        // A recording plays through its stills while the pointer rests on it.
        if capture.kind == Kind::Recording {
            let motion = gtk::EventControllerMotion::new();
            let (f, capture2, weak, play2) = (flip.clone(), capture.clone(), picture.downgrade(), play.downgrade());
            motion.connect_enter(move |_, _, _| {
                f.hovering.set(true);
                let start = {
                    let (f, weak, play2) = (f.clone(), weak.clone(), play2.clone());
                    move || {
                        if !f.hovering.get() || f.frames.borrow().is_empty() || f.timer.borrow().is_some() {
                            return;
                        }
                        if let Some(play) = play2.upgrade() {
                            play.set_visible(false);
                        }
                        let (f2, weak) = (f.clone(), weak.clone());
                        let id = glib::timeout_add_local(Duration::from_millis(170), move || {
                            let Some(picture) = weak.upgrade() else {
                                f2.timer.borrow_mut().take();
                                return glib::ControlFlow::Break;
                            };
                            let frames = f2.frames.borrow();
                            let i = f2.index.get() % frames.len().max(1);
                            if let Some(frame) = frames.get(i) {
                                picture.set_paintable(Some(frame));
                            }
                            f2.index.set(i + 1);
                            glib::ControlFlow::Continue
                        });
                        f.timer.replace(Some(id));
                    }
                };
                if !f.frames.borrow().is_empty() {
                    start();
                } else if !f.loading.replace(true) {
                    let (f, capture) = (f.clone(), capture2.clone());
                    glib::spawn_future_local(async move {
                        let decoded = gio::spawn_blocking(move || captures::prepare_frames(&cache_dir(), &capture, FRAMES).iter().filter_map(|p| crate::thumbs::decode_image(p, 640)).collect::<Vec<_>>()).await.unwrap_or_default();
                        f.frames.replace(decoded.iter().map(|d| d.to_paintable()).collect());
                        f.loading.set(false);
                        start();
                    });
                }
            });
            let (f, weak, play2) = (flip.clone(), picture.downgrade(), play.downgrade());
            motion.connect_leave(move |_| {
                f.hovering.set(false);
                f.index.set(0);
                if let Some(id) = f.timer.borrow_mut().take() {
                    id.remove();
                }
                if let (Some(picture), Some(poster)) = (weak.upgrade(), f.poster.borrow().clone()) {
                    picture.set_paintable(Some(&poster));
                }
                if let Some(play) = play2.upgrade() {
                    play.set_visible(true);
                }
            });
            overlay.add_controller(motion);
        }

        // Right click: the rest.
        let right = gtk::GestureClick::new();
        right.set_button(3);
        let (this, capture2, anchor) = (Rc::downgrade(window), capture.clone(), overlay.downgrade());
        right.connect_pressed(move |gesture, _, x, y| {
            let (Some(w), Some(anchor)) = (this.upgrade(), anchor.upgrade()) else { return };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            w.capture_menu(&capture2, &anchor, x, y);
        });
        overlay.add_controller(right);
        overlay.upcast()
    }
}

impl Window {
    fn captures_page(self: &Rc<Self>) -> Rc<CapturesPage> {
        // Cloned out first: the borrow must not live through the arm that fills the slot.
        let existing = self.captures_page.borrow().clone();
        match existing {
            Some(p) => p,
            None => {
                let p = CapturesPage::new(self);
                self.content_stack.add_named(&p.root, Some("captures"));
                self.captures_page.replace(Some(p.clone()));
                p
            }
        }
    }

    /// The page with the screenshots and recordings.
    pub fn open_captures(self: &Rc<Self>) {
        let page = self.captures_page();
        if self.content_stack.visible_child_name().as_deref() == Some("nowplaying") {
            self.show_now_playing(false);
        }
        page.reload(self);
        self.content_stack.set_visible_child_name("captures");
        page.root.grab_focus();
    }

    fn captures_reload(self: &Rc<Self>) {
        let page = self.captures_page.borrow().clone();
        if let Some(page) = page {
            page.reload(self);
        }
    }

    fn capture_open(self: &Rc<Self>, path: &std::path::Path) {
        let uri = gio::File::for_path(path).uri();
        if let Err(e) = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE) {
            self.toast_error(&e.to_string());
        }
    }

    /// A screenshot goes onto the clipboard as a picture (to paste into a chat) and as a
    /// file (to paste into a folder); a recording as a file.
    fn capture_copy(self: &Rc<Self>, capture: &Capture) {
        let file = gio::File::for_path(&capture.path);
        let list = gdk::FileList::from_array(std::slice::from_ref(&file));
        let mut offers = Vec::new();
        if capture.kind == Kind::Screenshot {
            if let Ok(texture) = gdk::Texture::from_file(&file) {
                offers.push(gdk::ContentProvider::for_value(&texture.to_value()));
            }
        }
        offers.push(gdk::ContentProvider::for_value(&list.to_value()));
        match self.window.clipboard().set_content(Some(&gdk::ContentProvider::new_union(&offers))) {
            Ok(()) => self.toast(&tr("Copied to the clipboard")),
            Err(e) => self.toast_error(&e.to_string()),
        }
    }

    /// Draw on a screenshot with the editor Omarchy uses (Satty unless set otherwise).
    fn capture_annotate(self: &Rc<Self>, capture: &Capture) {
        let editor = captures::editor();
        if glib::find_program_in_path(&editor).is_none() {
            self.toast_error(&crate::trf!("{program} is not installed", program = editor));
            return;
        }
        let argv = captures::annotate_argv(&editor, &capture.path);
        let args: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
        if let Err(e) = gio::Subprocess::newv(&args, gio::SubprocessFlags::NONE) {
            self.toast_error(&e.to_string());
        }
    }

    fn capture_trash(self: &Rc<Self>, capture: &Capture) {
        let mut gone = Vec::new();
        for path in std::iter::once(&capture.path).chain(capture.poster.iter()) {
            let file = gio::File::for_path(path);
            match file.trash(gio::Cancellable::NONE) {
                Ok(()) => gone.push(file),
                Err(e) => {
                    self.toast_error(&e.to_string());
                    break;
                }
            }
        }
        if !gone.is_empty() {
            let name = capture.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            self.record(Op::Trash(gone), &crate::trf!("Moved {name} to trash", name = name));
        }
        self.captures_reload();
    }

    fn capture_menu(self: &Rc<Self>, capture: &Capture, anchor: &gtk::Overlay, x: f64, y: f64) {
        let popover = gtk::Popover::new();
        popover.set_parent(anchor);
        popover.set_has_arrow(false);
        popover.set_position(gtk::PositionType::Bottom);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.add_css_class("menu");
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let item = |label: String, run: Run| {
            let button = gtk::Button::builder().child(&gtk::Label::builder().label(label).xalign(0.0).build()).build();
            button.add_css_class("flat");
            button.add_css_class("omafile-menu-item");
            let (this, pop, capture) = (Rc::downgrade(self), popover.downgrade(), capture.clone());
            button.connect_clicked(move |_| {
                if let Some(pop) = pop.upgrade() {
                    pop.popdown();
                }
                if let Some(w) = this.upgrade() {
                    run(&w, &capture);
                }
            });
            column.append(&button);
        };
        item(tr("Open"), Box::new(|w, c| w.capture_open(&c.path)));
        item(tr("Copy"), Box::new(|w, c| w.capture_copy(c)));
        match capture.kind {
            Kind::Screenshot => {
                item(tr("Draw on It"), Box::new(|w, c| w.capture_annotate(c)));
                item(tr("Create Theme from Picture…"), Box::new(|w, c| w.theme_dialog(c.path.clone())));
            }
            Kind::Recording => {
                item(tr("Trim…"), Box::new(|w, c| w.video_trim_dialog_for(c.path.clone())));
                item(tr("Save as GIF"), Box::new(|w, c| w.video_to_gif_for(c.path.clone())));
            }
        }
        item(tr("Add to Shelf"), Box::new(|w, c| w.shelf().add(vec![gio::File::for_path(&c.path)])));
        item(tr("Show in Folder"), Box::new(|w, c| {
            w.show_files();
            w.reveal(gio::File::for_path(&c.path));
        }));
        item(tr("Move to Trash"), Box::new(|w, c| w.capture_trash(c)));
        popover.set_child(Some(&column));
        popover.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    }
}
