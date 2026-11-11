//! Keep the computer awake while files are being copied: a sleep and idle inhibitor
//! from systemd-logind, held for as long as the guard lives. hypridle honours it too,
//! so the screen lock does not suspend a long transfer half way.

use gtk::prelude::*;
use gtk::{gio, glib};

pub struct Guard {
    // Closing the descriptor releases the inhibitor.
    _fd: std::os::fd::OwnedFd,
}

/// Ask logind to hold off sleep and idle actions. None where there is no logind.
pub fn take(why: &str) -> Option<Guard> {
    let conn = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE).ok()?;
    let args = ("sleep:idle", "omafile", why, "block").to_variant();
    let (reply, fds) = conn
        .call_with_unix_fd_list_sync(Some("org.freedesktop.login1"), "/org/freedesktop/login1", "org.freedesktop.login1.Manager", "Inhibit", Some(&args), Some(glib::VariantTy::new("(h)").ok()?), gio::DBusCallFlags::NONE, 2000, gio::UnixFDList::NONE, gio::Cancellable::NONE)
        .ok()?;
    let index = reply.child_value(0).get::<glib::variant::Handle>()?.0;
    let fd = fds?.get(index).ok()?;
    Some(Guard { _fd: fd })
}
