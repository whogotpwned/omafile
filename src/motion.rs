//! Small animations so panels slide and views fade the way the rest of
//! Omarchy moves (Hyprland's easeOutQuint windows, the shell's popups).
//! Everything goes through libadwaita's animations, so the system setting
//! for reduced motion (gtk-enable-animations) turns them off at once.

use adw::prelude::*;
use gtk::glib;

pub const FAST: u32 = 160;
pub const PANEL: u32 = 260;

/// Animate a numeric property from `from` to `to`, then run `done`.
pub fn property(widget: &impl IsA<gtk::Widget>, name: &str, from: f64, to: f64, ms: u32, done: Option<Box<dyn FnOnce()>>) {
    let widget: gtk::Widget = widget.clone().upcast();
    let target = adw::PropertyAnimationTarget::new(&widget, name);
    let animation = adw::TimedAnimation::new(&widget, from, to, ms, target);
    animation.set_easing(adw::Easing::EaseOutQuint);
    if let Some(done) = done {
        let done = std::cell::RefCell::new(Some(done));
        animation.connect_done(move |_| {
            if let Some(f) = done.borrow_mut().take() {
                f();
            }
        });
    }
    animation.play();
    // Keep it alive until it is finished.
    let keep = std::cell::RefCell::new(Some(animation.clone()));
    let started = std::time::Instant::now();
    let label = format!("{name} {from:.0}→{to:.0}");
    animation.connect_done(move |_| {
        if std::env::var_os("OMAFILE_TRACE").is_some() {
            eprintln!("motion: {label} in {} ms", started.elapsed().as_millis());
        }
        keep.borrow_mut().take();
    });
}

/// Slide a paned divider to `to` (pixels); `done` runs when it arrives.
pub fn slide_paned(paned: &gtk::Paned, to: i32, ms: u32, done: Option<Box<dyn FnOnce()>>) {
    let from = paned.position() as f64;
    if !animations_enabled(paned) || (from - to as f64).abs() < 2.0 {
        paned.set_position(to);
        if let Some(done) = done {
            glib::idle_add_local_once(done);
        }
        return;
    }
    property(paned, "position", from, to as f64, ms, done);
}

/// Let a widget come in: it fades up from nothing, a little later the further down the
/// list it is, so a page of cards fills like a hand of cards being dealt.
pub fn stagger_in(widget: &gtk::Widget, index: u32) {
    if !animations_enabled(widget) {
        return;
    }
    widget.set_opacity(0.0);
    let weak = widget.downgrade();
    glib::timeout_add_local_once(std::time::Duration::from_millis(30 + (index.min(24) * 28) as u64), move || {
        if let Some(widget) = weak.upgrade() {
            property(&widget, "opacity", 0.0, 1.0, 320, None);
        }
    });
}

pub fn animations_enabled(widget: &impl IsA<gtk::Widget>) -> bool {
    widget.settings().is_gtk_enable_animations()
}
