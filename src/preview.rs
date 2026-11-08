//! Preview pane: image, text or icon plus key details for the selected item.

use crate::item::FileItem;
use crate::ops;
use crate::thumbs::ThumbCache;
use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const TEXT_BYTES: usize = 64 * 1024;

pub struct PreviewPane {
    pub root: gtk::Box,
    stack: gtk::Stack,
    picture: gtk::Picture,
    text: gtk::TextView,
    icon: gtk::Image,
    name: gtk::Label,
    details: gtk::Label,
    hint: gtk::Label,
    thumbs: Rc<ThumbCache>,
    generation: Cell<u64>,
    /// Audio / video player.
    video: gtk::Video,
    cover: gtk::Picture,
    media_icon: gtk::Image,
    controls: gtk::MediaControls,
    media: RefCell<Option<gtk::MediaFile>>,
    /// Pixel size requested for images and PDF pages.
    image_size: i32,
}

impl PreviewPane {
    pub fn new(thumbs: Rc<ThumbCache>) -> Rc<Self> {
        Self::build(thumbs, 1024)
    }

    /// Quick Look variant: larger renders, same widgets.
    pub fn new_large(thumbs: Rc<ThumbCache>) -> Rc<Self> {
        Self::build(thumbs, 1800)
    }

    fn build(thumbs: Rc<ThumbCache>, image_size: i32) -> Rc<Self> {
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);

        let placeholder = adw::StatusPage::builder()
            .icon_name("view-reveal-symbolic")
            .title("Preview")
            .description("Select a file to preview it here")
            .build();
        placeholder.add_css_class("compact");

        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_can_shrink(true);
        picture.add_css_class("omafile-preview-image");
        let picture_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        picture_box.set_margin_top(12);
        picture_box.set_margin_start(12);
        picture_box.set_margin_end(12);
        picture_box.append(&picture);

        let text = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::None)
            .left_margin(10)
            .right_margin(10)
            .top_margin(10)
            .bottom_margin(10)
            .build();
        text.add_css_class("omafile-preview-text");
        let text_scroll = gtk::ScrolledWindow::builder().child(&text).vexpand(true).build();
        text_scroll.add_css_class("omafile-preview-card");
        text_scroll.set_margin_top(12);
        text_scroll.set_margin_start(12);
        text_scroll.set_margin_end(12);

        let icon = gtk::Image::new();
        icon.set_pixel_size(128);
        icon.set_vexpand(true);
        icon.set_valign(gtk::Align::Center);

        // Media: a video surface, or cover art / icon with transport controls for audio.
        let video = gtk::Video::builder().autoplay(false).loop_(false).vexpand(true).build();
        video.add_css_class("omafile-video");
        let cover = gtk::Picture::new();
        cover.set_content_fit(gtk::ContentFit::Contain);
        cover.set_can_shrink(true);
        cover.set_vexpand(true);
        cover.add_css_class("omafile-preview-image");
        let media_icon = gtk::Image::new();
        media_icon.set_pixel_size(128);
        media_icon.set_vexpand(true);
        media_icon.set_valign(gtk::Align::Center);
        let art = gtk::Stack::new();
        art.set_vexpand(true);
        art.add_named(&video, Some("video"));
        art.add_named(&cover, Some("cover"));
        art.add_named(&media_icon, Some("icon"));
        let controls = gtk::MediaControls::new(None::<&gtk::MediaStream>);
        controls.add_css_class("omafile-media-controls");
        controls.set_margin_start(12);
        controls.set_margin_end(12);
        let media_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        media_box.set_margin_top(12);
        media_box.append(&art);
        media_box.append(&controls);

        stack.add_named(&placeholder, Some("none"));
        stack.add_named(&picture_box, Some("image"));
        stack.add_named(&text_scroll, Some("text"));
        stack.add_named(&icon, Some("icon"));
        stack.add_named(&media_box, Some("media"));

        let name = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(pango::WrapMode::WordChar)
            .lines(2)
            .ellipsize(pango::EllipsizeMode::End)
            .css_classes(["title-4"])
            .build();
        let details = gtk::Label::builder().xalign(0.0).wrap(true).max_width_chars(48).css_classes(["omafile-dim", "caption"]).build();
        let hint = gtk::Label::builder().xalign(0.0).wrap(true).max_width_chars(48).css_classes(["omafile-dim", "caption"]).build();
        let info = gtk::Box::new(gtk::Orientation::Vertical, 4);
        info.set_margin_top(12);
        info.set_margin_bottom(14);
        info.set_margin_start(16);
        info.set_margin_end(16);
        info.append(&name);
        info.append(&details);
        info.append(&hint);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("omafile-preview");
        root.append(&stack);
        root.append(&info);

        Rc::new(PreviewPane { root, stack, picture, text, icon, name, details, hint, thumbs, generation: Cell::new(0), image_size, video, cover, media_icon, controls, media: RefCell::new(None) })
    }

    pub fn show(self: &Rc<Self>, item: Option<&FileItem>, count: usize) {
        self.show_git(item, count, None);
    }

    /// Like `show`, but a file with local git changes gets a coloured diff and a
    /// credential-looking file is masked.
    pub fn show_git(self: &Rc<Self>, item: Option<&FileItem>, count: usize, git: Option<(std::path::PathBuf, crate::git::Status)>) {
        let gen = self.generation.get() + 1;
        self.generation.set(gen);
        self.stop();
        if let Some(item) = item.filter(|i| i.is_media()) {
            self.show_media(item);
            return;
        }
        if let Some(item) = item {
            if item.is_secret() {
                self.name.set_text(&item.display_name());
                self.details.set_text(&format!("{}  ·  {}", item.type_description(), ops::human_size(item.size())));
                if let Some(icon) = item.icon() {
                    self.icon.set_from_gicon(&icon);
                }
                self.stack.set_visible_child_name("text");
                self.text.buffer().set_text("••••••••••••••••••••\n\nLooks like a credential file.\nContents are not shown here.\nOpen it in the editor if\nyou really need to see it.");
                self.hint.set_text("Secret file: preview masked");
                return;
            }
            if let (Some((root, status)), Some(path)) = (git.as_ref(), item.target_file().path()) {
                if !item.is_dir() && (item.is_text() || item.size() == 0 || matches!(status, crate::git::Status::Untracked | crate::git::Status::Added)) && !item.is_image() && !item.is_pdf() {
                    let (root, status) = (root.clone(), status.clone());
                    self.name.set_text(&item.display_name());
                    self.details.set_text(&format!("{}  ·  {}  ·  git: {}", item.type_description(), ops::human_size(item.size()), status.label()));
                    if let Some(icon) = item.icon() {
                        self.icon.set_from_gicon(&icon);
                    }
                    self.stack.set_visible_child_name("text");
                    self.text.buffer().set_text("");
                    self.hint.set_text("Loading diff…");
                    let this = Rc::downgrade(self);
                    glib::spawn_future_local(async move {
                        let diff = crate::git::diff(&root, &path, &status).await;
                        let Some(p) = this.upgrade() else { return };
                        if p.generation.get() != gen {
                            return;
                        }
                        if diff.trim().is_empty() {
                            p.hint.set_text("No textual changes (binary or identical)");
                            p.show_plain_text(&gio::File::for_path(&path), gen);
                        } else {
                            let (adds, dels) = p.render_diff(&diff);
                            p.hint.set_text(&format!("git diff  ·  +{adds} −{dels}"));
                        }
                    });
                    return;
                }
            }
        }
        let Some(item) = item else {
            self.stack.set_visible_child_name("none");
            self.name.set_text(if count > 1 { "Multiple items" } else { "" });
            let details = if count > 1 { format!("{count} items selected") } else { String::new() };
            self.details.set_text(&details);
            self.hint.set_text("");
            return;
        };
        self.name.set_text(&item.display_name());
        let mut details = vec![item.type_description()];
        if !item.is_dir() {
            details.push(ops::human_size(item.size()));
        }
        if let Some(m) = item.modified() {
            details.push(ops::format_time(&m));
        }
        self.details.set_text(&details.join("  ·  "));
        self.hint.set_text("");
        if let Some(icon) = item.icon() {
            self.icon.set_from_gicon(&icon);
        }
        if ThumbCache::can_thumbnail(item) {
            self.stack.set_visible_child_name("image");
            self.thumbs.assign(item, &self.picture, self.image_size);
            if item.is_pdf() {
                if let Some(path) = item.target_file().path() {
                    let this = Rc::downgrade(self);
                    glib::spawn_future_local(async move {
                        let pages = gio::spawn_blocking(move || crate::thumbs::pdf_pages(&path)).await.ok().flatten();
                        let Some(p) = this.upgrade() else { return };
                        if p.generation.get() == gen {
                            if let Some(n) = pages {
                                p.hint.set_text(&format!("{n} {}  ·  first page shown", if n == 1 { "page" } else { "pages" }));
                            }
                        }
                    });
                }
            }
            return;
        }
        if item.is_dir() {
            self.stack.set_visible_child_name("icon");
            let file = item.target_file();
            let this = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let count = count_children(&file).await;
                let Some(p) = this.upgrade() else { return };
                if p.generation.get() == gen {
                    if let Some(n) = count {
                        p.hint.set_text(&format!("{n} {}", if n == 1 { "item" } else { "items" }));
                    }
                }
            });
            return;
        }
        if item.is_text() && item.size() > 0 {
            self.stack.set_visible_child_name("text");
            self.text.buffer().set_text("");
            let file = item.target_file();
            let this = Rc::downgrade(self);
            glib::spawn_future_local(async move {
                let head = read_head(&file).await;
                let Some(p) = this.upgrade() else { return };
                if p.generation.get() != gen {
                    return;
                }
                match head {
                    Some(text) => p.text.buffer().set_text(&text),
                    None => p.stack.set_visible_child_name("icon"),
                }
            });
            return;
        }
        self.stack.set_visible_child_name("icon");
        self.picture.set_paintable(None::<&gdk::Paintable>);
    }
}

impl PreviewPane {
    /// Audio or video: GTK's media player (GStreamer backend) in the pane.
    fn show_media(self: &Rc<Self>, item: &FileItem) {
        self.name.set_text(&item.display_name());
        self.details.set_text(&format!("{}  ·  {}", item.type_description(), ops::human_size(item.size())));
        self.hint.set_text("");
        if let Some(icon) = item.icon() {
            self.icon.set_from_gicon(&icon);
            self.media_icon.set_from_gicon(&icon);
        }
        let media = gtk::MediaFile::for_file(&item.target_file());
        let art = self.video.parent().and_downcast::<gtk::Stack>().expect("media art stack");
        if item.is_video() {
            self.video.set_media_stream(Some(&media));
            art.set_visible_child_name("video");
        } else {
            self.video.set_media_stream(None::<&gtk::MediaStream>);
            if ThumbCache::can_thumbnail(item) {
                self.thumbs.assign(item, &self.cover, self.image_size);
                art.set_visible_child_name("cover");
                let art2 = art.clone();
                let cover = self.cover.clone();
                // No cover art inside the file: fall back to the icon once we know.
                glib::timeout_add_local_once(std::time::Duration::from_millis(900), move || {
                    if cover.paintable().is_none() && art2.visible_child_name().as_deref() == Some("cover") {
                        art2.set_visible_child_name("icon");
                    }
                });
            } else {
                art.set_visible_child_name("icon");
            }
        }
        self.controls.set_media_stream(Some(&media));
        let hint = self.hint.clone();
        media.connect_error_notify(move |m| {
            if let Some(e) = m.error() {
                // GStreamer errors are essays; keep the useful line.
                let msg = e.message();
                let missing: Vec<&str> = msg.lines().filter(|l| l.starts_with("Missing decoder") || l.starts_with("Missing demuxer")).map(|l| l.split(" (").next().unwrap_or(l)).collect();
                let short = if !missing.is_empty() {
                    format!("{} (install gst-plugins-good, gst-plugins-bad or gst-libav)", missing.join(", "))
                } else {
                    msg.lines().next().unwrap_or("unknown error").to_string()
                };
                hint.set_text(&format!("Cannot play: {short}"));
            }
        });
        let hint2 = self.hint.clone();
        media.connect_duration_notify(move |m| {
            let secs = m.duration() / 1_000_000;
            if secs > 0 && hint2.text().is_empty() {
                hint2.set_text(&format!("{}:{:02}  ·  Space in Quick Look plays, Enter opens the default player", secs / 60, secs % 60));
            }
        });
        *self.media.borrow_mut() = Some(media);
        self.stack.set_visible_child_name("media");
    }

    /// Start playback of the media shown (Quick Look on an audio file, "Play").
    pub fn play(&self) {
        if let Some(m) = self.media.borrow().as_ref() {
            m.play();
        }
    }

    /// Stop and release the player (selection changed, pane hidden, dialog closed).
    pub fn stop(&self) {
        if let Some(m) = self.media.borrow_mut().take() {
            m.pause();
            m.set_file(None::<&gio::File>);
        }
        self.controls.set_media_stream(None::<&gtk::MediaStream>);
        self.video.set_media_stream(None::<&gtk::MediaStream>);
        self.cover.set_widget_name("");
        self.cover.set_paintable(None::<&gdk::Paintable>);
    }

    fn show_plain_text(self: &Rc<Self>, file: &gio::File, gen: u64) {
        let file = file.clone();
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let head = read_head(&file).await;
            let Some(p) = this.upgrade() else { return };
            if p.generation.get() == gen {
                p.text.buffer().set_text(&head.unwrap_or_default());
            }
        });
    }

    /// Fill the text view with a unified diff, colouring added, removed and
    /// hunk lines with the theme palette. Returns (added, removed) line counts.
    fn render_diff(&self, diff: &str) -> (usize, usize) {
        render_diff_into(&self.text, diff)
    }
}

/// Fill a text view with a unified diff, colouring added, removed and hunk
/// lines with the theme palette. Returns (added, removed) line counts.
pub fn render_diff_into(text: &gtk::TextView, diff: &str) -> (usize, usize) {
    {
        let buffer = text.buffer();
        buffer.set_text("");
        let table = buffer.tag_table();
        let palette = crate::theme::current();
        let tag = |name: &str, color: Option<String>, dim: bool| -> gtk::TextTag {
            let t = match table.lookup(name) {
                Some(t) => t,
                None => {
                    let t = gtk::TextTag::new(Some(name));
                    table.add(&t);
                    t
                }
            };
            if let Some(c) = color {
                t.set_foreground(Some(&c));
            }
            if dim {
                t.set_style(pango::Style::Italic);
            }
            t
        };
        let add = tag("diff-add", palette.as_ref().map(|p| p.green.hex()), false);
        let del = tag("diff-del", palette.as_ref().map(|p| p.red.hex()), false);
        let hunk = tag("diff-hunk", palette.as_ref().map(|p| p.cyan.hex()), false);
        let meta = tag("diff-meta", palette.as_ref().map(|p| p.muted.hex()), true);
        let (mut adds, mut dels) = (0, 0);
        let mut end = buffer.end_iter();
        for line in diff.lines() {
            let t = if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") || line.starts_with("index ") || line.starts_with("# ") || line.starts_with("new file") || line.starts_with("deleted file") {
                Some(&meta)
            } else if line.starts_with('+') {
                adds += 1;
                Some(&add)
            } else if line.starts_with('-') {
                dels += 1;
                Some(&del)
            } else if line.starts_with("@@") {
                Some(&hunk)
            } else {
                None
            };
            let text = format!("{line}\n");
            match t {
                Some(t) => buffer.insert_with_tags(&mut end, &text, &[t]),
                None => buffer.insert(&mut end, &text),
            }
        }
        (adds, dels)
    }
}

async fn read_head(file: &gio::File) -> Option<String> {
    let stream = file.read_future(glib::Priority::DEFAULT).await.ok()?;
    let mut buf = Vec::new();
    while buf.len() < TEXT_BYTES {
        let chunk = stream.read_future(vec![0u8; 8192], glib::Priority::DEFAULT).await.ok()?;
        let (data, n) = chunk;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&data[..n]);
    }
    if buf.contains(&0) {
        return None;
    }
    let mut text = String::from_utf8_lossy(&buf).to_string();
    if buf.len() >= TEXT_BYTES {
        text.push_str("\n…");
    }
    Some(text)
}

async fn count_children(dir: &gio::File) -> Option<u32> {
    let enumerator = dir
        .enumerate_children_future("standard::name", gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS, glib::Priority::DEFAULT)
        .await
        .ok()?;
    let mut n = 0u32;
    loop {
        let batch = enumerator.next_files_future(500, glib::Priority::DEFAULT).await.ok()?;
        if batch.is_empty() {
            break;
        }
        n += batch.len() as u32;
        if n > 50_000 {
            break;
        }
    }
    Some(n)
}
