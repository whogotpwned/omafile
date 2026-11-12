//! The tour: a few stops through what omafile can do, shown once after it was installed.
//!
//! It appears with the first window of a fresh installation (no configuration and no
//! state from an earlier version), can be skipped at any point, and never comes back by
//! itself. Help › About has it for whoever wants it later, and so has the command palette.

use crate::i18n::tr;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gdk, glib};
use std::rc::Rc;

/// One stop: what is shown, said, and which keys belong to it.
pub struct Stop {
    /// Three icons: the middle one is the large one.
    pub icons: [&'static str; 3],
    pub title: &'static str,
    pub text: &'static str,
    /// Actions whose shortcut is shown, each with a word on what it does.
    pub keys: &'static [(&'static str, &'static str)],
    /// The stop shows the desktop in the colours of the current theme instead of icons.
    pub desktop: bool,
}

pub const STOPS: &[Stop] = &[
    Stop {
        icons: ["folder-symbolic", "io.github.whogotpwned.Omafile", "starred-symbolic"],
        title: "Welcome to omafile",
        text: "The file manager made for Omarchy. Seven short stops show what it can do; it takes half a minute. Skip it whenever you like: the tour stays in Help.",
        keys: &[],
        desktop: false,
    },
    Stop {
        icons: ["edit-find-symbolic", "input-keyboard-symbolic", "view-list-symbolic"],
        title: "Everything from the keyboard",
        text: "The command palette finds every command, folder and bookmark as you type. Just start typing in a folder to jump to a file. Every shortcut is listed, and can be searched.",
        keys: &[("palette", "Command palette"), ("search", "Search"), ("shortcuts", "All shortcuts")],
        desktop: false,
    },
    Stop {
        icons: ["view-grid-symbolic", "view-dual-symbolic", "view-paged-symbolic"],
        title: "Your way to look at files",
        text: "Four views, tabs and two panes side by side. Space shows a file without opening it: pictures, PDFs, code, music, even eBooks and archives.",
        keys: &[("split-view", "Two panes"), ("new-tab", "New tab"), ("terminal-panel", "Terminal below the files")],
        desktop: false,
    },
    Stop {
        icons: ["applications-graphics-symbolic", "preferences-desktop-wallpaper-symbolic", "camera-photo-symbolic"],
        title: "Part of your Omarchy",
        text: "omafile wears the theme that is on and changes with it. The theme atelier turns themes on with a click and makes a new one out of any picture; Captures collects your screenshots and screen recordings.",
        keys: &[("themes", "Theme atelier"), ("captures", "Captures")],
        desktop: true,
    },
    Stop {
        icons: ["changes-prevent-symbolic", "document-open-recent-symbolic", "edit-undo-symbolic"],
        title: "Nothing gets lost",
        text: "Undo takes back copies, moves, renames and deletions. Backup keeps complete copies on another drive, by itself; Earlier Versions brings back a file as it was. Vaults keep what is private behind a master password.",
        keys: &[("undo", "Undo"), ("backup", "Backup"), ("versions", "Earlier versions")],
        desktop: false,
    },
    Stop {
        icons: ["phone-symbolic", "network-server-symbolic", "application-rss+xml-symbolic"],
        title: "Beyond this computer",
        text: "Servers open in two panes, local files on the left. Phones and other computers nearby send and receive files without a cable or a cloud. Even your news feeds have a place in the sidebar.",
        keys: &[("nearby", "Nearby devices"), ("feeds", "Feeds")],
        desktop: false,
    },
    Stop {
        icons: ["utilities-terminal-symbolic", "help-browser-symbolic", "emblem-ok-symbolic"],
        title: "That was the tour",
        text: "There is a lot more: Git, bulk rename, tags, smart folders, a disk usage map, Now Playing. Help lists every feature with its shortcut, and opens what you click.",
        keys: &[("help", "All features")],
        desktop: false,
    },
];

/// The tour is due when the installation is fresh and it has not been shown or skipped.
pub fn pending(state: Option<&str>) -> bool {
    state == Some("pending")
}

/// The scheme of the theme that is on, for the small desktop.
fn current_scheme() -> Option<crate::scheme::Scheme> {
    let text = std::fs::read_to_string(crate::theme::theme_dir().join("colors.toml")).ok()?;
    crate::scheme::Scheme::from_colors_toml(&text)
}

fn slide(stop: &Stop) -> gtk::Widget {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 14);
    column.set_valign(gtk::Align::Center);
    column.set_halign(gtk::Align::Center);
    column.set_margin_start(36);
    column.set_margin_end(36);
    column.add_css_class("omafile-tour-slide");

    let scheme = if stop.desktop { current_scheme() } else { None };
    match scheme {
        Some(scheme) => {
            let area = gtk::DrawingArea::new();
            area.set_content_width(336);
            area.set_content_height(189);
            area.set_halign(gtk::Align::Center);
            area.set_overflow(gtk::Overflow::Hidden);
            area.add_css_class("omafile-theme-mock");
            area.add_css_class("omafile-tour-art");
            area.set_draw_func(move |_, cr, w, h| crate::atelier::draw_mock(cr, w as f64, h as f64, &scheme, None));
            column.append(&area);
        }
        None => {
            let art = gtk::Box::new(gtk::Orientation::Horizontal, 18);
            art.set_halign(gtk::Align::Center);
            art.set_margin_bottom(6);
            let known = gtk::IconTheme::for_display(&gdk::Display::default().expect("a display"));
            for (i, icon) in stop.icons.iter().enumerate() {
                // The application's own icon is missing when omafile runs from a build folder.
                let image = gtk::Image::from_icon_name(if known.has_icon(icon) { icon } else { "system-file-manager-symbolic" });
                image.set_pixel_size(if i == 1 { 64 } else { 30 });
                image.set_valign(gtk::Align::Center);
                image.add_css_class("omafile-tour-bubble");
                if i == 1 {
                    image.add_css_class("omafile-tour-main");
                }
                art.append(&image);
            }
            column.append(&art);
        }
    }
    column.append(&gtk::Label::builder().label(tr(stop.title)).css_classes(["title-1"]).wrap(true).justify(gtk::Justification::Center).build());
    column.append(&gtk::Label::builder().label(tr(stop.text)).wrap(true).justify(gtk::Justification::Center).max_width_chars(58).css_classes(["omafile-tour-text"]).build());
    if !stop.keys.is_empty() {
        let keys = gtk::Box::new(gtk::Orientation::Horizontal, 18);
        keys.set_halign(gtk::Align::Center);
        keys.set_margin_top(6);
        for (action, what) in stop.keys {
            let Some(key) = crate::bindings::accels_for(action).into_iter().next() else { continue };
            let pair = gtk::Box::new(gtk::Orientation::Horizontal, 7);
            pair.append(&gtk::Label::builder().label(key).css_classes(["omafile-keycap"]).build());
            pair.append(&gtk::Label::builder().label(tr(what)).css_classes(["omafile-dim"]).build());
            keys.append(&pair);
        }
        column.append(&keys);
    }
    // Each stop fills the whole page, so its neighbours stay out of sight.
    let page = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    page.set_hexpand(true);
    page.set_vexpand(true);
    column.set_hexpand(true);
    page.append(&column);
    page.upcast()
}

impl Window {
    /// With the first window of a fresh installation: the tour, once.
    pub fn maybe_show_tour(self: &Rc<Self>) {
        if !pending(self.prefs.get("tour").as_deref()) {
            return;
        }
        // Whatever happens next, it does not come back by itself.
        self.prefs.set("tour", Some("seen"));
        let this = Rc::downgrade(self);
        glib::timeout_add_local_once(std::time::Duration::from_millis(650), move || {
            if let Some(w) = this.upgrade() {
                w.show_tour();
            }
        });
    }

    pub fn show_tour(self: &Rc<Self>) {
        let carousel = adw::Carousel::new();
        carousel.set_vexpand(true);
        carousel.set_hexpand(true);
        carousel.set_allow_long_swipes(false);
        let slides: Vec<gtk::Widget> = STOPS.iter().map(slide).collect();
        for s in &slides {
            carousel.append(s);
        }
        if let Some(first) = slides.first() {
            first.add_css_class("omafile-tour-in");
        }
        let dots = adw::CarouselIndicatorDots::new();
        dots.set_carousel(Some(&carousel));

        let skip = gtk::Button::builder().label(tr("Skip the Tour")).tooltip_text(tr("Close the tour; it stays in Help › About (Esc)")).build();
        skip.add_css_class("flat");
        let back = gtk::Button::builder().label(tr("Back")).build();
        back.add_css_class("flat");
        back.set_visible(false);
        let next = gtk::Button::builder().label(tr("Next")).build();
        next.add_css_class("pill");
        next.add_css_class("suggested-action");
        let help = gtk::Button::builder().label(tr("Show All Features")).build();
        help.add_css_class("pill");
        help.set_visible(false);
        let count = gtk::Label::builder().css_classes(["omafile-dim", "caption"]).build();
        let bottom = gtk::CenterBox::new();
        bottom.set_margin_top(4);
        bottom.set_margin_bottom(16);
        bottom.set_margin_start(18);
        bottom.set_margin_end(18);
        let left = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        left.append(&back);
        left.append(&count);
        let right = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        right.append(&help);
        right.append(&next);
        bottom.set_start_widget(Some(&left));
        bottom.set_center_widget(Some(&dots));
        bottom.set_end_widget(Some(&right));

        let header = adw::HeaderBar::new();
        header.set_show_title(false);
        header.set_show_end_title_buttons(false);
        header.set_show_start_title_buttons(false);
        header.pack_end(&skip);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&carousel));
        view.add_bottom_bar(&bottom);
        let dialog = adw::Dialog::builder().title(tr("Tour")).content_width(720).content_height(520).child(&view).build();
        dialog.add_css_class("omafile-tour");

        let total = slides.len() as u32;
        let update: Rc<dyn Fn(u32)> = {
            let (back, next, help, count, slides, dots) = (back.clone(), next.clone(), help.clone(), count.clone(), slides.clone(), dots.clone());
            Rc::new(move |page: u32| {
                let last = page + 1 >= total;
                back.set_visible(page > 0);
                next.set_label(&if last { tr("Get Started") } else { tr("Next") });
                help.set_visible(last);
                // The last stop has two buttons: the dots make room, the count still says where one is.
                dots.set_visible(!last);
                count.set_text(&format!("{} / {total}", page + 1));
                for (i, s) in slides.iter().enumerate() {
                    if i as u32 == page { s.add_css_class("omafile-tour-in") } else { s.remove_css_class("omafile-tour-in") }
                }
            })
        };
        update(0);
        let u = update.clone();
        carousel.connect_page_changed(move |_, page| u(page));

        let go: Rc<dyn Fn(i32)> = {
            let (carousel, dialog) = (carousel.clone(), dialog.downgrade());
            Rc::new(move |step: i32| {
                let page = carousel.position().round() as i32 + step;
                if page >= carousel.n_pages() as i32 {
                    if let Some(dialog) = dialog.upgrade() {
                        dialog.close();
                    }
                } else if page >= 0 {
                    carousel.scroll_to(&carousel.nth_page(page as u32), true);
                }
            })
        };
        let g = go.clone();
        next.connect_clicked(move |_| g(1));
        let g = go.clone();
        back.connect_clicked(move |_| g(-1));
        let weak = dialog.downgrade();
        skip.connect_clicked(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.close();
            }
        });
        let (weak, this) = (dialog.downgrade(), Rc::downgrade(self));
        help.connect_clicked(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.close();
            }
            if let Some(w) = this.upgrade() {
                w.show_help("features");
            }
        });
        // Arrow keys and Enter walk through it; Esc closes the dialog as it does everywhere.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let g = go.clone();
        keys.connect_key_pressed(move |_, key, _, _| match key {
            gdk::Key::Right | gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space | gdk::Key::Page_Down => {
                g(1);
                glib::Propagation::Stop
            }
            gdk::Key::Left | gdk::Key::BackSpace | gdk::Key::Page_Up => {
                g(-1);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        dialog.add_controller(keys);
        let this = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            if let Some(w) = this.upgrade() {
                w.refocus();
            }
        });
        dialog.present(Some(&self.window));
        next.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tour_is_due_once_and_only_on_a_fresh_installation() {
        assert!(pending(Some("pending")));
        assert!(!pending(Some("seen")) && !pending(None) && !pending(Some("")));
    }

    #[test]
    fn every_stop_can_be_said_and_every_key_exists() {
        // The catalogs all hold the same keys (scripts/check-i18n.py), so English stands for all.
        let catalog = crate::json::parse(include_str!("../data/i18n/en.json")).unwrap();
        let has = |key: &str| catalog.get(key).is_some();
        assert!((5..=8).contains(&STOPS.len()), "a tour, not a manual");
        for stop in STOPS {
            assert!(has(stop.title), "not in the catalogs: {}", stop.title);
            assert!(has(stop.text), "not in the catalogs: {}", stop.text);
            assert!(stop.text.len() < 260, "too long for a glance: {}", stop.title);
            for (action, what) in stop.keys {
                assert!(!crate::bindings::accels_for(action).is_empty(), "{action} has no shortcut to show");
                assert!(has(what), "not in the catalogs: {what}");
            }
        }
        for key in ["Skip the Tour", "Next", "Back", "Get Started", "Show All Features", "Take the Tour", "Tour"] {
            assert!(has(key), "not in the catalogs: {key}");
        }
    }
}
