//! Preview pane: image, text or icon plus key details for the selected item.

use crate::i18n::tr;
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
    /// Page turner under a multi-page document.
    pager: gtk::Box,
    page_label: gtk::Label,
    /// (path, current page, page count) of the document on show.
    doc: RefCell<Option<(std::path::PathBuf, u32, u32)>>,
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
            .title(tr("Preview"))
            .description(tr("Select a file to preview it here"))
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
        // Page turner: ‹ 3 / 12 ›, shown for documents with more than one page.
        let pager = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        pager.set_halign(gtk::Align::Center);
        pager.set_margin_top(6);
        pager.set_visible(false);
        let prev = gtk::Button::builder().icon_name("go-previous-symbolic").tooltip_text(tr("Previous page")).css_classes(["flat", "circular"]).build();
        let next = gtk::Button::builder().icon_name("go-next-symbolic").tooltip_text(tr("Next page")).css_classes(["flat", "circular"]).build();
        let page_label = gtk::Label::builder().css_classes(["omafile-dim", "caption", "numeric"]).width_chars(9).build();
        pager.append(&prev);
        pager.append(&page_label);
        pager.append(&next);
        picture_box.append(&pager);

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

        let this = Rc::new(PreviewPane { root, stack, picture, text, icon, name, details, hint, thumbs, generation: Cell::new(0), image_size, video, cover, media_icon, controls, media: RefCell::new(None), pager, page_label, doc: RefCell::new(None) });
        let weak = Rc::downgrade(&this);
        prev.connect_clicked(move |_| { if let Some(p) = weak.upgrade() { p.doc_step(-1); } });
        let weak = Rc::downgrade(&this);
        next.connect_clicked(move |_| { if let Some(p) = weak.upgrade() { p.doc_step(1); } });
        // The wheel over the page turns it too.
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE);
        let weak = Rc::downgrade(&this);
        scroll.connect_scroll(move |_, _, dy| {
            match weak.upgrade() {
                Some(p) if dy != 0.0 && p.doc_step(if dy > 0.0 { 1 } else { -1 }) => glib::Propagation::Stop,
                _ => glib::Propagation::Proceed,
            }
        });
        picture_box.add_controller(scroll);
        this
    }

    /// Turn the page of the document on show. Returns false when there is no
    /// multi-page document, so callers can fall back to another meaning of the key.
    pub fn doc_step(self: &Rc<Self>, delta: i64) -> bool {
        let Some((path, page, pages)) = self.doc.borrow().clone() else { return false };
        if pages < 2 {
            return false;
        }
        let next = (page as i64 + delta).clamp(1, pages as i64) as u32;
        if next != page {
            self.doc.replace(Some((path.clone(), next, pages)));
            self.show_doc_page(self.generation.get(), path, next, pages);
        }
        true
    }

    fn show_doc_page(self: &Rc<Self>, gen: u64, path: std::path::PathBuf, page: u32, pages: u32) {
        self.page_label.set_text(&trf!("{page} / {pages}", page = page, pages = pages));
        self.pager.set_visible(pages > 1);
        let size = self.image_size;
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let decoded = gio::spawn_blocking(move || crate::thumbs::pdf_page(&path, page, size)).await.ok().flatten();
            let Some(p) = this.upgrade() else { return };
            let current = p.doc.borrow().as_ref().map(|d| d.1);
            if p.generation.get() == gen && current == Some(page) {
                if let Some(decoded) = decoded {
                    p.picture.set_paintable(Some(&decoded.to_paintable()));
                }
            }
        });
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
        self.doc.replace(None);
        self.pager.set_visible(false);
        if let Some(item) = item.filter(|i| i.is_media()) {
            self.show_media(item);
            return;
        }
        if let Some(item) = item {
            if let Some(path) = item.target_file().path() {
                if item.is_dir() && crate::pyenv::is_venv_dir(&path) {
                    if let Some(v) = crate::pyenv::read(&path) {
                        self.show_python(item, &trf!("Python {version}  ·  {tool}  ·  {n} packages", version = v.version, tool = v.tool, n = v.packages.len()), v.packages.iter().map(|(n, ver)| (format!("{n} {ver}"), Some(()))).collect(), "Virtual environment  ·  Python › Activate puts it into the terminal panel");
                        return;
                    }
                }
                let name = item.name();
                if name == "requirements.txt" || name == "pyproject.toml" || name.starts_with("requirements") && name.ends_with(".txt") {
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        let reqs = if name == "pyproject.toml" { crate::pyenv::pyproject_deps(&text) } else { crate::pyenv::requirements_txt(&text) };
                        if !reqs.is_empty() {
                            let venv = path.parent().and_then(crate::pyenv::find);
                            let checked = crate::pyenv::check(&reqs, venv.as_ref());
                            let missing = checked.iter().filter(|(_, v)| v.is_none()).count();
                            let rows: Vec<(String, Option<()>)> = checked.iter().map(|(n, v)| (match v { Some(ver) => format!("{n} {ver}"), None => n.clone() }, v.as_ref().map(|_| ()))).collect();
                            let hint = match &venv {
                                Some(v) => format!("{}  ·  {} of {} installed{}", crate::pyenv::summary(v), reqs.len() - missing, reqs.len(), if missing > 0 { "  ·  Python › Install Requirements" } else { "" }),
                                None => tr("No venv found  ·  Python › Create venv here"),
                            };
                            self.show_python(item, &format!("{}  ·  {} requirements", item.type_description(), reqs.len()), rows, &hint);
                            return;
                        }
                    }
                }
            }
            if item.is_secret() {
                self.name.set_text(&item.display_name());
                self.details.set_text(&format!("{}  ·  {}", item.type_description(), ops::human_size(item.size())));
                if let Some(icon) = item.icon() {
                    self.icon.set_from_gicon(&icon);
                }
                self.stack.set_visible_child_name("text");
                self.text.buffer().set_text(&tr("••••••••••••••••••••\n\nLooks like a credential file.\nContents are not shown here.\nOpen it in the editor if\nyou really need to see it."));
                self.hint.set_text(&tr("Secret file: preview masked"));
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
                    self.hint.set_text(&tr("Loading diff…"));
                    let this = Rc::downgrade(self);
                    glib::spawn_future_local(async move {
                        let diff = crate::git::diff(&root, &path, &status).await;
                        let Some(p) = this.upgrade() else { return };
                        if p.generation.get() != gen {
                            return;
                        }
                        if diff.trim().is_empty() {
                            p.hint.set_text(&tr("No textual changes (binary or identical)"));
                            p.show_plain_text(&gio::File::for_path(&path), gen);
                        } else {
                            let (adds, dels) = p.render_diff(&diff);
                            p.hint.set_text(&trf!("git diff  ·  +{adds} −{dels}", adds = adds, dels = dels));
                        }
                    });
                    return;
                }
            }
        }
        let Some(item) = item else {
            self.stack.set_visible_child_name("none");
            self.name.set_text(&if count > 1 { tr("Multiple items") } else { String::new() });
            let details = if count > 1 { trf!("{count} items selected", count = count) } else { String::new() };
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
        self.load_facts(item, gen);
        // Text-like files get the renderers below, not their page thumbnail.
        if ThumbCache::can_thumbnail(item) && !item.is_text() {
            self.stack.set_visible_child_name("image");
            self.thumbs.assign(item, &self.picture, self.image_size);
            if item.is_pdf() {
                if let Some(path) = item.target_file().path() {
                    let this = Rc::downgrade(self);
                    glib::spawn_future_local(async move {
                        let info_path = path.clone();
                        let pages = gio::spawn_blocking(move || crate::thumbs::pdf_pages(&info_path)).await.ok().flatten();
                        let Some(p) = this.upgrade() else { return };
                        if p.generation.get() == gen {
                            if let Some(n) = pages {
                                if n > 1 {
                                    p.doc.replace(Some((path.clone(), 1, n)));
                                    p.page_label.set_text(&trf!("{page} / {pages}", page = 1, pages = n));
                                    p.pager.set_visible(true);
                                    p.hint.set_text(&if p.image_size > 1024 { trf!("{n} pages  ·  ↑ ↓ or the wheel turn the pages", n = n) } else { trf!("{n} pages  ·  Alt+PgUp / Alt+PgDn or the wheel turn the pages", n = n) });
                                } else {
                                    p.hint.set_text(&tr("1 page"));
                                }
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
                        p.hint.set_text(&format!("{n} {}", if n == 1 { tr("item") } else { tr("items") }));
                    }
                }
            });
            return;
        }
        // Rich renderers: fonts as a specimen, Markdown / CSV / JSON / code in the text view.
        if let Some(path) = item.target_file().path().filter(|_| item.size() > 0) {
            let name = item.name();
            let dark = adw::StyleManager::default().is_dark();
            if crate::render::is_font(&name) {
                self.stack.set_visible_child_name("image");
                self.picture.set_paintable(None::<&gdk::Paintable>);
                let this = Rc::downgrade(self);
                glib::spawn_future_local(async move {
                    let png = gio::spawn_blocking(move || crate::render::font_specimen(&path, dark)).await.ok().flatten();
                    let Some(p) = this.upgrade() else { return };
                    if p.generation.get() != gen {
                        return;
                    }
                    match png.and_then(|b| gdk::Texture::from_bytes(&glib::Bytes::from(&b)).ok()) {
                        Some(t) => {
                            p.picture.set_paintable(Some(&t));
                            p.hint.set_text(&tr("Font specimen (ImageMagick)"));
                        }
                        None => p.stack.set_visible_child_name("icon"),
                    }
                });
                return;
            }
            let kind = if crate::render::is_markdown(&name) {
                Some("markdown")
            } else if crate::render::is_csv(&name) {
                Some("csv")
            } else if crate::render::is_json(&name) {
                Some("json")
            } else if crate::render::is_code(&name) && crate::render::bat_binary().is_some() {
                Some("code")
            } else {
                None
            };
            if let Some(kind) = kind {
                self.stack.set_visible_child_name("text");
                self.text.buffer().set_text("");
                let this = Rc::downgrade(self);
                let file = item.target_file();
                glib::spawn_future_local(async move {
                    let path2 = path.clone();
                    let rendered: Option<Rendered> = gio::spawn_blocking(move || match kind {
                        "json" => crate::render::pretty_json(&path2).map(Rendered::Code),
                        "code" => crate::render::highlight(&path2, 400, dark).map(Rendered::Ansi),
                        _ => None,
                    })
                    .await
                    .ok()
                    .flatten();
                    let head = read_head(&file).await;
                    let Some(p) = this.upgrade() else { return };
                    if p.generation.get() != gen {
                        return;
                    }
                    match (kind, rendered, head) {
                        ("markdown", _, Some(text)) => {
                            p.render_spans(crate::render::markdown(&text));
                            p.hint.set_text(&tr("Markdown"));
                        }
                        ("csv", _, Some(text)) => {
                            let rows = crate::render::csv_rows(&text, 200);
                            p.render_table(&rows);
                            p.hint.set_text(&format!("{} rows shown  ·  {} columns", rows.len(), rows.first().map(|r| r.len()).unwrap_or(0)));
                        }
                        ("json", Some(Rendered::Code(pretty)), _) => {
                            p.text.buffer().set_text(&pretty);
                            p.hint.set_text(&tr("JSON, formatted with jq"));
                        }
                        ("code", Some(Rendered::Ansi(ansi)), _) => {
                            p.render_ansi(&ansi);
                            p.hint.set_text(&tr("Syntax highlighting by bat"));
                        }
                        (_, _, Some(text)) => p.text.buffer().set_text(&text),
                        _ => p.stack.set_visible_child_name("icon"),
                    }
                });
                return;
            }
        }
        if item.is_text() && item.size() > 0 {
            self.stack.set_visible_child_name("text");
            self.text.buffer().set_text("");
            self.text.set_monospace(true);
            self.text.set_wrap_mode(gtk::WrapMode::None);
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

enum Rendered {
    Code(String),
    Ansi(String),
}

fn gen_of(p: &PreviewPane) -> u64 {
    p.generation.get()
}

impl PreviewPane {
    /// Dimensions, camera, duration, codec… appended to the hint line once known.
    fn load_facts(self: &Rc<Self>, item: &FileItem, gen: u64) {
        let (Some(path), Some(ct)) = (item.target_file().path(), item.content_type()) else { return };
        if !(ct.starts_with("image/") || ct.starts_with("audio/") || ct.starts_with("video/")) {
            return;
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let facts = gio::spawn_blocking(move || crate::render::media_facts(&path, &ct)).await.unwrap_or_default();
            let Some(p) = this.upgrade() else { return };
            if p.generation.get() != gen || facts.is_empty() {
                return;
            }
            // The player already shows the duration; everything else goes on the hint line.
            let line: Vec<String> = facts.iter().filter(|(k, _)| k != "Duration").map(|(k, v)| if k == "Dimensions" || k == "Video" || k == "Audio" { v.clone() } else { format!("{}: {v}", tr(k)) }).collect();
            let existing = p.hint.text().to_string();
            let joined = line.join("  ·  ");
            p.hint.set_text(&if existing.is_empty() { joined } else { format!("{joined}\n{existing}") });
        });
    }

    fn tag(&self, name: &str, setup: impl Fn(&gtk::TextTag)) -> gtk::TextTag {
        let table = self.text.buffer().tag_table();
        match table.lookup(name) {
            Some(t) => t,
            None => {
                let t = gtk::TextTag::new(Some(name));
                setup(&t);
                table.add(&t);
                t
            }
        }
    }

    /// Markdown spans into the text view with heading / bold / code tags.
    fn render_spans(&self, spans: Vec<crate::render::Span>) {
        let palette = crate::theme::current();
        let accent = palette.as_ref().map(|p| p.accent.hex());
        let muted = palette.as_ref().map(|p| p.muted.hex());
        let buffer = self.text.buffer();
        buffer.set_text("");
        let h1 = self.tag("md-h1", |t| { t.set_weight(800); t.set_scale(1.5); t.set_pixels_above_lines(10); });
        let h2 = self.tag("md-h2", |t| { t.set_weight(700); t.set_scale(1.25); t.set_pixels_above_lines(8); });
        let h3 = self.tag("md-h3", |t| { t.set_weight(700); t.set_scale(1.1); t.set_pixels_above_lines(6); });
        let bold = self.tag("md-bold", |t| t.set_weight(700));
        let italic = self.tag("md-italic", |t| t.set_style(pango::Style::Italic));
        let code = self.tag("md-code", |t| { t.set_family(Some("monospace")); t.set_scale(0.95); });
        let quote = self.tag("md-quote", |t| t.set_style(pango::Style::Italic));
        let link = self.tag("md-link", |t| t.set_underline(pango::Underline::Single));
        let dim = self.tag("md-dim", |_| {});
        if let Some(a) = &accent {
            h1.set_foreground(Some(a));
            h2.set_foreground(Some(a));
            link.set_foreground(Some(a));
        }
        if let Some(m) = &muted {
            quote.set_foreground(Some(m));
            dim.set_foreground(Some(m));
        }
        let mut end = buffer.end_iter();
        for (text, kind) in spans {
            let t = match kind {
                "h1" => Some(&h1),
                "h2" => Some(&h2),
                "h3" => Some(&h3),
                "bold" => Some(&bold),
                "italic" => Some(&italic),
                "code" => Some(&code),
                "quote" => Some(&quote),
                "link" => Some(&link),
                "dim" => Some(&dim),
                _ => None,
            };
            match t {
                Some(t) => buffer.insert_with_tags(&mut end, &text, &[t]),
                None => buffer.insert(&mut end, &text),
            }
        }
        self.text.set_monospace(false);
        self.text.set_wrap_mode(gtk::WrapMode::WordChar);
    }

    /// CSV rows as aligned columns, header bold.
    fn render_table(&self, rows: &[Vec<String>]) {
        let buffer = self.text.buffer();
        buffer.set_text("");
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let widths: Vec<usize> = (0..cols).map(|c| rows.iter().map(|r| r.get(c).map(|v| v.chars().count()).unwrap_or(0)).max().unwrap_or(0).min(28)).collect();
        let header = self.tag("csv-head", |t| t.set_weight(700));
        let mut end = buffer.end_iter();
        for (i, row) in rows.iter().enumerate() {
            let line: Vec<String> = (0..cols)
                .map(|c| {
                    let v: String = row.get(c).map(|v| v.chars().take(28).collect()).unwrap_or_default();
                    format!("{:<w$}", v, w = widths[c])
                })
                .collect();
            let text = format!("{}\n", line.join(" │ "));
            if i == 0 {
                buffer.insert_with_tags(&mut end, &text, &[&header]);
                buffer.insert(&mut end, &format!("{}\n", widths.iter().map(|w| "─".repeat(*w)).collect::<Vec<_>>().join("─┼─")));
            } else {
                buffer.insert(&mut end, &text);
            }
        }
        self.text.set_monospace(true);
        self.text.set_wrap_mode(gtk::WrapMode::None);
    }

    /// bat's ANSI output into coloured text tags.
    fn render_ansi(&self, ansi: &str) {
        let buffer = self.text.buffer();
        buffer.set_text("");
        let mut end = buffer.end_iter();
        for (text, style) in crate::render::ansi_spans(ansi) {
            let name = format!("ansi-{}-{}-{:?}", style.bold as u8, style.italic as u8, style.fg);
            let t = self.tag(&name, |t| {
                if style.bold {
                    t.set_weight(700);
                }
                if style.italic {
                    t.set_style(pango::Style::Italic);
                }
                if let Some((r, g, b)) = style.fg {
                    t.set_foreground(Some(&format!("#{r:02x}{g:02x}{b:02x}")));
                }
            });
            buffer.insert_with_tags(&mut end, &text, &[&t]);
        }
        self.text.set_monospace(true);
        self.text.set_wrap_mode(gtk::WrapMode::None);
    }

    /// Package / requirement list with ✓ (installed, green) and ✗ (missing, red).
    fn show_python(self: &Rc<Self>, item: &FileItem, details: &str, rows: Vec<(String, Option<()>)>, hint: &str) {
        self.name.set_text(&item.display_name());
        self.details.set_text(details);
        self.hint.set_text(hint);
        if let Some(icon) = item.icon() {
            self.icon.set_from_gicon(&icon);
        }
        self.stack.set_visible_child_name("text");
        let buffer = self.text.buffer();
        buffer.set_text("");
        let table = buffer.tag_table();
        let palette = crate::theme::current();
        let tag = |name: &str, color: Option<String>| -> gtk::TextTag {
            let t = table.lookup(name).unwrap_or_else(|| {
                let t = gtk::TextTag::new(Some(name));
                table.add(&t);
                t
            });
            if let Some(c) = color {
                t.set_foreground(Some(&c));
            }
            t
        };
        let ok = tag("diff-add", palette.as_ref().map(|p| p.green.hex()));
        let bad = tag("diff-del", palette.as_ref().map(|p| p.red.hex()));
        let mut end = buffer.end_iter();
        for (label, installed) in rows {
            let (mark, t) = if installed.is_some() { ("✓ ", &ok) } else { ("✗ ", &bad) };
            buffer.insert_with_tags(&mut end, mark, &[t]);
            buffer.insert(&mut end, &format!("{label}\n"));
        }
    }

    /// Audio or video: GTK's media player (GStreamer backend) in the pane.
    fn show_media(self: &Rc<Self>, item: &FileItem) {
        self.name.set_text(&item.display_name());
        self.details.set_text(&format!("{}  ·  {}", item.type_description(), ops::human_size(item.size())));
        self.hint.set_text("");
        if let Some(icon) = item.icon() {
            self.icon.set_from_gicon(&icon);
            self.media_icon.set_from_gicon(&icon);
        }
        self.load_facts(item, gen_of(self));
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
                hint.set_text(&trf!("Cannot play: {short}", short = short));
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
