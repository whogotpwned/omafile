//! Thumbnails for images, decoded off the main thread and cached per URI.

use crate::item::FileItem;
use gtk::{gdk, gio, glib};
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const MAX_BYTES: u64 = 64 * 1024 * 1024;

pub struct ThumbCache {
    textures: RefCell<HashMap<String, Option<gdk::Texture>>>,
    in_flight: RefCell<HashSet<String>>,
    waiting: RefCell<HashMap<String, Vec<(gtk::Picture, String)>>>,
}

impl ThumbCache {
    pub fn new() -> Rc<Self> {
        Rc::new(ThumbCache {
            textures: RefCell::new(HashMap::new()),
            in_flight: RefCell::new(HashSet::new()),
            waiting: RefCell::new(HashMap::new()),
        })
    }

    pub fn can_thumbnail(item: &FileItem) -> bool {
        item.is_image() && item.size() > 0 && item.size() < MAX_BYTES && item.target_file().path().is_some()
    }

    /// Show the thumbnail for `item` in `picture` once it is ready. The picture
    /// is tagged with the URI so a recycled cell never shows a stale image.
    pub fn assign(self: &Rc<Self>, item: &FileItem, picture: &gtk::Picture, size: i32) {
        let key = format!("{}@{}", item.target_file().uri(), size);
        picture.set_widget_name(&key);
        if let Some(cached) = self.textures.borrow().get(&key) {
            picture.set_paintable(cached.as_ref());
            return;
        }
        picture.set_paintable(None::<&gdk::Paintable>);
        self.waiting.borrow_mut().entry(key.clone()).or_default().push((picture.clone(), key.clone()));
        if !self.in_flight.borrow_mut().insert(key.clone()) {
            return;
        }
        let path = item.thumbnail_path().or_else(|| item.target_file().path());
        let Some(path) = path else { return };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let decoded = gio::spawn_blocking(move || decode(&path, size)).await.ok().flatten();
            let texture = decoded.map(|d| {
                let format = if d.alpha { gdk::MemoryFormat::R8g8b8a8 } else { gdk::MemoryFormat::R8g8b8 };
                gdk::MemoryTexture::new(d.width, d.height, format, &d.bytes, d.stride as usize).upcast::<gdk::Texture>()
            });
            let Some(cache) = this.upgrade() else { return };
            cache.textures.borrow_mut().insert(key.clone(), texture.clone());
            cache.in_flight.borrow_mut().remove(&key);
            let waiting = cache.waiting.borrow_mut().remove(&key).unwrap_or_default();
            for (picture, tag) in waiting {
                if picture.widget_name() == tag {
                    picture.set_paintable(texture.as_ref());
                }
            }
        });
    }
}

struct Decoded {
    width: i32,
    height: i32,
    stride: i32,
    alpha: bool,
    bytes: glib::Bytes,
}

fn decode(path: &std::path::Path, size: i32) -> Option<Decoded> {
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, size, size, true).ok()?;
    let pixbuf = pixbuf.apply_embedded_orientation().unwrap_or(pixbuf);
    Some(Decoded {
        width: pixbuf.width(),
        height: pixbuf.height(),
        stride: pixbuf.rowstride(),
        alpha: pixbuf.has_alpha(),
        bytes: pixbuf.read_pixel_bytes(),
    })
}
