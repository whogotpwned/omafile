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
    queue: RefCell<Vec<Job>>,
    active: std::cell::Cell<usize>,
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
    /// Cover of an EPUB / comic / FB2 (content type).
    Ebook(String),
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
            queue: RefCell::new(Vec::new()),
            active: std::cell::Cell::new(0),
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
        } else if item.is_readable_ebook() && item.size() < MAX_MEDIA_BYTES {
            Some(Source::Ebook(item.content_type().unwrap_or_default()))
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
    /// Requests queue up and the newest (the rows on screen) decode first; a
    /// request whose cell was recycled before its turn is dropped.
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
        if self.in_flight.borrow().contains(&key) || self.queue.borrow().iter().any(|j| j.key == key) {
            return;
        }
        let path = match &source {
            Source::Image => item.thumbnail_path().or_else(|| item.target_file().path()),
            Source::Pdf | Source::Text | Source::Video | Source::Audio | Source::Ebook(_) => item.target_file().path(),
            Source::Folder(p) => Some(p.clone()),
        };
        let Some(path) = path else { return };
        let disk = item.target_file().is_native() && matches!(source, Source::Image | Source::Pdf | Source::Video | Source::Audio | Source::Ebook(_));
        self.queue.borrow_mut().push(Job { key, source, path, size, uri: item.target_file().uri().to_string(), mtime: item.modified_unix(), disk, widget: picture.clone() });
        self.pump();
    }

    /// Decode into the cache without showing it anywhere (the next picture in a gallery).
    pub fn warm(self: &Rc<Self>, item: &FileItem, size: i32) {
        self.assign(item, &gtk::Picture::new(), size);
    }

    fn pump(self: &Rc<Self>) {
        while self.active.get() < MAX_ACTIVE {
            let Some(job) = self.queue.borrow_mut().pop() else { return };
            // Still wanted? Every waiting picture may have been rebound meanwhile.
            let wanted = self.waiting.borrow().get(&job.key).map(|v| v.iter().any(|(p, tag)| p.widget_name() == *tag)).unwrap_or(false);
            if !wanted {
                self.waiting.borrow_mut().remove(&job.key);
                continue;
            }
            self.in_flight.borrow_mut().insert(job.key.clone());
            self.active.set(self.active.get() + 1);
            let this = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let key = job.key.clone();
                let widget = job.widget.clone();
                let (source, path, size, uri, mtime, disk) = (job.source.clone(), job.path.clone(), job.size, job.uri.clone(), job.mtime, job.disk);
                let loaded = gio::spawn_blocking(move || {
                    if disk {
                        if let Some(d) = disk_cache::load(&uri, mtime, size) {
                            return Some(Loaded::Pixels(d));
                        }
                        // Decode at the XDG size, keep that on disk, show it scaled.
                        let cache_size = disk_cache::bucket(size);
                        let full = match source {
                            Source::Image => decode_image(&path, cache_size),
                            Source::Pdf => decode_pdf(&path, cache_size),
                            Source::Video => decode_video(&path, cache_size),
                            Source::Audio => decode_cover(&path, cache_size),
                            Source::Ebook(ct) => decode_ebook(&path, &ct, cache_size),
                            _ => None,
                        }?;
                        disk_cache::store(&uri, mtime, cache_size, &full);
                        return Some(Loaded::Pixels(full.scaled_to(size)));
                    }
                    match source {
                        Source::Image | Source::Folder(_) => decode_image(&path, size).map(Loaded::Pixels),
                        Source::Pdf => decode_pdf(&path, size).map(Loaded::Pixels),
                        Source::Video => decode_video(&path, size).map(Loaded::Pixels),
                        Source::Audio => decode_cover(&path, size).map(Loaded::Pixels),
                        Source::Ebook(ct) => decode_ebook(&path, &ct, size).map(Loaded::Pixels),
                        Source::Text => read_text_head(&path).map(Loaded::Text),
                    }
                })
                .await
                .ok()
                .flatten();
                let texture: Option<gdk::Paintable> = match loaded {
                    Some(Loaded::Pixels(d)) => Some(d.to_paintable()),
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
                cache.active.set(cache.active.get().saturating_sub(1));
                cache.pump();
            });
        }
    }
}

/// Decodes running at once; the rest wait in the queue, newest first.
const MAX_ACTIVE: usize = 4;

struct Job {
    key: String,
    source: Source,
    path: std::path::PathBuf,
    size: i32,
    uri: String,
    mtime: i64,
    /// Goes through the XDG thumbnail cache on disk.
    disk: bool,
    widget: gtk::Picture,
}

/// The freedesktop thumbnail cache (`~/.cache/thumbnails/<bucket>/<md5(uri)>.png`),
/// shared with Nautilus, Loupe and friends: thumbnails survive a restart and
/// are made only once per file on the whole desktop.
mod disk_cache {
    use super::Decoded;
    use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
    use gtk::glib;
    use std::path::PathBuf;

    pub fn bucket(size: i32) -> i32 {
        if size <= 256 {
            256
        } else if size <= 512 {
            512
        } else {
            1024
        }
    }

    fn dir(bucket: i32) -> PathBuf {
        glib::user_cache_dir().join("thumbnails").join(match bucket {
            256 => "large",
            512 => "x-large",
            _ => "xx-large",
        })
    }

    pub fn path_for(uri: &str, bucket: i32) -> PathBuf {
        let md5 = glib::compute_checksum_for_string(glib::ChecksumType::Md5, uri).map(|s| s.to_string()).unwrap_or_default();
        dir(bucket).join(format!("{md5}.png"))
    }

    pub fn load(uri: &str, mtime: i64, size: i32) -> Option<Decoded> {
        let path = path_for(uri, bucket(size));
        let pixbuf = Pixbuf::from_file(&path).ok()?;
        if pixbuf.option("tEXt::Thumb::MTime").map(|m| m.as_str() != mtime.to_string()).unwrap_or(true) {
            return None;
        }
        Some(super::to_decoded(pixbuf).scaled_to(size))
    }

    pub fn store(uri: &str, mtime: i64, bucket: i32, decoded: &Decoded) {
        let target = path_for(uri, bucket);
        let Some(parent) = target.parent() else { return };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let Some(pixbuf) = Pixbuf::from_bytes(&decoded.bytes, Colorspace::Rgb, decoded.alpha, 8, decoded.width, decoded.height, decoded.stride).into() else { return };
        let tmp = parent.join(format!(".{}-{}.png", std::process::id(), glib::random_int()));
        let mtime = mtime.to_string();
        if pixbuf.savev(&tmp, "png", &[("tEXt::Thumb::URI", uri), ("tEXt::Thumb::MTime", mtime.as_str()), ("tEXt::Software", "omafile")]).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            }
            let _ = std::fs::rename(&tmp, &target);
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
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
    /// Fit into `size` × `size` (never upscales).
    pub fn scaled_to(self, size: i32) -> Decoded {
        if self.width.max(self.height) <= size {
            return self;
        }
        let Some(pixbuf) = gtk::gdk_pixbuf::Pixbuf::from_bytes(&self.bytes, gtk::gdk_pixbuf::Colorspace::Rgb, self.alpha, 8, self.width, self.height, self.stride).into() else { return self };
        let scale = size as f64 / self.width.max(self.height) as f64;
        match pixbuf.scale_simple(((self.width as f64) * scale).max(1.0) as i32, ((self.height as f64) * scale).max(1.0) as i32, gtk::gdk_pixbuf::InterpType::Bilinear) {
            Some(p) => to_decoded(p),
            None => self,
        }
    }

    pub fn to_pixbuf(&self) -> gtk::gdk_pixbuf::Pixbuf {
        gtk::gdk_pixbuf::Pixbuf::from_bytes(&self.bytes, gtk::gdk_pixbuf::Colorspace::Rgb, self.alpha, 8, self.width, self.height, self.stride)
    }

    /// The pixels as red, green, blue; transparent ones are left out.
    pub fn pixels(&self) -> Vec<[u8; 3]> {
        let channels = if self.alpha { 4 } else { 3 };
        let mut out = Vec::with_capacity((self.width * self.height).max(0) as usize);
        for y in 0..self.height.max(0) as usize {
            let row = y * self.stride.max(0) as usize;
            for x in 0..self.width.max(0) as usize {
                let o = row + x * channels;
                if o + channels > self.bytes.len() {
                    return out;
                }
                if !self.alpha || self.bytes[o + 3] > 40 {
                    out.push([self.bytes[o], self.bytes[o + 1], self.bytes[o + 2]]);
                }
            }
        }
        out
    }

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

pub(crate) fn decode_image(path: &std::path::Path, size: i32) -> Option<Decoded> {
    let pixbuf = match gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, size, size, true) {
        Ok(p) => p,
        // An animated GIF larger than the thumbnail: the scaling loader stops after the first
        // frame and gdk-pixbuf reports "not all frames were loaded". Take the first frame of
        // the animation and scale it here.
        Err(_) => {
            let full = gtk::gdk_pixbuf::PixbufAnimation::from_file(path).ok()?.static_image()?;
            let k = (size as f64 / full.width().max(full.height()).max(1) as f64).min(1.0);
            let (w, h) = (((full.width() as f64 * k) as i32).max(1), ((full.height() as f64 * k) as i32).max(1));
            full.scale_simple(w, h, gtk::gdk_pixbuf::InterpType::Bilinear)?
        }
    };
    let pixbuf = pixbuf.apply_embedded_orientation().unwrap_or(pixbuf);
    Some(to_decoded(pixbuf))
}

/// Cover of an eBook, decoded from the bytes inside the archive.
fn decode_ebook(path: &std::path::Path, ct: &str, size: i32) -> Option<Decoded> {
    let bytes = crate::ebook::cover(path, ct)?;
    decode_bytes(&bytes, size)
}

/// Decode an image from memory, fitted into `size`.
pub fn decode_bytes(bytes: &[u8], size: i32) -> Option<Decoded> {
    let loader = gtk::gdk_pixbuf::PixbufLoader::new();
    loader.set_size(size, size);
    let (s2, l2) = (size, loader.clone());
    loader.connect_size_prepared(move |_, w, h| {
        let scale = (s2 as f64 / w.max(h) as f64).min(1.0);
        l2.set_size(((w as f64) * scale).max(1.0) as i32, ((h as f64) * scale).max(1.0) as i32);
    });
    loader.write(bytes).ok()?;
    loader.close().ok()?;
    let pixbuf = loader.pixbuf()?;
    Some(to_decoded(pixbuf.apply_embedded_orientation().unwrap_or(pixbuf)))
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

pub(crate) fn decode_video(path: &std::path::Path, size: i32) -> Option<Decoded> {
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
