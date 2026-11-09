//! Thumbnails: images and PDFs decoded off the main thread, text files drawn as
//! a small page with their first lines (the macOS / Windows look), video frames
//! via ffmpegthumbnailer or ffmpeg, audio cover art via ffmpeg. Cached per URI.

use crate::item::FileItem;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PDF_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MEDIA_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// Bytes of a text file used for its page thumbnail.
const TEXT_THUMB_BYTES: usize = 3 * 1024;

pub struct ThumbCache {
    textures: RefCell<HashMap<String, Option<gdk::Paintable>>>,
    in_flight: RefCell<HashSet<String>>,
    waiting: RefCell<HashMap<String, Vec<(gtk::Picture, String)>>>,
}

#[derive(Clone, PartialEq)]
enum Source {
    Image,
    Pdf,
    /// A folder shown through one of its images (Omarchy theme preview, cover).
    Folder(std::path::PathBuf),
    /// Text and source files: a page with the first lines.
    Text,
    /// A frame from the first seconds of a video.
    Video,
    /// Cover art embedded in an audio file.
    Audio,
}

/// Bytes read for a text page, or pixels decoded for the others.
enum Loaded {
    Pixels(Decoded),
    Text(String),
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
        if item.is_dir() {
            // Omarchy theme folders (preview.png) and folders with a cover image.
            let path = item.target_file().path()?;
            return crate::omarchy::folder_preview(&path).map(Source::Folder);
        }
        if item.size() == 0 || item.target_file().path().is_none() {
            return None;
        }
        if item.is_image() && item.size() < MAX_IMAGE_BYTES {
            Some(Source::Image)
        } else if item.is_pdf() && item.size() < MAX_PDF_BYTES && pdftoppm_available() {
            Some(Source::Pdf)
        } else if item.is_video() && item.size() < MAX_MEDIA_BYTES && video_tool().is_some() {
            Some(Source::Video)
        } else if item.is_audio() && item.size() < MAX_MEDIA_BYTES && ffmpeg_available() {
            Some(Source::Audio)
        } else if item.is_text() && !item.is_secret() {
            Some(Source::Text)
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
        let key = match &source {
            Source::Folder(p) => format!("{}@{}", p.display(), size),
            _ => format!("{}@{}@{}", item.target_file().uri(), item.modified_unix(), size),
        };
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
        let path = match &source {
            Source::Image => item.thumbnail_path().or_else(|| item.target_file().path()),
            Source::Pdf | Source::Text | Source::Video | Source::Audio => item.target_file().path(),
            Source::Folder(p) => Some(p.clone()),
        };
        let Some(path) = path else { return };
        let this = Rc::downgrade(self);
        let widget = picture.clone();
        glib::spawn_future_local(async move {
            let loaded = gio::spawn_blocking(move || match source {
                Source::Image | Source::Folder(_) => decode_image(&path, size).map(Loaded::Pixels),
                Source::Pdf => decode_pdf(&path, size).map(Loaded::Pixels),
                Source::Video => decode_video(&path, size).map(Loaded::Pixels),
                Source::Audio => decode_cover(&path, size).map(Loaded::Pixels),
                Source::Text => read_text_head(&path).map(Loaded::Text),
            })
            .await
            .ok()
            .flatten();
            let texture: Option<gdk::Paintable> = match loaded {
                Some(Loaded::Pixels(d)) => {
                    let format = if d.alpha { gdk::MemoryFormat::R8g8b8a8 } else { gdk::MemoryFormat::R8g8b8 };
                    Some(gdk::MemoryTexture::new(d.width, d.height, format, &d.bytes, d.stride as usize).upcast::<gdk::Paintable>())
                }
                Some(Loaded::Text(text)) => Some(render_text_page(&widget, &text, size)),
                None => None,
            };
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

pub struct Decoded {
    width: i32,
    height: i32,
    stride: i32,
    alpha: bool,
    bytes: glib::Bytes,
}

impl Decoded {
    pub fn to_paintable(&self) -> gdk::Paintable {
        let format = if self.alpha { gdk::MemoryFormat::R8g8b8a8 } else { gdk::MemoryFormat::R8g8b8 };
        gdk::MemoryTexture::new(self.width, self.height, format, &self.bytes, self.stride as usize).upcast::<gdk::Paintable>()
    }
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

pub fn ffmpeg_available() -> bool {
    glib::find_program_in_path("ffmpeg").is_some()
}

/// `ffmpegthumbnailer` (Omarchy ships it) or plain `ffmpeg`.
fn video_tool() -> Option<&'static str> {
    if glib::find_program_in_path("ffmpegthumbnailer").is_some() {
        Some("ffmpegthumbnailer")
    } else if ffmpeg_available() {
        Some("ffmpeg")
    } else {
        None
    }
}

fn pixbuf_from_bytes(bytes: &[u8], size: i32) -> Option<Decoded> {
    if bytes.is_empty() {
        return None;
    }
    let loader = gtk::gdk_pixbuf::PixbufLoader::new();
    loader.set_size(size, size);
    // Keep the aspect ratio: size-prepared gives the real dimensions.
    loader.connect_size_prepared(move |l, w, h| {
        let scale = (size as f64 / w.max(h) as f64).min(1.0);
        l.set_size(((w as f64 * scale) as i32).max(1), ((h as f64 * scale) as i32).max(1));
    });
    loader.write(bytes).ok()?;
    loader.close().ok()?;
    loader.pixbuf().map(to_decoded)
}

fn decode_video(path: &std::path::Path, size: i32) -> Option<Decoded> {
    let output = match video_tool()? {
        "ffmpegthumbnailer" => std::process::Command::new("ffmpegthumbnailer")
            .args(["-c", "png", "-s", &size.to_string(), "-o", "/dev/stdout", "-i"])
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?,
        _ => std::process::Command::new("ffmpeg")
            .args(["-v", "quiet", "-ss", "2", "-i"])
            .arg(path)
            .args(["-frames:v", "1", "-vf", &format!("scale='min({size},iw)':-2"), "-f", "image2pipe", "-vcodec", "png", "-"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?,
    };
    if !output.status.success() {
        return None;
    }
    pixbuf_from_bytes(&output.stdout, size)
}

/// Embedded cover art (ID3 APIC, FLAC picture, MP4 covr) through ffmpeg.
fn decode_cover(path: &std::path::Path, size: i32) -> Option<Decoded> {
    let output = std::process::Command::new("ffmpeg")
        .args(["-v", "quiet", "-i"])
        .arg(path)
        .args(["-an", "-map", "0:v:0", "-frames:v", "1", "-f", "image2pipe", "-vcodec", "png", "-"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    pixbuf_from_bytes(&output.stdout, size)
}

fn read_text_head(path: &std::path::Path) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; TEXT_THUMB_BYTES];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    if buf.iter().take(512).any(|b| *b == 0) {
        return None; // binary after all
    }
    let text = String::from_utf8_lossy(&buf).replace('\t', "    ");
    let lines: Vec<&str> = text.lines().take(40).collect();
    Some(lines.join("\n"))
}

/// Draw the first lines of a text file on a small page, like Finder and Explorer do.
pub fn render_text_page(widget: &impl IsA<gtk::Widget>, text: &str, size: i32) -> gdk::Paintable {
    let height = size as f32;
    let width = (size as f32 * 0.78).round();
    let pad = (size as f32 * 0.07).max(2.0);
    let radius = (size as f32 * 0.04).max(1.5);
    let snapshot = gtk::Snapshot::new();
    let page = gtk::graphene::Rect::new(0.0, 0.0, width, height);
    let rounded = gtk::gsk::RoundedRect::from_rect(page, radius);
    snapshot.push_rounded_clip(&rounded);
    snapshot.append_color(&gdk::RGBA::new(0.98, 0.98, 0.97, 1.0), &page);
    // Text block.
    let layout = widget.create_pango_layout(Some(text));
    let mut font = gtk::pango::FontDescription::from_string("monospace");
    font.set_absolute_size(((size as f64) / 26.0).max(2.0) * gtk::pango::SCALE as f64);
    layout.set_font_description(Some(&font));
    layout.set_width(((width - 2.0 * pad) * gtk::pango::SCALE as f32) as i32);
    layout.set_wrap(gtk::pango::WrapMode::Char);
    layout.set_height(((height - 2.0 * pad) * gtk::pango::SCALE as f32) as i32);
    layout.set_ellipsize(gtk::pango::EllipsizeMode::None);
    snapshot.save();
    snapshot.translate(&gtk::graphene::Point::new(pad, pad));
    snapshot.push_clip(&gtk::graphene::Rect::new(0.0, 0.0, width - 2.0 * pad, height - 2.0 * pad));
    snapshot.append_layout(&layout, &gdk::RGBA::new(0.18, 0.18, 0.2, 1.0));
    snapshot.pop();
    snapshot.restore();
    snapshot.pop();
    // Hairline so the page separates from light backgrounds too.
    snapshot.append_border(&rounded, &[1.0; 4], &[gdk::RGBA::new(0.0, 0.0, 0.0, 0.18); 4]);
    snapshot.to_paintable(Some(&gtk::graphene::Size::new(width, height))).expect("snapshot with content")
}

pub fn pdftoppm_available() -> bool {
    glib::find_program_in_path("pdftoppm").is_some()
}

/// First page of a PDF rendered by poppler's pdftoppm (Arch package `poppler`).
fn decode_pdf(path: &std::path::Path, size: i32) -> Option<Decoded> {
    decode_pdf_page(path, 1, size)
}

/// One page (1-based) of a PDF, decoded off the main thread; `to_paintable` on the main thread.
pub fn pdf_page(path: &std::path::Path, page: u32, size: i32) -> Option<Decoded> {
    decode_pdf_page(path, page, size)
}

fn decode_pdf_page(path: &std::path::Path, page: u32, size: i32) -> Option<Decoded> {
    // 72 dpi renders an A4 page at ~595 px wide; scale the resolution to the target.
    let dpi = ((size as f32 / 595.0) * 72.0).clamp(12.0, 200.0).round() as i32;
    let page = page.max(1).to_string();
    let output = std::process::Command::new("pdftoppm")
        .args(["-png", "-f", &page, "-l", &page, "-singlefile", "-r", &dpi.to_string()])
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
