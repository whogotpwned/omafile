//! `FileItem`: a GObject wrapping a `gio::File` plus its queried `gio::FileInfo`,
//! so it can live in a `gio::ListStore` behind the list and grid views.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, pango};
use std::cell::RefCell;

pub const ATTRIBUTES: &str = "standard::name,standard::display-name,standard::type,standard::size,\
standard::icon,standard::is-hidden,standard::is-backup,standard::is-symlink,standard::symlink-target,\
standard::content-type,standard::fast-content-type,time::modified,unix::mode,access::can-write,\
access::can-execute,owner::user,owner::group,standard::target-uri,thumbnail::path,thumbnail::is-valid,\
metadata::custom-icon,metadata::custom-icon-name,trash::deletion-date,trash::orig-path,unix::uid,unix::gid";

/// File names that usually hold credentials. They get a warning badge, a masked
/// preview and a confirmation before they are copied.
pub fn looks_secret(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let exact = [".env", ".netrc", ".npmrc", ".pypirc", ".htpasswd", ".git-credentials", ".pgpass", ".my.cnf", "credentials", "credentials.json", "secrets.yaml", "secrets.yml", "secrets.json", "secrets.toml", "id_rsa", "id_dsa", "id_ecdsa", "id_ed25519", "shadow", "wp-config.php", ".docker/config.json", "token", ".token", "master.key", "service-account.json", "kubeconfig"];
    if exact.contains(&lower.as_str()) {
        return true;
    }
    if lower.starts_with(".env.") && !lower.ends_with(".example") && !lower.ends_with(".sample") && !lower.ends_with(".template") {
        return true;
    }
    if lower.starts_with("id_") && lower.ends_with(".pub") {
        return false;
    }
    let suffixes = [".pem", ".key", ".p12", ".pfx", ".kdbx", ".keystore", ".jks", ".asc", ".gpg", ".secret", ".ppk", ".ovpn", ".crt.key", ".token"];
    suffixes.iter().any(|s| lower.ends_with(s)) || lower.starts_with("secret") && !lower.ends_with(".example")
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct FileItem {
        pub file: RefCell<Option<gio::File>>,
        pub info: RefCell<Option<gio::FileInfo>>,
        pub label: RefCell<Option<String>>,
        /// Computed recursive size for folders (Properties, "Show Folder Sizes").
        pub dir_size: std::cell::Cell<Option<u64>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FileItem {
        const NAME: &'static str = "OmafileFileItem";
        type Type = super::FileItem;
    }

    impl ObjectImpl for FileItem {}
}

glib::wrapper! {
    pub struct FileItem(ObjectSubclass<imp::FileItem>);
}

impl FileItem {
    pub fn new(file: gio::File, info: gio::FileInfo) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().file.replace(Some(file));
        obj.imp().info.replace(Some(info));
        obj
    }

    /// Label shown instead of the name, e.g. a relative path in search results.
    pub fn set_label(&self, label: Option<String>) {
        self.imp().label.replace(label);
    }

    pub fn display_label(&self) -> String {
        self.imp().label.borrow().clone().unwrap_or_else(|| self.display_name())
    }

    pub fn file(&self) -> gio::File {
        self.imp().file.borrow().clone().expect("FileItem without file")
    }

    pub fn info(&self) -> gio::FileInfo {
        self.imp().info.borrow().clone().expect("FileItem without info")
    }

    pub fn name(&self) -> String {
        self.info().name().to_string_lossy().to_string()
    }

    pub fn display_name(&self) -> String {
        let info = self.info();
        let name = info.display_name();
        if name.is_empty() {
            self.name()
        } else {
            name.to_string()
        }
    }

    pub fn file_type(&self) -> gio::FileType {
        self.info().file_type()
    }

    pub fn is_dir(&self) -> bool {
        self.file_type() == gio::FileType::Directory
    }

    pub fn is_symlink(&self) -> bool {
        let info = self.info();
        // Remote backends (sftp, ...) do not always provide these attributes.
        info.has_attribute(gio::FILE_ATTRIBUTE_STANDARD_IS_SYMLINK) && info.is_symlink()
    }

    pub fn is_hidden(&self) -> bool {
        let info = self.info();
        let hidden = if info.has_attribute(gio::FILE_ATTRIBUTE_STANDARD_IS_HIDDEN) { info.is_hidden() } else { self.name().starts_with('.') };
        let backup = if info.has_attribute(gio::FILE_ATTRIBUTE_STANDARD_IS_BACKUP) { info.is_backup() } else { self.name().ends_with('~') };
        hidden || backup
    }

    pub fn is_executable(&self) -> bool {
        !self.is_dir() && self.info().boolean(gio::FILE_ATTRIBUTE_ACCESS_CAN_EXECUTE)
    }

    pub fn size(&self) -> u64 {
        if let Some(d) = self.imp().dir_size.get() {
            return d;
        }
        self.info().size().max(0) as u64
    }

    pub fn dir_size(&self) -> Option<u64> {
        self.imp().dir_size.get()
    }

    pub fn set_dir_size(&self, size: Option<u64>) {
        self.imp().dir_size.set(size);
    }

    pub fn is_secret(&self) -> bool {
        !self.is_dir() && looks_secret(&self.name())
    }

    /// `trash:///` entries: when the item was deleted.
    pub fn trash_deleted(&self) -> Option<glib::DateTime> {
        let s = self.info().attribute_string("trash::deletion-date")?;
        glib::DateTime::from_iso8601(&s, Some(&glib::TimeZone::local())).ok()
    }

    /// `trash:///` entries: where the item used to live.
    pub fn trash_origin(&self) -> Option<String> {
        self.info().attribute_byte_string("trash::orig-path").map(|p| p.to_string())
    }

    pub fn modified(&self) -> Option<glib::DateTime> {
        self.info().modification_date_time()
    }

    pub fn modified_unix(&self) -> i64 {
        self.modified().map(|d| d.to_unix()).unwrap_or(0)
    }

    /// Custom icons set by omafile or Nautilus take precedence over the type icon.
    pub fn icon(&self) -> Option<gio::Icon> {
        let info = self.info();
        if let Some(name) = info.attribute_string("metadata::custom-icon-name") {
            if !name.is_empty() {
                return Some(gio::ThemedIcon::new(&name).upcast());
            }
        }
        if let Some(uri) = info.attribute_string("metadata::custom-icon") {
            if !uri.is_empty() {
                return Some(gio::FileIcon::new(&gio::File::for_uri(&uri)).upcast());
            }
        }
        info.icon()
    }

    pub fn content_type(&self) -> Option<String> {
        let info = self.info();
        info.attribute_string(gio::FILE_ATTRIBUTE_STANDARD_CONTENT_TYPE)
            .or_else(|| info.attribute_string(gio::FILE_ATTRIBUTE_STANDARD_FAST_CONTENT_TYPE))
            .map(|s| s.to_string())
            .or_else(|| {
                // Remote backends may omit the content type; guess from the name.
                let (guess, uncertain) = gio::content_type_guess(Some(std::path::Path::new(&self.name())), None);
                if uncertain && self.is_dir() { Some("inode/directory".into()) } else { Some(guess.to_string()) }
            })
    }

    pub fn type_description(&self) -> String {
        if self.is_symlink() && self.is_dir() {
            return crate::i18n::tr("Link to folder");
        }
        match self.content_type() {
            Some(ct) => gio::content_type_get_description(&ct).to_string(),
            None if self.is_dir() => crate::i18n::tr("Folder"),
            None => crate::i18n::tr("Unknown"),
        }
    }

    /// For `recent:///` and `trash:///` entries: the real file behind the item.
    pub fn target_file(&self) -> gio::File {
        match self.info().attribute_string(gio::FILE_ATTRIBUTE_STANDARD_TARGET_URI) {
            Some(uri) if !uri.is_empty() => gio::File::for_uri(&uri),
            _ => self.file(),
        }
    }

    pub fn is_image(&self) -> bool {
        self.content_type().map(|ct| ct.starts_with("image/")).unwrap_or(false)
    }

    pub fn is_audio(&self) -> bool {
        media_kind(self.content_type().as_deref(), &self.name()) == Some(Media::Audio)
    }

    pub fn is_video(&self) -> bool {
        media_kind(self.content_type().as_deref(), &self.name()) == Some(Media::Video)
    }

    pub fn is_media(&self) -> bool {
        self.is_audio() || self.is_video()
    }

    pub fn is_pdf(&self) -> bool {
        self.content_type().as_deref() == Some("application/pdf")
    }

    /// EPUB, comic archive, FictionBook, Kindle formats, DjVu.
    pub fn is_ebook(&self) -> bool {
        self.content_type().map(|ct| crate::ebook::is_ebook_type(&ct)).unwrap_or(false)
    }

    /// An eBook whose cover and chapters we can read (EPUB, CBZ / CBR, FB2).
    pub fn is_readable_ebook(&self) -> bool {
        self.content_type().map(|ct| ct == "application/epub+zip" || crate::ebook::is_comic_type(&ct) || ct.contains("fictionbook")).unwrap_or(false)
    }

    pub fn is_text(&self) -> bool {
        self.content_type()
            .map(|ct| ct.starts_with("text/") || gio::content_type_is_a(&ct, "text/plain") || matches!(ct.as_str(), "application/json" | "application/xml" | "application/x-shellscript" | "application/toml" | "application/x-yaml" | "application/javascript"))
            .unwrap_or(false)
    }

    pub fn is_archive(&self) -> bool {
        let Some(ct) = self.content_type() else { return false };
        matches!(
            ct.as_str(),
            "application/zip" | "application/x-tar" | "application/gzip" | "application/x-xz" | "application/zstd"
                | "application/x-bzip2" | "application/x-7z-compressed" | "application/vnd.rar" | "application/x-compressed-tar"
                | "application/x-xz-compressed-tar" | "application/x-zstd-compressed-tar" | "application/x-bzip-compressed-tar"
                | "application/x-lzma-compressed-tar" | "application/x-cpio" | "application/x-iso9660-image"
        ) || gio::content_type_is_a(&ct, "application/x-archive")
    }

    /// Existing freedesktop thumbnail, if the system already made one.
    pub fn thumbnail_path(&self) -> Option<std::path::PathBuf> {
        let info = self.info();
        if !info.boolean(gio::FILE_ATTRIBUTE_THUMBNAIL_IS_VALID) {
            return None;
        }
        info.attribute_byte_string(gio::FILE_ATTRIBUTE_THUMBNAIL_PATH).map(|p| std::path::PathBuf::from(p.to_string()))
    }

    pub fn mode(&self) -> u32 {
        self.info().attribute_uint32(gio::FILE_ATTRIBUTE_UNIX_MODE)
    }

    pub fn owner(&self) -> String {
        let info = self.info();
        let user = info.attribute_string(gio::FILE_ATTRIBUTE_OWNER_USER).map(|s| s.to_string()).unwrap_or_default();
        let group = info.attribute_string(gio::FILE_ATTRIBUTE_OWNER_GROUP).map(|s| s.to_string()).unwrap_or_default();
        if group.is_empty() { user } else { format!("{user}:{group}") }
    }

    pub fn symlink_target(&self) -> Option<String> {
        let info = self.info();
        if !info.has_attribute(gio::FILE_ATTRIBUTE_STANDARD_SYMLINK_TARGET) {
            return None;
        }
        info.symlink_target().map(|p| p.to_string_lossy().to_string())
    }

    /// Sort key: directories first, then case-insensitive natural-ish name order.
    pub fn sort_key(&self) -> (bool, glib::GString) {
        (!self.is_dir(), glib::GString::from(self.display_name()).to_lowercase().into())
    }

    pub fn weight(&self) -> pango::Weight {
        if self.is_dir() { pango::Weight::Medium } else { pango::Weight::Normal }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Media {
    Audio,
    Video,
}

const AUDIO_ENDINGS: &[&str] = &["flac", "mp3", "m4a", "m4b", "aac", "ogg", "oga", "opus", "wav", "wave", "wma", "aiff", "aif", "ape", "wv", "mka", "alac", "dsf", "dff", "tta", "mpc", "ac3", "dts", "amr", "mid", "midi", "spx", "caf"];
const VIDEO_ENDINGS: &[&str] = &["mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "flv", "mpg", "mpeg", "m2ts", "mts", "ts", "ogv", "3gp", "3g2", "vob", "divx", "asf", "rmvb"];

/// Whether a file is a song or a video. The type the system names decides when it is one of
/// the two; when it is not (a FLAC that the type database calls `application/x-flac` or
/// plain data, a file on a share that reports no type), the name's ending does. Without
/// this such a file has no play button, though the player would play it.
pub fn media_kind(content_type: Option<&str>, name: &str) -> Option<Media> {
    let kind = content_type.unwrap_or("");
    if kind.starts_with("audio/") {
        return Some(Media::Audio);
    }
    if kind.starts_with("video/") {
        return Some(Media::Video);
    }
    // A type that says what it is and is neither: a folder, a picture, a text, a document.
    if kind.starts_with("inode/directory") || kind.starts_with("image/") || kind.starts_with("text/") || kind == "application/pdf" {
        return None;
    }
    let ending = name.rsplit_once('.').map(|(_, e)| e.to_lowercase())?;
    if AUDIO_ENDINGS.contains(&ending.as_str()) {
        Some(Media::Audio)
    } else if VIDEO_ENDINGS.contains(&ending.as_str()) && ending != "ts" {
        // (.ts is TypeScript far more often than a transport stream; that one needs its type.)
        Some(Media::Video)
    } else {
        None
    }
}

#[cfg(test)]
mod media_tests {
    use super::*;

    #[test]
    fn a_song_is_a_song_whatever_the_type_database_calls_it() {
        assert_eq!(media_kind(Some("audio/flac"), "a.flac"), Some(Media::Audio));
        assert_eq!(media_kind(Some("audio/mpeg"), "a.mp3"), Some(Media::Audio));
        assert_eq!(media_kind(Some("video/mp4"), "a.mp4"), Some(Media::Video));
        // The cases that had no play button.
        assert_eq!(media_kind(Some("application/x-flac"), "01 - Intro.flac"), Some(Media::Audio));
        assert_eq!(media_kind(Some("application/octet-stream"), "Live Set.FLAC"), Some(Media::Audio));
        assert_eq!(media_kind(None, "song.opus"), Some(Media::Audio));
        assert_eq!(media_kind(Some("application/ogg"), "talk.ogg"), Some(Media::Audio));
        assert_eq!(media_kind(Some("application/x-matroska"), "film.mkv"), Some(Media::Video));
        assert_eq!(media_kind(Some("inode/symlink"), "link to song.m4a"), Some(Media::Audio));
        // The type wins where it knows better.
        assert_eq!(media_kind(Some("video/mp2t"), "recording.ts"), Some(Media::Video));
        assert_eq!(media_kind(Some("text/vnd.trolltech.linguist"), "app.ts"), None, "TypeScript");
        assert_eq!(media_kind(Some("application/octet-stream"), "app.ts"), None);
        assert_eq!(media_kind(Some("inode/directory"), "Albums.flac"), None, "a folder is a folder");
        assert_eq!(media_kind(Some("image/png"), "cover.mp3.png"), None);
        assert_eq!(media_kind(Some("application/zip"), "archive.zip"), None);
        assert_eq!(media_kind(Some("application/octet-stream"), "README"), None);
    }
}
