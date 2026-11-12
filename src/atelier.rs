//! The theme atelier: every Omarchy theme as a card to turn on with one click, and
//! a new theme made from any picture.
//!
//! Nothing here replaces what Omarchy does. A theme is turned on with
//! `omarchy-theme-set`, a background with `omarchy-theme-bg-set`; a new theme is a
//! folder in `~/.config/omarchy/themes` holding a `colors.toml`, from which
//! Omarchy's own templates build the files for the terminal, the bar, Hyprland
//! and every other program. The colours come out of the picture (`scheme.rs`).

use crate::i18n::tr;
use crate::omarchy::{self, ThemeInfo};
use crate::ops;
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
    stack: gtk::Stack,
    status: gtk::Label,
    next_background: gtk::Button,
    themes: RefCell<Vec<ThemeInfo>>,
    busy: Cell<bool>,
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
        let title = gtk::Label::builder().label(tr("Themes")).css_classes(["title-4"]).xalign(0.0).margin_start(6).build();
        let status = gtk::Label::builder().css_classes(["omafile-dim"]).xalign(0.0).hexpand(true).margin_start(8).ellipsize(gtk::pango::EllipsizeMode::End).build();
        let next_background = gtk::Button::builder().icon_name("preferences-desktop-wallpaper-symbolic").tooltip_text(tr("Next background of the current theme")).build();
        let folder = gtk::Button::builder().icon_name("folder-open-symbolic").tooltip_text(tr("Open the folder with your themes")).build();
        for b in [&next_background, &folder] {
            b.add_css_class("flat");
        }
        let create = gtk::Button::builder().label(tr("New from Picture…")).tooltip_text(tr("Make a theme out of any picture: its colours, with the picture as background")).build();
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
        top.append(&next_background);
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
        let nothing = adw::StatusPage::builder()
            .icon_name("applications-graphics-symbolic")
            .title(tr("No Omarchy themes here"))
            .description(tr("Themes live in ~/.config/omarchy/themes and in Omarchy's own folder. A theme made from a picture is saved there and waits for Omarchy."))
            .build();
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.add_named(&nothing, Some("empty"));
        stack.add_named(&scroll, Some("themes"));

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("omafile-theme-page");
        root.set_focusable(true);
        root.append(&top);
        root.append(&stack);

        let page = Rc::new(ThemesPage { root, flow, stack, status, next_background, themes: RefCell::new(Vec::new()), busy: Cell::new(false) });
        let this = Rc::downgrade(window);
        back.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.show_files();
            }
        });
        let this = Rc::downgrade(window);
        folder.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                let dir = omarchy::install_dir();
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
        let this = Rc::downgrade(window);
        page.next_background.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.theme_run(vec!["omarchy-theme-bg-next".into()], None);
            }
        });
        let (weak, this) = (Rc::downgrade(&page), Rc::downgrade(window));
        page.flow.connect_child_activated(move |_, child| {
            let (Some(p), Some(w)) = (weak.upgrade(), this.upgrade()) else { return };
            let name = p.themes.borrow().get(child.index() as usize).map(|t| t.name.clone());
            if let Some(name) = name {
                w.theme_apply(&name);
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

    /// Read the themes again and rebuild the cards.
    pub fn reload(self: &Rc<Self>, window: &Rc<Window>) {
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        let mut themes = omarchy::list_themes();
        let current = omarchy::current_theme_name();
        // The one that is on comes first; the rest stay in Omarchy's order, by name.
        if let Some(at) = themes.iter().position(|t| current.as_deref() == Some(t.name.as_str())) {
            let on = themes.remove(at);
            themes.insert(0, on);
        }
        self.stack.set_visible_child_name(if themes.is_empty() { "empty" } else { "themes" });
        let mine = themes.iter().filter(|t| t.user).count();
        let mut parts = vec![if themes.len() == 1 { tr("1 theme") } else { crate::trf!("{n} themes", n = themes.len()) }];
        if mine > 0 {
            parts.push(crate::trf!("{n} of your own", n = mine));
        }
        if let Some(name) = current.as_deref() {
            parts.push(format!("{}: {}", tr("on now"), omarchy::display_name(name)));
        }
        self.status.set_text(&parts.join("  ·  "));
        self.next_background.set_visible(current.is_some() && glib::find_program_in_path("omarchy-theme-bg-next").is_some());
        for (i, info) in themes.iter().enumerate() {
            let is_current = current.as_deref() == Some(info.name.as_str());
            let card = self.card(window, info, is_current);
            self.flow.insert(&card, -1);
            if let Some(child) = self.flow.child_at_index(i as i32) {
                child.add_css_class("omafile-theme-child");
                // Cards come in one after the other.
                crate::motion::stagger_in(child.upcast_ref::<gtk::Widget>(), i as u32);
            }
        }
        self.themes.replace(themes);
    }

    fn card(self: &Rc<Self>, window: &Rc<Window>, info: &ThemeInfo, current: bool) -> gtk::Widget {
        let overlay = gtk::Overlay::new();
        overlay.set_size_request(CARD_W, CARD_H);
        overlay.set_overflow(gtk::Overflow::Hidden);
        overlay.add_css_class("omafile-theme-card");
        if current {
            overlay.add_css_class("omafile-theme-current");
        }
        // Underneath: the theme's own colours as a small desktop, so a theme without a
        // picture still shows what it looks like; the picture fades in over it.
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
            glib::spawn_future_local(async move {
                let decoded = gio::spawn_blocking(move || crate::thumbs::decode_image(&preview, 640)).await.ok().flatten();
                if let (Some(picture), Some(decoded)) = (weak.upgrade(), decoded) {
                    picture.set_paintable(Some(&decoded.to_paintable()));
                    picture.add_css_class("omafile-theme-loaded");
                }
            });
        }
        // The caption: name, light or dark, and the colours.
        let name = gtk::Label::builder().label(omarchy::display_name(&info.name)).xalign(0.0).hexpand(true).ellipsize(gtk::pango::EllipsizeMode::End).css_classes(["omafile-theme-name"]).build();
        let caption = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        caption.set_valign(gtk::Align::End);
        caption.add_css_class("omafile-theme-caption");
        caption.append(&name);
        if info.light {
            let sun = gtk::Image::from_icon_name("weather-clear-symbolic");
            sun.set_tooltip_text(Some(&tr("A light theme")));
            caption.append(&sun);
        }
        let dots = swatches(&info.swatches, 16);
        let count = info.backgrounds.len();
        dots.set_tooltip_text(Some(&if count == 1 { tr("1 background") } else { crate::trf!("{n} backgrounds", n = count) }));
        caption.append(&dots);
        overlay.add_overlay(&caption);
        if current {
            let badge = gtk::Image::from_icon_name("object-select-symbolic");
            badge.set_halign(gtk::Align::End);
            badge.set_valign(gtk::Align::Start);
            badge.add_css_class("omafile-theme-badge");
            overlay.add_overlay(&badge);
        } else if info.user {
            let badge = gtk::Label::new(Some(&tr("yours")));
            badge.set_halign(gtk::Align::Start);
            badge.set_valign(gtk::Align::Start);
            badge.add_css_class("omafile-theme-own");
            overlay.add_overlay(&badge);
        }

        // Right click: what else can be done with this theme.
        let right = gtk::GestureClick::new();
        right.set_button(3);
        let (weak, this, theme, anchor) = (Rc::downgrade(self), Rc::downgrade(window), info.clone(), overlay.downgrade());
        right.connect_pressed(move |gesture, _, x, y| {
            let (Some(p), Some(w), Some(anchor)) = (weak.upgrade(), this.upgrade(), anchor.upgrade()) else { return };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            p.card_menu(&w, &theme, current, &anchor, x, y);
        });
        overlay.add_controller(right);

        // Pictures dropped on a card become backgrounds of that theme.
        let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        let (this, theme) = (Rc::downgrade(window), info.name.clone());
        drop.connect_drop(move |_, value, _, _| {
            let (Some(w), Ok(list)) = (this.upgrade(), value.get::<gdk::FileList>()) else { return false };
            let files: Vec<PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
            w.theme_add_backgrounds(&theme, files)
        });
        overlay.add_controller(drop);
        overlay.upcast()
    }

    fn card_menu(self: &Rc<Self>, window: &Rc<Window>, theme: &ThemeInfo, current: bool, anchor: &gtk::Overlay, x: f64, y: f64) {
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
        let name = theme.name.clone();
        if !current {
            let n = name.clone();
            item(tr("Turn On"), Box::new(move |w| w.theme_apply(&n)));
        } else if glib::find_program_in_path("omarchy-theme-bg-next").is_some() {
            item(tr("Next Background"), Box::new(|w| w.theme_run(vec!["omarchy-theme-bg-next".into()], None)));
        }
        let n = name.clone();
        item(tr("Add Backgrounds…"), Box::new(move |w| w.theme_choose_backgrounds(&n)));
        let dir = theme.dir.clone();
        item(tr("Open Folder"), Box::new(move |w| w.navigate(gio::File::for_path(&dir))));
        if theme.user && !current {
            let (n, dir) = (name.clone(), theme.dir.clone());
            item(tr("Move to Trash"), Box::new(move |w| w.theme_delete(&n, &dir)));
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
    if name.is_empty() { tr("My Theme") } else { name }
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

    /// The gallery of Omarchy themes.
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

    /// Run one of Omarchy's theme commands; the gallery waits and then shows the new state.
    pub fn theme_run(self: &Rc<Self>, argv: Vec<String>, done: Option<String>) {
        let Some(program) = argv.first().cloned() else { return };
        if glib::find_program_in_path(&program).is_none() {
            self.toast_error(&crate::trf!("{program} not found (not on Omarchy?)", program = program));
            return;
        }
        let page = self.themes_page.borrow().clone();
        if let Some(p) = page.as_ref() {
            if p.busy.replace(true) {
                return;
            }
            p.flow.set_sensitive(false);
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let args: Vec<&str> = argv.iter().map(String::as_str).collect();
            let result = ops::run_command(&args, None).await;
            let Some(w) = this.upgrade() else { return };
            if let Some(p) = page.as_ref() {
                p.busy.set(false);
                p.flow.set_sensitive(true);
            }
            match result {
                Ok(()) => {
                    if let Some(text) = done {
                        w.toast(&text);
                    }
                }
                Err(e) => w.toast_error(&format!("{program}: {}", e.message())),
            }
            w.themes_reload();
        });
    }

    /// Turn a theme on, the way Omarchy's own menu does.
    pub fn theme_apply(self: &Rc<Self>, name: &str) {
        if omarchy::current_theme_name().as_deref() == Some(name) {
            return;
        }
        self.theme_run(vec!["omarchy-theme-set".into(), name.to_string()], Some(format!("{}: {}", tr("Theme"), omarchy::display_name(name))));
    }

    fn theme_delete(self: &Rc<Self>, name: &str, dir: &Path) {
        match gio::File::for_path(dir).trash(gio::Cancellable::NONE) {
            Ok(()) => self.toast(&crate::trf!("{name} is in the trash", name = omarchy::display_name(name))),
            Err(e) => self.toast_error(&e.to_string()),
        }
        self.themes_reload();
        self.refresh_sidebar();
    }

    fn theme_choose_backgrounds(self: &Rc<Self>, theme: &str) {
        let filter = gtk::FileFilter::new();
        filter.add_pixbuf_formats();
        filter.set_name(Some(&tr("Pictures")));
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let chooser = gtk::FileDialog::builder().title(tr("Pictures to use as backgrounds")).modal(true).filters(&filters).build();
        if let Some(dir) = glib::user_special_dir(glib::UserDirectory::Pictures) {
            chooser.set_initial_folder(Some(&gio::File::for_path(dir)));
        }
        let (this, theme) = (Rc::downgrade(self), theme.to_string());
        chooser.open_multiple(Some(&self.window), gio::Cancellable::NONE, move |result| {
            let (Some(w), Ok(list)) = (this.upgrade(), result) else { return };
            let files: Vec<PathBuf> = (0..list.n_items()).filter_map(|i| list.item(i).and_downcast::<gio::File>()).filter_map(|f| f.path()).collect();
            w.theme_add_backgrounds(&theme, files);
        });
    }

    /// Copy pictures into the theme's own background folder (where Omarchy looks first); for
    /// the theme that is on, the first one is shown at once.
    fn theme_add_backgrounds(self: &Rc<Self>, theme: &str, files: Vec<PathBuf>) -> bool {
        let pictures: Vec<PathBuf> = files.into_iter().filter(|p| omarchy::is_picture(p)).collect();
        if pictures.is_empty() {
            self.toast_error(&tr("Only pictures can be backgrounds"));
            return false;
        }
        let into = omarchy::user_backgrounds_dir(theme);
        if let Err(e) = std::fs::create_dir_all(&into) {
            self.toast_error(&format!("{}: {e}", into.display()));
            return false;
        }
        let mut added = Vec::new();
        for picture in &pictures {
            let Some(name) = picture.file_name() else { continue };
            let target = into.join(name);
            if target.exists() || std::fs::copy(picture, &target).is_ok() {
                added.push(target);
            }
        }
        if added.is_empty() {
            self.toast_error(&tr("Cannot copy the pictures"));
            return false;
        }
        let text = if added.len() == 1 { crate::trf!("1 background added to {name}", name = omarchy::display_name(theme)) } else { crate::trf!("{n} backgrounds added to {name}", n = added.len(), name = omarchy::display_name(theme)) };
        if omarchy::current_theme_name().as_deref() == Some(theme) && omarchy::bg_set_available() {
            self.theme_run(omarchy::set_wallpaper_argv(&added[0]), Some(text));
        } else {
            self.toast(&text);
            self.themes_reload();
        }
        true
    }

    /// "Create Theme from Picture": the selected picture, or one to choose.
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
        let chooser = gtk::FileDialog::builder().title(tr("Picture to make a theme from")).modal(true).filters(&filters).build();
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

    /// Read the picture, then show the dialog in which the theme takes shape.
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
        let apply = gtk::Button::builder().label(tr("Save and Turn On")).build();
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
        let dialog = adw::Dialog::builder().title(tr("Theme from Picture")).content_width(780).child(&view).build();

        // The name decides the folder: say where it goes, and when it cannot.
        let can_turn_on = omarchy::theme_set_available();
        let check: Rc<dyn Fn()> = {
            let (name, note, save, apply) = (name.clone(), note.clone(), save.clone(), apply.clone());
            Rc::new(move || {
                let folder = omarchy::folder_name(&name.text());
                let dir = omarchy::install_dir().join(&folder);
                let problem = if folder.is_empty() {
                    Some(tr("The name needs a letter or a digit."))
                } else if dir.exists() || omarchy::stock_themes_dir().join(&folder).is_dir() {
                    Some(tr("A theme with this name exists already."))
                } else {
                    None
                };
                let fine = problem.is_none();
                save.set_sensitive(fine);
                apply.set_sensitive(fine && can_turn_on);
                if fine { name.remove_css_class("error") } else { name.add_css_class("error") }
                note.set_text(&problem.unwrap_or_else(|| {
                    let place = crate::window::abbreviate_home(&dir);
                    if omarchy::templates_available() {
                        format!("{} {}. {}", tr("Saved in"), place, tr("Only the colours and the picture are stored: Omarchy builds the files for the terminal, the bar, Hyprland and the other programs from them when the theme is turned on."))
                    } else {
                        format!("{} {}. {}", tr("Saved in"), place, tr("Omarchy's theme templates were not found on this computer: an up-to-date Omarchy is needed to turn the theme on."))
                    }
                }));
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
                match omarchy::write_theme(&omarchy::install_dir(), &name.text(), &scheme, &draft.picture, png.as_deref(), icons) {
                    Ok(dir) => {
                        if let Some(dialog) = dialog.upgrade() {
                            dialog.close();
                        }
                        let folder = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        w.record(Op::Derive(vec![gio::File::for_path(&dir)]), &crate::trf!("Theme {name} saved", name = omarchy::display_name(&folder)));
                        w.refresh_sidebar();
                        if turn_on {
                            w.theme_apply(&folder);
                        } else {
                            w.themes_reload();
                        }
                    }
                    Err(e) => w.toast_error(&format!("{}: {e}", tr("Cannot save the theme"))),
                }
            });
        }
        dialog.present(Some(&self.window));
        name.grab_focus();
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
