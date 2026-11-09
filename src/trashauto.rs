//! Trash automation: size summary for the sidebar and "empty items older
//! than N days" (preference `trash-auto-days`, 0 = off), run at startup.

use crate::i18n::tr;
use crate::window::Window;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;

const ATTRS: &str = "standard::name,standard::size,standard::type,trash::deletion-date";

async fn entries() -> Vec<(gio::File, gio::FileInfo)> {
    let trash = gio::File::for_uri("trash:///");
    let Ok(en) = trash.enumerate_children_future(ATTRS, gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, glib::Priority::LOW).await else { return Vec::new() };
    let mut out = Vec::new();
    loop {
        let Ok(batch) = en.next_files_future(200, glib::Priority::LOW).await else { break };
        if batch.is_empty() {
            break;
        }
        for info in batch {
            out.push((en.child(&info), info));
        }
    }
    out
}

/// (items, bytes) currently in the trash. Folder sizes are measured too.
pub async fn summary() -> (usize, u64) {
    let items = entries().await;
    let mut bytes = 0;
    for (file, info) in &items {
        bytes += if info.file_type() == gio::FileType::Directory { crate::ops::dir_size(file).await } else { info.size().max(0) as u64 };
    }
    (items.len(), bytes)
}

pub fn deletion_unix(info: &gio::FileInfo) -> Option<i64> {
    let s = info.attribute_string("trash::deletion-date")?;
    glib::DateTime::from_iso8601(&s, Some(&glib::TimeZone::local())).ok().map(|d| d.to_unix())
}

/// Delete trash entries older than the configured number of days.
pub fn run_auto_empty(window: &Rc<Window>) {
    let days: i64 = window.prefs.get("trash-auto-days").and_then(|v| v.parse().ok()).unwrap_or(0);
    if days <= 0 {
        return;
    }
    let this = Rc::downgrade(window);
    glib::spawn_future_local(async move {
        let now = glib::real_time() / 1_000_000;
        let mut removed = 0;
        for (file, info) in entries().await {
            if let Some(deleted) = deletion_unix(&info) {
                if now - deleted > days * 86_400 && crate::ops::delete_recursive(file).await.is_ok() {
                    removed += 1;
                }
            }
        }
        let Some(w) = this.upgrade() else { return };
        if removed > 0 {
            w.toast(&format!("Trash: removed {removed} {} older than {days} days", if removed == 1 { tr("item") } else { tr("items") }));
            w.refresh_trash_summary();
        }
    });
}
