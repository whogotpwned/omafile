//! The Radio page: the stations as cards by genre, and what is on the air.
//!
//! A click plays a station; what it plays shows in the bar at the top, and, because the
//! radio is a media player like any other on the desktop, in Now Playing, in the sidebar's
//! mini player and on the media keys. Stations are added with their address (a stream, a
//! playlist or a podcast's feed), changed and removed here.

use crate::i18n::tr;
use crate::radio::{self, Kind, Station};
use crate::radioplayer::{self, Now, State};
use crate::scheme::from_hsl;
use crate::window::Window;
use adw::prelude::*;
use gtk::gdk_pixbuf::Pixbuf;
use gtk::{cairo, gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

/// What a menu entry does with a station.
type Run = Box<dyn Fn(&Rc<Window>, &Station)>;

const CARD_W: i32 = 196;
const CARD_H: i32 = 118;

/// A genre as it is shown: the built-in ones in the user's language.
pub fn genre_title(genre: &str) -> String {
    match genre {
        radio::ELECTRONIC => tr("Electronic"),
        radio::NEWS => tr("Tech and hacking news"),
        radio::OWN => tr("My stations"),
        other => other.to_string(),
    }
}

/// And back: what was typed as a genre, as it is stored.
fn genre_id(text: &str) -> String {
    let text = text.trim();
    [radio::ELECTRONIC, radio::NEWS, radio::OWN].iter().find(|g| genre_title(g) == text).map(|g| g.to_string()).unwrap_or_else(|| text.to_string())
}

/// A station's picture: its own colour (from its name), with waves for a stream and a
/// voice's waveform for a podcast. `with_name` writes the name on it, for the cover that
/// other programs show.
pub fn draw_art(cr: &cairo::Context, w: f64, h: f64, station: &Station, with_name: bool) {
    let hue = radio::hue(&station.name);
    let rgb = |c: crate::theme::Rgb| (c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0);
    let (a, b) = (rgb(from_hsl(hue, 0.58, 0.44)), rgb(from_hsl(hue + 52.0, 0.66, 0.2)));
    let gradient = cairo::LinearGradient::new(0.0, 0.0, w, h);
    gradient.add_color_stop_rgb(0.0, a.0, a.1, a.2);
    gradient.add_color_stop_rgb(1.0, b.0, b.1, b.2);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.set_source(&gradient);
    let _ = cr.fill();
    let unit = w.min(h);
    match station.kind {
        Kind::Live => {
            let (cx, cy) = (w * 0.8, h * if with_name { 0.26 } else { 0.34 });
            for i in 1..=5 {
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.22 - i as f64 * 0.035);
                cr.set_line_width(unit * 0.022);
                cr.arc(cx, cy, unit * 0.13 * i as f64, 0.0, std::f64::consts::TAU);
                let _ = cr.stroke();
            }
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.85);
            cr.arc(cx, cy, unit * 0.04, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
        Kind::Podcast => {
            let mut seed = (hue as u32).wrapping_mul(7919).wrapping_add(station.name.len() as u32);
            let bars = 24;
            let step = w / bars as f64;
            let middle = h * if with_name { 0.3 } else { 0.4 };
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.22);
            for i in 0..bars {
                seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                let swell = (i as f64 / (bars - 1) as f64 * std::f64::consts::PI).sin();
                let level = (0.08 + ((seed >> 16) % 100) as f64 / 100.0 * 0.42) * (0.35 + 0.65 * swell);
                let tall = h * level;
                cr.rectangle(i as f64 * step + step * 0.28, middle - tall / 2.0, step * 0.44, tall);
            }
            let _ = cr.fill();
        }
    }
    if with_name {
        cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(h * 0.105);
        // The name in lines that fit, standing on the lower edge.
        let mut lines: Vec<String> = Vec::new();
        for word in station.name.split_whitespace() {
            let candidate = match lines.last() {
                Some(last) => format!("{last} {word}"),
                None => word.to_string(),
            };
            let fits = cr.text_extents(&candidate).map(|e| e.width() <= w * 0.82).unwrap_or(true);
            match lines.last_mut() {
                Some(last) if fits => *last = candidate,
                _ => lines.push(word.to_string()),
            }
        }
        lines.truncate(4);
        let line_height = h * 0.13;
        let mut y = h * 0.9 - line_height * (lines.len() as f64 - 1.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.96);
        for line in &lines {
            cr.move_to(w * 0.09, y);
            let _ = cr.show_text(line);
            y += line_height;
        }
    }
}

fn art_png(station: &Station, size: usize) -> Option<Vec<u8>> {
    let mut surface = cairo::ImageSurface::create(cairo::Format::Rgb24, size as i32, size as i32).ok()?;
    {
        let cr = cairo::Context::new(&surface).ok()?;
        draw_art(&cr, size as f64, size as f64, station, true);
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?;
    let mut rgb = Vec::with_capacity(size * size * 3);
    for y in 0..size {
        for x in 0..size {
            let o = y * stride + x * 4;
            // Cairo keeps pixels as native-endian 0xRRGGBB words: blue first on these machines.
            rgb.extend_from_slice(&[data[o + 2], data[o + 1], data[o]]);
        }
    }
    drop(data);
    let pixbuf = Pixbuf::from_bytes(&glib::Bytes::from_owned(rgb), gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, size as i32, size as i32, (size * 3) as i32);
    pixbuf.save_to_bufferv("png", &[]).ok()
}

/// The station's cover as a file, for the programs that show what plays: made once.
pub fn art_file(station: &Station) -> Option<PathBuf> {
    let dir = glib::user_cache_dir().join("omafile/radio");
    let path = dir.join(format!("{}.png", crate::feeds::hash(&format!("{}:{}", station.kind.id(), station.name))));
    if !path.exists() {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&path, art_png(station, 512)?).ok()?;
    }
    Some(path)
}

/// The first few thousand bytes an address sends: enough to tell a feed from sound, and
/// quick even for a stream that never ends.
async fn probe(url: &str) -> Vec<u8> {
    let script = "curl -sL --max-time 12 --proto =http,https -A 'omafile radio' -- \"$1\" 2>/dev/null | head -c 6000";
    let args = ["sh", "-c", script, "sh", url];
    let argv: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
    let Ok(process) = gio::Subprocess::newv(&argv, gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE) else { return Vec::new() };
    match process.communicate_future(None).await {
        Ok((Some(out), _)) => out.to_vec(),
        _ => Vec::new(),
    }
}

/// The name a feed gives itself, from its beginning.
pub fn feed_title(head: &str) -> Option<String> {
    let start = head.find("<title")?;
    let rest = &head[start..];
    let rest = &rest[rest.find('>')? + 1..];
    let inner = &rest[..rest.find("</title")?];
    let inner = inner.trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>").trim();
    let title = crate::feeds::decode_entities(inner);
    (!title.is_empty()).then_some(title)
}

struct Card {
    url: String,
    frame: gtk::Overlay,
    badge: gtk::Image,
}

pub struct RadioPage {
    pub root: gtk::Box,
    groups: gtk::Box,
    stack: gtk::Stack,
    status: gtk::Label,
    banner: adw::Banner,
    restore: gtk::Button,
    air: gtk::Revealer,
    air_art: gtk::DrawingArea,
    air_station: Rc<RefCell<Option<Station>>>,
    air_title: gtk::Label,
    air_sub: gtk::Label,
    air_play: gtk::Button,
    air_spinner: gtk::Spinner,
    volume: gtk::Scale,
    setting_volume: Cell<bool>,
    cards: RefCell<Vec<Card>>,
    first: Cell<bool>,
}

impl RadioPage {
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
        let title = gtk::Label::builder().label(tr("Radio")).css_classes(["title-4"]).xalign(0.0).margin_start(6).build();
        let status = gtk::Label::builder().css_classes(["omafile-dim"]).xalign(0.0).hexpand(true).margin_start(8).ellipsize(gtk::pango::EllipsizeMode::End).build();
        let restore = gtk::Button::builder().icon_name("edit-undo-symbolic").tooltip_text(tr("Bring back the built-in stations that were removed")).build();
        restore.add_css_class("flat");
        let add = gtk::Button::builder().label(tr("Add Station")).tooltip_text(tr("Add a stream or a podcast by its address")).build();
        add.add_css_class("pill");
        add.add_css_class("suggested-action");
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        top.set_margin_top(8);
        top.set_margin_bottom(6);
        top.set_margin_start(10);
        top.set_margin_end(10);
        top.append(&back);
        top.append(&title);
        top.append(&status);
        top.append(&restore);
        top.append(&add);

        let banner = adw::Banner::new(&tr("mpv plays the radio, and it is not installed. It is part of Omarchy; elsewhere, install the package mpv."));

        // What is on the air.
        let air_station: Rc<RefCell<Option<Station>>> = Rc::new(RefCell::new(None));
        let air_art = gtk::DrawingArea::new();
        air_art.set_content_width(52);
        air_art.set_content_height(52);
        air_art.set_overflow(gtk::Overflow::Hidden);
        air_art.add_css_class("omafile-radio-art");
        let shown = air_station.clone();
        air_art.set_draw_func(move |_, cr, w, h| {
            if let Some(station) = shown.borrow().as_ref() {
                draw_art(cr, w as f64, h as f64, station, false);
            }
        });
        let air_title = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["omafile-radio-title"]).build();
        let air_sub = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["omafile-dim", "caption"]).build();
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 2);
        texts.set_valign(gtk::Align::Center);
        texts.set_hexpand(true);
        texts.append(&air_title);
        texts.append(&air_sub);
        let air_spinner = gtk::Spinner::new();
        let button = |icon: &str, tip: String| {
            let b = gtk::Button::builder().icon_name(icon).tooltip_text(tip).valign(gtk::Align::Center).build();
            b.add_css_class("flat");
            b.add_css_class("circular");
            b
        };
        let previous = button("media-skip-backward-symbolic", tr("Previous station, or the newer episode"));
        let air_play = button("media-playback-pause-symbolic", tr("Play or pause"));
        air_play.remove_css_class("flat");
        air_play.add_css_class("suggested-action");
        let next = button("media-skip-forward-symbolic", tr("Next station, or the older episode"));
        let stop = button("media-playback-stop-symbolic", tr("Stop the radio"));
        let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.05);
        volume.set_width_request(110);
        volume.set_valign(gtk::Align::Center);
        volume.set_tooltip_text(Some(&tr("Volume of the radio")));
        let player_view = button("view-fullscreen-symbolic", tr("Show in Now Playing"));
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        bar.add_css_class("omafile-radio-air");
        bar.append(&air_art);
        bar.append(&texts);
        bar.append(&air_spinner);
        bar.append(&previous);
        bar.append(&air_play);
        bar.append(&next);
        bar.append(&stop);
        bar.append(&gtk::Image::from_icon_name("audio-volume-high-symbolic"));
        bar.append(&volume);
        bar.append(&player_view);
        let air = gtk::Revealer::builder().child(&bar).transition_type(gtk::RevealerTransitionType::SlideDown).transition_duration(220).build();

        let groups = gtk::Box::new(gtk::Orientation::Vertical, 6);
        groups.set_margin_top(4);
        groups.set_margin_bottom(28);
        groups.set_margin_start(18);
        groups.set_margin_end(18);
        groups.set_halign(gtk::Align::Center);
        let scroll = gtk::ScrolledWindow::builder().child(&groups).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build();
        let nothing = adw::StatusPage::builder().icon_name("audio-x-generic-symbolic").title(tr("No stations")).description(tr("Add a station by its address, or bring the built-in ones back.")).build();
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.add_named(&nothing, Some("empty"));
        stack.add_named(&scroll, Some("stations"));

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("omafile-theme-page");
        root.set_focusable(true);
        root.append(&top);
        root.append(&banner);
        root.append(&air);
        root.append(&stack);
        let page = Rc::new(RadioPage {
            root,
            groups,
            stack,
            status,
            banner,
            restore: restore.clone(),
            air,
            air_art,
            air_station,
            air_title,
            air_sub,
            air_play: air_play.clone(),
            air_spinner,
            volume: volume.clone(),
            setting_volume: Cell::new(false),
            cards: RefCell::new(Vec::new()),
            first: Cell::new(true),
        });

        let this = Rc::downgrade(window);
        back.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.show_files();
            }
        });
        let this = Rc::downgrade(window);
        add.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.radio_station_dialog(None);
            }
        });
        let this = Rc::downgrade(window);
        restore.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.radio_restore_defaults();
            }
        });
        previous.connect_clicked(|_| radioplayer::player().skip(-1));
        next.connect_clicked(|_| radioplayer::player().skip(1));
        air_play.connect_clicked(|_| radioplayer::player().toggle());
        stop.connect_clicked(|_| radioplayer::player().stop());
        let weak = Rc::downgrade(&page);
        volume.connect_value_changed(move |scale| {
            if weak.upgrade().is_some_and(|p| !p.setting_volume.get()) {
                radioplayer::player().set_volume(scale.value());
            }
        });
        let this = Rc::downgrade(window);
        player_view.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.show_now_playing(true);
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

        let weak = Rc::downgrade(&page);
        radioplayer::player().connect_changed(move |now| {
            if let Some(page) = weak.upgrade() {
                page.update(now);
            }
        });
        page
    }

    /// Show what the radio does: the bar, and which card is on.
    fn update(&self, now: &Now) {
        let on_air = now.station.is_some() && now.state != State::Stopped;
        self.air.set_reveal_child(on_air);
        if on_air {
            self.air_station.replace(now.station.clone());
            self.air_art.queue_draw();
            let (title, by) = now.shown();
            self.air_title.set_text(&title);
            let failed = now.state == State::Failed;
            self.air_sub.set_text(&match now.state {
                State::Failed => now.error.clone(),
                State::Loading => tr("Connecting…"),
                _ if by.is_empty() => match now.station.as_ref().map(|s| s.kind) {
                    Some(Kind::Podcast) => tr("Podcast"),
                    _ => tr("Live"),
                },
                _ => by,
            });
            if failed {
                self.air_sub.add_css_class("error");
            } else {
                self.air_sub.remove_css_class("error");
            }
            self.air_play.set_icon_name(if now.state == State::Playing { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" });
            self.air_spinner.set_visible(now.state == State::Loading);
            self.air_spinner.set_spinning(now.state == State::Loading);
        }
        if (self.volume.value() - now.volume).abs() > 0.004 {
            self.setting_volume.set(true);
            self.volume.set_value(now.volume);
            self.setting_volume.set(false);
        }
        for card in self.cards.borrow().iter() {
            let on = now.is_on(&card.url);
            if on {
                card.frame.add_css_class("omafile-theme-current");
            } else {
                card.frame.remove_css_class("omafile-theme-current");
            }
            card.badge.set_visible(on);
            card.badge.set_icon_name(Some(if now.state == State::Paused { "media-playback-pause-symbolic" } else { "media-playback-start-symbolic" }));
        }
    }

    pub fn reload(self: &Rc<Self>, window: &Rc<Window>) {
        while let Some(child) = self.groups.first_child() {
            self.groups.remove(&child);
        }
        self.cards.borrow_mut().clear();
        let stations = radio::load();
        self.banner.set_revealed(!radioplayer::mpv_installed());
        self.stack.set_visible_child_name(if stations.is_empty() { "empty" } else { "stations" });
        self.status.set_text(&if stations.len() == 1 { tr("1 station") } else { crate::trf!("{n} stations", n = stations.len()) });
        self.restore.set_visible(radio::defaults().iter().any(|d| !stations.iter().any(|s| s.url == d.url)));
        let first = self.first.replace(false);
        let mut index = 0u32;
        for genre in radio::genres(&stations) {
            let list: Vec<Station> = stations.iter().filter(|s| s.genre == genre).cloned().collect();
            self.groups.append(&gtk::Label::builder().label(genre_title(&genre)).xalign(0.0).css_classes(["omafile-capture-day"]).build());
            let flow = gtk::FlowBox::new();
            flow.set_selection_mode(gtk::SelectionMode::None);
            flow.set_activate_on_single_click(true);
            flow.set_homogeneous(true);
            flow.set_halign(gtk::Align::Start);
            flow.set_min_children_per_line(2);
            flow.set_max_children_per_line(4);
            flow.set_column_spacing(14);
            flow.set_row_spacing(14);
            flow.set_margin_bottom(10);
            flow.add_css_class("omafile-theme-flow");
            for (i, station) in list.iter().enumerate() {
                flow.insert(&self.card(window, station), -1);
                if let (true, Some(child)) = (first, flow.child_at_index(i as i32)) {
                    crate::motion::stagger_in(child.upcast_ref::<gtk::Widget>(), index);
                }
                index += 1;
            }
            let this = Rc::downgrade(window);
            flow.connect_child_activated(move |_, child| {
                if let (Some(w), Some(station)) = (this.upgrade(), list.get(child.index() as usize)) {
                    w.radio_play(station);
                }
            });
            self.groups.append(&flow);
        }
        self.update(&radioplayer::player().now());
    }

    fn card(self: &Rc<Self>, window: &Rc<Window>, station: &Station) -> gtk::Widget {
        let overlay = gtk::Overlay::new();
        overlay.set_size_request(CARD_W, CARD_H);
        overlay.set_overflow(gtk::Overflow::Hidden);
        overlay.add_css_class("omafile-theme-card");
        overlay.set_tooltip_text(Some(&station.url));
        let art = gtk::DrawingArea::new();
        let drawn = station.clone();
        art.set_draw_func(move |_, cr, w, h| draw_art(cr, w as f64, h as f64, &drawn, false));
        overlay.set_child(Some(&art));

        // The name on up to two lines, and under it what kind of station it is.
        let name = gtk::Label::builder().label(&station.name).xalign(0.0).wrap(true).wrap_mode(gtk::pango::WrapMode::WordChar).lines(2).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["omafile-theme-name"]).build();
        let kind = gtk::Label::builder().label(if station.kind == Kind::Podcast { tr("Podcast") } else { tr("Live") }).xalign(0.0).css_classes(["caption", "omafile-radio-kind"]).build();
        let caption = gtk::Box::new(gtk::Orientation::Vertical, 1);
        caption.set_valign(gtk::Align::End);
        caption.add_css_class("omafile-theme-caption");
        caption.append(&name);
        caption.append(&kind);
        overlay.add_overlay(&caption);

        let badge = gtk::Image::from_icon_name("media-playback-start-symbolic");
        badge.set_halign(gtk::Align::Start);
        badge.set_valign(gtk::Align::Start);
        badge.add_css_class("omafile-theme-badge");
        badge.set_visible(false);
        overlay.add_overlay(&badge);

        // Change and remove, shown while the pointer is over the card.
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        actions.set_halign(gtk::Align::End);
        actions.set_valign(gtk::Align::Start);
        actions.add_css_class("omafile-capture-actions");
        let action = |icon: &str, tip: String, run: Run| {
            let button = gtk::Button::builder().icon_name(icon).tooltip_text(tip).build();
            button.add_css_class("circular");
            button.add_css_class("osd");
            let (this, station) = (Rc::downgrade(window), station.clone());
            button.connect_clicked(move |_| {
                if let Some(w) = this.upgrade() {
                    run(&w, &station);
                }
            });
            actions.append(&button);
        };
        action("document-edit-symbolic", tr("Change name, address or genre"), Box::new(|w, s| w.radio_station_dialog(Some(s.clone()))));
        action("user-trash-symbolic", tr("Remove this station"), Box::new(|w, s| w.radio_remove(s)));
        overlay.add_overlay(&actions);

        let right = gtk::GestureClick::new();
        right.set_button(3);
        let (this, station2, anchor) = (Rc::downgrade(window), station.clone(), overlay.downgrade());
        right.connect_pressed(move |gesture, _, x, y| {
            let (Some(w), Some(anchor)) = (this.upgrade(), anchor.upgrade()) else { return };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            w.radio_menu(&station2, &anchor, x, y);
        });
        overlay.add_controller(right);
        self.cards.borrow_mut().push(Card { url: station.url.clone(), frame: overlay.clone(), badge });
        overlay.upcast()
    }
}

impl Window {
    fn radio_page(self: &Rc<Self>) -> Rc<RadioPage> {
        // Cloned out first: the borrow must not live through the arm that fills the slot.
        let existing = self.radio_page.borrow().clone();
        match existing {
            Some(p) => p,
            None => {
                let p = RadioPage::new(self);
                self.content_stack.add_named(&p.root, Some("radio"));
                self.radio_page.replace(Some(p.clone()));
                p
            }
        }
    }

    /// The page with the stations.
    pub fn open_radio(self: &Rc<Self>) {
        let page = self.radio_page();
        if self.content_stack.visible_child_name().as_deref() == Some("nowplaying") {
            self.show_now_playing(false);
        }
        page.reload(self);
        self.content_stack.set_visible_child_name("radio");
        page.root.grab_focus();
    }

    fn radio_reload(self: &Rc<Self>) {
        let page = self.radio_page.borrow().clone();
        if let Some(page) = page {
            page.reload(self);
        }
    }

    /// Tell when a station cannot be played, wherever the window is.
    pub fn radio_watch(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        let before = Cell::new(State::Stopped);
        radioplayer::player().connect_changed(move |now| {
            let was = before.replace(now.state);
            if now.state == State::Failed && was != State::Failed {
                if let (Some(w), Some(station)) = (this.upgrade(), now.station.as_ref()) {
                    w.toast_error(&crate::trf!("{station}: {error}", station = station.name, error = now.error));
                }
            }
        });
    }

    /// A click on a station: play it; the one that is on pauses and goes on.
    pub fn radio_play(self: &Rc<Self>, station: &Station) {
        let player = radioplayer::player();
        if player.now().is_on(&station.url) {
            player.toggle();
        } else {
            player.play(station);
        }
    }

    /// Play or pause the radio; from silence, the station that was on last.
    pub fn radio_toggle(self: &Rc<Self>) {
        let player = radioplayer::player();
        if !player.now().active() && radioplayer::RadioPlayer::resume_station().is_none() {
            self.toast_error(&tr("There is no station to play"));
            return;
        }
        player.toggle();
    }

    pub fn radio_stop(self: &Rc<Self>) {
        radioplayer::player().stop();
    }

    fn radio_remove(self: &Rc<Self>, station: &Station) {
        let mut stations = radio::load();
        stations.retain(|s| s.url != station.url);
        radio::save(&stations);
        let player = radioplayer::player();
        if player.now().station.as_ref().is_some_and(|s| s.url == station.url) {
            player.stop();
        }
        self.radio_reload();
        self.toast(&crate::trf!("Removed {name}", name = station.name));
    }

    fn radio_restore_defaults(self: &Rc<Self>) {
        let mut stations = radio::load();
        let n = radio::restore_defaults(&mut stations);
        radio::save(&stations);
        self.radio_reload();
        self.toast(&if n == 1 { tr("1 station is back") } else { crate::trf!("{n} stations are back", n = n) });
    }

    /// Add a station, or change one.
    pub fn radio_station_dialog(self: &Rc<Self>, existing: Option<Station>) {
        let name = adw::EntryRow::builder().title(tr("Name (found by itself when left empty)")).activates_default(true).build();
        let address = adw::EntryRow::builder().title(tr("Address of the stream or of the podcast's feed")).activates_default(true).input_purpose(gtk::InputPurpose::Url).build();
        let genre = adw::EntryRow::builder().title(tr("Genre")).activates_default(true).build();
        genre.set_text(&genre_title(existing.as_ref().map(|s| s.genre.as_str()).unwrap_or(radio::OWN)));
        if let Some(station) = &existing {
            name.set_text(&station.name);
            address.set_text(&station.url);
        }
        let rows = gtk::ListBox::new();
        rows.set_selection_mode(gtk::SelectionMode::None);
        rows.add_css_class("boxed-list");
        rows.set_width_request(420);
        rows.append(&address);
        rows.append(&name);
        rows.append(&genre);
        let heading = if existing.is_some() { tr("Change Station") } else { tr("Add Station") };
        let dialog = adw::AlertDialog::new(Some(heading.as_str()), Some(tr("A stream plays as long as you listen; its address may also be a .pls or .m3u playlist. Of a podcast's feed the newest episode is played, and Next goes back through the older ones.").as_str()));
        dialog.set_extra_child(Some(&rows));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("save", if existing.is_some() { tr("Save") } else { tr("Add") }.as_str())]);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        let full = |text: &str| {
            let text = text.trim();
            if text.is_empty() || text.contains("://") { text.to_string() } else { format!("https://{text}") }
        };
        dialog.set_response_enabled("save", radio::valid_url(&full(&address.text())));
        let weak = dialog.downgrade();
        address.connect_changed(move |row| {
            if let Some(dialog) = weak.upgrade() {
                dialog.set_response_enabled("save", radio::valid_url(&full(&row.text())));
            }
        });
        let this = Rc::downgrade(self);
        let (name2, address2, genre2) = (name.clone(), address.clone(), genre.clone());
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "save" {
                if w.content_stack.visible_child_name().as_deref() != Some("radio") {
                    w.refocus();
                }
                return;
            }
            let station = Station { kind: existing.as_ref().map(|s| s.kind).unwrap_or(Kind::Live), genre: genre_id(&genre2.text()), name: name2.text().trim().to_string(), url: full(&address2.text()) };
            w.radio_save(existing.clone(), station);
        });
        address.grab_focus();
    }

    /// Store a station from the dialog. A new address is asked what it is: a feed is a
    /// podcast, everything else a stream; a missing name comes from the feed or the address.
    fn radio_save(self: &Rc<Self>, old: Option<Station>, mut station: Station) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            let same_address = old.as_ref().is_some_and(|o| o.url == station.url);
            let mut answered = true;
            if !same_address {
                let head = probe(&station.url).await;
                answered = !head.is_empty();
                let head = String::from_utf8_lossy(&head).to_string();
                let feed = radio::looks_like_feed(&head);
                station.kind = if feed { Kind::Podcast } else { Kind::Live };
                if station.name.is_empty() {
                    station.name = feed.then(|| feed_title(&head)).flatten().unwrap_or_else(|| radio::name_from_url(&station.url));
                }
            }
            if station.name.is_empty() {
                station.name = radio::name_from_url(&station.url);
            }
            let mut stations = radio::load();
            if let Some(old) = old.as_ref().filter(|o| o.url != station.url) {
                // The address changed: the station keeps its place in the list.
                match stations.iter().position(|s| s.url == old.url) {
                    Some(at) if !stations.iter().any(|s| s.url == station.url) => stations[at].url = station.url.clone(),
                    _ => stations.retain(|s| s.url != old.url),
                }
            }
            let name = station.name.clone();
            if !radio::put(&mut stations, station) {
                this.toast_error(&tr("That is not an address that can be played"));
                return;
            }
            radio::save(&stations);
            this.radio_reload();
            if answered {
                this.toast(&if old.is_some() { crate::trf!("Saved {name}", name = name) } else { crate::trf!("Added {name}", name = name) });
            } else {
                this.toast_error(&crate::trf!("Added {name}, but the address did not answer just now", name = name));
            }
        });
    }

    fn radio_menu(self: &Rc<Self>, station: &Station, anchor: &gtk::Overlay, x: f64, y: f64) {
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
            let (this, pop, station) = (Rc::downgrade(self), popover.downgrade(), station.clone());
            button.connect_clicked(move |_| {
                if let Some(pop) = pop.upgrade() {
                    pop.popdown();
                }
                if let Some(w) = this.upgrade() {
                    run(&w, &station);
                }
            });
            column.append(&button);
        };
        let now = radioplayer::player().now();
        item(if now.is_on(&station.url) && now.state != State::Paused { tr("Pause") } else { tr("Play") }, Box::new(|w, s| w.radio_play(s)));
        if now.is_on(&station.url) {
            item(tr("Stop"), Box::new(|w, _| w.radio_stop()));
        }
        item(tr("Change…"), Box::new(|w, s| w.radio_station_dialog(Some(s.clone()))));
        item(tr("Copy Address"), Box::new(|w, s| {
            w.window.clipboard().set_text(&s.url);
            w.toast(&tr("Copied to the clipboard"));
        }));
        item(tr("Remove"), Box::new(|w, s| w.radio_remove(s)));
        popover.set_child(Some(&column));
        popover.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feed_names_itself() {
        assert_eq!(feed_title("<?xml version=\"1.0\"?><rss><channel><title>Risky Business</title><item><title>Ep 1</title>").as_deref(), Some("Risky Business"));
        assert_eq!(feed_title("<feed><title type=\"text\"><![CDATA[Tom &amp; Jerry]]></title>").as_deref(), Some("Tom & Jerry"));
        assert_eq!(feed_title("<rss><channel><title>Cut off he"), None);
        assert_eq!(feed_title("ICY 200 OK"), None);
    }

    #[test]
    fn every_station_gets_a_cover() {
        for (kind, name) in [(Kind::Live, "SomaFM Groove Salad"), (Kind::Podcast, "A podcast with quite a long name that must wrap")] {
            let station = Station { kind, genre: radio::OWN.into(), name: name.into(), url: "https://example.org/x".into() };
            let png = art_png(&station, 256).expect("a picture");
            assert!(png.starts_with(&[0x89, b'P', b'N', b'G']), "a PNG");
            let pixbuf = Pixbuf::from_read(std::io::Cursor::new(png)).unwrap();
            assert_eq!((pixbuf.width(), pixbuf.height()), (256, 256));
        }
    }
}
