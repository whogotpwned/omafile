//! Skins: a look of its own for omafile. Every Omarchy theme, and every skin made from a
//! picture, as a card; one click and omafile wears it.
//!
//! Only omafile changes. The desktop, the terminal and every other program keep the theme
//! Omarchy has on, and nothing here calls any of Omarchy's theme commands. The first card
//! is "Follow Omarchy": omafile wears whatever theme is on and changes with it, which is
//! how it starts out. A skin made from a picture is a folder in `~/.config/omafile/skins`
//! holding a `colors.toml`; the colours come out of the picture (`scheme.rs`).

use crate::i18n::tr;
use crate::omarchy::{self, ThemeInfo};
use crate::skins::{self, Origin, Skin};
use crate::scheme::{self, Analysis, Scheme};
use crate::theme::Rgb;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::cairo;
use gtk::gdk::prelude::GdkCairoContextExt;
use gtk::gdk_pixbuf::Pixbuf;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// What a menu entry does.
type Run = Box<dyn Fn(&Rc<Window>)>;

const CARD_W: i32 = 264;
const CARD_H: i32 = 165;

// ----- drawing ---------------------------------------------------------------------------

fn set(cr: &cairo::Context, c: Rgb, alpha: f64) {
    cr.set_source_rgba(c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0, alpha);
}

fn rounded(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

fn surface_of(pixbuf: &Pixbuf) -> Option<cairo::ImageSurface> {
    let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, pixbuf.width(), pixbuf.height()).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;
    cr.set_source_pixbuf(pixbuf, 0.0, 0.0);
    cr.paint().ok()?;
    drop(cr);
    Some(surface)
}

/// A small desktop in the colours of a scheme: the wallpaper, the bar, a terminal with
/// the active border, a second window with a selection, and the sixteen colours. This is
/// what the dialog shows while the colours are chosen, and what a new theme gets as its
/// `preview.png`.
pub fn draw_mock(cr: &cairo::Context, w: f64, h: f64, s: &Scheme, wallpaper: Option<&cairo::ImageSurface>) {
    let u = h / 100.0;
    // Wallpaper, filling the frame.
    set(cr, s.background, 1.0);
    let _ = cr.paint();
    if let Some(surface) = wallpaper {
        let (sw, sh) = (surface.width() as f64, surface.height() as f64);
        if sw > 0.0 && sh > 0.0 {
            let k = (w / sw).max(h / sh);
            let _ = cr.save();
            cr.translate((w - sw * k) / 2.0, (h - sh * k) / 2.0);
            cr.scale(k, k);
            let _ = cr.set_source_surface(surface, 0.0, 0.0);
            let _ = cr.paint();
            let _ = cr.restore();
        }
    }
    // The bar.
    let bar = 5.2 * u;
    set(cr, s.background, 0.93);
    cr.rectangle(0.0, 0.0, w, bar);
    let _ = cr.fill();
    for i in 0..5 {
        set(cr, if i == 0 { s.accent } else { s.foreground }, if i == 0 { 1.0 } else { 0.45 });
        cr.arc(3.0 * u + i as f64 * 3.2 * u, bar / 2.0, if i == 0 { 1.0 * u } else { 0.7 * u }, 0.0, 2.0 * PI);
        let _ = cr.fill();
    }
    set(cr, s.foreground, 0.9);
    rounded(cr, w / 2.0 - 7.0 * u, bar / 2.0 - 0.7 * u, 14.0 * u, 1.4 * u, 0.7 * u);
    let _ = cr.fill();
    for i in 0..4 {
        set(cr, s.foreground, 0.7);
        rounded(cr, w - 4.5 * u - i as f64 * 3.4 * u, bar / 2.0 - 0.9 * u, 1.8 * u, 1.8 * u, 0.5 * u);
        let _ = cr.fill();
    }
    // The windows leave a frame of wallpaper around them, as Hyprland's gaps do.
    let gap = 3.0 * u;
    let edge = 6.0 * u;
    let top = bar + 4.5 * u;
    let window = |x: f64, y: f64, ww: f64, hh: f64, border: Rgb, width: f64| {
        set(cr, s.background, 0.92);
        rounded(cr, x, y, ww, hh, 1.6 * u);
        let _ = cr.fill();
        set(cr, border, 1.0);
        cr.set_line_width(width);
        rounded(cr, x, y, ww, hh, 1.6 * u);
        let _ = cr.stroke();
    };
    // A line of "text": bars in the given colours and lengths.
    let line = |x: f64, y: f64, parts: &[(Rgb, f64)]| {
        let mut at = x;
        for (color, length) in parts {
            set(cr, *color, 1.0);
            rounded(cr, at, y, length * u, 1.5 * u, 0.75 * u);
            let _ = cr.fill();
            at += length * u + 1.4 * u;
        }
    };
    let c = &s.colors;
    // The terminal, with the active border.
    let (tx, tw, th) = (edge, (w - 2.0 * edge - gap) * 0.58, h - top - 5.5 * u);
    window(tx, top, tw, th, s.accent, 0.5 * u);
    let (lx, mut ly) = (tx + 3.0 * u, top + 3.4 * u);
    let rows: [&[(Rgb, f64)]; 11] = [
        &[(c[2], 5.0), (c[4], 9.0), (s.foreground, 14.0)],
        &[(s.foreground, 22.0), (c[3], 7.0)],
        &[(c[5], 6.0), (s.foreground, 11.0), (c[6], 9.0), (s.foreground, 5.0)],
        &[(c[8], 30.0)],
        &[(c[1], 8.0), (s.foreground, 17.0)],
        &[(c[2], 5.0), (c[4], 9.0), (s.foreground, 8.0), (c[11], 6.0)],
        &[(c[12], 12.0), (c[13], 7.0), (c[14], 10.0)],
        &[(s.foreground, 26.0), (c[9], 4.0)],
        &[(c[8], 18.0)],
        &[(c[10], 9.0), (s.foreground, 19.0)],
        &[(c[2], 5.0), (c[4], 9.0)],
    ];
    for (i, row) in rows.iter().enumerate() {
        if ly + 2.0 * u > top + th - 2.0 * u {
            break;
        }
        line(lx, ly, row);
        if i == rows.len() - 1 {
            set(cr, s.cursor, 1.0);
            cr.rectangle(lx + 17.4 * u, ly - 0.5 * u, 1.3 * u, 2.5 * u);
            let _ = cr.fill();
        }
        ly += 3.6 * u;
    }
    // A second window: text and a selected row.
    let (rx, rw) = (tx + tw + gap, w - edge - (tx + tw + gap));
    let rh = (th - gap) * 0.56;
    window(rx, top, rw, rh, c[8], 0.3 * u);
    let mut y = top + 3.4 * u;
    for (i, length) in [16.0, 22.0, 12.0, 19.0, 9.0, 15.0].iter().enumerate() {
        if y + 2.0 * u > top + rh - 2.0 * u {
            break;
        }
        if i == 1 {
            set(cr, s.selection_background, 1.0);
            rounded(cr, rx + 1.6 * u, y - 1.0 * u, rw - 3.2 * u, 3.5 * u, 0.9 * u);
            let _ = cr.fill();
            line(rx + 3.0 * u, y, &[(s.selection_foreground, *length)]);
        } else {
            line(rx + 3.0 * u, y, &[(if i == 4 { s.accent } else { s.foreground }, *length)]);
        }
        y += 4.2 * u;
    }
    // The sixteen colours.
    let (py, ph) = (top + rh + gap, th - rh - gap);
    window(rx, py, rw, ph, c[8], 0.3 * u);
    let cell = ((rw - 6.0 * u) / 8.0).min((ph - 6.0 * u) / 2.0);
    let (ox, oy) = (rx + (rw - cell * 8.0) / 2.0, py + (ph - cell * 2.0) / 2.0);
    for (i, color) in c.iter().enumerate() {
        set(cr, *color, 1.0);
        cr.arc(ox + (i % 8) as f64 * cell + cell / 2.0, oy + (i / 8) as f64 * cell + cell / 2.0, cell * 0.36, 0.0, 2.0 * PI);
        let _ = cr.fill();
    }
}

/// The mock desktop as PNG bytes.
fn mock_png(s: &Scheme, wallpaper: Option<&cairo::ImageSurface>) -> Option<Vec<u8>> {
    let (w, h) = (1280usize, 800usize);
    let mut surface = cairo::ImageSurface::create(cairo::Format::Rgb24, w as i32, h as i32).ok()?;
    {
        let cr = cairo::Context::new(&surface).ok()?;
        draw_mock(&cr, w as f64, h as f64, s, wallpaper);
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.data().ok()?;
    let mut rgb = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let o = y * stride + x * 4;
            // Cairo keeps pixels as native-endian 0xRRGGBB words: blue first on these machines.
            rgb.extend_from_slice(&[data[o + 2], data[o + 1], data[o]]);
        }
    }
    drop(data);
    let pixbuf = Pixbuf::from_bytes(&glib::Bytes::from_owned(rgb), gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, w as i32, h as i32, (w * 3) as i32);
    pixbuf.save_to_bufferv("png", &[]).ok()
}

/// A row of colour dots, as a widget.
fn swatches(colors: &[Rgb], size: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    let step = size as f64 * 0.78;
    area.set_content_width((step * (colors.len().max(1) - 1) as f64 + size as f64).ceil() as i32);
    area.set_content_height(size);
    area.set_valign(gtk::Align::Center);
    let colors = colors.to_vec();
    area.set_draw_func(move |_, cr, _, h| {
        let r = h as f64 / 2.0;
        // Right to left, so each dot overlaps the next one's edge like a fan.
        for (i, c) in colors.iter().enumerate().rev() {
            let x = r + i as f64 * step;
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.55);
            cr.arc(x, r, r, 0.0, 2.0 * PI);
            let _ = cr.fill();
            set(cr, *c, 1.0);
            cr.arc(x, r, r - 1.0, 0.0, 2.0 * PI);
            let _ = cr.fill();
        }
    });
    area
}

// ----- the gallery -------------------------------------------------------------------------

pub struct ThemesPage {
    pub root: gtk::Box,
    flow: gtk::FlowBox,
    status: gtk::Label,
    /// What each card stands for, in the order of the cards: None is "Follow Omarchy".
    cards: RefCell<Vec<Option<Skin>>>,
}

impl ThemesPage {
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
        let title = gtk::Label::builder().label(tr("Skins")).css_classes(["title-4"]).xalign(0.0).margin_start(6).build();
        let status = gtk::Label::builder().css_classes(["omafile-dim"]).xalign(0.0).hexpand(true).margin_start(8).ellipsize(gtk::pango::EllipsizeMode::End).build();
        let folder = gtk::Button::builder().icon_name("folder-open-symbolic").tooltip_text(tr("Open the folder with your skins")).build();
        folder.add_css_class("flat");
        let create = gtk::Button::builder().label(tr("New from Picture…")).tooltip_text(tr("Make a skin out of any picture: omafile in its colours")).build();
        create.add_css_class("pill");
        create.add_css_class("suggested-action");
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        top.set_margin_top(8);
        top.set_margin_bottom(6);
        top.set_margin_start(10);
        top.set_margin_end(10);
        top.append(&back);
        top.append(&title);
        top.append(&status);
        top.append(&folder);
        top.append(&create);

        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_activate_on_single_click(true);
        flow.set_homogeneous(true);
        flow.set_valign(gtk::Align::Start);
        flow.set_halign(gtk::Align::Center);
        flow.set_min_children_per_line(1);
        flow.set_max_children_per_line(6);
        flow.set_column_spacing(14);
        flow.set_row_spacing(14);
        flow.set_margin_top(14);
        flow.set_margin_bottom(28);
        flow.set_margin_start(18);
        flow.set_margin_end(18);
        flow.add_css_class("omafile-theme-flow");
        let scroll = gtk::ScrolledWindow::builder().child(&flow).hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).build();

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("omafile-theme-page");
        root.set_focusable(true);
        root.append(&top);
        root.append(&scroll);

        let page = Rc::new(ThemesPage { root, flow, status, cards: RefCell::new(Vec::new()) });
        let this = Rc::downgrade(window);
        back.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.show_files();
            }
        });
        let this = Rc::downgrade(window);
        folder.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                let dir = skins::dir();
                let _ = std::fs::create_dir_all(&dir);
                w.navigate(gio::File::for_path(dir));
            }
        });
        let this = Rc::downgrade(window);
        create.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.theme_pick_picture();
            }
        });
        let (weak, this) = (Rc::downgrade(&page), Rc::downgrade(window));
        page.flow.connect_child_activated(move |_, child| {
            let (Some(p), Some(w)) = (weak.upgrade(), this.upgrade()) else { return };
            let card = p.cards.borrow().get(child.index() as usize).cloned();
            if let Some(card) = card {
                w.skin_apply(card.map(|skin| skin.info.name));
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
        page
    }

    /// Read the skins again and rebuild the cards.
    pub fn reload(self: &Rc<Self>, window: &Rc<Window>) {
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        let all = skins::list();
        // The skin that is worn; a name that no longer leads anywhere means "follow".
        let worn = window.prefs.get("skin").filter(|name| all.iter().any(|s| &s.info.name == name));
        let own = all.iter().filter(|s| s.origin == Origin::Own).count();
        let mut parts = vec![if all.len() == 1 { tr("1 skin") } else { crate::trf!("{n} skins", n = all.len()) }];
        if own > 0 {
            parts.push(crate::trf!("{n} of your own", n = own));
        }
        parts.push(match worn.as_deref() {
            Some(name) => format!("{}: {}", tr("on now"), omarchy::display_name(name)),
            None => tr("omafile follows Omarchy's theme"),
        });
        self.status.set_text(&parts.join("  ·  "));

        // "Follow Omarchy" first: it shows the theme Omarchy has on right now.
        let omarchy_now = omarchy::current_theme_name();
        let followed = omarchy_now.as_deref().and_then(|name| all.iter().find(|s| s.origin == Origin::Omarchy && s.info.name == name)).map(|s| s.info.clone()).unwrap_or_else(|| {
            let dir = crate::theme::theme_dir();
            let p = crate::theme::load_from(&dir);
            ThemeInfo { name: String::new(), dir, preview: None, light: p.light, swatches: vec![p.background, p.foreground, p.accent, p.red, p.yellow, p.green, p.cyan, p.blue, p.magenta] }
        });
        let mut cards: Vec<Option<Skin>> = vec![None];
        cards.extend(all.into_iter().map(Some));
        for (i, entry) in cards.iter().enumerate() {
            let card = match entry {
                None => self.card(window, &followed, tr("Follow Omarchy"), omarchy_now.as_deref().map(omarchy::display_name), None, worn.is_none()),
                Some(skin) => self.card(window, &skin.info, omarchy::display_name(&skin.info.name), None, Some(skin), worn.as_deref() == Some(skin.info.name.as_str())),
            };
            self.flow.insert(&card, -1);
            if let Some(child) = self.flow.child_at_index(i as i32) {
                child.add_css_class("omafile-theme-child");
                // Cards come in one after the other.
                crate::motion::stagger_in(child.upcast_ref::<gtk::Widget>(), i as u32);
            }
        }
        self.cards.replace(cards);
    }

    /// One card. `skin` is None for "Follow Omarchy", which shows `info`, the theme that
    /// Omarchy has on; `detail` is a second line under the title.
    fn card(self: &Rc<Self>, window: &Rc<Window>, info: &ThemeInfo, title: String, detail: Option<String>, skin: Option<&Skin>, on: bool) -> gtk::Widget {
        let overlay = gtk::Overlay::new();
        overlay.set_size_request(CARD_W, CARD_H);
        overlay.set_overflow(gtk::Overflow::Hidden);
        overlay.add_css_class("omafile-theme-card");
        if on {
            overlay.add_css_class("omafile-theme-current");
        }
        // Underneath: the skin's own colours as a small desktop, so one without a picture
        // still shows what it looks like; the picture fades in over it.
        let scheme = std::fs::read_to_string(info.dir.join("colors.toml")).ok().and_then(|t| Scheme::from_colors_toml(&t));
        let base = gtk::DrawingArea::new();
        let fallback = info.swatches.first().copied().unwrap_or(Rgb(26, 27, 38));
        base.set_draw_func(move |_, cr, w, h| match &scheme {
            Some(s) => draw_mock(cr, w as f64, h as f64, s, None),
            None => {
                set(cr, fallback, 1.0);
                let _ = cr.paint();
            }
        });
        overlay.set_child(Some(&base));
        if let Some(preview) = info.preview.clone() {
            let picture = gtk::Picture::new();
            picture.set_content_fit(gtk::ContentFit::Cover);
            picture.set_can_shrink(true);
            picture.add_css_class("omafile-theme-picture");
            overlay.add_overlay(&picture);
            let weak = picture.downgrade();
            // The background first; where its format cannot be read (WebP without the
            // loader), the folder's preview picture; failing both, the drawn desktop stays.
            let second = omarchy::folder_preview(&info.dir).filter(|p| *p != preview);
            glib::spawn_future_local(async move {
                let decoded = gio::spawn_blocking(move || crate::thumbs::decode_image(&preview, 640).or_else(|| second.and_then(|p| crate::thumbs::decode_image(&p, 640)))).await.ok().flatten();
                if let (Some(picture), Some(decoded)) = (weak.upgrade(), decoded) {
                    picture.set_paintable(Some(&decoded.to_paintable()));
                    picture.add_css_class("omafile-theme-loaded");
                }
            });
        }
        // The caption: name, light or dark, and the colours.
        let name = gtk::Label::builder().label(&title).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["omafile-theme-name"]).build();
        let names = gtk::Box::new(gtk::Orientation::Vertical, 1);
        names.set_hexpand(true);
        names.set_valign(gtk::Align::Center);
        names.append(&name);
        if let Some(detail) = detail {
            names.append(&gtk::Label::builder().label(detail).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["caption", "omafile-radio-kind"]).build());
        }
        let caption = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        caption.set_valign(gtk::Align::End);
        caption.add_css_class("omafile-theme-caption");
        caption.append(&names);
        if info.light {
            let sun = gtk::Image::from_icon_name("weather-clear-symbolic");
            sun.set_tooltip_text(Some(&tr("A light skin")));
            caption.append(&sun);
        }
        caption.append(&swatches(&info.swatches, 16));
        overlay.add_overlay(&caption);
        if on {
            let badge = gtk::Image::from_icon_name("object-select-symbolic");
            badge.set_halign(gtk::Align::End);
            badge.set_valign(gtk::Align::Start);
            badge.add_css_class("omafile-theme-badge");
            overlay.add_overlay(&badge);
        }
        if skin.is_some_and(|s| s.origin == Origin::Own) {
            let badge = gtk::Label::new(Some(&tr("yours")));
            badge.set_halign(gtk::Align::Start);
            badge.set_valign(gtk::Align::Start);
            badge.add_css_class("omafile-theme-own");
            overlay.add_overlay(&badge);
        }
        overlay.set_tooltip_text(Some(&match skin {
            None => tr("omafile wears the theme Omarchy has on, and changes with it"),
            Some(_) => tr("Only omafile wears it: the desktop keeps its Omarchy theme"),
        }));

        // Right click: what else can be done with this skin.
        let right = gtk::GestureClick::new();
        right.set_button(3);
        let (weak, this, entry, anchor) = (Rc::downgrade(self), Rc::downgrade(window), skin.cloned(), overlay.downgrade());
        right.connect_pressed(move |gesture, _, x, y| {
            let (Some(p), Some(w), Some(anchor)) = (weak.upgrade(), this.upgrade(), anchor.upgrade()) else { return };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            p.card_menu(&w, entry.as_ref(), on, &anchor, x, y);
        });
        overlay.add_controller(right);
        overlay.upcast()
    }

    fn card_menu(self: &Rc<Self>, window: &Rc<Window>, skin: Option<&Skin>, on: bool, anchor: &gtk::Overlay, x: f64, y: f64) {
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
            let (this, pop) = (Rc::downgrade(window), popover.downgrade());
            button.connect_clicked(move |_| {
                if let Some(pop) = pop.upgrade() {
                    pop.popdown();
                }
                if let Some(w) = this.upgrade() {
                    run(&w);
                }
            });
            column.append(&button);
        };
        if !on {
            let name = skin.map(|s| s.info.name.clone());
            item(if skin.is_some() { tr("Use This Skin") } else { tr("Follow Omarchy") }, Box::new(move |w| w.skin_apply(name.clone())));
        }
        if let Some(skin) = skin {
            let dir = skin.info.dir.clone();
            item(tr("Open Folder"), Box::new(move |w| w.navigate(gio::File::for_path(&dir))));
            // Only omafile's own can go: an Omarchy theme is Omarchy's.
            if skin.origin == Origin::Own && !on {
                let (name, dir) = (skin.info.name.clone(), skin.info.dir.clone());
                item(tr("Move to Trash"), Box::new(move |w| w.theme_delete(&name, &dir)));
            }
        }
        if column.first_child().is_none() {
            return;
        }
        popover.set_child(Some(&column));
        popover.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    }
}

// ----- a theme from a picture ----------------------------------------------------------------

struct Draft {
    picture: PathBuf,
    analysis: Analysis,
    wallpaper: Option<cairo::ImageSurface>,
    light: Cell<bool>,
    accent: Cell<Option<Rgb>>,
}

impl Draft {
    fn scheme(&self) -> Scheme {
        scheme::scheme(&self.analysis, self.light.get(), self.accent.get())
    }
}

/// A name to start with: the picture's file name without its ending, as Omarchy would show it.
pub fn suggested_name(picture: &Path) -> String {
    let stem = picture.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let name = omarchy::display_name(&omarchy::folder_name(&stem));
    if name.is_empty() { tr("My Skin") } else { name }
}

impl Window {
    fn themes_page(self: &Rc<Self>) -> Rc<ThemesPage> {
        // Cloned out first: the borrow must not live through the arm that fills the slot.
        let existing = self.themes_page.borrow().clone();
        match existing {
            Some(p) => p,
            None => {
                let p = ThemesPage::new(self);
                self.content_stack.add_named(&p.root, Some("themes"));
                self.themes_page.replace(Some(p.clone()));
                p
            }
        }
    }

    /// The gallery of skins.
    pub fn open_themes(self: &Rc<Self>) {
        let page = self.themes_page();
        if self.content_stack.visible_child_name().as_deref() == Some("nowplaying") {
            self.show_now_playing(false);
        }
        page.reload(self);
        self.content_stack.set_visible_child_name("themes");
        page.root.grab_focus();
    }

    fn themes_reload(self: &Rc<Self>) {
        let page = self.themes_page.borrow().clone();
        if let Some(page) = page {
            page.reload(self);
        }
    }

    /// Wear a skin, or (None) follow Omarchy's theme again. Only omafile changes: the
    /// choice is kept in the preferences and applied to every open window at once.
    pub fn skin_apply(self: &Rc<Self>, name: Option<String>) {
        let folder = name.as_deref().and_then(skins::resolve);
        if name.is_some() && folder.is_none() {
            self.toast_error(&tr("This skin is no longer there"));
            self.themes_reload();
            return;
        }
        self.prefs.set("skin", name.as_deref());
        self.theme.set_skin(folder);
        self.toast(&match name.as_deref() {
            Some(name) => crate::trf!("omafile wears {name}", name = omarchy::display_name(name)),
            None => tr("omafile follows Omarchy's theme"),
        });
        self.themes_reload();
    }

    fn theme_delete(self: &Rc<Self>, name: &str, dir: &Path) {
        match gio::File::for_path(dir).trash(gio::Cancellable::NONE) {
            Ok(()) => self.toast(&crate::trf!("{name} is in the trash", name = omarchy::display_name(name))),
            Err(e) => self.toast_error(&e.to_string()),
        }
        self.themes_reload();
        self.refresh_sidebar();
    }

    /// "Create Skin from Picture": the selected picture, or one to choose.
    pub fn theme_from_picture(self: &Rc<Self>) {
        let selected = self.selected_items().iter().find(|i| i.is_image()).and_then(|i| i.target_file().path());
        match selected.filter(|_| self.content_stack.visible_child_name().as_deref() == Some("files")) {
            Some(path) => self.theme_dialog(path),
            None => self.theme_pick_picture(),
        }
    }

    fn theme_pick_picture(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.add_pixbuf_formats();
        filter.set_name(Some(&tr("Pictures")));
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let chooser = gtk::FileDialog::builder().title(tr("Picture to make a skin from")).modal(true).filters(&filters).build();
        if let Some(dir) = glib::user_special_dir(glib::UserDirectory::Pictures) {
            chooser.set_initial_folder(Some(&gio::File::for_path(dir)));
        }
        let this = Rc::downgrade(self);
        chooser.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if let (Some(w), Some(path)) = (this.upgrade(), result.ok().and_then(|f| f.path())) {
                w.theme_dialog(path);
            }
        });
    }

    /// Read the picture, then show the dialog in which the skin takes shape.
    pub fn theme_dialog(self: &Rc<Self>, picture: PathBuf) {
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let path = picture.clone();
            let decoded = gio::spawn_blocking(move || (crate::thumbs::decode_image(&path, 96), crate::thumbs::decode_image(&path, 1280))).await;
            let Some(w) = this.upgrade() else { return };
            let Ok((Some(small), large)) = decoded else {
                w.toast_error(&tr("Cannot read this picture"));
                return;
            };
            let analysis = scheme::analyse(&small.pixels());
            let light = analysis.suggests_light();
            let draft = Rc::new(Draft { picture, analysis, wallpaper: large.and_then(|d| surface_of(&d.to_pixbuf())), light: Cell::new(light), accent: Cell::new(None) });
            w.theme_dialog_show(draft);
        });
    }

    fn theme_dialog_show(self: &Rc<Self>, draft: Rc<Draft>) {
        let mock = gtk::DrawingArea::new();
        mock.set_content_width(720);
        mock.set_content_height(405);
        mock.set_overflow(gtk::Overflow::Hidden);
        mock.add_css_class("omafile-theme-mock");
        let d = draft.clone();
        mock.set_draw_func(move |_, cr, w, h| draw_mock(cr, w as f64, h as f64, &d.scheme(), d.wallpaper.as_ref()));
        let frame = gtk::AspectFrame::builder().ratio(16.0 / 9.0).obey_child(false).child(&mock).build();

        // The accents the picture offers.
        let accents = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        accents.set_valign(gtk::Align::Center);
        let fill_accents: Rc<dyn Fn()> = {
            let (accents, draft, mock) = (accents.clone(), draft.clone(), mock.clone());
            Rc::new(move || {
                while let Some(child) = accents.first_child() {
                    accents.remove(&child);
                }
                let offered = scheme::accents(&draft.analysis, draft.light.get());
                let chosen = draft.scheme().accent;
                let mut group: Option<gtk::ToggleButton> = None;
                for color in offered {
                    let dot = gtk::DrawingArea::new();
                    dot.set_content_width(26);
                    dot.set_content_height(26);
                    dot.set_draw_func(move |_, cr, w, h| {
                        set(cr, color, 1.0);
                        cr.arc(w as f64 / 2.0, h as f64 / 2.0, w.min(h) as f64 / 2.0 - 1.0, 0.0, 2.0 * PI);
                        let _ = cr.fill();
                    });
                    let button = gtk::ToggleButton::builder().child(&dot).tooltip_text(color.hex()).build();
                    button.add_css_class("circular");
                    button.add_css_class("flat");
                    button.add_css_class("omafile-theme-accent");
                    match &group {
                        Some(first) => button.set_group(Some(first)),
                        None => group = Some(button.clone()),
                    }
                    button.set_active(color == chosen);
                    let (draft, mock) = (draft.clone(), mock.clone());
                    button.connect_toggled(move |b| {
                        if b.is_active() {
                            draft.accent.set(Some(color));
                            mock.queue_draw();
                        }
                    });
                    accents.append(&button);
                }
            })
        };
        fill_accents();

        let dark = gtk::ToggleButton::builder().label(tr("Dark")).active(!draft.light.get()).build();
        let light = gtk::ToggleButton::builder().label(tr("Light")).group(&dark).active(draft.light.get()).build();
        let modes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        modes.add_css_class("linked");
        modes.set_valign(gtk::Align::Center);
        modes.append(&dark);
        modes.append(&light);
        let (d, m, fill) = (draft.clone(), mock.clone(), fill_accents.clone());
        light.connect_toggled(move |b| {
            if d.light.replace(b.is_active()) != b.is_active() {
                // The accents are fitted to the background: a fresh choice for the other mode.
                d.accent.set(None);
                fill();
                m.queue_draw();
            }
        });
        let accent_label = gtk::Label::builder().label(tr("Accent")).css_classes(["omafile-dim"]).build();
        let choices = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        choices.append(&accent_label);
        choices.append(&accents);
        choices.append(&gtk::Box::builder().hexpand(true).build());
        choices.append(&modes);

        let name = adw::EntryRow::builder().title(tr("Name")).text(suggested_name(&draft.picture)).build();
        let names = adw::PreferencesGroup::new();
        names.add(&name);
        let note = gtk::Label::builder().wrap(true).xalign(0.0).css_classes(["omafile-dim", "caption"]).build();

        let save = gtk::Button::builder().label(tr("Save")).build();
        let apply = gtk::Button::builder().label(tr("Save and Use")).build();
        apply.add_css_class("suggested-action");
        for b in [&save, &apply] {
            b.add_css_class("pill");
        }
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        buttons.set_halign(gtk::Align::End);
        buttons.append(&save);
        buttons.append(&apply);

        let column = gtk::Box::new(gtk::Orientation::Vertical, 14);
        column.set_margin_top(6);
        column.set_margin_bottom(18);
        column.set_margin_start(18);
        column.set_margin_end(18);
        column.append(&frame);
        column.append(&choices);
        column.append(&names);
        column.append(&note);
        column.append(&buttons);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        view.set_content(Some(&column));
        let dialog = adw::Dialog::builder().title(tr("Skin from Picture")).content_width(780).child(&view).build();

        // The name decides the folder: say where it goes, and when it cannot.
        let check: Rc<dyn Fn()> = {
            let (name, note, save, apply) = (name.clone(), note.clone(), save.clone(), apply.clone());
            Rc::new(move || {
                let folder = omarchy::folder_name(&name.text());
                let dir = skins::dir().join(&folder);
                let problem = if folder.is_empty() {
                    Some(tr("The name needs a letter or a digit."))
                } else if dir.exists() || skins::resolve(&folder).is_some() {
                    Some(tr("A skin with this name exists already."))
                } else {
                    None
                };
                let fine = problem.is_none();
                save.set_sensitive(fine);
                apply.set_sensitive(fine);
                if fine { name.remove_css_class("error") } else { name.add_css_class("error") }
                note.set_text(&problem.unwrap_or_else(|| format!("{} {}. {}", tr("Saved in"), crate::window::abbreviate_home(&dir), tr("Only omafile wears it; the desktop keeps its Omarchy theme. The folder has the format of an Omarchy theme, should you ever want it there too."))));
            })
        };
        check();
        let c = check.clone();
        name.connect_changed(move |_| c());

        for (button, turn_on) in [(&save, false), (&apply, true)] {
            let (this, draft, name, dialog) = (Rc::downgrade(self), draft.clone(), name.clone(), dialog.downgrade());
            button.connect_clicked(move |_| {
                let Some(w) = this.upgrade() else { return };
                let scheme = draft.scheme();
                let png = mock_png(&scheme, draft.wallpaper.as_ref());
                let icons = Some(scheme::icon_theme_for(scheme.accent)).filter(|set| omarchy::icon_theme_installed(set));
                match omarchy::write_theme(&skins::dir(), &name.text(), &scheme, &draft.picture, png.as_deref(), icons) {
                    Ok(dir) => {
                        if let Some(dialog) = dialog.upgrade() {
                            dialog.close();
                        }
                        let folder = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        w.record(Op::Derive(vec![gio::File::for_path(&dir)]), &crate::trf!("Skin {name} saved", name = omarchy::display_name(&folder)));
                        w.refresh_sidebar();
                        if turn_on {
                            w.skin_apply(Some(folder));
                        } else {
                            w.themes_reload();
                        }
                    }
                    Err(e) => w.toast_error(&format!("{}: {e}", tr("Cannot save the skin"))),
                }
            });
        }
        dialog.present(Some(&self.window));
        name.grab_focus();
    }
}

/// `omafile --theme-from-picture <picture> [--name <name>] [--dark | --light] [--into <folder>]`:
/// the same skin the dialog saves, without a window. Prints its folder. The mode follows
/// the picture unless one is asked for; `--into` names another folder than
/// `~/.config/omafile/skins` (Omarchy's `~/.config/omarchy/themes`, for example, to have it
/// as a theme of the desktop too; the checks use it).
pub fn create_headless(args: &[String]) -> u8 {
    let (mut picture, mut name, mut light, mut into) = (None, None, None, skins::dir());
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--name" => name = rest.next().cloned(),
            "--into" => into = rest.next().map(PathBuf::from).unwrap_or(into),
            "--dark" => light = Some(false),
            "--light" => light = Some(true),
            other if picture.is_none() && !other.starts_with("--") => picture = Some(PathBuf::from(other)),
            other => {
                eprintln!("omafile: unknown argument {other}");
                return 2;
            }
        }
    }
    let Some(picture) = picture else {
        eprintln!("usage: omafile --theme-from-picture <picture> [--name <name>] [--dark | --light] [--into <folder>]");
        return 2;
    };
    let Some(small) = crate::thumbs::decode_image(&picture, 96) else {
        eprintln!("omafile: cannot read {}", picture.display());
        return 1;
    };
    let analysis = scheme::analyse(&small.pixels());
    let scheme = scheme::scheme(&analysis, light.unwrap_or_else(|| analysis.suggests_light()), None);
    let wallpaper = crate::thumbs::decode_image(&picture, 1280).and_then(|d| surface_of(&d.to_pixbuf()));
    let png = mock_png(&scheme, wallpaper.as_ref());
    let icons = Some(scheme::icon_theme_for(scheme.accent)).filter(|set| omarchy::icon_theme_installed(set));
    match omarchy::write_theme(&into, &name.unwrap_or_else(|| suggested_name(&picture)), &scheme, &picture, png.as_deref(), icons) {
        Ok(dir) => {
            println!("{}", dir.display());
            0
        }
        Err(e) => {
            eprintln!("omafile: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_a_theme_after_its_picture() {
        assert_eq!(suggested_name(Path::new("/home/me/Pictures/lake-at-dusk.jpg")), "Lake At Dusk");
        assert_eq!(suggested_name(Path::new("/x/IMG_2041.JPG")), "Img 2041");
        assert_eq!(omarchy::folder_name(&suggested_name(Path::new("/x/Lake at Dusk.png"))), "lake-at-dusk");
    }

    #[test]
    fn the_preview_is_a_png_in_the_scheme_s_colours() {
        let analysis = scheme::analyse(&[[14, 20, 44], [14, 20, 44], [240, 140, 40]]);
        let s = scheme::scheme(&analysis, false, None);
        let png = mock_png(&s, None).expect("a picture is drawn without a display");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let loader = gtk::gdk_pixbuf::PixbufLoader::new();
        loader.write(&png).unwrap();
        loader.close().unwrap();
        let back = loader.pixbuf().unwrap();
        assert_eq!((back.width(), back.height()), (1280, 800));
        // The lower left corner is wallpaper: without a picture, the background colour.
        let bytes = back.read_pixel_bytes();
        let o = (790 * back.rowstride() + 4 * back.n_channels()) as usize;
        assert_eq!(Rgb(bytes[o], bytes[o + 1], bytes[o + 2]), s.background);
    }
}
