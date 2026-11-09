//! Group keys for "Use Groups": kind, date, size, tag. `rank` orders the
//! sections, `label` names them.

use crate::item::FileItem;
use crate::tags::TagIndex;
use gtk::glib;

fn kind_rank(item: &FileItem) -> (u8, &'static str) {
    if item.is_dir() {
        return (0, "Folders");
    }
    if item.is_image() {
        return (1, "Images");
    }
    if item.is_pdf() {
        return (2, "PDF documents");
    }
    if item.is_ebook() {
        return (3, "eBooks");
    }
    if item.is_audio() {
        return (6, "Music");
    }
    if item.is_video() {
        return (7, "Videos");
    }
    if item.is_archive() {
        return (8, "Archives");
    }
    let ct = item.content_type().unwrap_or_default();
    let name = item.name().to_lowercase();
    let code = ["rs", "py", "js", "ts", "tsx", "jsx", "go", "c", "h", "cpp", "hpp", "java", "kt", "rb", "sh", "fish", "lua", "toml", "yaml", "yml", "json", "css", "html", "qml", "nix", "conf"];
    if code.iter().any(|e| name.ends_with(&format!(".{e}"))) || ct.contains("script") || ct.contains("x-") && item.is_text() {
        return (5, "Code and config");
    }
    if item.is_text() || ct.contains("document") || ct.contains("opendocument") || ct.contains("msword") || ct.contains("spreadsheet") || ct.contains("presentation") {
        return (4, "Documents");
    }
    (9, "Other")
}

fn date_rank(item: &FileItem) -> (u8, &'static str) {
    let now = glib::DateTime::now_local().ok();
    let Some(m) = item.modified() else { return (6, "Unknown date") };
    let Some(now) = now else { return (6, "Unknown date") };
    let days = (now.to_unix() - m.to_unix()) / 86_400;
    let same_day = now.day_of_year() == m.day_of_year() && now.year() == m.year();
    if same_day {
        (0, "Today")
    } else if days < 2 {
        (1, "Yesterday")
    } else if days < 7 {
        (2, "Previous 7 days")
    } else if days < 30 {
        (3, "Previous 30 days")
    } else if now.year() == m.year() {
        (4, "Earlier this year")
    } else {
        (5, "Older")
    }
}

fn size_rank(item: &FileItem) -> (u8, &'static str) {
    if item.is_dir() {
        return (0, "Folders");
    }
    match item.size() {
        0 => (1, "Empty"),
        s if s < 100 * 1024 => (2, "Small (under 100 kB)"),
        s if s < 10 * 1024 * 1024 => (3, "Medium (under 10 MB)"),
        s if s < 1024 * 1024 * 1024 => (4, "Large (under 1 GB)"),
        _ => (5, "Huge (1 GB and more)"),
    }
}

fn tag_rank(item: &FileItem, tags: Option<&TagIndex>) -> (u8, String) {
    match tags.and_then(|t| t.get(&item.file())) {
        Some(color) => {
            let idx = crate::tags::COLORS.iter().position(|(n, _, _)| *n == color).unwrap_or(7) as u8;
            (idx, crate::tags::label(&color))
        }
        None => (9, "No tag".into()),
    }
}

/// Sort key for the section sorter.
pub fn rank(key: &str, item: &FileItem, tags: Option<&TagIndex>) -> (u8, String) {
    match key {
        "kind" => {
            let (r, l) = kind_rank(item);
            (r, l.into())
        }
        "date" => {
            let (r, l) = date_rank(item);
            (r, l.into())
        }
        "size" => {
            let (r, l) = size_rank(item);
            (r, l.into())
        }
        "tag" => tag_rank(item, tags),
        _ => (0, String::new()),
    }
}

pub fn label(key: &str, item: &FileItem, tags: Option<&TagIndex>) -> String {
    rank(key, item, tags).1
}
