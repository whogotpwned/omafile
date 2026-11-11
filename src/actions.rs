//! Window actions: navigation, tabs, clipboard, file operations and dialogs.

use crate::i18n::tr;
use crate::item::FileItem;
use crate::ops;
use crate::pane::Pane;
use crate::remote;
use crate::undo::Op;
use crate::window::{Clipboard, PlaceKind, Window};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::rc::Rc;

type Handler = Box<dyn Fn(&Rc<Window>)>;

/// Actions offered by the command palette: (action name, label, icon).
pub const PALETTE_ACTIONS: &[(&str, &str, &str)] = &[
    ("new-folder", "New Folder", "folder-new-symbolic"),
    ("new-file", "New File", "document-new-symbolic"),
    ("vault-new", "New Encrypted Folder", "changes-prevent-symbolic"),
    ("new-tab", "New Tab", "tab-new-symbolic"),
    ("split-view", "Toggle Split View", "view-dual-symbolic"),
    ("show-preview", "Toggle Preview Pane", "sidebar-show-right-symbolic"),
    ("show-sidebar", "Toggle Sidebar", "sidebar-show-symbolic"),
    ("show-hidden", "Toggle Hidden Files", "view-reveal-symbolic"),
    ("toggle-view", "Toggle List / Grid", "view-grid-symbolic"),
    ("quick-look", "Quick Look", "view-reveal-symbolic"),
    ("play", "Play (audio / video)", "media-playback-start-symbolic"),
    ("search", "Filter This Folder", "edit-find-symbolic"),
    ("deep-search", "Search Subfolders", "folder-saved-search-symbolic"),
    ("location", "Edit Path", "folder-symbolic"),
    ("terminal-panel", "Toggle Terminal Panel", "utilities-terminal-symbolic"),
    ("terminal", "Open Terminal Window Here", "utilities-terminal-symbolic"),
    ("open-editor-cwd", "Open Folder in Editor", "text-editor-symbolic"),
    ("open-with", "Open With…", "document-open-symbolic"),
    ("rename", "Rename", "document-edit-symbolic"),
    ("duplicate", "Duplicate", "edit-copy-symbolic"),
    ("copy-to", "Copy To…", "edit-copy-symbolic"),
    ("move-to", "Move To…", "edit-cut-symbolic"),
    ("compress", "Compress…", "package-x-generic-symbolic"),
    ("extract", "Extract Here", "package-x-generic-symbolic"),
    ("change-icon", "Change Icon…", "image-x-generic-symbolic"),
    ("bookmark", "Bookmark This Folder", "user-bookmarks-symbolic"),
    ("connect", "Connect to Server…", "network-server-symbolic"),
    ("trash", "Move to Trash", "user-trash-symbolic"),
    ("empty-trash", "Empty Trash", "user-trash-full-symbolic"),
    ("undo", "Undo", "edit-undo-symbolic"),
    ("redo", "Redo", "edit-redo-symbolic"),
    ("properties", "Properties", "dialog-information-symbolic"),
    ("cwd-properties", "Folder Properties", "dialog-information-symbolic"),
    ("preferences", "Preferences", "emblem-system-symbolic"),
    ("shortcuts", "Keyboard Shortcuts", "input-keyboard-symbolic"),
    ("help", "Help: Features, Shortcuts, Version", "help-browser-symbolic"),
    ("home", "Go Home", "user-home-symbolic"),
    ("up", "Parent Folder", "go-up-symbolic"),
    ("reload", "Reload", "view-refresh-symbolic"),
    ("folder-sizes", "Show Folder Sizes (disk usage)", "drive-harddisk-symbolic"),
    ("treemap", "Disk Usage Map", "drive-harddisk-symbolic"),
    ("find-duplicates", "Find Duplicates", "edit-copy-symbolic"),
    ("rules", "Folder Rules", "emblem-system-symbolic"),
    ("shelf-add", "Add to Shelf", "folder-open-symbolic"),
    ("shelf-show", "Show Shelf", "folder-open-symbolic"),
    ("nearby", "Nearby Devices (LocalSend)", "phone-symbolic"),
    ("now-playing", "Now Playing (media controls)", "audio-x-generic-symbolic"),
    ("import-photos", "Import Photos from iPhone", "camera-photo-symbolic"),
    ("ebook-rename", "eBook: Rename from Metadata", "x-office-address-book-symbolic"),
    ("ebook-send", "eBook: Send to eReader", "send-to-symbolic"),
    ("ebook-reader", "eBook: Open in Reader", "x-office-address-book-symbolic"),
    ("restore", "Restore from Trash", "edit-undo-symbolic"),
    ("git-commit", "Git: Commit…", "vcs-commit-symbolic"),
    ("git-branch", "Git: Switch Branch…", "vcs-branch-symbolic"),
    ("git-stage", "Git: Stage Selection", "list-add-symbolic"),
    ("git-unstage", "Git: Unstage Selection", "list-remove-symbolic"),
    ("git-discard", "Git: Discard Changes…", "edit-clear-symbolic"),
    ("share-scp", "Share: Copy scp Command", "network-transmit-symbolic"),
    ("share-lan", "Share on Local Network…", "network-wireless-symbolic"),
    ("set-wallpaper", "Set as Wallpaper", "preferences-desktop-wallpaper-symbolic"),
    ("apply-theme", "Apply Omarchy Theme", "applications-graphics-symbolic"),
    ("image-resize", "Image: Resize…", "image-x-generic-symbolic"),
    ("image-rotate-left", "Image: Rotate Left", "object-rotate-left-symbolic"),
    ("image-rotate-right", "Image: Rotate Right", "object-rotate-right-symbolic"),
    ("image-to-png", "Image: Convert to PNG", "image-x-generic-symbolic"),
    ("image-to-jpg", "Image: Convert to JPEG", "image-x-generic-symbolic"),
    ("image-to-webp", "Image: Convert to WebP", "image-x-generic-symbolic"),
    ("vault-lock-all", "Lock All Encrypted Folders", "changes-prevent-symbolic"),
    ("vault-add", "Add Vault (encrypted folder in the sidebar)", "changes-prevent-symbolic"),
    ("versions", "Earlier Versions (snapshots)", "document-open-recent-symbolic"),
    ("backup", "Backup: Status, History and Settings", "document-open-recent-symbolic"),
    ("backup-now", "Back Up Now", "document-open-recent-symbolic"),
    ("feeds", "Feeds: Read the News", "application-rss+xml-symbolic"),
    ("feed-add", "Add Feed (RSS / Atom)", "application-rss+xml-symbolic"),
    ("new-folder-with-selection", "New Folder with Selection", "folder-new-symbolic"),
    ("make-alias", "Make Alias (symbolic link)", "insert-link-symbolic"),
    ("show-original", "Show Original of Alias", "go-jump-symbolic"),
    ("always-open-with", "Always Open With…", "document-open-symbolic"),
    ("pdf-from-images", "PDF: Create from Selected Images", "x-office-document-symbolic"),
    ("pdf-merge", "PDF: Merge Selected", "x-office-document-symbolic"),
    ("pdf-split", "PDF: Split into Pages", "x-office-document-symbolic"),
    ("pdf-extract", "PDF: Extract Pages…", "x-office-document-symbolic"),
    ("video-trim", "Video: Trim…", "video-x-generic-symbolic"),
    ("video-gif", "Video: Make GIF", "video-x-generic-symbolic"),
    ("video-audio", "Video: Extract Audio", "audio-x-generic-symbolic"),
    ("audio-mp3", "Audio: Convert to MP3", "audio-x-generic-symbolic"),
    ("audio-opus", "Audio: Convert to Opus", "audio-x-generic-symbolic"),
    ("audio-flac", "Audio: Convert to FLAC", "audio-x-generic-symbolic"),
    ("view::columns", "Column View", "view-dual-symbolic"),
    ("view::gallery", "Gallery View", "view-paged-symbolic"),
    ("group::kind", "Group by Kind", "view-list-symbolic"),
    ("group::date", "Group by Date", "view-list-symbolic"),
    ("group::size", "Group by Size", "view-list-symbolic"),
    ("group::tag", "Group by Tag", "view-list-symbolic"),
    ("group::none", "Group: None", "view-list-symbolic"),
    ("save-smart", "Save Search as Smart Folder…", "folder-saved-search-symbolic"),
    ("copy-other", "Copy to Other Pane", "edit-copy-symbolic"),
    ("move-other", "Move to Other Pane", "edit-cut-symbolic"),
    ("compare", "Compare…", "view-dual-symbolic"),
    ("verify-checksum", "Verify Checksum (SHA-256)", "security-high-symbolic"),
    ("edit-note", "Edit Note…", "user-available-symbolic"),
    ("open-templates", "Open Templates Folder", "folder-templates-symbolic"),
    ("browse-archive", "Browse Archive", "package-x-generic-symbolic"),
    ("open-workspace", "Open in New Hyprland Workspace", "view-grid-symbolic"),
];

pub const ACCELS: &[(&str, &[&str])] = &[
    ("win.back", &["<Alt>Left"]),
    ("win.forward", &["<Alt>Right"]),
    ("win.up", &["<Alt>Up"]),
    ("win.home", &["<Alt>Home"]),
    ("win.reload", &["F5", "<Control>r"]),
    ("win.show-hidden", &["<Control>h"]),
    ("win.show-sidebar", &["F9"]),
    ("win.show-preview", &["F3"]),
    ("win.search", &["<Control>f"]),
    ("win.location", &["<Control>l"]),
    ("win.view::list", &["<Control>1"]),
    ("win.view::grid", &["<Control>2"]),
    ("win.view::columns", &["<Control>3"]),
    ("win.view::gallery", &["<Control>4"]),
    ("win.zoom-in", &["<Control>plus", "<Control>equal", "<Control>KP_Add"]),
    ("win.zoom-out", &["<Control>minus", "<Control>KP_Subtract"]),
    ("win.zoom-reset", &["<Control>0", "<Control>KP_0"]),
    ("win.open", &["<Control>o", "<Control>Down"]),
    ("win.up-alt", &["<Control>Up"]),
    ("win.new-folder-with-selection", &["<Control><Shift>m"]),
    ("win.new-tab", &["<Control>t"]),
    ("win.close-tab", &["<Control>w"]),
    ("win.next-tab", &["<Control>Tab", "<Control>Page_Down"]),
    ("win.prev-tab", &["<Control><Shift>Tab", "<Control>Page_Up"]),
    ("win.new-folder", &["<Control><Shift>n"]),
    ("win.new-file", &["<Control><Alt>n"]),
    ("win.rename", &["F2"]),
    ("win.rename-dialog", &["<Shift>F2"]),
    ("win.restore", &["<Control><Shift>r"]),
    ("win.folder-sizes", &["<Control><Shift>u"]),
    ("win.treemap", &["<Control><Alt>u"]),
    ("win.find-duplicates", &["<Control><Alt>f"]),
    ("win.rules", &["<Control><Alt>r"]),
    ("win.shelf-add", &["<Control><Shift>s"]),
    ("win.shelf-show", &["<Control><Shift>b"]),
    ("win.nearby", &["<Control><Alt>p"]),
    ("win.feeds", &["<Control><Alt>w"]),
    ("win.versions", &["<Control><Alt>h"]),
    ("win.backup", &["<Control><Alt>k"]),
    ("win.now-playing", &["F8"]),
    ("win.git-commit", &["<Control><Alt>c"]),
    ("win.git-branch", &["<Control><Alt>b"]),
    ("win.delete", &["<Shift>Delete"]),
    ("win.copy", &["<Control>c"]),
    ("win.cut", &["<Control>x"]),
    ("win.paste", &["<Control>v"]),
    ("win.select-all", &["<Control>a"]),
    ("win.copy-path", &["<Control><Shift>c"]),
    ("win.terminal", &["F4", "<Control><Alt>t"]),
    ("win.properties", &["<Control>i", "<Alt>Return"]),
    ("win.open-with", &["<Control><Shift>o"]),
    ("win.open-new-tab", &["<Control>Return"]),
    ("win.bookmark", &["<Control>d"]),
    ("win.connect", &["<Control><Shift>k"]),
    ("win.split-view", &["F6"]),
    ("win.undo", &["<Control>z"]),
    ("win.palette", &["<Control>k", "<Control><Shift>p"]),
    ("win.terminal-panel", &["<Control>grave", "F7"]),
    ("win.open-editor", &["<Control>e"]),
    ("win.open-editor-cwd", &["<Control><Shift>e"]),
    ("win.redo", &["<Control><Shift>z", "<Control>y"]),
    ("win.duplicate", &["<Control><Shift>d"]),
    ("win.deep-search", &["<Control><Shift>f"]),
    ("win.preferences", &["<Control>comma"]),
    ("win.change-icon", &["<Control><Alt>i"]),
    ("win.tag::red", &["<Control><Alt>1"]),
    ("win.tag::orange", &["<Control><Alt>2"]),
    ("win.tag::yellow", &["<Control><Alt>3"]),
    ("win.tag::green", &["<Control><Alt>4"]),
    ("win.tag::blue", &["<Control><Alt>5"]),
    ("win.tag::purple", &["<Control><Alt>6"]),
    ("win.tag::gray", &["<Control><Alt>7"]),
    ("win.tag::none", &["<Control><Alt>0"]),
    ("win.shortcuts", &["<Control>question"]),
    ("win.help", &["F1"]),
    ("win.copy-other", &["<Shift>F5"]),
    ("win.move-other", &["<Shift>F6"]),
    ("win.compare", &["<Control><Alt>d"]),
    ("win.edit-note", &["<Control><Alt>m"]),
    ("win.save-smart", &["<Control><Alt>s"]),
    ("win.py-activate", &["<Control><Alt>v"]),
];

impl Window {
    pub fn setup_actions(self: &Rc<Self>) {
        let trace = std::env::var_os("OMAFILE_TRACE").is_some();
        let simple = |name: &str, f: Handler| {
            let action = gio::SimpleAction::new(name, None);
            let this = Rc::downgrade(self);
            let name = name.to_string();
            action.connect_activate(move |_, _| {
                if trace {
                    eprintln!("action {name}");
                }
                if let Some(w) = this.upgrade() {
                    f(&w);
                }
            });
            self.window.add_action(&action);
        };
        let pane = |w: &Rc<Window>| w.active_pane();

        simple("back", Box::new(move |w| { if let Some(p) = pane(w) { p.go_back() } }));
        simple("forward", Box::new(move |w| { if let Some(p) = pane(w) { p.go_forward() } }));
        simple("up", Box::new(move |w| { if let Some(p) = pane(w) { p.go_up() } }));
        simple("home", Box::new(|w| w.navigate(gio::File::for_path(glib::home_dir()))));
        simple("reload", Box::new(move |w| { if let Some(p) = pane(w) { p.reload() } }));
        simple("search", Box::new(move |w| {
            if let Some(p) = pane(w) {
                p.search_bar.set_search_mode(true);
                p.search_entry.grab_focus();
            }
        }));
        simple("location", Box::new(|w| w.show_location_entry()));
        simple("toggle-view", Box::new(|w| {
            let next = match w.view_mode().as_str() {
                "list" => "grid",
                "grid" => "columns",
                "columns" => "gallery",
                _ => "list",
            };
            gio::prelude::ActionGroupExt::activate_action(&w.window, "view", Some(&next.to_variant()));
        }));
        simple("new-tab", Box::new(|w| {
            let dir = w.active_pane().map(|p| p.cwd()).unwrap_or_else(|| gio::File::for_path(glib::home_dir()));
            w.open_in_new_tab(dir, true);
        }));
        simple("close-tab", Box::new(|w| w.close_active_tab()));
        simple("next-tab", Box::new(|w| { w.tab_view().select_next_page(); }));
        simple("prev-tab", Box::new(|w| { w.tab_view().select_previous_page(); }));
        simple("open", Box::new(move |w| {
            if let Some(p) = pane(w) {
                for item in p.selected_items() {
                    p.open_item(&item);
                }
            }
        }));
        simple("open-new-tab", Box::new(|w| {
            if w.focus_in_editable() {
                return; // Ctrl+Enter inside a dialog or text field is not a request for tabs
            }
            for item in w.selected_items() {
                if item.is_dir() {
                    w.open_in_new_tab(item.target_file(), false);
                }
            }
        }));
        simple("open-with", Box::new(|w| {
            if let Some(item) = w.selected_items().first() {
                w.launch(&item.target_file(), true);
            }
        }));
        simple("terminal", Box::new(move |w| { if let Some(p) = pane(w) { w.open_terminal(&p.cwd()) } }));
        simple("terminal-selected", Box::new(|w| {
            if let Some(item) = w.selected_items().first() {
                w.open_terminal(&item.target_file());
            }
        }));
        simple("new-folder", Box::new(|w| w.prompt_create(true)));
        simple("new-file", Box::new(|w| w.prompt_create(false)));
        simple("rename", Box::new(move |w| {
            let Some(p) = pane(w) else { return };
            let items = p.selected_items();
            if items.len() > 1 {
                w.show_batch_rename(p, items);
            } else if let Some(item) = items.first() {
                if !p.inline_rename(item) {
                    w.prompt_rename();
                }
            }
        }));
        simple("rename-dialog", Box::new(|w| w.prompt_rename()));
        simple("folder-sizes", Box::new(move |w| { if let Some(p) = pane(w) { p.compute_folder_sizes() } }));
        simple("treemap", Box::new(move |w| {
            match pane(w).and_then(|p| p.cwd().path()) {
                Some(path) => crate::treemap::Treemap::open(w, path),
                None => w.toast_error(&tr("The disk usage map works on local folders")),
            }
        }));
        simple("treemap-selected", Box::new(move |w| {
            match pane(w).and_then(|p| p.selected_items().into_iter().next()).filter(|i| i.is_dir()).and_then(|i| i.target_file().path()) {
                Some(path) => crate::treemap::Treemap::open(w, path),
                None => w.toast_error(&tr("Select a folder first")),
            }
        }));
        simple("find-duplicates", Box::new(move |w| {
            match pane(w).and_then(|p| p.cwd().path()) {
                Some(path) => crate::dupes::Finder::open(w, path),
                None => w.toast_error(&tr("The duplicate finder works on local folders")),
            }
        }));
        simple("find-duplicates-selected", Box::new(move |w| {
            match pane(w).and_then(|p| p.selected_items().into_iter().next()).filter(|i| i.is_dir()).and_then(|i| i.target_file().path()) {
                Some(path) => crate::dupes::Finder::open(w, path),
                None => w.toast_error(&tr("Select a folder first")),
            }
        }));
        simple("rules", Box::new(|w| w.open_rules()));
        simple("shelf-add", Box::new(move |w| {
            let files: Vec<gio::File> = pane(w).map(|p| p.selected_items().iter().map(|i| i.target_file()).collect()).unwrap_or_default();
            if files.is_empty() {
                w.toast_error(&tr("Select something to put on the shelf"));
                return;
            }
            let shelf = w.shelf();
            shelf.add(files);
            shelf.present();
        }));
        simple("shelf-show", Box::new(|w| w.shelf().toggle()));
        simple("ebook-reader", Box::new(|w| w.ebook_open_reader()));
        simple("ebook-rename", Box::new(|w| w.ebook_rename()));
        simple("ebook-send", Box::new(|w| w.ebook_send()));
        {
            let convert = gio::SimpleAction::new("ebook-convert", Some(glib::VariantTy::STRING));
            let this = Rc::downgrade(self);
            convert.connect_activate(move |_, param| {
                if let (Some(w), Some(fmt)) = (this.upgrade(), param.and_then(|p| p.get::<String>())) {
                    w.ebook_convert(&fmt);
                }
            });
            self.window.add_action(&convert);
        }
        simple("nearby", Box::new(|w| w.open_nearby()));
        simple("now-playing", Box::new(|w| w.toggle_now_playing()));
        simple("nearby-show-received", Box::new(|w| w.nearby_show_received()));
        simple("import-photos", Box::new(|w| w.import_photos()));
        {
            let send = gio::SimpleAction::new("nearby-send", Some(glib::VariantTy::STRING));
            let this = Rc::downgrade(self);
            send.connect_activate(move |_, param| {
                if let (Some(w), Some(fp)) = (this.upgrade(), param.and_then(|p| p.get::<String>())) {
                    let files: Vec<gio::File> = w.selected_items().iter().map(|i| i.target_file()).collect();
                    w.nearby_send(&fp, files);
                }
            });
            self.window.add_action(&send);
        }
        simple("retry", Box::new(|w| {
            let retry = w.retry.borrow_mut().take();
            if let Some(f) = retry {
                f(w);
            }
        }));
        simple("git-stage", Box::new(|w| w.git_stage_selected(false)));
        simple("git-unstage", Box::new(|w| w.git_stage_selected(true)));
        simple("git-discard", Box::new(|w| w.git_discard_selected()));
        simple("git-commit", Box::new(|w| w.git_commit_dialog()));
        simple("git-branch", Box::new(|w| w.git_branch_dialog()));
        simple("image-resize", Box::new(|w| w.image_resize_dialog()));
        simple("play", Box::new(|w| {
            if let Some(p) = w.active_pane() {
                w.quicklook.play(&p, &w.window);
            }
        }));
        simple("image-rotate-left", Box::new(|w| w.image_rotate(-90)));
        simple("image-rotate-right", Box::new(|w| w.image_rotate(90)));
        simple("image-to-png", Box::new(|w| w.image_convert("png")));
        simple("image-to-jpg", Box::new(|w| w.image_convert("jpg")));
        simple("image-to-webp", Box::new(|w| w.image_convert("webp")));
        simple("set-wallpaper", Box::new(|w| w.set_wallpaper()));
        simple("apply-theme", Box::new(|w| w.apply_omarchy_theme()));
        simple("share-scp", Box::new(|w| w.share_copy_scp()));
        simple("share-lan", Box::new(|w| w.share_lan_dialog()));
        simple("save-smart", Box::new(|w| w.save_smart_dialog()));
        simple("new-folder-with-selection", Box::new(|w| w.new_folder_with_selection()));
        simple("make-alias", Box::new(|w| w.make_alias()));
        simple("show-original", Box::new(|w| w.show_original()));
        simple("always-open-with", Box::new(|w| w.always_open_with()));
        simple("pdf-from-images", Box::new(|w| w.pdf_from_images()));
        simple("pdf-merge", Box::new(|w| w.pdf_merge()));
        simple("pdf-split", Box::new(|w| w.pdf_split()));
        simple("pdf-extract", Box::new(|w| w.pdf_extract_dialog()));
        simple("video-trim", Box::new(|w| w.video_trim_dialog()));
        simple("video-gif", Box::new(|w| w.video_to_gif()));
        simple("video-audio", Box::new(|w| w.video_extract_audio()));
        simple("audio-mp3", Box::new(|w| w.audio_convert("mp3")));
        simple("audio-opus", Box::new(|w| w.audio_convert("opus")));
        simple("audio-flac", Box::new(|w| w.audio_convert("flac")));
        simple("zoom-in", Box::new(move |w| { if let Some(p) = pane(w) { p.zoom(16) } }));
        simple("zoom-out", Box::new(move |w| { if let Some(p) = pane(w) { p.zoom(-16) } }));
        simple("zoom-reset", Box::new(move |w| { if let Some(p) = pane(w) { p.zoom(0) } }));
        simple("up-alt", Box::new(move |w| { if let Some(p) = pane(w) { p.go_up() } }));
        {
            let group = gio::SimpleAction::new_stateful("group", Some(glib::VariantTy::STRING), &self.prefs.get("group").unwrap_or_else(|| "none".into()).to_variant());
            let this = Rc::downgrade(self);
            group.connect_activate(move |action, param| {
                let Some(w) = this.upgrade() else { return };
                let Some(key) = param.and_then(|p| p.get::<String>()) else { return };
                action.set_state(&key.to_variant());
                w.prefs.set("group", Some(&key));
                for p in w.all_panes() {
                    p.set_group(&key);
                }
                if let Some(p) = w.active_pane() {
                    p.remember_view();
                }
            });
            self.window.add_action(&group);
        }
        simple("py-create", Box::new(|w| w.py_create()));
        simple("py-activate", Box::new(|w| w.py_activate()));
        simple("py-install", Box::new(|w| w.py_install()));
        simple("py-freeze", Box::new(|w| w.py_freeze()));
        simple("py-repl", Box::new(|w| w.py_repl()));
        simple("py-run", Box::new(|w| w.py_run()));
        simple("py-gitignore", Box::new(|w| w.py_gitignore()));
        simple("py-recreate", Box::new(|w| w.py_recreate()));
        simple("copy-other", Box::new(|w| w.transfer_to_other_pane(crate::transfer::Mode::Copy)));
        simple("move-other", Box::new(|w| w.transfer_to_other_pane(crate::transfer::Mode::Move)));
        simple("compare", Box::new(|w| w.compare_selected()));
        simple("verify-checksum", Box::new(|w| w.verify_checksum()));
        simple("edit-note", Box::new(|w| w.edit_note()));
        simple("open-templates", Box::new(|w| w.open_templates()));
        simple("export-theme", Box::new(|w| w.export_theme()));
        simple("install-theme", Box::new(|w| w.install_theme()));
        simple("browse-archive", Box::new(|w| w.browse_archive_selected()));
        simple("open-workspace", Box::new(|w| w.open_in_workspace()));
        {
            let tpl = gio::SimpleAction::new("new-from-template", Some(glib::VariantTy::STRING));
            let this = Rc::downgrade(self);
            tpl.connect_activate(move |_, v| {
                let i: usize = v.and_then(|v| v.str()).and_then(|s| s.parse().ok()).unwrap_or(0);
                if let Some(w) = this.upgrade() {
                    w.new_from_template(i);
                }
            });
            self.window.add_action(&tpl);
            let smart = gio::SimpleAction::new("smart-open", Some(glib::VariantTy::STRING));
            let this = Rc::downgrade(self);
            smart.connect_activate(move |_, v| {
                if let (Some(w), Some(name)) = (this.upgrade(), v.and_then(|v| v.str())) {
                    w.open_smart(name);
                }
            });
            self.window.add_action(&smart);
        }
        simple("vault-lock-all", Box::new(|w| {
            let n = crate::status::lock_all();
            crate::status::write(w);
            w.refresh_sidebar();
            if let Some(p) = w.active_pane() {
                p.reload();
            }
            if n == 0 { w.toast(&tr("All encrypted folders locked")) } else { w.toast_error(&tr("Some encrypted folders are still in use")) }
        }));
        {
            let rotate = gio::SimpleAction::new("image-rotate", Some(glib::VariantTy::STRING));
            let this = Rc::downgrade(self);
            rotate.connect_activate(move |_, v| {
                let deg: i32 = v.and_then(|v| v.str()).and_then(|s| s.parse().ok()).unwrap_or(90);
                if let Some(w) = this.upgrade() {
                    if trace {
                        eprintln!("action: image-rotate");
                    }
                    w.image_rotate(deg);
                }
            });
            self.window.add_action(&rotate);
            let convert = gio::SimpleAction::new("image-convert", Some(glib::VariantTy::STRING));
            let this = Rc::downgrade(self);
            convert.connect_activate(move |_, v| {
                let fmt = v.and_then(|v| v.str()).unwrap_or("png").to_string();
                if let Some(w) = this.upgrade() {
                    if trace {
                        eprintln!("action: image-convert");
                    }
                    w.image_convert(&fmt);
                }
            });
            self.window.add_action(&convert);
        }
        simple("change-icon", Box::new(|w| {
            if let Some(item) = w.selected_items().first() {
                w.show_icon_chooser(item);
            }
        }));
        simple("preferences", Box::new(|w| w.show_preferences()));
        simple("quick-look", Box::new(|w| w.toggle_quick_look()));
        simple("palette", Box::new(|w| w.open_palette()));
        simple("terminal-panel", Box::new(move |w| { if let Some(p) = pane(w) { p.toggle_terminal() } }));
        simple("open-editor", Box::new(|w| {
            match w.selected_items().first() {
                Some(item) => w.open_in_editor(&item.target_file()),
                None => {
                    if let Some(p) = w.active_pane() {
                        w.open_in_editor(&p.cwd());
                    }
                }
            }
        }));
        simple("open-editor-cwd", Box::new(move |w| { if let Some(p) = pane(w) { w.open_in_editor(&p.cwd()) } }));
        simple("git-status", Box::new(|w| w.run_in_terminal("git status")));
        simple("git-diff-all", Box::new(|w| w.run_in_terminal("git diff")));
        simple("git-diff", Box::new(|w| {
            let names: Vec<String> = w.selected_items().iter().map(|i| format!("'{}'", i.name().replace('\'', "'\\''"))).collect();
            if names.is_empty() {
                w.run_in_terminal("git diff");
            } else {
                w.run_in_terminal(&format!("git diff -- {}", names.join(" ")));
            }
        }));
        simple("vault-new", Box::new(|w| w.vault_new()));
        simple("vault-unlock", Box::new(|w| w.vault_unlock()));
        simple("vault-lock", Box::new(|w| w.vault_lock()));
        simple("vault-passwd", Box::new(|w| w.vault_passwd()));
        simple("vault-register", Box::new(|w| w.vault_register_selected()));
        simple("vault-add", Box::new(|w| w.vault_add()));
        simple("vault-export", Box::new(|w| w.vault_export_selected()));
        simple("vault-import", Box::new(|w| w.vault_import_selected()));
        simple("sidebar-vault-export", Box::new(|w| {
            if let Some((_, PlaceKind::Vault(v))) = w.sidebar_target_place() {
                w.vault_export(v);
            }
        }));
        simple("sidebar-vault-open", Box::new(|w| {
            if let Some((_, PlaceKind::Vault(v))) = w.sidebar_target_place() {
                w.open_vault(&v);
            }
        }));
        simple("sidebar-vault-lock", Box::new(|w| {
            if let Some((_, PlaceKind::Vault(v))) = w.sidebar_target_place() {
                w.lock_vault(v);
            }
        }));
        simple("sidebar-vault-passwd", Box::new(|w| {
            if let Some((_, PlaceKind::Vault(v))) = w.sidebar_target_place() {
                w.vault_passwd_for(v);
            }
        }));
        simple("sidebar-vault-reveal", Box::new(|w| {
            if let Some((_, PlaceKind::Vault(v))) = w.sidebar_target_place() {
                w.navigate(gio::File::for_path(v));
            }
        }));
        simple("sidebar-vault-forget", Box::new(|w| {
            if let Some((_, PlaceKind::Vault(v))) = w.sidebar_target_place() {
                w.vault_forget(&v);
            }
        }));
        simple("undo", Box::new(|w| if !w.forward_to_editable("text.undo") { w.undo() }));
        simple("redo", Box::new(|w| if !w.forward_to_editable("text.redo") { w.redo() }));
        simple("copy-to", Box::new(|w| w.transfer_to(false)));
        simple("move-to", Box::new(|w| w.transfer_to(true)));
        simple("duplicate", Box::new(|w| w.duplicate_selected()));
        simple("sidebar-tags", Box::new(|w| w.refresh_sidebar()));
        simple("split-view", Box::new(|w| w.toggle_split()));
        simple("deep-search", Box::new(move |w| {
            if let Some(p) = pane(w) {
                p.search_bar.set_search_mode(true);
                p.deep_toggle.set_active(true);
                p.search_entry.grab_focus();
            }
        }));
        simple("trash", Box::new(|w| w.trash_selected()));
        simple("delete", Box::new(|w| if !w.focus_in_editable() { w.delete_selected() }));
        simple("restore", Box::new(|w| w.restore_selected()));
        simple("empty-trash", Box::new(|w| w.empty_trash()));
        // Clipboard shortcuts belong to the text field while one has focus.
        simple("copy", Box::new(|w| if !w.forward_to_editable("clipboard.copy") { w.clip_selected(false) }));
        simple("cut", Box::new(|w| if !w.forward_to_editable("clipboard.cut") { w.clip_selected(true) }));
        simple("paste", Box::new(|w| if !w.forward_to_editable("clipboard.paste") { w.paste() }));
        simple("select-all", Box::new(move |w| {
            if !w.forward_to_editable("selection.select-all") {
                if let Some(p) = pane(w) {
                    p.selection.select_all();
                }
            }
        }));
        simple("copy-path", Box::new(|w| {
            let paths: Vec<String> = w.selected_items().iter().map(|i| crate::window::location_text(&i.target_file())).collect();
            if !paths.is_empty() {
                w.window.clipboard().set_text(&paths.join("\n"));
                w.toast(if paths.len() == 1 { "Path copied" } else { "Paths copied" });
            }
        }));
        simple("copy-cwd-path", Box::new(move |w| {
            if let Some(p) = pane(w) {
                w.window.clipboard().set_text(&crate::window::location_text(&p.cwd()));
                w.toast(&tr("Path copied"));
            }
        }));
        simple("properties", Box::new(|w| {
            let items = w.selected_items();
            match items.len() {
                0 => {}
                1 => w.show_properties(&items[0]),
                _ => w.show_multi_properties(items),
            }
        }));
        simple("cwd-properties", Box::new(move |w| {
            let Some(p) = pane(w) else { return };
            let cwd = p.cwd();
            let this = Rc::downgrade(w);
            glib::spawn_future_local(async move {
                if let (Ok(item), Some(w)) = (ops::query(&cwd).await, this.upgrade()) {
                    w.show_properties(&item);
                }
            });
        }));
        simple("bookmark", Box::new(move |w| { if let Some(p) = pane(w) { w.toggle_bookmark(&p.cwd()) } }));
        simple("bookmark-selected", Box::new(|w| {
            if let Some(item) = w.selected_items().first() {
                w.toggle_bookmark(&item.target_file());
            }
        }));
        simple("extract", Box::new(|w| w.extract_selected()));
        simple("compress", Box::new(|w| w.prompt_compress()));
        simple("connect", Box::new(|w| w.prompt_connect()));
        simple("sidebar-new-tab", Box::new(|w| {
            if let Some((file, kind)) = w.sidebar_target_place() {
                match kind {
                    PlaceKind::Ssh(_) | PlaceKind::Connection(_) => w.connect_remote(file, crate::connectui::ConnectOpts { plain_tab: true, ..Default::default() }),
                    _ => w.open_in_new_tab(file, true),
                }
            }
        }));
        simple("sidebar-remove-bookmark", Box::new(|w| {
            if let Some((file, PlaceKind::Bookmark)) = w.sidebar_target_place() {
                if let Err(e) = remote::remove_bookmark(&file) {
                    w.toast_error(&trf!("Cannot update bookmarks: {e}", e = e));
                }
                w.refresh_sidebar();
            }
        }));
        simple("versions", Box::new(|w| w.versions_selected()));
        simple("backup", Box::new(|w| w.open_backup()));
        simple("backup-now", Box::new(|w| w.backup_now()));
        simple("feed-add", Box::new(|w| w.feed_add_dialog()));
        simple("feeds", Box::new(|w| {
            if crate::feeds::list().is_empty() {
                w.feed_add_dialog();
            } else {
                w.open_feed(crate::feedsui::ALL);
            }
        }));
        simple("sidebar-feed-refresh", Box::new(|w| {
            if let Some((_, PlaceKind::Feed(url))) = w.sidebar_target_place() {
                w.feed_refresh(if url.is_empty() { None } else { Some(url) }, true);
            }
        }));
        simple("sidebar-feed-read", Box::new(|w| {
            if let Some((_, PlaceKind::Feed(url))) = w.sidebar_target_place() {
                w.feed_mark_read(&url);
            }
        }));
        simple("sidebar-feed-copy", Box::new(|w| {
            if let Some((_, PlaceKind::Feed(url))) = w.sidebar_target_place() {
                w.window.clipboard().set_text(&url);
                w.toast(&tr("Address copied"));
            }
        }));
        simple("sidebar-feed-remove", Box::new(|w| {
            if let Some((_, PlaceKind::Feed(url))) = w.sidebar_target_place() {
                w.feed_remove(&url);
            }
        }));
        simple("sidebar-connect", Box::new(|w| {
            if let Some((file, PlaceKind::Ssh(_) | PlaceKind::Connection(_))) = w.sidebar_target_place() {
                w.connect_remote(file, Default::default());
            }
        }));
        simple("sidebar-connection-edit", Box::new(|w| {
            if let Some((file, PlaceKind::Connection(name))) = w.sidebar_target_place() {
                w.prompt_connection(Some(crate::connections::Connection { name, uri: file.uri().to_string() }));
            }
        }));
        simple("sidebar-connection-remove", Box::new(|w| {
            if let Some((_, PlaceKind::Connection(name))) = w.sidebar_target_place() {
                w.connection_remove(&name);
            }
        }));
        simple("sidebar-mount", Box::new(|w| {
            if let Some((_, PlaceKind::Volume(v))) = w.sidebar_target_place() {
                w.mount_volume(&v, true);
            }
        }));
        simple("sidebar-unmount", Box::new(|w| {
            if let Some((_, PlaceKind::Mount(m))) = w.sidebar_target_place() {
                w.unmount(m);
            }
        }));
        simple("shortcuts", Box::new(|w| w.show_shortcuts()));
        simple("help", Box::new(|w| w.show_help("features")));
        simple("help-about", Box::new(|w| w.show_help("about")));

        // Stateful actions.
        let hidden = gio::SimpleAction::new_stateful("show-hidden", None, &false.to_variant());
        let this = Rc::downgrade(self);
        hidden.connect_activate(move |action, _| {
            if trace {
                eprintln!("action show-hidden");
            }
            let Some(w) = this.upgrade() else { return };
            let on = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            action.set_state(&on.to_variant());
            w.state.borrow_mut().show_hidden = on;
            let panes: Vec<Rc<Pane>> = w.panes.borrow().clone();
            for p in panes {
                p.filter.changed(gtk::FilterChange::Different);
                p.apply_view_mode();
            }
        });
        self.window.add_action(&hidden);

        let sidebar = gio::SimpleAction::new_stateful("show-sidebar", None, &true.to_variant());
        let this = Rc::downgrade(self);
        sidebar.connect_activate(move |action, _| {
            if trace {
                eprintln!("action show-sidebar");
            }
            let Some(w) = this.upgrade() else { return };
            let on = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(true);
            action.set_state(&on.to_variant());
            w.split.set_show_sidebar(on);
        });
        self.window.add_action(&sidebar);

        let preview = gio::SimpleAction::new_stateful("show-preview", None, &false.to_variant());
        let this = Rc::downgrade(self);
        preview.connect_activate(move |action, _| {
            if trace {
                eprintln!("action show-preview");
            }
            let Some(w) = this.upgrade() else { return };
            let on = !action.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
            action.set_state(&on.to_variant());
            w.preview_split.set_show_sidebar(on);
            if on {
                w.sync_active();
            } else {
                w.preview.stop();
            }
        });
        self.window.add_action(&preview);

        let view = gio::SimpleAction::new_stateful("view", Some(glib::VariantTy::STRING), &"list".to_variant());
        let this = Rc::downgrade(self);
        view.connect_activate(move |action, param| {
            if trace {
                eprintln!("action view {:?}", param.and_then(|p| p.get::<String>()));
            }
            let Some(w) = this.upgrade() else { return };
            let Some(mode) = param.and_then(|p| p.get::<String>()) else { return };
            action.set_state(&mode.to_variant());
            w.state.borrow_mut().view_mode = mode.clone();
            // The header button shows the *next* view in the cycle list → grid → columns → gallery.
            let (icon, tip) = match mode.as_str() {
                "list" => ("view-grid-symbolic", tr("Grid view (Ctrl+2)")),
                "grid" => ("view-dual-symbolic", tr("Column view (Ctrl+3)")),
                "columns" => ("view-paged-symbolic", tr("Gallery view (Ctrl+4)")),
                _ => ("view-list-symbolic", tr("List view (Ctrl+1)")),
            };
            w.view_btn.set_icon_name(icon);
            w.view_btn.set_tooltip_text(Some(&tip));
            let panes: Vec<Rc<Pane>> = w.panes.borrow().clone();
            for p in panes {
                p.apply_view_mode();
            }
            if let Some(p) = w.active_pane() {
                p.focus_view();
                p.remember_view();
            }
        });
        self.window.add_action(&view);

        let project_run = gio::SimpleAction::new("project-run", Some(glib::VariantTy::STRING));
        let this = Rc::downgrade(self);
        project_run.connect_activate(move |_, param| {
            let Some(w) = this.upgrade() else { return };
            let Some(index) = param.and_then(|p| p.get::<String>()).and_then(|s| s.parse::<usize>().ok()) else { return };
            let Some(dir) = w.active_pane().and_then(|p| p.cwd().path()) else { return };
            if let Some(action) = crate::projects::detect(&dir).and_then(|p| p.actions.get(index).cloned()) {
                w.run_in_terminal(&action.command);
            }
        });
        self.window.add_action(&project_run);

        let tag = gio::SimpleAction::new("tag", Some(glib::VariantTy::STRING));
        let this = Rc::downgrade(self);
        tag.connect_activate(move |_, param| {
            if trace {
                eprintln!("action tag {:?}", param.and_then(|p| p.get::<String>()));
            }
            let Some(w) = this.upgrade() else { return };
            let Some(color) = param.and_then(|p| p.get::<String>()) else { return };
            w.tag_selected(if color == "none" { None } else { Some(color) });
        });
        self.window.add_action(&tag);

        let sort = gio::SimpleAction::new_stateful("sort", Some(glib::VariantTy::STRING), &"name".to_variant());
        let this = Rc::downgrade(self);
        sort.connect_activate(move |action, param| {
            if trace {
                eprintln!("action sort {:?}", param.and_then(|p| p.get::<String>()));
            }
            let Some(w) = this.upgrade() else { return };
            let Some(key) = param.and_then(|p| p.get::<String>()) else { return };
            action.set_state(&key.to_variant());
            let panes: Vec<Rc<Pane>> = w.panes.borrow().clone();
            for p in panes {
                p.sort_by(&key);
                p.remember_view();
            }
        });
        self.window.add_action(&sort);
    }

    /// True when keyboard focus is inside a text field.
    pub fn focus_in_editable(&self) -> bool {
        gtk::prelude::GtkWindowExt::focus(&self.window)
            .map(|f| f.is::<gtk::Text>() || f.is::<gtk::Editable>() || f.is::<gtk::TextView>() || f.ancestor(gtk::Entry::static_type()).is_some() || f.ancestor(gtk::SearchEntry::static_type()).is_some())
            .unwrap_or(false)
    }

    /// Run a text-field action (`clipboard.copy`, …) on the focused widget instead of the file action.
    pub fn forward_to_editable(&self, action: &str) -> bool {
        let Some(focus) = gtk::prelude::GtkWindowExt::focus(&self.window) else { return false };
        if !self.focus_in_editable() {
            return false;
        }
        gtk::prelude::WidgetExt::activate_action(&focus, action, None).is_ok()
    }

    /// Dialogs take keyboard focus with them when they close; hand it back to the files.
    pub fn refocus(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            let Some(w) = this.upgrade() else { return };
            match w.active_pane() {
                Some(p) => p.focus_view(),
                None => gtk::prelude::GtkWindowExt::set_focus(&w.window, None::<&gtk::Widget>),
            }
            if std::env::var_os("OMAFILE_TRACE").is_some() {
                let focus = gtk::prelude::GtkWindowExt::focus(&w.window).map(|f| f.type_().name().to_string());
                eprintln!("refocus: pane={} focus={focus:?}", w.active_pane().is_some());
            }
        });
    }

    /// Present an adw::Dialog and restore focus to the file view afterwards.
    pub fn present_dialog(self: &Rc<Self>, dialog: &adw::Dialog) {
        let this = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            if let Some(w) = this.upgrade() {
                w.refocus();
            }
        });
        dialog.present(Some(&self.window));
    }

    fn open_terminal(&self, dir: &gio::File) {
        if let Err(e) = ops::open_terminal(dir) {
            self.toast_error(&format!("Cannot open terminal: {}", e.message()));
        }
    }

    fn toggle_bookmark(self: &Rc<Self>, file: &gio::File) {
        let result = if remote::is_bookmarked(file) {
            remote::remove_bookmark(file).map(|_| tr("Bookmark removed"))
        } else {
            remote::add_bookmark(file).map(|added| if added { tr("Bookmark added") } else { tr("Already bookmarked") })
        };
        match result {
            Ok(msg) => self.toast(&msg),
            Err(e) => self.toast_error(&trf!("Cannot update bookmarks: {e}", e = e)),
        }
        self.refresh_sidebar();
    }

    // ----- clipboard -----------------------------------------------------------

    fn clip_selected(self: &Rc<Self>, cut: bool) {
        let items = self.selected_items();
        let secrets: Vec<String> = items.iter().filter(|i| i.is_secret()).map(|i| i.display_name()).collect();
        if !secrets.is_empty() && !cut {
            let body = format!("{} looks like a credential file. Copying it puts the secret on the clipboard and wherever you paste it.", secrets.join(", "));
            let dialog = adw::AlertDialog::new(Some(tr("Copy secrets?").as_str()), Some(&body));
            dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("copy", tr("Copy Anyway").as_str())]);
            dialog.set_response_appearance("copy", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");
            let this = Rc::downgrade(self);
            dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
                let Some(w) = this.upgrade() else { return };
                w.refocus();
                if response == "copy" {
                    w.clip_files(items.iter().map(|i| i.target_file()).collect(), false);
                }
            });
            return;
        }
        self.clip_files(items.iter().map(|i| i.target_file()).collect(), cut);
    }

    fn clip_files(&self, files: Vec<gio::File>, cut: bool) {
        if files.is_empty() {
            return;
        }
        let n = files.len();
        let list = gdk::FileList::from_array(&files);
        let uris: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
        let provider = gdk::ContentProvider::new_union(&[
            gdk::ContentProvider::for_value(&list.to_value()),
            gdk::ContentProvider::for_value(&uris.join("\n").to_value()),
        ]);
        let _ = self.window.clipboard().set_content(Some(&provider));
        self.state.borrow_mut().clipboard = Some(Clipboard { files, cut });
        self.toast(&format!("{} {n} {}", if cut { "Cut" } else { "Copied" }, if n == 1 { tr("item") } else { tr("items") }));
    }

    fn paste(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Some(w) = this.upgrade() else { return };
            let (files, cut) = match w.state.borrow().clipboard.as_ref() {
                Some(c) => (c.files.clone(), c.cut),
                None => (Vec::new(), false),
            };
            let (files, cut) = if files.is_empty() {
                match w.window.clipboard().read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT).await {
                    Ok(value) => (value.get::<gdk::FileList>().map(|l| l.files()).unwrap_or_default(), false),
                    Err(_) => (Vec::new(), false),
                }
            } else {
                (files, cut)
            };
            if files.is_empty() {
                w.toast(&tr("Nothing to paste"));
                return;
            }
            if cut {
                w.state.borrow_mut().clipboard = None;
            }
            let dest = pane.cwd();
            w.transfer(files, dest, if cut { crate::transfer::Mode::Move } else { crate::transfer::Mode::Copy }, pane, false);
        });
    }

    /// Copy To… / Move To…: pick a destination folder in a dialog.
    fn transfer_to(self: &Rc<Self>, move_files: bool) {
        let Some(pane) = self.active_pane() else { return };
        let items = pane.selected_items();
        if items.is_empty() {
            return;
        }
        let dialog = gtk::FileDialog::builder().title(if move_files { "Move to folder" } else { "Copy to folder" }).modal(true).accept_label(if move_files { "Move" } else { "Copy" }).build();
        dialog.set_initial_folder(Some(&pane.cwd()));
        let this = Rc::downgrade(self);
        dialog.select_folder(Some(&self.window), gio::Cancellable::NONE, move |result| {
            let (Ok(dest), Some(w)) = (result, this.upgrade()) else { return };
            let files: Vec<gio::File> = items.iter().map(|i| i.target_file()).collect();
            w.transfer(files, dest, if move_files { crate::transfer::Mode::Move } else { crate::transfer::Mode::Copy }, pane, false);
        });
    }

    /// Finder's Duplicate: copies next to the originals as "name (1)".
    fn duplicate_selected(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let files: Vec<gio::File> = pane.selected_items().iter().map(|i| i.target_file()).collect();
        let dest = pane.cwd();
        self.transfer(files, dest, crate::transfer::Mode::Copy, pane, true);
    }

    // ----- trash / delete / restore -------------------------------------------------

    fn trash_selected(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let items = pane.selected_items();
        if items.is_empty() {
            return;
        }
        if pane.cwd().uri_scheme().as_deref() == Some("trash") {
            self.delete_selected();
            return;
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let (done, error) = run_each(&items, |item| ops::trash(item.file())).await;
            let Some(w) = this.upgrade() else { return };
            for item in items.iter().take(done) {
                w.tags.removed(&item.file());
            }
            pane.reload();
            let message = format!("Moved {done} {} to trash", if done == 1 { tr("item") } else { tr("items") });
            match error {
                Some(e) => w.toast_error(&e),
                None => w.record(Op::Trash(items.iter().take(done).map(|i| i.file()).collect()), &message),
            }
        });
    }

    fn restore_selected(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let items = pane.selected_items();
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let mut done = 0;
            let mut error = None;
            for item in &items {
                let Some(orig) = item.info().attribute_byte_string("trash::orig-path") else { continue };
                let dest = gio::File::for_path(&orig);
                if let Some(parent) = dest.parent() {
                    let _ = parent.make_directory_with_parents(gio::Cancellable::NONE);
                }
                match ops::move_file(item.file(), dest).await {
                    Ok(()) => done += 1,
                    Err(e) => {
                        error = Some(format!("{}: {}", item.display_name(), e.message()));
                        break;
                    }
                }
            }
            let Some(w) = this.upgrade() else { return };
            pane.reload();
            match error {
                Some(e) => w.toast_error(&e),
                None => w.toast(&format!("Restored {done} {}", if done == 1 { tr("item") } else { tr("items") })),
            }
        });
    }

    fn empty_trash(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let dialog = adw::AlertDialog::new(Some(tr("Empty the trash?").as_str()), Some(tr("All items in the trash will be permanently deleted.").as_str()));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("empty", tr("Empty Trash").as_str())]);
        dialog.set_response_appearance("empty", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let refocus = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            if let Some(w) = refocus.upgrade() {
                w.refocus();
            }
        });
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            if response != "empty" {
                return;
            }
            let this = this.clone();
            glib::spawn_future_local(async move {
                let items = ops::list_dir(&gio::File::for_uri("trash:///")).await.unwrap_or_default();
                let (done, error) = run_each(&items, |item| ops::delete_recursive(item.file())).await;
                let Some(w) = this.upgrade() else { return };
                pane.reload();
                match error {
                    Some(e) => w.toast_error(&e),
                    None => w.toast(&trf!("Trash emptied ({done} items)", done = done)),
                }
            });
        });
    }

    fn delete_selected(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let items = pane.selected_items();
        if items.is_empty() {
            return;
        }
        let what = if items.len() == 1 { format!("“{}”", items[0].display_name()) } else { format!("{} items", items.len()) };
        let dialog = adw::AlertDialog::new(
            Some(&format!("Permanently delete {what}?")),
            Some(tr("This cannot be undone. The files will not go to the trash.").as_str()),
        );
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("delete", tr("Delete").as_str())]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let refocus = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            if let Some(w) = refocus.upgrade() {
                w.refocus();
            }
        });
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            if response != "delete" {
                return;
            }
            let this = this.clone();
            glib::spawn_future_local(async move {
                let (done, error) = run_each(&items, |item| ops::delete_recursive(item.file())).await;
                let Some(w) = this.upgrade() else { return };
                for item in items.iter().take(done) {
                    w.tags.removed(&item.file());
                }
                pane.reload();
                match error {
                    Some(e) => w.toast_error(&e),
                    None => w.toast(&format!("Deleted {done} {}", if done == 1 { tr("item") } else { tr("items") })),
                }
            });
        });
    }

    // ----- archives ---------------------------------------------------------------------

    fn extract_selected(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let Some(item) = pane.selected_items().into_iter().find(|i| i.is_archive()) else { return };
        let (Some(archive), Some(into)) = (item.target_file().path(), pane.cwd().path()) else {
            self.toast_error(&tr("Archives can only be extracted on local file systems"));
            return;
        };
        let this = Rc::downgrade(self);
        let name = item.display_name();
        self.toast(&trf!("Extracting {name}…", name = name));
        glib::spawn_future_local(async move {
            let result = ops::extract(&archive, &into).await;
            let Some(w) = this.upgrade() else { return };
            pane.reload();
            match result {
                Ok(()) => w.toast(&trf!("Extracted {name}", name = name)),
                Err(e) => w.toast_error(&format!("Cannot extract {name}: {}", e.message())),
            }
        });
    }

    fn prompt_compress(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let items = pane.selected_items();
        let Some(cwd) = pane.cwd().path() else {
            self.toast_error(&tr("Archives can only be created on local file systems"));
            return;
        };
        if items.is_empty() {
            return;
        }
        let default = if items.len() == 1 {
            format!("{}.tar.zst", items[0].display_name().trim_end_matches('/'))
        } else {
            tr("Archive.tar.zst")
        };
        let entry = gtk::Entry::builder().text(&default).activates_default(true).build();
        let dialog = adw::AlertDialog::new(Some(tr("Compress").as_str()), Some(tr("The extension picks the format: .zip, .tar.gz, .tar.xz, .tar.zst, .7z").as_str()));
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("create", tr("Create").as_str())]);
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("create"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let entry2 = entry.clone();
        let refocus = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            if let Some(w) = refocus.upgrade() {
                w.refocus();
            }
        });
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "create" {
                return;
            }
            let archive = entry2.text().trim().to_string();
            if archive.is_empty() || archive.contains('/') {
                w.toast_error(&tr("Invalid archive name"));
                return;
            }
            let names: Vec<String> = items.iter().map(|i| i.name()).collect();
            let this = Rc::downgrade(&w);
            let target = pane.cwd().child(&archive);
            w.toast(&trf!("Creating {archive}…", archive = archive));
            glib::spawn_future_local(async move {
                let result = ops::compress(&cwd, &archive, &names).await;
                let Some(w) = this.upgrade() else { return };
                pane.reload();
                match result {
                    Ok(()) => {
                        w.toast(&trf!("Created {archive}", archive = archive));
                        pane.select_file_later(target);
                    }
                    Err(e) => w.toast_error(&format!("Cannot create {archive}: {}", e.message())),
                }
            });
        });
        entry.grab_focus();
        if let Some(dot) = entry.text().find('.') {
            entry.select_region(0, dot as i32);
        }
    }

    // ----- dialogs ------------------------------------------------------------------------

    fn prompt_create(self: &Rc<Self>, folder: bool) {
        let Some(pane) = self.active_pane() else { return };
        let heading = if folder { "New Folder" } else { "New File" };
        let entry = gtk::Entry::builder().text(if folder { tr("Untitled Folder") } else { tr("Untitled.txt") }).activates_default(true).build();
        let dialog = adw::AlertDialog::new(Some(heading), None);
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("create", tr("Create").as_str())]);
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("create"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let entry2 = entry.clone();
        let refocus = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            if let Some(w) = refocus.upgrade() {
                w.refocus();
            }
        });
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "create" {
                return;
            }
            let name = entry2.text().trim().to_string();
            if name.is_empty() || name.contains('/') {
                w.toast_error(&tr("Invalid name"));
                return;
            }
            let target = pane.cwd().child(&name);
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let result = if folder { ops::make_dir(target.clone()).await } else { ops::create_file(target.clone()).await };
                let Some(w) = this.upgrade() else { return };
                match result {
                    Ok(()) => {
                        pane.reload();
                        pane.select_file_later(target.clone());
                        w.record(Op::Create(vec![(target, folder)]), &trf!("Created {name}", name = name));
                    }
                    Err(e) => w.toast_error(&format!("Cannot create {name}: {}", e.message())),
                }
            });
        });
        entry.grab_focus();
        let end = if folder { None } else { entry.text().rfind('.') };
        entry.select_region(0, end.map(|i| i as i32).unwrap_or(-1));
    }

    /// Rename `item` to `name` (used by the inline editor and the dialog).
    pub fn do_rename(self: &Rc<Self>, pane: Rc<Pane>, item: FileItem, name: String) {
        let old = item.display_name();
        if name.is_empty() || name.contains('/') || name == old {
            return;
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = ops::rename(item.file(), &name).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(new_file) => {
                    w.tags.moved(&item.file(), &new_file);
                    pane.reload();
                    pane.select_file_later(new_file.clone());
                    w.record(Op::Move(vec![(item.file(), new_file)]), &trf!("Renamed to {name}", name = name));
                }
                Err(e) => w.toast_error(&format!("Cannot rename: {}", e.message())),
            }
        });
    }

    fn prompt_rename(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let Some(item) = pane.selected_items().first().cloned() else { return };
        let old = item.display_name();
        let stem_end = if item.is_dir() { None } else { old.rfind('.').filter(|&i| i > 0) };
        let entry = gtk::Entry::builder().text(&old).activates_default(true).build();
        let dialog = adw::AlertDialog::new(Some(tr("Rename").as_str()), None);
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("rename", tr("Rename").as_str())]);
        dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("rename"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let entry2 = entry.clone();
        let refocus = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            if let Some(w) = refocus.upgrade() {
                w.refocus();
            }
        });
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "rename" {
                return;
            }
            w.do_rename(pane, item, entry2.text().trim().to_string());
        });
        entry.grab_focus();
        entry.select_region(0, stem_end.map(|i| i as i32).unwrap_or(-1));
    }

    pub fn show_properties(self: &Rc<Self>, item: &FileItem) {
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::new();
        let row = |title: &str, subtitle: &str| {
            let r = adw::ActionRow::builder().title(tr(title)).subtitle(subtitle).build();
            r.add_css_class("property");
            r.set_subtitle_selectable(true);
            r
        };
        group.add(&row("Name", &item.display_name()));
        group.add(&row("Type", &item.type_description()));
        if item.is_dir() {
            let size_row = row("Size", &tr("Calculating…"));
            group.add(&size_row);
            let file = item.target_file();
            let item2 = item.clone();
            glib::spawn_future_local(async move {
                let bytes = ops::dir_size(&file).await;
                item2.set_dir_size(Some(bytes));
                size_row.set_subtitle(&trf!("{human} ({bytes} bytes)", human = ops::human_size(bytes), bytes = bytes));
            });
        } else {
            group.add(&row("Size", &trf!("{human} ({bytes} bytes)", human = ops::human_size(item.size()), bytes = item.size())));
        }
        if item.is_secret() {
            group.add(&row("Security", &tr("Looks like a credential file: preview masked, copying asks first")));
        }
        match item.target_file().parent() {
            Some(parent) => group.add(&row("Location", &crate::window::location_text(&parent))),
            None => group.add(&row("Location", &item.file().uri())),
        }
        if let Some(m) = item.modified() {
            group.add(&row("Modified", &m.format("%A, %-d %B %Y %H:%M").map(|s| s.to_string()).unwrap_or_default()));
        }
        if let Some(target) = item.symlink_target() {
            group.add(&row("Link target", &target));
        }

        if !item.is_dir() && item.target_file().is_native() {
            let hash_row = row("SHA-256", &tr("Calculating…"));
            let copy = gtk::Button::from_icon_name("edit-copy-symbolic");
            copy.add_css_class("flat");
            copy.set_valign(gtk::Align::Center);
            copy.set_tooltip_text(Some(tr("Copy hash").as_str()));
            copy.set_sensitive(false);
            hash_row.add_suffix(&copy);
            group.add(&hash_row);
            if let Some(path) = item.target_file().path() {
                let clip = self.window.clipboard();
                glib::spawn_future_local(async move {
                    let hash = crate::checksum::sha256(path).await.unwrap_or_else(|| tr("sha256sum not available"));
                    hash_row.set_subtitle(&hash);
                    copy.set_sensitive(true);
                    let hash2 = hash.clone();
                    copy.connect_clicked(move |_| clip.set_text(&hash2));
                });
            }
        }
        if let Some(note) = self.notes.get(&item.target_file()) {
            group.add(&row("Note", &note));
        }
        page.add(&group);
        if item.is_readable_ebook() {
            if let (Some(path), Some(ct)) = (item.target_file().path(), item.content_type()) {
                if let Some(meta) = crate::ebook::meta(&path, &ct) {
                    let book = adw::PreferencesGroup::builder().title(tr("eBook")).build();
                    let add = |title: &str, value: &str| {
                        if !value.is_empty() {
                            book.add(&row(title, value));
                        }
                    };
                    add("Title", &meta.title);
                    add("Author", &meta.authors.join(", "));
                    add("Series", &if meta.series_index.is_empty() { meta.series.clone() } else { format!("{} #{}", meta.series, meta.series_index) });
                    add("Publisher", &meta.publisher);
                    add("Published", &meta.date);
                    add("Language", &meta.language);
                    add("ISBN", &meta.isbn);
                    add("Chapters", &if meta.chapters.is_empty() { String::new() } else { meta.chapters.len().to_string() });
                    page.add(&book);
                }
            }
        }
        if let Some(venv) = item.target_file().path().filter(|p| item.is_dir() && crate::pyenv::is_venv_dir(p)).and_then(|p| crate::pyenv::read(&p)) {
            let py = adw::PreferencesGroup::builder().title(tr("Python virtual environment")).build();
            py.add(&row("Python", if venv.version.is_empty() { "unknown" } else { &venv.version }));
            py.add(&row("Base interpreter", &venv.base));
            py.add(&row("Created with", &venv.tool));
            if !venv.prompt.is_empty() {
                py.add(&row("Prompt", &venv.prompt));
            }
            let list: Vec<String> = venv.packages.iter().map(|(n, v)| format!("{n} {v}")).collect();
            py.add(&row(&format!("{} packages", venv.packages.len()), &list.join(", ")));
            page.add(&py);
        }
        page.add(&self.permissions_group(item));

        let icon = gtk::Image::new();
        icon.set_pixel_size(64);
        if let Some(gicon) = item.icon() {
            icon.set_from_gicon(&gicon);
        }
        icon.set_margin_top(18);
        let content = adw::ToolbarView::new();
        content.add_top_bar(&adw::HeaderBar::new());
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        vbox.append(&icon);
        page.set_vexpand(true);
        vbox.append(&page);
        content.set_content(Some(&vbox));
        let dialog = adw::Dialog::builder().title(tr("Properties")).content_width(520).content_height(780).child(&content).build();
        self.present_dialog(&dialog);
    }

    fn show_shortcuts(self: &Rc<Self>) {
        self.show_help("keys");
    }
}

/// Run `op` for every item, stopping at the first error.
async fn run_each<F, Fut>(items: &[FileItem], op: F) -> (usize, Option<String>)
where
    F: Fn(&FileItem) -> Fut,
    Fut: std::future::Future<Output = ops::OpResult>,
{
    let mut done = 0;
    for item in items {
        match op(item).await {
            Ok(()) => done += 1,
            Err(e) => return (done, Some(format!("{}: {}", item.display_name(), e.message()))),
        }
    }
    (done, None)
}

pub fn about(app: &adw::Application) {
    let dialog = adw::AboutDialog::builder()
        .application_name("Files")
        .application_icon("io.github.whogotpwned.Omafile")
        .developer_name("omafile")
        .version(env!("CARGO_PKG_VERSION"))
        .website("https://github.com/whogotpwned/omafile")
        .issue_url("https://github.com/whogotpwned/omafile/issues")
        .license_type(gtk::License::MitX11)
        .comments("A file manager that follows your Omarchy theme.")
        .build();
    dialog.present(app.active_window().as_ref());
}
