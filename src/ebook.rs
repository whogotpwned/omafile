//! eBooks: covers, metadata and chapters of EPUB, comic archives (CBZ / CBR)
//! and FictionBook (FB2), read with libarchive's `bsdtar` and a small XML
//! scanner, no extra crates. Also the eBook menu: reader, Calibre
//! conversion, rename from metadata, send to an eReader.

use gtk::{gio, glib};
use gtk::prelude::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn is_ebook_type(ct: &str) -> bool {
    matches!(
        ct,
        "application/epub+zip" | "application/vnd.comicbook+zip" | "application/vnd.comicbook-rar" | "application/x-cbz" | "application/x-cbr"
            | "application/x-fictionbook+xml" | "application/x-fictionbook" | "application/x-mobipocket-ebook" | "application/vnd.amazon.mobi8-ebook"
            | "application/vnd.amazon.ebook" | "image/vnd.djvu" | "image/vnd.djvu+multipage"
    )
}

pub fn is_comic_type(ct: &str) -> bool {
    matches!(ct, "application/vnd.comicbook+zip" | "application/vnd.comicbook-rar" | "application/x-cbz" | "application/x-cbr")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub title: String,
    pub authors: Vec<String>,
    pub publisher: String,
    pub language: String,
    pub date: String,
    pub description: String,
    pub series: String,
    pub series_index: String,
    pub isbn: String,
    /// (title, href) in reading order.
    pub chapters: Vec<(String, String)>,
}

impl Meta {
    pub fn year(&self) -> &str {
        self.date.get(..4).filter(|y| y.chars().all(|c| c.is_ascii_digit())).unwrap_or("")
    }

    /// `Author · Publisher · 2024 · de · 12 chapters`, pieces that exist.
    pub fn summary_line(&self, chapters_label: &str) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.authors.is_empty() {
            parts.push(self.authors.join(", "));
        }
        if !self.series.is_empty() {
            parts.push(if self.series_index.is_empty() { self.series.clone() } else { format!("{} #{}", self.series, self.series_index) });
        }
        if !self.publisher.is_empty() {
            parts.push(self.publisher.clone());
        }
        if !self.year().is_empty() {
            parts.push(self.year().to_string());
        }
        if !self.language.is_empty() {
            parts.push(self.language.clone());
        }
        if !self.chapters.is_empty() {
            parts.push(format!("{} {chapters_label}", self.chapters.len()));
        }
        parts.join("  ·  ")
    }
}

// ----- archive access ------------------------------------------------------------------

fn archive_list(path: &Path) -> Vec<String> {
    let out = Command::new("bsdtar").arg("-tf").arg(path).stdin(Stdio::null()).stderr(Stdio::null()).output().ok();
    out.filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim_end().to_string()).filter(|l| !l.is_empty() && !l.ends_with('/')).collect()).unwrap_or_default()
}

fn archive_entry(path: &Path, name: &str) -> Option<Vec<u8>> {
    let out = Command::new("bsdtar").args(["-xOf"]).arg(path).arg(name).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if out.status.success() && !out.stdout.is_empty() {
        return Some(out.stdout);
    }
    // Some writers store entries with a leading "./".
    let out = Command::new("unzip").args(["-p"]).arg(path).arg(name).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout)
}

fn archive_text(path: &Path, name: &str) -> Option<String> {
    archive_entry(path, name).map(|b| String::from_utf8_lossy(&b).to_string())
}

// ----- tiny XML helpers -------------------------------------------------------------------

/// Every `<tag ...>` opening (or self-closing) element as (attributes string, inner text).
fn elements<'a>(xml: &'a str, tag: &str) -> Vec<(&'a str, &'a str)> {
    let mut out = Vec::new();
    let lower = xml.to_ascii_lowercase();
    let needle = format!("<{tag}");
    let mut from = 0;
    while let Some(rel) = lower[from..].find(&needle) {
        let start = from + rel;
        // Make sure we matched the whole tag name (not <itemref for <item).
        let after = lower[start + needle.len()..].chars().next();
        if !matches!(after, Some(' ') | Some('>') | Some('/') | Some('\n') | Some('\t')) {
            from = start + needle.len();
            continue;
        }
        let Some(close_rel) = lower[start..].find('>') else { break };
        let open_end = start + close_rel + 1;
        let attrs = &xml[start + needle.len()..open_end - 1];
        if attrs.trim_end().ends_with('/') {
            out.push((attrs, ""));
            from = open_end;
            continue;
        }
        let end_tag = format!("</{tag}");
        let inner_end = lower[open_end..].find(&end_tag).map(|e| open_end + e).unwrap_or(xml.len());
        out.push((attrs, &xml[open_end..inner_end]));
        from = inner_end.max(open_end);
    }
    out
}

/// First `<prefix:tag>` or `<tag>` text, namespace prefixes tolerated.
fn first_text(xml: &str, local: &str) -> String {
    for prefix in ["dc:", "", "opf:"] {
        if let Some((_, inner)) = elements(xml, &format!("{prefix}{local}")).into_iter().find(|(_, inner)| !inner.trim().is_empty()) {
            return decode_entities(strip_tags(inner)).trim().to_string();
        }
    }
    String::new()
}

fn all_texts(xml: &str, local: &str) -> Vec<String> {
    for prefix in ["dc:", ""] {
        let found: Vec<String> = elements(xml, &format!("{prefix}{local}")).into_iter().map(|(_, inner)| decode_entities(strip_tags(inner)).trim().to_string()).filter(|s| !s.is_empty()).collect();
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let needle = format!("{name}=");
    let mut from = 0;
    while let Some(rel) = lower[from..].find(&needle) {
        let at = from + rel;
        let before = if at == 0 { ' ' } else { lower.as_bytes()[at - 1] as char };
        if !before.is_whitespace() {
            from = at + needle.len();
            continue;
        }
        let rest = &attrs[at + needle.len()..];
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let end = rest[1..].find(quote)?;
        return Some(decode_entities(rest[1..1 + end].to_string()));
    }
    None
}

pub fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut tag = String::new();
    let mut skip_depth: u32 = 0; // inside <style> / <script> / <head>
    for c in html.chars() {
        if in_tag {
            if c == '>' {
                in_tag = false;
                let t = tag.trim().to_ascii_lowercase();
                let name: String = t.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                let closing = t.starts_with('/');
                if matches!(name.as_str(), "style" | "script" | "head" | "title") {
                    if closing { skip_depth = skip_depth.saturating_sub(1) } else if !t.ends_with('/') { skip_depth += 1 }
                }
                if matches!(name.as_str(), "p" | "div" | "br" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "tr" | "blockquote" | "section") && (closing || name == "br") {
                    out.push('\n');
                    if matches!(name.as_str(), "p" | "h1" | "h2" | "h3" | "blockquote") {
                        out.push('\n');
                    }
                }
                tag.clear();
            } else {
                tag.push(c);
            }
        } else if c == '<' {
            in_tag = true;
        } else if skip_depth == 0 {
            out.push(c);
        }
    }
    // Collapse runs of blank lines and spaces.
    let mut cleaned = String::with_capacity(out.len());
    let mut blank = 0;
    for line in out.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank += 1;
            if blank <= 1 {
                cleaned.push('\n');
            }
        } else {
            blank = 0;
            cleaned.push_str(&line);
            cleaned.push('\n');
        }
    }
    cleaned.trim().to_string()
}

pub fn decode_entities(s: String) -> String {
    if !s.contains('&') {
        return s;
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s.as_str();
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let decoded = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "laquo" => Some('«'),
            "raquo" => Some('»'),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => out.push(c),
            None => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn join_href(base_dir: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or("");
    let href = decode_entities(href.replace("%20", " "));
    if base_dir.is_empty() {
        return href;
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in href.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

// ----- EPUB -----------------------------------------------------------------------------

struct Opf {
    dir: String,
    xml: String,
    /// id → (href, media-type, properties)
    manifest: Vec<(String, String, String, String)>,
    spine: Vec<String>,
}

fn load_opf(path: &Path) -> Option<Opf> {
    let container = archive_text(path, "META-INF/container.xml")?;
    let opf_path = elements(&container, "rootfile").into_iter().find_map(|(a, _)| attr(a, "full-path"))?;
    let xml = archive_text(path, &opf_path)?;
    let dir = opf_path.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    let manifest = elements(&xml, "item")
        .into_iter()
        .filter_map(|(a, _)| Some((attr(a, "id")?, attr(a, "href")?, attr(a, "media-type").unwrap_or_default(), attr(a, "properties").unwrap_or_default())))
        .collect();
    let spine = elements(&xml, "itemref").into_iter().filter_map(|(a, _)| attr(a, "idref")).collect();
    Some(Opf { dir, xml, manifest, spine })
}

fn epub_cover(path: &Path) -> Option<Vec<u8>> {
    let opf = load_opf(path)?;
    let by_props = opf.manifest.iter().find(|(_, _, _, props)| props.split_whitespace().any(|p| p == "cover-image"));
    let cover_id = elements(&opf.xml, "meta").into_iter().find(|(a, _)| attr(a, "name").map(|n| n.eq_ignore_ascii_case("cover")).unwrap_or(false)).and_then(|(a, _)| attr(a, "content"));
    let by_meta = cover_id.and_then(|id| opf.manifest.iter().find(|(i, _, _, _)| *i == id));
    let by_name = opf.manifest.iter().find(|(id, href, mt, _)| mt.starts_with("image/") && (id.to_lowercase().contains("cover") || href.to_lowercase().contains("cover")));
    let any_image = opf.manifest.iter().find(|(_, _, mt, _)| mt.starts_with("image/"));
    let (_, href, _, _) = by_props.or(by_meta).or(by_name).or(any_image)?;
    archive_entry(path, &join_href(&opf.dir, href))
}

fn epub_meta(path: &Path) -> Option<Meta> {
    let opf = load_opf(path)?;
    let x = &opf.xml;
    let mut meta = Meta { title: first_text(x, "title"), authors: all_texts(x, "creator"), publisher: first_text(x, "publisher"), language: first_text(x, "language"), date: first_text(x, "date"), description: first_text(x, "description"), ..Default::default() };
    for (a, _) in elements(x, "meta") {
        match attr(a, "name").unwrap_or_default().as_str() {
            "calibre:series" => meta.series = attr(a, "content").unwrap_or_default(),
            "calibre:series_index" => meta.series_index = attr(a, "content").unwrap_or_default().trim_end_matches(".0").to_string(),
            _ => {}
        }
    }
    if meta.series.is_empty() {
        if let Some((_, inner)) = elements(x, "meta").into_iter().find(|(a, _)| attr(a, "property").as_deref() == Some("belongs-to-collection")) {
            meta.series = decode_entities(inner.trim().to_string());
        }
    }
    for id in all_texts(x, "identifier") {
        let digits: String = id.chars().filter(|c| c.is_ascii_digit() || *c == 'X').collect();
        if id.to_lowercase().contains("isbn") || digits.len() == 13 || digits.len() == 10 {
            meta.isbn = digits;
            break;
        }
    }
    // Table of contents: EPUB 3 nav, else NCX, else spine with chapter titles.
    let nav = opf.manifest.iter().find(|(_, _, _, props)| props.split_whitespace().any(|p| p == "nav")).map(|(_, href, _, _)| join_href(&opf.dir, href));
    if let Some(nav_text) = nav.and_then(|n| archive_text(path, &n)) {
        for (a, inner) in elements(&nav_text, "a") {
            if let Some(href) = attr(a, "href") {
                let title = decode_entities(strip_tags(inner));
                if !title.is_empty() {
                    meta.chapters.push((title, join_href(&opf.dir, &href)));
                }
            }
        }
    }
    if meta.chapters.is_empty() {
        let ncx = opf.manifest.iter().find(|(_, _, mt, _)| mt == "application/x-dtbncx+xml").map(|(_, href, _, _)| join_href(&opf.dir, href));
        if let Some(ncx_text) = ncx.and_then(|n| archive_text(path, &n)) {
            for (_, inner) in elements(&ncx_text, "navpoint") {
                let title = first_text(inner, "text");
                let href = elements(inner, "content").into_iter().find_map(|(a, _)| attr(a, "src"));
                if let (false, Some(href)) = (title.is_empty(), href) {
                    meta.chapters.push((title, join_href(&opf.dir, &href)));
                }
            }
        }
    }
    if meta.chapters.is_empty() {
        for id in &opf.spine {
            if let Some((_, href, _, _)) = opf.manifest.iter().find(|(i, _, _, _)| i == id) {
                let full = join_href(&opf.dir, href);
                let title = archive_text(path, &full).map(|t| {
                    let h = first_text(&t, "h1");
                    if h.is_empty() { first_text(&t, "title") } else { h }
                }).unwrap_or_default();
                meta.chapters.push((if title.is_empty() { format!("{}", meta.chapters.len() + 1) } else { title }, full));
            }
        }
    }
    meta.chapters.dedup_by(|a, b| a.1 == b.1);
    Some(meta)
}

// ----- comics and FB2 ---------------------------------------------------------------------

fn is_image_name(n: &str) -> bool {
    let l = n.to_lowercase();
    [".jpg", ".jpeg", ".png", ".webp", ".gif", ".avif"].iter().any(|e| l.ends_with(e))
}

fn comic_pages(path: &Path) -> Vec<String> {
    let mut pages: Vec<String> = archive_list(path).into_iter().filter(|n| is_image_name(n) && !n.rsplit('/').next().unwrap_or("").starts_with('.')).collect();
    pages.sort_by_key(|a| natural_key(a));
    pages
}

fn natural_key(s: &str) -> Vec<(u8, u64, String)> {
    let mut key = Vec::new();
    let mut num = String::new();
    let mut text = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !text.is_empty() {
                key.push((1, 0, std::mem::take(&mut text).to_lowercase()));
            }
            num.push(c);
        } else {
            if !num.is_empty() {
                key.push((0, num.parse().unwrap_or(0), String::new()));
                num.clear();
            }
            text.push(c);
        }
    }
    if !num.is_empty() {
        key.push((0, num.parse().unwrap_or(0), String::new()));
    }
    if !text.is_empty() {
        key.push((1, 0, text.to_lowercase()));
    }
    key
}

fn fb2_cover(path: &Path) -> Option<Vec<u8>> {
    let xml = std::fs::read_to_string(path).ok()?;
    let cover_href = elements(&xml, "coverpage").into_iter().next().and_then(|(_, inner)| elements(inner, "image").into_iter().next()).and_then(|(a, _)| attr(a, "l:href").or_else(|| attr(a, "xlink:href")).or_else(|| attr(a, "href")));
    let id = cover_href.map(|h| h.trim_start_matches('#').to_string());
    let (_, inner) = elements(&xml, "binary").into_iter().find(|(a, _)| id.as_ref().map(|i| attr(a, "id").as_ref() == Some(i)).unwrap_or(true))?;
    Some(glib::base64_decode(&inner.split_whitespace().collect::<String>()).to_vec())
}

fn fb2_meta(path: &Path) -> Option<Meta> {
    let xml = std::fs::read_to_string(path).ok()?;
    let title_info = elements(&xml, "title-info").into_iter().next().map(|(_, i)| i.to_string()).unwrap_or_default();
    let authors = elements(&title_info, "author").into_iter().map(|(_, a)| [first_text(a, "first-name"), first_text(a, "middle-name"), first_text(a, "last-name")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")).filter(|s| !s.is_empty()).collect();
    let publish = elements(&xml, "publish-info").into_iter().next().map(|(_, i)| i.to_string()).unwrap_or_default();
    let chapters = elements(&xml, "section").into_iter().filter_map(|(_, inner)| { let t = first_text(inner, "title"); (!t.is_empty()).then_some((t, String::new())) }).collect();
    let series = elements(&title_info, "sequence").into_iter().next();
    Some(Meta {
        title: first_text(&title_info, "book-title"),
        authors,
        publisher: first_text(&publish, "publisher"),
        language: first_text(&title_info, "lang"),
        date: first_text(&publish, "year"),
        description: first_text(&title_info, "annotation"),
        series: series.as_ref().and_then(|(a, _)| attr(a, "name")).unwrap_or_default(),
        series_index: series.as_ref().and_then(|(a, _)| attr(a, "number")).unwrap_or_default(),
        isbn: first_text(&publish, "isbn"),
        chapters,
    })
}

// ----- public entry points ----------------------------------------------------------------

/// Raw bytes of the cover image, whatever the format.
pub fn cover(path: &Path, ct: &str) -> Option<Vec<u8>> {
    if ct == "application/epub+zip" {
        epub_cover(path)
    } else if is_comic_type(ct) {
        let first = comic_pages(path).into_iter().next()?;
        archive_entry(path, &first)
    } else if ct.contains("fictionbook") {
        fb2_cover(path)
    } else {
        None
    }
}

pub fn meta(path: &Path, ct: &str) -> Option<Meta> {
    if ct == "application/epub+zip" {
        epub_meta(path)
    } else if is_comic_type(ct) {
        let pages = comic_pages(path);
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        Some(Meta { title: stem, chapters: pages.iter().enumerate().map(|(i, p)| (format!("{}", i + 1), p.clone())).collect(), ..Default::default() })
    } else if ct.contains("fictionbook") {
        fb2_meta(path)
    } else {
        None
    }
}

/// Text of chapter `index` (0-based) for reading in Quick Look; comics return the page image instead.
pub enum Page {
    Text(String, String),
    Image(Vec<u8>),
}

pub fn page(path: &Path, ct: &str, meta: &Meta, index: usize) -> Option<Page> {
    let (title, href) = meta.chapters.get(index)?;
    if is_comic_type(ct) {
        return archive_entry(path, href).map(Page::Image);
    }
    if ct == "application/epub+zip" {
        let html = archive_text(path, href)?;
        let body = elements(&html, "body").into_iter().next().map(|(_, b)| b.to_string()).unwrap_or(html);
        return Some(Page::Text(title.clone(), decode_entities(strip_tags(&body))));
    }
    if ct.contains("fictionbook") {
        let xml = std::fs::read_to_string(path).ok()?;
        let (_, inner) = elements(&xml, "section").into_iter().nth(index)?;
        return Some(Page::Text(title.clone(), decode_entities(strip_tags(inner))));
    }
    None
}

/// `Author - Title.ext`, safe for a file name.
pub fn suggested_name(meta: &Meta, ext: &str) -> Option<String> {
    if meta.title.trim().is_empty() {
        return None;
    }
    let mut base = if meta.authors.is_empty() { meta.title.trim().to_string() } else { format!("{} - {}", meta.authors.join(", "), meta.title.trim()) };
    if !meta.series.is_empty() && !meta.series_index.is_empty() {
        base = format!("{base} ({} {})", meta.series, meta.series_index);
    }
    let cleaned: String = base.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0') { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned: String = cleaned.chars().take(180).collect();
    Some(if ext.is_empty() { cleaned } else { format!("{cleaned}.{ext}") })
}

/// Mounted eReaders: (name, folder to drop books into).
pub fn ereaders() -> Vec<(String, gio::File)> {
    let mut out = Vec::new();
    for mount in gio::VolumeMonitor::get().mounts() {
        let root = mount.root();
        let Some(path) = root.path() else { continue };
        let name = mount.name().to_string();
        let lname = name.to_lowercase();
        let kindle = path.join("documents").is_dir() && (path.join("system").is_dir() || lname.contains("kindle"));
        let kobo = path.join(".kobo").is_dir() || lname.contains("kobo");
        let other = ["tolino", "pocketbook", "boox", "remarkable", "nook", "sony reader"].iter().any(|k| lname.contains(k));
        if kindle {
            out.push((name, gio::File::for_path(path.join("documents"))));
        } else if kobo || other {
            out.push((name, root));
        }
    }
    out
}

pub fn calibre_available() -> bool {
    glib::find_program_in_path("ebook-convert").is_some()
}

/// A dedicated eBook reader on this system, if any.
pub fn reader() -> Option<&'static str> {
    ["foliate", "ebook-viewer", "koreader", "okular"].into_iter().find(|r| glib::find_program_in_path(r).is_some())
}

pub fn output_path(input: &Path, fmt: &str) -> PathBuf {
    input.with_extension(fmt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_helpers() {
        let xml = r#"<package><metadata xmlns:dc="x"><dc:title>Das &amp; Haus</dc:title><dc:creator>A</dc:creator><dc:creator>B</dc:creator><meta name="cover" content="c1"/></metadata><manifest><item id="c1" href="img/c.jpg" media-type="image/jpeg"/><itemref idref="x"/></manifest></package>"#;
        assert_eq!(first_text(xml, "title"), "Das & Haus");
        assert_eq!(all_texts(xml, "creator"), vec!["A", "B"]);
        let items = elements(xml, "item");
        assert_eq!(items.len(), 1, "itemref must not match item");
        assert_eq!(attr(items[0].0, "href").as_deref(), Some("img/c.jpg"));
        assert_eq!(join_href("OEBPS", "../img/c.jpg"), "img/c.jpg");
        assert_eq!(join_href("OEBPS/text", "ch1.xhtml#p2"), "OEBPS/text/ch1.xhtml");
    }

    #[test]
    fn strips_html_to_paragraphs() {
        let html = "<html><head><title>T</title><style>p{}</style></head><body><h1>Kap</h1><p>Eins &amp; zwei.</p><p>Drei<br/>vier</p></body></html>";
        assert_eq!(decode_entities(strip_tags(html)), "Kap\n\nEins & zwei.\n\nDrei\nvier");
    }

    #[test]
    fn suggests_file_names() {
        let m = Meta { title: "Das Haus: am Fjord?".into(), authors: vec!["Nora Lindqvist".into()], series: "Fjord".into(), series_index: "1".into(), ..Default::default() };
        assert_eq!(suggested_name(&m, "epub").as_deref(), Some("Nora Lindqvist - Das Haus am Fjord (Fjord 1).epub"));
        assert!(suggested_name(&Meta::default(), "epub").is_none());
    }

    #[test]
    fn natural_page_order() {
        let mut pages = vec!["p10.png".to_string(), "p2.png".into(), "p1.png".into()];
        pages.sort_by_key(|a| natural_key(a));
        assert_eq!(pages, vec!["p1.png", "p2.png", "p10.png"]);
    }
}
