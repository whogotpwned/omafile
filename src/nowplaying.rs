//! Now Playing: whatever plays on this computer (a browser tab, Spotify,
//! mpv, …) shows up in omafile. A small equalizer button in the header and a
//! mini player at the bottom of the sidebar while something is there, and a
//! full view (F8) with cover art, a live spectrum, seeking and every
//! transport control. Data and commands go through `mpris`.

use crate::i18n::tr;
use crate::mpris::{self, Mpris, Player, Status};
use crate::visualizer::{self, Spectrum, BANDS};
use crate::window::Window;
use adw::prelude::*;
use gtk::gdk::prelude::GdkCairoContextExt;
use gtk::gdk_pixbuf::Pixbuf;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Page {
    root: gtk::Overlay,
    body: gtk::Stack,
    backdrop: gtk::Stack,
    backdrops: [gtk::Picture; 2],
    art: gtk::DrawingArea,
    vis: gtk::DrawingArea,
    title: gtk::Label,
    subtitle: gtk::Label,
    source: gtk::Label,
    pos: gtk::Label,
    len: gtk::Label,
    scale: gtk::Scale,
    play: gtk::Button,
    prev: gtk::Button,
    next: gtk::Button,
    shuffle: gtk::ToggleButton,
    repeat: gtk::Button,
    volume_box: gtk::Box,
    volume: gtk::Scale,
    chooser: gtk::DropDown,
    raise: gtk::Button,
}

/// Rendered layers and the keys they were rendered for (size, scale, colours, cover).
type HaloLayer = Option<((i32, i32, i32, [u8; 3]), gtk::cairo::ImageSurface)>;
type BodyLayer = Option<((i32, i32, i32, Instant, [u8; 3]), gtk::cairo::ImageSurface)>;
type BarLayers = Option<((i32, i32, i32, [u8; 3], [u8; 3]), gtk::cairo::ImageSurface, gtk::cairo::ImageSurface)>;

pub struct NowPlaying {
    window: Weak<Window>,
    mpris: RefCell<Option<Rc<Mpris>>>,
    header_btn: gtk::Button,
    header_eq: gtk::DrawingArea,
    mini: gtk::Revealer,
    mini_cover: gtk::Picture,
    mini_title: gtk::Label,
    mini_artist: gtk::Label,
    mini_play: gtk::Button,
    mini_prev: gtk::Button,
    mini_next: gtk::Button,
    mini_progress: gtk::ProgressBar,
    page: RefCell<Option<Page>>,
    /// Current and previous cover (the previous one fades out).
    cover: RefCell<Option<Pixbuf>>,
    /// The covers as surfaces ready to paint: converting a pixbuf on every frame is what costs.
    cover_surface: RefCell<Option<gtk::cairo::ImageSurface>>,
    old_cover_surface: RefCell<Option<gtk::cairo::ImageSurface>>,
    /// Rendered layers of the big cover, keyed by what they depend on.
    halo_layer: RefCell<HaloLayer>,
    body_layer: RefCell<BodyLayer>,
    bar_layers: RefCell<BarLayers>,
    cover_at: Cell<Instant>,
    cover_url: RefCell<String>,
    /// Dominant colour of the cover, for the spectrum and the glow.
    tint: Cell<(f64, f64, f64)>,
    bars: RefCell<[f32; BANDS]>,
    peaks: RefCell<[f32; BANDS]>,
    bass: Cell<f32>,
    spectrum: RefCell<Option<Spectrum>>,
    started: Instant,
    last_ui: Cell<Instant>,
    last_frame: Cell<Instant>,
    /// Whether the frame callback is installed; it removes itself once everything stands still.
    ticking: Cell<bool>,
    last_sync: Cell<Instant>,
    updating: Cell<bool>,
    backdrop_flip: Cell<usize>,
}

fn icon_button(icon: &str, tooltip: &str, classes: &[&str]) -> gtk::Button {
    let b = gtk::Button::builder().icon_name(icon).tooltip_text(tooltip).valign(gtk::Align::Center).build();
    for c in classes {
        b.add_css_class(c);
    }
    b
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
    cr.arc(x + r, y + r, r, pi, 1.5 * pi);
    cr.close_path();
}

/// Average colour of a picture, nudged towards something vivid enough to draw with.
pub fn dominant_color(pixbuf: &Pixbuf) -> (f64, f64, f64) {
    let Some(tiny) = pixbuf.scale_simple(4, 4, gtk::gdk_pixbuf::InterpType::Bilinear) else { return (0.48, 0.64, 0.97) };
    let (bytes, n, stride) = (tiny.read_pixel_bytes(), tiny.n_channels() as usize, tiny.rowstride() as usize);
    let (mut r, mut g, mut b, mut count) = (0.0, 0.0, 0.0, 0.0);
    for y in 0..4usize {
        for x in 0..4usize {
            let i = y * stride + x * n;
            if i + 2 < bytes.len() {
                r += bytes[i] as f64;
                g += bytes[i + 1] as f64;
                b += bytes[i + 2] as f64;
                count += 1.0;
            }
        }
    }
    if count == 0.0 {
        return (0.48, 0.64, 0.97);
    }
    let (r, g, b) = (r / count / 255.0, g / count / 255.0, b / count / 255.0);
    vivid(r, g, b)
}

/// Push a colour away from grey and into a readable brightness.
pub fn vivid(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let mean = (r + g + b) / 3.0;
    if max - min < 0.04 {
        // Greys carry no hue: leave them, just keep them visible.
        let v = mean.clamp(0.55, 0.9);
        return (v, v, v);
    }
    let boost = 1.6;
    let f = |c: f64| (mean + (c - mean) * boost).clamp(0.0, 1.0);
    let (r, g, b) = (f(r), f(g), f(b));
    let lum = 0.299 * r + 0.587 * g + 0.114 * b;
    let lift = if lum < 0.45 { 0.45 - lum } else { 0.0 };
    ((r + lift).min(1.0), (g + lift).min(1.0), (b + lift).min(1.0))
}

async fn load_cover(url: String) -> Option<Pixbuf> {
    if url.is_empty() {
        return None;
    }
    if let Some(rest) = url.strip_prefix("data:") {
        let b64 = rest.split_once("base64,")?.1;
        let bytes = glib::base64_decode(b64);
        return pixbuf_from_bytes(&bytes);
    }
    let file = gio::File::for_uri(&url);
    if let Some(path) = file.path() {
        return Pixbuf::from_file_at_scale(path, 720, 720, true).ok();
    }
    // Remote art (Spotify, web radio): GVFS first, curl as a fallback.
    if let Ok((bytes, _)) = file.load_bytes_future().await {
        if let Some(p) = pixbuf_from_bytes(&bytes) {
            return Some(p);
        }
    }
    let bytes = gio::spawn_blocking(move || std::process::Command::new("curl").args(["-sL", "--max-time", "6", &url]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok().filter(|o| o.status.success()).map(|o| o.stdout)).await.ok().flatten()?;
    pixbuf_from_bytes(&bytes)
}

/// The centre square of a picture, scaled to `size`.
fn square(pixbuf: &Pixbuf, size: i32) -> Pixbuf {
    let side = pixbuf.width().min(pixbuf.height()).max(1);
    let cropped = pixbuf.new_subpixbuf((pixbuf.width() - side) / 2, (pixbuf.height() - side) / 2, side, side);
    cropped.scale_simple(size, size, gtk::gdk_pixbuf::InterpType::Bilinear).unwrap_or(cropped)
}

/// A pixbuf as a cairo surface, converted once.
fn surface_of(pixbuf: &Pixbuf) -> Option<gtk::cairo::ImageSurface> {
    let surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, pixbuf.width(), pixbuf.height()).ok()?;
    let cr = gtk::cairo::Context::new(&surface).ok()?;
    cr.set_source_pixbuf(pixbuf, 0.0, 0.0);
    cr.paint().ok()?;
    drop(cr);
    Some(surface)
}

fn pixbuf_from_bytes(bytes: &[u8]) -> Option<Pixbuf> {
    let loader = gtk::gdk_pixbuf::PixbufLoader::new();
    loader.write(bytes).ok()?;
    loader.close().ok()?;
    loader.pixbuf()
}

impl NowPlaying {
    /// Build the header button and the mini player, connect to the bus.
    pub fn install(window: &Rc<Window>) {
        if !window.prefs.get_bool("now-playing", true) {
            return;
        }
        // Header: a tiny live equalizer.
        let header_eq = gtk::DrawingArea::new();
        header_eq.set_content_width(20);
        header_eq.set_content_height(16);
        let header_btn = gtk::Button::builder().child(&header_eq).tooltip_text(tr("Now Playing (F8)")).action_name("win.now-playing").visible(false).build();
        header_btn.add_css_class("flat");
        header_btn.add_css_class("omafile-np-header");
        window.np_header_slot.append(&header_btn);

        // Sidebar: the mini player.
        let mini_cover = gtk::Picture::new();
        mini_cover.set_size_request(42, 42);
        mini_cover.set_content_fit(gtk::ContentFit::Cover);
        mini_cover.set_can_shrink(true);
        mini_cover.add_css_class("omafile-np-mini-cover");
        // A note shows through while a track has no cover.
        let cover_frame = gtk::Overlay::new();
        cover_frame.set_overflow(gtk::Overflow::Hidden);
        cover_frame.add_css_class("omafile-np-mini-cover");
        cover_frame.set_size_request(42, 42);
        cover_frame.set_valign(gtk::Align::Center);
        cover_frame.set_child(Some(&gtk::Image::builder().icon_name("audio-x-generic-symbolic").pixel_size(20).opacity(0.6).build()));
        cover_frame.add_overlay(&mini_cover);
        let mini_title = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["omafile-np-mini-title"]).build();
        let mini_artist = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["omafile-dim", "caption"]).build();
        let labels = gtk::Box::new(gtk::Orientation::Vertical, 1);
        labels.set_hexpand(true);
        labels.set_valign(gtk::Align::Center);
        labels.append(&mini_title);
        labels.append(&mini_artist);
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        top.append(&cover_frame);
        top.append(&labels);
        let mini_prev = icon_button("media-skip-backward-symbolic", &tr("Previous"), &["flat", "circular"]);
        let mini_play = icon_button("media-playback-start-symbolic", &tr("Play / Pause"), &["circular", "omafile-np-mini-play"]);
        let mini_next = icon_button("media-skip-forward-symbolic", &tr("Next"), &["flat", "circular"]);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        buttons.set_halign(gtk::Align::Center);
        buttons.append(&mini_prev);
        buttons.append(&mini_play);
        buttons.append(&mini_next);
        let mini_progress = gtk::ProgressBar::new();
        mini_progress.add_css_class("omafile-np-progress");
        let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        card.add_css_class("omafile-np-mini");
        card.append(&top);
        card.append(&mini_progress);
        card.append(&buttons);
        card.set_tooltip_text(Some(&tr("Open Now Playing (F8)")));
        let mini = gtk::Revealer::builder().transition_type(gtk::RevealerTransitionType::SlideUp).transition_duration(crate::motion::PANEL).child(&card).reveal_child(false).build();
        window.np_sidebar_slot.append(&mini);

        let now = Instant::now();
        let this = Rc::new(NowPlaying {
            window: Rc::downgrade(window),
            mpris: RefCell::new(None),
            header_btn,
            header_eq,
            mini,
            mini_cover,
            mini_title,
            mini_artist,
            mini_play,
            mini_prev,
            mini_next,
            mini_progress,
            page: RefCell::new(None),
            cover: RefCell::new(None),
            cover_surface: RefCell::new(None),
            old_cover_surface: RefCell::new(None),
            halo_layer: RefCell::new(None),
            body_layer: RefCell::new(None),
            bar_layers: RefCell::new(None),
            cover_at: Cell::new(now),
            cover_url: RefCell::new(String::new()),
            tint: Cell::new((0.48, 0.64, 0.97)),
            bars: RefCell::new([0.0; BANDS]),
            peaks: RefCell::new([0.0; BANDS]),
            bass: Cell::new(0.0),
            spectrum: RefCell::new(None),
            started: now,
            last_ui: Cell::new(now),
            last_frame: Cell::new(now),
            ticking: Cell::new(false),
            last_sync: Cell::new(now),
            updating: Cell::new(false),
            backdrop_flip: Cell::new(0),
        });
        window.now_playing.replace(Some(this.clone()));

        // A click on the card (not on its buttons) opens the full view.
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(window);
        click.connect_released(move |g, _, _, _| {
            if let Some(w) = weak.upgrade() {
                g.set_state(gtk::EventSequenceState::Claimed);
                w.show_now_playing(true);
            }
        });
        top.add_controller(click);

        let weak = Rc::downgrade(&this);
        this.mini_play.connect_clicked(move |_| { if let Some(n) = weak.upgrade() { n.with_mpris(|m| m.play_pause()); } });
        let weak = Rc::downgrade(&this);
        this.mini_prev.connect_clicked(move |_| { if let Some(n) = weak.upgrade() { n.with_mpris(|m| m.previous()); } });
        let weak = Rc::downgrade(&this);
        this.mini_next.connect_clicked(move |_| { if let Some(n) = weak.upgrade() { n.with_mpris(|m| m.next()); } });

        let weak = Rc::downgrade(&this);
        this.header_eq.set_draw_func(move |area, cr, w, h| {
            if let Some(n) = weak.upgrade() {
                n.draw_header_eq(area, cr, w as f64, h as f64);
            }
        });
        this.ensure_tick();

        let weak = Rc::downgrade(&this);
        glib::spawn_future_local(async move {
            let weak2 = weak.clone();
            let mpris = Mpris::start(Box::new(move || {
                if let Some(n) = weak2.upgrade() {
                    n.update();
                }
            }))
            .await;
            if let (Some(n), Some(m)) = (weak.upgrade(), mpris) {
                n.mpris.replace(Some(m));
                n.update();
            }
        });
    }

    fn with_mpris(&self, f: impl FnOnce(&Rc<Mpris>)) {
        if let Some(m) = self.mpris.borrow().as_ref() {
            f(m);
        }
    }

    pub fn active(&self) -> Option<Player> {
        self.mpris.borrow().as_ref().and_then(|m| m.active())
    }

    fn players(&self) -> Vec<Player> {
        self.mpris.borrow().as_ref().map(|m| m.players()).unwrap_or_default()
    }

    fn page_visible(&self) -> bool {
        self.window.upgrade().map(|w| w.content_stack.visible_child_name().as_deref() == Some("nowplaying")).unwrap_or(false)
    }

    // ----- state → widgets -------------------------------------------------------------

    /// Something changed on the bus: refresh every widget.
    pub fn update(self: &Rc<Self>) {
        self.ensure_tick();
        let players = self.players();
        let active = self.active();
        let any = !players.is_empty();
        self.header_btn.set_visible(any);
        self.mini.set_reveal_child(any);
        if let Some(w) = self.window.upgrade() {
            // The slot keeps no height while empty.
            w.np_sidebar_slot.set_visible(any || self.mini.is_child_revealed());
        }
        let playing = active.as_ref().map(|p| p.status == Status::Playing).unwrap_or(false);
        let play_icon = if playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" };
        self.mini_play.set_icon_name(play_icon);
        if let Some(p) = &active {
            self.mini_title.set_text(&p.display_title());
            self.mini_artist.set_text(&if p.artist.is_empty() { p.identity.clone() } else { p.artist.clone() });
            self.mini_prev.set_sensitive(p.can_previous);
            self.mini_next.set_sensitive(p.can_next);
            self.header_btn.set_tooltip_text(Some(&format!("{}  ·  {}", p.display_title(), tr("Now Playing (F8)"))));
            let url = p.art_url.clone();
            if *self.cover_url.borrow() != url {
                self.cover_url.replace(url.clone());
                let this = Rc::downgrade(self);
                glib::spawn_future_local(async move {
                    let pixbuf = load_cover(url.clone()).await;
                    let Some(n) = this.upgrade() else { return };
                    if *n.cover_url.borrow() == url {
                        n.set_cover(pixbuf);
                    }
                });
            }
        }
        if !any && (self.cover.borrow().is_some() || !self.cover_url.borrow().is_empty()) {
            // The last player left: its cover goes with it.
            self.cover_url.replace(String::new());
            self.set_cover(None);
        }
        let Some(page) = self.page.borrow().clone() else { return };
        page.body.set_visible_child_name(if any { "player" } else { "empty" });
        page.play.set_icon_name(play_icon);
        let Some(p) = active else { return };
        self.updating.set(true);
        if page.title.text() != p.display_title() {
            page.title.set_text(&p.display_title());
            crate::motion::property(&page.title, "opacity", 0.0, 1.0, 380, None);
        }
        let subtitle = p.display_subtitle();
        page.subtitle.set_text(&subtitle);
        page.subtitle.set_visible(!subtitle.is_empty());
        page.source.set_text(&p.identity);
        page.len.set_text(&if p.length_us > 0 { mpris::fmt_time(p.length_us) } else { tr("Live") });
        page.scale.set_sensitive(p.can_seek && p.length_us > 0);
        page.prev.set_sensitive(p.can_previous);
        page.next.set_sensitive(p.can_next);
        page.shuffle.set_visible(p.shuffle.is_some());
        page.shuffle.set_active(p.shuffle.unwrap_or(false));
        page.repeat.set_visible(p.loop_status.is_some());
        match p.loop_status.as_deref() {
            Some("Track") => {
                page.repeat.set_icon_name("media-playlist-repeat-song-symbolic");
                page.repeat.add_css_class("omafile-np-on");
            }
            Some("Playlist") => {
                page.repeat.set_icon_name("media-playlist-repeat-symbolic");
                page.repeat.add_css_class("omafile-np-on");
            }
            _ => {
                page.repeat.set_icon_name("media-playlist-repeat-symbolic");
                page.repeat.remove_css_class("omafile-np-on");
            }
        }
        page.volume_box.set_visible(p.volume.is_some());
        if let Some(v) = p.volume {
            if (page.volume.value() - v).abs() > 0.01 {
                page.volume.set_value(v);
            }
        }
        // Source chooser: only when there is a choice.
        let names: Vec<String> = players.iter().map(|x| if x.title.is_empty() { x.identity.clone() } else { format!("{}  ·  {}", x.identity, x.title) }).collect();
        // Only swap the model when the list changed: a swap would close the open popover.
        let current: Vec<String> = page.chooser.model().and_downcast::<gtk::StringList>().map(|m| (0..m.n_items()).filter_map(|i| m.string(i)).map(|s| s.to_string()).collect()).unwrap_or_default();
        if current != names {
            let model = gtk::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
            page.chooser.set_model(Some(&model));
        }
        page.chooser.set_selected(players.iter().position(|x| x.bus == p.bus).unwrap_or(0) as u32);
        page.chooser.set_visible(players.len() > 1);
        self.updating.set(false);
        self.refresh_progress(&p, &page);
    }

    fn refresh_progress(&self, p: &Player, page: &Page) {
        page.pos.set_text(&mpris::fmt_time(p.position_now()));
        // Do not fight the user's finger: the scale is only moved when it is not being dragged.
        if !page.scale.has_css_class("dragging") {
            page.scale.set_value(p.fraction());
        }
    }

    fn set_cover(self: &Rc<Self>, pixbuf: Option<Pixbuf>) {
        let old = self.cover_surface.replace(pixbuf.as_ref().and_then(|p| surface_of(&square(p, 480))));
        self.old_cover_surface.replace(old);
        self.cover_at.set(Instant::now());
        self.ensure_tick();
        match &pixbuf {
            Some(p) => {
                self.tint.set(dominant_color(p));
                self.mini_cover.set_paintable(Some(&gdk::Texture::for_pixbuf(&square(p, 96))));
            }
            None => {
                self.mini_cover.set_paintable(None::<&gdk::Paintable>);
                if let Some(w) = self.window.upgrade() {
                    let a = w.theme.palette().accent;
                    self.tint.set((a.0 as f64 / 255.0, a.1 as f64 / 255.0, a.2 as f64 / 255.0));
                }
            }
        }
        self.cover.replace(pixbuf);
        self.update_backdrop();
    }

    /// The cover, blurred by scaling it down and up again, behind everything.
    fn update_backdrop(&self) {
        let Some(page) = self.page.borrow().clone() else { return };
        let next = 1 - self.backdrop_flip.get();
        let picture = &page.backdrops[next];
        match self.cover.borrow().as_ref().and_then(|p| p.scale_simple(5, 5, gtk::gdk_pixbuf::InterpType::Bilinear)).and_then(|p| p.scale_simple(400, 400, gtk::gdk_pixbuf::InterpType::Bilinear)) {
            Some(blurred) => picture.set_paintable(Some(&gdk::Texture::for_pixbuf(&blurred))),
            None => picture.set_paintable(None::<&gdk::Paintable>),
        }
        page.backdrop.set_visible_child(picture);
        self.backdrop_flip.set(next);
    }

    // ----- animation -------------------------------------------------------------------

    /// One frame clock drives every animation. It runs only while the header button is on
    /// screen and something moves; `update` and friends start it again.
    fn ensure_tick(self: &Rc<Self>) {
        if self.ticking.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        self.header_eq.add_tick_callback(move |_, _| {
            let Some(n) = weak.upgrade() else { return glib::ControlFlow::Break };
            if n.tick() {
                glib::ControlFlow::Continue
            } else {
                n.ticking.set(false);
                glib::ControlFlow::Break
            }
        });
    }

    /// Advance the animation by a frame; false once nothing is left to animate.
    fn tick(self: &Rc<Self>) -> bool {
        let page_visible = self.page_visible();
        // 60 frames a second in the full view (also on faster screens), 20 for the small equalizer.
        let since = self.last_frame.get().elapsed();
        if since < Duration::from_millis(if page_visible { 15 } else { 48 }) {
            return true;
        }
        self.last_frame.set(Instant::now());
        let (attack, decay, fall) = if page_visible { (0.5, 0.16, 0.009) } else { (0.875, 0.41, 0.027) };
        let active = self.active();
        let playing = active.as_ref().map(|p| p.status == Status::Playing).unwrap_or(false);
        let t = self.started.elapsed().as_secs_f64();
        let live = if page_visible { self.spectrum.borrow().as_ref().and_then(|s| s.latest()) } else { None };
        let target = match live {
            Some(bands) if playing => bands,
            _ => visualizer::procedural(t, playing),
        };
        let mut moving = false;
        {
            let mut bars = self.bars.borrow_mut();
            let mut peaks = self.peaks.borrow_mut();
            for i in 0..BANDS {
                let k = if target[i] > bars[i] { attack } else { decay };
                let new = bars[i] + (target[i] - bars[i]) * k;
                if (new - bars[i]).abs() > 0.002 {
                    moving = true;
                }
                bars[i] = new;
                if peaks[i] > new + 0.002 {
                    moving = true;
                }
                peaks[i] = if new >= peaks[i] { new } else { (peaks[i] - fall).max(new) };
            }
            let bass: f32 = bars[..6].iter().sum::<f32>() / 6.0;
            self.bass.set(self.bass.get() + (bass - self.bass.get()) * 0.35);
        }
        let fading = self.cover_at.get().elapsed() < Duration::from_millis(600);
        if moving || playing {
            self.header_eq.queue_draw();
        }
        if page_visible {
            if let Some(page) = self.page.borrow().as_ref() {
                if moving || playing || fading {
                    page.vis.queue_draw();
                    page.art.queue_draw();
                }
            }
        }
        // Clock-driven widgets a few times a second.
        if self.last_ui.get().elapsed() > Duration::from_millis(250) {
            self.last_ui.set(Instant::now());
            if let Some(p) = &active {
                self.mini_progress.set_fraction(p.fraction());
                if page_visible {
                    if let Some(page) = self.page.borrow().clone() {
                        self.refresh_progress(p, &page);
                    }
                }
            }
        }
        // Players do not announce their position while playing: ask now and then.
        if playing && self.last_sync.get().elapsed() > Duration::from_secs(if page_visible { 4 } else { 15 }) {
            self.last_sync.set(Instant::now());
            self.with_mpris(|m| m.sync_position());
        }
        playing || moving || fading
    }

    fn accent(&self) -> (f64, f64, f64) {
        self.window.upgrade().map(|w| w.theme.palette().accent).map(|a| (a.0 as f64 / 255.0, a.1 as f64 / 255.0, a.2 as f64 / 255.0)).unwrap_or((0.48, 0.64, 0.97))
    }

    fn draw_header_eq(&self, area: &gtk::DrawingArea, cr: &gtk::cairo::Context, w: f64, h: f64) {
        let bars = self.bars.borrow();
        let fg = area.color();
        let (ar, ag, ab) = self.accent();
        let playing = self.active().map(|p| p.status == Status::Playing).unwrap_or(false);
        let n = 4;
        let bw = 3.0;
        let gap = (w - n as f64 * bw) / (n as f64 - 1.0);
        for i in 0..n {
            // Spread the four bars over the low and mid bands.
            let v = bars[2 + i * 7].clamp(0.12, 1.0) as f64;
            let bh = (h * v).max(3.0);
            rounded(cr, i as f64 * (bw + gap), h - bh, bw, bh, 1.5);
            if playing {
                cr.set_source_rgb(ar, ag, ab);
            } else {
                cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.6);
            }
            let _ = cr.fill();
        }
    }

    /// The spectrum. The bars and their reflection are rendered once at full height; a frame
    /// only copies pixel-aligned slices of them, which costs a fraction of drawing 44 rounded,
    /// shaded bars sixty times a second.
    fn draw_vis(&self, area: &gtk::DrawingArea, cr: &gtk::cairo::Context, w: f64, h: f64) {
        let bars = self.bars.borrow();
        let peaks = self.peaks.borrow();
        let scale = area.scale_factor().max(1);
        let n = BANDS as f64;
        let gap = 4.0;
        let bw = ((w - gap * (n - 1.0)) / n).floor().max(2.0);
        let x0 = ((w - (bw * n + gap * (n - 1.0))) / 2.0).floor().max(0.0);
        let base = (h * 0.72).floor(); // bars above, their reflection below
        let room = (h - base - 3.0).floor();
        if base < 8.0 {
            return;
        }
        let x_of = |i: usize| x0 + i as f64 * (bw + gap);
        let height = |v: f32| ((base - 4.0) * v as f64 + 3.0).round().clamp(3.0, base);
        let byte = |c: (f64, f64, f64)| [(c.0 * 255.0) as u8, (c.1 * 255.0) as u8, (c.2 * 255.0) as u8];
        let (accent, tint) = (self.accent(), self.tint.get());
        let key = (w as i32, h as i32, scale, byte(accent), byte(tint));
        let mut layers = self.bar_layers.borrow_mut();
        if layers.as_ref().map(|(k, _, _)| *k != key).unwrap_or(true) {
            let surface = |height: f64| {
                let s = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, w as i32 * scale, (height.max(1.0)) as i32 * scale).ok()?;
                s.set_device_scale(scale as f64, scale as f64);
                Some(s)
            };
            // Accent on the left (bass) to the cover's colour on the right (treble).
            let colour = |i: usize| {
                let f = i as f64 / (n - 1.0);
                (accent.0 + (tint.0 - accent.0) * f, accent.1 + (tint.1 - accent.1) * f, accent.2 + (tint.2 - accent.2) * f)
            };
            let full = surface(base).and_then(|s| {
                let c = gtk::cairo::Context::new(&s).ok()?;
                for i in 0..BANDS {
                    let (r, g, b) = colour(i);
                    // A bright tip that melts into the bar's colour.
                    let shade = gtk::cairo::LinearGradient::new(0.0, 0.0, 0.0, base);
                    shade.add_color_stop_rgba(0.0, (r + 0.32).min(1.0), (g + 0.32).min(1.0), (b + 0.32).min(1.0), 1.0);
                    shade.add_color_stop_rgba(0.55, r, g, b, 0.92);
                    shade.add_color_stop_rgba(1.0, r, g, b, 0.85);
                    rounded(&c, x_of(i), 0.0, bw, base + bw, bw / 2.0);
                    let _ = c.set_source(&shade);
                    let _ = c.fill();
                }
                drop(c);
                Some(s)
            });
            let mirror = surface(room).and_then(|s| {
                let c = gtk::cairo::Context::new(&s).ok()?;
                for i in 0..BANDS {
                    let (r, g, b) = colour(i);
                    let fade = gtk::cairo::LinearGradient::new(0.0, 0.0, 0.0, room.max(1.0));
                    fade.add_color_stop_rgba(0.0, r, g, b, 0.30);
                    fade.add_color_stop_rgba(1.0, r, g, b, 0.0);
                    c.rectangle(x_of(i), 0.0, bw, room.max(1.0));
                    let _ = c.set_source(&fade);
                    let _ = c.fill();
                }
                drop(c);
                Some(s)
            });
            *layers = match (full, mirror) {
                (Some(full), Some(mirror)) => Some((key, full, mirror)),
                _ => None,
            };
        }
        let Some((_, full, mirror)) = layers.as_ref() else { return };
        for (i, v) in bars.iter().enumerate() {
            let bh = height(*v);
            // The layer is moved so that its rounded tip sits at this bar's top.
            let _ = cr.set_source_surface(full, 0.0, base - bh);
            cr.rectangle(x_of(i), base - bh, bw, bh);
            let _ = cr.fill();
        }
        if room >= 2.0 {
            for (i, v) in bars.iter().enumerate() {
                cr.rectangle(x_of(i), base + 3.0, bw, room.min((height(*v) * 0.45).round()));
            }
            let _ = cr.set_source_surface(mirror, 0.0, base + 3.0);
            let _ = cr.fill();
        }
        // Falling peak caps.
        let fg = area.color();
        let mut caps = false;
        for i in 0..BANDS {
            let (bh, ph) = (height(bars[i]), height(peaks[i]));
            if ph > bh + 4.0 {
                cr.rectangle(x_of(i), base - ph - 3.0, bw, 2.0);
                caps = true;
            }
        }
        if caps {
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.55);
            let _ = cr.fill();
        }
    }

    /// The cover and its halo. Both are rendered once into layers and only composited per
    /// frame: the halo scaled and faded with the bass, the cover as it is.
    fn draw_art(&self, area: &gtk::DrawingArea, cr: &gtk::cairo::Context, w: f64, h: f64) {
        let bass = self.bass.get() as f64;
        let scale = area.scale_factor().max(1);
        let byte = |c: (f64, f64, f64)| [(c.0 * 255.0) as u8, (c.1 * 255.0) as u8, (c.2 * 255.0) as u8];
        let layer = |halo: bool| -> Option<gtk::cairo::ImageSurface> {
            let surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, w as i32 * scale, h as i32 * scale).ok()?;
            surface.set_device_scale(scale as f64, scale as f64);
            let c = gtk::cairo::Context::new(&surface).ok()?;
            self.render_art(&c, w, h, halo, !halo);
            Some(surface)
        };
        let halo_key = (w as i32, h as i32, scale, byte(self.tint.get()));
        let mut halo = self.halo_layer.borrow_mut();
        if halo.as_ref().map(|(k, _)| *k != halo_key).unwrap_or(true) {
            *halo = layer(true).map(|s| (halo_key, s));
        }
        if let Some((_, surface)) = halo.as_ref() {
            let grow = 1.0 - 0.09 * (1.0 - bass);
            let _ = cr.save();
            cr.translate(w / 2.0, h / 2.0);
            cr.scale(grow, grow);
            cr.translate(-w / 2.0, -h / 2.0);
            let _ = cr.set_source_surface(surface, 0.0, 0.0);
            let _ = cr.paint_with_alpha(0.5 + 0.5 * bass);
            let _ = cr.restore();
        }
        // While two covers cross-fade the body is drawn directly; afterwards from its layer.
        if self.cover_at.get().elapsed() < Duration::from_millis(520) {
            self.render_art(cr, w, h, false, true);
            return;
        }
        let body_key = (w as i32, h as i32, scale, self.cover_at.get(), byte(self.accent()));
        let mut body = self.body_layer.borrow_mut();
        if body.as_ref().map(|(k, _)| *k != body_key).unwrap_or(true) {
            *body = layer(false).map(|s| (body_key, s));
        }
        match body.as_ref() {
            Some((_, surface)) => {
                let _ = cr.set_source_surface(surface, 0.0, 0.0);
                let _ = cr.paint();
            }
            None => self.render_art(cr, w, h, false, true),
        }
    }

    /// Paint the halo (at full strength) and / or the cover with its shadow.
    fn render_art(&self, cr: &gtk::cairo::Context, w: f64, h: f64, halo: bool, body: bool) {
        let size = (w.min(h) - 64.0).clamp(40.0, 440.0);
        let (x, y) = ((w - size) / 2.0, (h - size) / 2.0);
        let radius = 22.0;
        let (tr_, tg, tb) = self.tint.get();
        // Rings around the cover (even-odd fill), so the cover's own area is not blended 18 times.
        cr.set_fill_rule(gtk::cairo::FillRule::EvenOdd);
        if halo {
            for i in 1..=13 {
                let grow = i as f64 * 2.4;
                rounded(cr, x - grow, y - grow, size + 2.0 * grow, size + 2.0 * grow, radius + grow);
                rounded(cr, x + 1.0, y + 1.0, size - 2.0, size - 2.0, radius);
                cr.set_source_rgba(tr_, tg, tb, 0.048);
                let _ = cr.fill();
            }
        }
        if !body {
            cr.set_fill_rule(gtk::cairo::FillRule::Winding);
            return;
        }
        for i in 1..=5 {
            let grow = i as f64 * 2.0;
            rounded(cr, x - grow, y - grow + 8.0, size + 2.0 * grow, size + 2.0 * grow, radius + grow);
            rounded(cr, x + 1.0, y + 1.0, size - 2.0, size - 2.0, radius);
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.07);
            let _ = cr.fill();
        }
        cr.set_fill_rule(gtk::cairo::FillRule::Winding);
        let _ = cr.save();
        rounded(cr, x, y, size, size, radius);
        cr.clip();
        let paint = |surface: &gtk::cairo::ImageSurface, alpha: f64| {
            let scale = size / surface.width().max(1) as f64;
            let _ = cr.save();
            cr.translate(x, y);
            cr.scale(scale, scale);
            let _ = cr.set_source_surface(surface, 0.0, 0.0);
            let _ = cr.paint_with_alpha(alpha);
            let _ = cr.restore();
        };
        let placeholder = |alpha: f64| {
            let (ar, ag, ab) = self.accent();
            let gradient = gtk::cairo::LinearGradient::new(x, y, x + size, y + size);
            gradient.add_color_stop_rgba(0.0, ar, ag, ab, alpha);
            gradient.add_color_stop_rgba(1.0, ar * 0.35, ag * 0.35, ab * 0.55, alpha);
            cr.rectangle(x, y, size, size);
            let _ = cr.set_source(&gradient);
            let _ = cr.fill();
            // A note drawn by hand: two heads, two stems, a beam.
            let u = size / 10.0;
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.85 * alpha);
            for hx in [3.6, 6.6] {
                let _ = cr.save();
                cr.translate(x + hx * u, y + (if hx < 5.0 { 6.9 } else { 6.3 }) * u);
                cr.rotate(-0.35);
                cr.scale(0.95 * u, 0.7 * u);
                cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * std::f64::consts::PI);
                let _ = cr.restore();
                let _ = cr.fill();
            }
            cr.rectangle(x + 4.2 * u, y + 3.3 * u, 0.32 * u, 3.5 * u);
            cr.rectangle(x + 7.2 * u, y + 2.7 * u, 0.32 * u, 3.5 * u);
            let _ = cr.fill();
            cr.move_to(x + 4.2 * u, y + 3.3 * u);
            cr.line_to(x + 7.52 * u, y + 2.7 * u);
            cr.line_to(x + 7.52 * u, y + 3.6 * u);
            cr.line_to(x + 4.2 * u, y + 4.2 * u);
            cr.close_path();
            let _ = cr.fill();
        };
        // Crossfade from the previous cover.
        let t = (self.cover_at.get().elapsed().as_secs_f64() / 0.5).min(1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        if eased < 1.0 {
            match self.old_cover_surface.borrow().as_ref() {
                Some(old) => paint(old, 1.0),
                None => placeholder(1.0),
            }
        }
        match self.cover_surface.borrow().as_ref() {
            Some(new) => paint(new, eased),
            None => placeholder(eased),
        }
        let _ = cr.restore();
        // A hairline so dark covers do not melt into the backdrop.
        rounded(cr, x + 0.5, y + 0.5, size - 1.0, size - 1.0, radius);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.10);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
    }

    // ----- the full view ---------------------------------------------------------------

    fn build_page(self: &Rc<Self>, window: &Rc<Window>) -> Page {
        let backdrops = [gtk::Picture::new(), gtk::Picture::new()];
        let backdrop = gtk::Stack::new();
        backdrop.set_transition_type(gtk::StackTransitionType::Crossfade);
        backdrop.set_transition_duration(700);
        backdrop.add_css_class("omafile-np-backdrop");
        for (i, b) in backdrops.iter().enumerate() {
            b.set_content_fit(gtk::ContentFit::Cover);
            b.set_can_shrink(true);
            backdrop.add_named(b, Some(&format!("b{i}")));
        }

        let art = gtk::DrawingArea::new();
        // Takes whatever height the window has left, so the controls always fit.
        art.set_content_width(200);
        art.set_content_height(150);
        // A square slot: only the cover's own area is redrawn on every frame.
        let art_slot = gtk::AspectFrame::new(0.5, 0.5, 1.0, false);
        art_slot.set_vexpand(true);
        art_slot.set_child(Some(&art));
        let title = gtk::Label::builder().ellipsize(gtk::pango::EllipsizeMode::End).max_width_chars(42).justify(gtk::Justification::Center).css_classes(["omafile-np-title"]).build();
        let subtitle = gtk::Label::builder().ellipsize(gtk::pango::EllipsizeMode::End).max_width_chars(60).css_classes(["omafile-np-subtitle"]).build();
        let source = gtk::Label::builder().css_classes(["omafile-dim", "caption-heading", "omafile-np-source"]).build();

        let vis = gtk::DrawingArea::new();
        vis.set_content_height(96);
        vis.set_hexpand(true);

        let pos = gtk::Label::builder().label("0:00").css_classes(["omafile-dim", "caption", "numeric"]).width_chars(6).xalign(1.0).build();
        let len = gtk::Label::builder().label("0:00").css_classes(["omafile-dim", "caption", "numeric"]).width_chars(6).xalign(0.0).build();
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.001);
        scale.set_hexpand(true);
        scale.set_draw_value(false);
        scale.add_css_class("omafile-np-seek");
        let progress = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        progress.append(&pos);
        progress.append(&scale);
        progress.append(&len);

        let shuffle = gtk::ToggleButton::builder().icon_name("media-playlist-shuffle-symbolic").tooltip_text(tr("Shuffle")).valign(gtk::Align::Center).build();
        shuffle.add_css_class("flat");
        shuffle.add_css_class("circular");
        let prev = icon_button("media-skip-backward-symbolic", &tr("Previous (P)"), &["flat", "circular", "omafile-np-skip"]);
        let play = icon_button("media-playback-start-symbolic", &tr("Play / Pause (Space)"), &["circular", "omafile-np-play"]);
        let next = icon_button("media-skip-forward-symbolic", &tr("Next (N)"), &["flat", "circular", "omafile-np-skip"]);
        let repeat = icon_button("media-playlist-repeat-symbolic", &tr("Repeat"), &["flat", "circular"]);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 18);
        controls.set_halign(gtk::Align::Center);
        for b in [shuffle.upcast_ref::<gtk::Widget>(), prev.upcast_ref(), play.upcast_ref(), next.upcast_ref(), repeat.upcast_ref()] {
            controls.append(b);
        }

        let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
        volume.set_size_request(180, -1);
        volume.set_draw_value(false);
        let volume_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        volume_box.set_halign(gtk::Align::Center);
        volume_box.append(&gtk::Image::from_icon_name("audio-volume-low-symbolic"));
        volume_box.append(&volume);
        volume_box.append(&gtk::Image::from_icon_name("audio-volume-high-symbolic"));

        let column = gtk::Box::new(gtk::Orientation::Vertical, 10);
        column.set_margin_start(28);
        column.set_margin_end(28);
        column.set_margin_top(34);
        column.set_margin_bottom(18);
        column.append(&art_slot);
        column.append(&source);
        column.append(&title);
        column.append(&subtitle);
        column.append(&vis);
        column.append(&progress);
        column.append(&controls);
        column.append(&volume_box);
        let clamp = adw::Clamp::builder().maximum_size(760).child(&column).build();

        let empty = adw::StatusPage::builder().icon_name("audio-x-generic-symbolic").title(tr("Nothing is playing")).description(tr("Start something in your browser or music player: YouTube, Spotify, mpv, anything that speaks MPRIS shows up here with its controls.")).build();
        let body = gtk::Stack::new();
        body.set_transition_type(gtk::StackTransitionType::Crossfade);
        body.add_named(&empty, Some("empty"));
        body.add_named(&clamp, Some("player"));

        // Top row: back to the files, which player, bring it to the front.
        let back = gtk::Button::builder().child(&{
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            b.append(&gtk::Image::from_icon_name("go-down-symbolic"));
            b.append(&gtk::Label::new(Some(&tr("Files"))));
            b
        }).tooltip_text(tr("Back to the files (Esc or F8)")).action_name("win.now-playing").build();
        back.add_css_class("flat");
        let chooser = gtk::DropDown::from_strings(&[]);
        chooser.set_tooltip_text(Some(&tr("Which player to control")));
        let raise = gtk::Button::builder().icon_name("go-jump-symbolic").tooltip_text(tr("Bring the player to the front")).build();
        raise.add_css_class("flat");
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        top.set_margin_top(10);
        top.set_margin_start(12);
        top.set_margin_end(12);
        top.append(&back);
        top.append(&gtk::Box::builder().hexpand(true).build());
        top.append(&chooser);
        top.append(&raise);

        top.set_valign(gtk::Align::Start);

        let root = gtk::Overlay::new();
        root.add_css_class("omafile-np-page");
        root.set_child(Some(&backdrop));
        root.add_overlay(&body);
        root.add_overlay(&top);
        root.set_focusable(true);

        let page = Page { root, body, backdrop, backdrops, art, vis, title, subtitle, source, pos, len, scale, play, prev, next, shuffle, repeat, volume_box, volume, chooser, raise };

        let weak = Rc::downgrade(self);
        page.art.set_draw_func(move |a, cr, w, h| { if let Some(n) = weak.upgrade() { n.draw_art(a, cr, w as f64, h as f64); } });
        let weak = Rc::downgrade(self);
        page.vis.set_draw_func(move |a, cr, w, h| { if let Some(n) = weak.upgrade() { n.draw_vis(a, cr, w as f64, h as f64); } });
        let weak = Rc::downgrade(self);
        page.play.connect_clicked(move |_| { if let Some(n) = weak.upgrade() { n.with_mpris(|m| m.play_pause()); } });
        let weak = Rc::downgrade(self);
        page.prev.connect_clicked(move |_| { if let Some(n) = weak.upgrade() { n.with_mpris(|m| m.previous()); } });
        let weak = Rc::downgrade(self);
        page.next.connect_clicked(move |_| { if let Some(n) = weak.upgrade() { n.with_mpris(|m| m.next()); } });
        let weak = Rc::downgrade(self);
        page.shuffle.connect_clicked(move |_| { if let Some(n) = weak.upgrade().filter(|n| !n.updating.get()) { n.with_mpris(|m| m.toggle_shuffle()); } });
        let weak = Rc::downgrade(self);
        page.repeat.connect_clicked(move |_| { if let Some(n) = weak.upgrade() { n.with_mpris(|m| m.cycle_loop()); } });
        let weak = Rc::downgrade(self);
        page.raise.connect_clicked(move |_| { if let Some(n) = weak.upgrade() { n.with_mpris(|m| m.raise()); } });
        // change-value only fires for the user's own moves.
        let weak = Rc::downgrade(self);
        page.scale.connect_change_value(move |_, _, value| {
            if let Some(n) = weak.upgrade() {
                if let Some(p) = n.active().filter(|p| p.length_us > 0) {
                    n.with_mpris(|m| m.seek_to((value.clamp(0.0, 1.0) * p.length_us as f64) as i64));
                }
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(self);
        page.volume.connect_change_value(move |_, _, value| {
            if let Some(n) = weak.upgrade() {
                n.with_mpris(|m| m.set_volume(value));
            }
            glib::Propagation::Proceed
        });
        let weak = Rc::downgrade(self);
        page.chooser.connect_selected_notify(move |d| {
            let Some(n) = weak.upgrade().filter(|n| !n.updating.get()) else { return };
            if let Some(p) = n.players().get(d.selected() as usize) {
                let bus = p.bus.clone();
                n.with_mpris(|m| m.choose(Some(bus)));
            }
        });
        // Keys while the view is on show.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        let wweak = Rc::downgrade(window);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(n) = weak.upgrade() else { return glib::Propagation::Proceed };
            if state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::space | gdk::Key::k => n.with_mpris(|m| m.play_pause()),
                gdk::Key::Right | gdk::Key::l => n.with_mpris(|m| m.seek_by(5_000_000)),
                gdk::Key::Left | gdk::Key::h => n.with_mpris(|m| m.seek_by(-5_000_000)),
                gdk::Key::n => n.with_mpris(|m| m.next()),
                gdk::Key::p => n.with_mpris(|m| m.previous()),
                gdk::Key::Up => {
                    if let Some(v) = n.active().and_then(|p| p.volume) {
                        n.with_mpris(|m| m.set_volume(v + 0.05));
                    }
                }
                gdk::Key::Down => {
                    if let Some(v) = n.active().and_then(|p| p.volume) {
                        n.with_mpris(|m| m.set_volume(v - 0.05));
                    }
                }
                gdk::Key::Escape => {
                    if let Some(w) = wweak.upgrade() {
                        w.show_now_playing(false);
                    }
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        page.root.add_controller(keys);
        page
    }

    fn ensure_page(self: &Rc<Self>, window: &Rc<Window>) -> Page {
        if let Some(p) = self.page.borrow().clone() {
            return p;
        }
        let page = self.build_page(window);
        window.content_stack.add_named(&page.root, Some("nowplaying"));
        self.page.replace(Some(page.clone()));
        self.update_backdrop();
        page
    }

    fn page_shown(self: &Rc<Self>) {
        self.ensure_tick();
        self.update();
        self.with_mpris(|m| m.sync_position());
        let wants_live = self.window.upgrade().map(|w| w.prefs.get_bool("now-playing-spectrum", true)).unwrap_or(true);
        if wants_live && self.spectrum.borrow().is_none() {
            self.spectrum.replace(Spectrum::start());
        }
    }

    fn page_hidden(&self) {
        // Stop recording the monitor as soon as nobody looks at the spectrum.
        self.spectrum.replace(None);
    }
}

impl Window {
    /// Switch between the files and the Now Playing view.
    pub fn show_now_playing(self: &Rc<Self>, show: bool) {
        let Some(np) = self.now_playing.borrow().clone() else {
            if show {
                self.toast(&tr("Now Playing is off (Preferences › Now Playing)"));
            }
            return;
        };
        if show {
            let page = np.ensure_page(self);
            self.content_stack.set_visible_child_name("nowplaying");
            np.page_shown();
            page.root.grab_focus();
        } else if self.content_stack.visible_child_name().as_deref() == Some("nowplaying") {
            self.content_stack.set_visible_child_name("files");
            np.page_hidden();
            self.refocus();
        }
    }

    /// Like `show_now_playing`, but silent when the feature is off.
    pub fn show_now_playing_quiet(self: &Rc<Self>, show: bool) {
        if self.now_playing.borrow().is_some() {
            self.show_now_playing(show);
        }
    }

    pub fn toggle_now_playing(self: &Rc<Self>) {
        let showing = self.content_stack.visible_child_name().as_deref() == Some("nowplaying");
        self.show_now_playing(!showing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_become_vivid_but_stay_in_range() {
        let (r, g, b) = vivid(0.2, 0.1, 0.1);
        assert!(r > g && r > b, "hue survives");
        assert!((0.0..=1.0).contains(&r) && (0.0..=1.0).contains(&g) && (0.0..=1.0).contains(&b));
        assert!(0.299 * r + 0.587 * g + 0.114 * b >= 0.3, "dark covers are lifted");
        let (r, g, b) = vivid(0.5, 0.5, 0.5);
        assert!((r - g).abs() < 1e-9 && (g - b).abs() < 1e-9, "greys stay grey");
        assert_eq!(vivid(0.05, 0.05, 0.05), (0.55, 0.55, 0.55));
    }
}
