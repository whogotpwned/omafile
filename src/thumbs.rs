//! Thumbnails for images and PDFs, decoded off the main thread and cached per URI.

use crate::item::FileItem;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PDF_BYTES: u64 = 256 * 1024 * 1024;

pub struct ThumbCache {
    textures: RefCell<HashMap<String, Option<gdk::Texture>>>,
    in_flight: RefCell<HashSet<String>>,
    waiting: RefCell<HashMap<String, Vec<(gtk::Picture, String)>>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Image,
    Pdf,
}

impl ThumbCache {
    pub fn new() -> Rc<Self> {
        Rc::new(ThumbCache {
            textures: RefCell::new(HashMap::new()),
            in_flight: RefCell::new(HashSet::new()),
            waiting: RefCell::new(HashMap::new()),
        })
    }

    fn source_for(item: &FileItem) -> Option<Source> {
        if item.size() == 0 || item.target_file().path().is_none() {
            return None;
        }
        if item.is_image() && item.size() < MAX_IMAGE_BYTES {
            Some(Source::Image)
        } else if item.is_pdf() && item.size() < MAX_PDF_BYTES && pdftoppm_available() {
            Some(Source::Pdf)
        } else {
            None
        }
    }

    pub fn can_thumbnail(item: &FileItem) -> bool {
        Self::source_for(item).is_some()
    }

    /// Show the thumbnail for `item` in `picture` once it is ready. The picture
    /// is tagged with the key so a recycled cell never shows a stale image.
    pub fn assign(self: &Rc<Self>, item: &FileItem, picture: &gtk::Picture, size: i32) {
        let Some(source) = Self::source_for(item) else {
            picture.set_widget_name("");
            picture.set_paintable(None::<&gdk::Paintable>);
            return;
        };
        let key = format!("{}@{}@{}", item.target_file().uri(), item.modified_unix(), size);
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
        let path = match source {
            Source::Image => item.thumbnail_path().or_else(|| item.target_file().path()),
            Source::Pdf => item.target_file().path(),
        };
        let Some(path) = path else { return };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let decoded = gio::spawn_blocking(move || match source {
                Source::Image => decode_image(&path, size),
                Source::Pdf => decode_pdf(&path, size),
            })
            .await
            .ok()
            .flatten();
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

fn to_decoded(pixbuf: gtk::gdk_pixbuf::Pixbuf) -> Decoded {
    Decoded {
        width: pixbuf.width(),
        height: pixbuf.height(),
        stride: pixbuf.rowstride(),
        alpha: pixbuf.has_alpha(),
        bytes: pixbuf.read_pixel_bytes(),
    }
}

fn decode_image(path: &std::path::Path, size: i32) -> Option<Decoded> {
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, size, size, true).ok()?;
    let pixbuf = pixbuf.apply_embedded_orientation().unwrap_or(pixbuf);
    Some(to_decoded(pixbuf))
}

pub fn pdftoppm_available() -> bool {
    glib::find_program_in_path("pdftoppm").is_some()
}

/// First page of a PDF rendered by poppler's pdftoppm (Arch package `poppler`).
fn decode_pdf(path: &std::path::Path, size: i32) -> Option<Decoded> {
    // 72 dpi renders an A4 page at ~595 px wide; scale the resolution to the target.
    let dpi = ((size as f32 / 595.0) * 72.0).clamp(12.0, 200.0).round() as i32;
    let output = std::process::Command::new("pdftoppm")
        .args(["-png", "-f", "1", "-l", "1", "-singlefile", "-r", &dpi.to_string()])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    let loader = gtk::gdk_pixbuf::PixbufLoader::new();
    loader.write(&output.stdout).ok()?;
    loader.close().ok()?;
    let pixbuf = loader.pixbuf()?;
    // Fit into the requested box.
    let (w, h) = (pixbuf.width(), pixbuf.height());
    let scale = (size as f64 / w.max(h) as f64).min(1.0);
    let pixbuf = if scale < 1.0 {
        pixbuf.scale_simple(((w as f64) * scale) as i32, ((h as f64) * scale) as i32, gtk::gdk_pixbuf::InterpType::Bilinear)?
    } else {
        pixbuf
    };
    Some(to_decoded(pixbuf))
}

/// Page count via pdfinfo, for the preview pane.
pub fn pdf_pages(path: &std::path::Path) -> Option<u32> {
    let output = std::process::Command::new("pdfinfo").arg(path).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().find_map(|l| l.strip_prefix("Pages:")).and_then(|v| v.trim().parse().ok())
}
