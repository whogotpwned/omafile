//! `FileItem`: a GObject wrapping a `gio::File` plus its queried `gio::FileInfo`,
//! so it can live in a `gio::ListStore` behind the list and grid views.

use gtk::glib;
use gtk::subclass::prelude::*;
use gtk::{gio, pango};
use std::cell::RefCell;

pub const ATTRIBUTES: &str = "standard::name,standard::display-name,standard::type,standard::size,\
standard::icon,standard::is-hidden,standard::is-backup,standard::is-symlink,standard::symlink-target,\
standard::content-type,standard::fast-content-type,time::modified,unix::mode,access::can-write,\
access::can-execute,owner::user,owner::group,standard::target-uri,thumbnail::path,thumbnail::is-valid";

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct FileItem {
        pub file: RefCell<Option<gio::File>>,
        pub info: RefCell<Option<gio::FileInfo>>,
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
        self.info().size().max(0) as u64
    }

    pub fn modified(&self) -> Option<glib::DateTime> {
        self.info().modification_date_time()
    }

    pub fn modified_unix(&self) -> i64 {
        self.modified().map(|d| d.to_unix()).unwrap_or(0)
    }

    pub fn icon(&self) -> Option<gio::Icon> {
        self.info().icon()
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
            return "Link to folder".into();
        }
        match self.content_type() {
            Some(ct) => gio::content_type_get_description(&ct).to_string(),
            None if self.is_dir() => "Folder".into(),
            None => "Unknown".into(),
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
        self.info().symlink_target().map(|p| p.to_string_lossy().to_string())
    }

    /// Sort key: directories first, then case-insensitive natural-ish name order.
    pub fn sort_key(&self) -> (bool, glib::GString) {
        (!self.is_dir(), glib::GString::from(self.display_name()).to_lowercase().into())
    }

    pub fn weight(&self) -> pango::Weight {
        if self.is_dir() { pango::Weight::Medium } else { pango::Weight::Normal }
    }
}
