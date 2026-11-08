//! Window actions: navigation, tabs, clipboard, file operations and dialogs.

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
    ("home", "Go Home", "user-home-symbolic"),
    ("up", "Parent Folder", "go-up-symbolic"),
    ("reload", "Reload", "view-refresh-symbolic"),
    ("folder-sizes", "Show Folder Sizes (disk usage)", "drive-harddisk-symbolic"),
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
    ("win.shortcuts", &["<Control>question", "F1"]),
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
            let next = if w.view_mode() == "grid" { "list" } else { "grid" };
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
            if let Some(p) = w.active_pane() {
                p.reload();
            }
            if n == 0 { w.toast("All encrypted folders locked") } else { w.toast_error("Some encrypted folders are still in use") }
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
                w.toast("Path copied");
            }
        }));
        simple("properties", Box::new(|w| {
            if let Some(item) = w.selected_items().first() {
                w.show_properties(item);
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
                    PlaceKind::Ssh(_) => w.connect_to(file, true),
                    _ => w.open_in_new_tab(file, true),
                }
            }
        }));
        simple("sidebar-remove-bookmark", Box::new(|w| {
            if let Some((file, PlaceKind::Bookmark)) = w.sidebar_target_place() {
                if let Err(e) = remote::remove_bookmark(&file) {
                    w.toast_error(&format!("Cannot update bookmarks: {e}"));
                }
                w.refresh_sidebar();
            }
        }));
        simple("sidebar-unmount", Box::new(|w| {
            if let Some((_, PlaceKind::Mount(m))) = w.sidebar_target_place() {
                w.unmount(m);
            }
        }));
        simple("shortcuts", Box::new(|w| w.show_shortcuts()));

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
            let grid = mode == "grid";
            w.view_btn.set_icon_name(if grid { "view-list-symbolic" } else { "view-grid-symbolic" });
            w.view_btn.set_tooltip_text(Some(if grid { "List view (Ctrl+1)" } else { "Grid view (Ctrl+2)" }));
            let panes: Vec<Rc<Pane>> = w.panes.borrow().clone();
            for p in panes {
                p.apply_view_mode();
            }
            if let Some(p) = w.active_pane() {
                p.focus_view();
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
            remote::remove_bookmark(file).map(|_| "Bookmark removed")
        } else {
            remote::add_bookmark(file).map(|added| if added { "Bookmark added" } else { "Already bookmarked" })
        };
        match result {
            Ok(msg) => self.toast(msg),
            Err(e) => self.toast_error(&format!("Cannot update bookmarks: {e}")),
        }
        self.refresh_sidebar();
    }

    // ----- clipboard -----------------------------------------------------------

    fn clip_selected(self: &Rc<Self>, cut: bool) {
        let items = self.selected_items();
        let secrets: Vec<String> = items.iter().filter(|i| i.is_secret()).map(|i| i.display_name()).collect();
        if !secrets.is_empty() && !cut {
            let body = format!("{} looks like a credential file. Copying it puts the secret on the clipboard and wherever you paste it.", secrets.join(", "));
            let dialog = adw::AlertDialog::new(Some("Copy secrets?"), Some(&body));
            dialog.add_responses(&[("cancel", "Cancel"), ("copy", "Copy Anyway")]);
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
        self.toast(&format!("{} {n} {}", if cut { "Cut" } else { "Copied" }, if n == 1 { "item" } else { "items" }));
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
                w.toast("Nothing to paste");
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
            let message = format!("Moved {done} {} to trash", if done == 1 { "item" } else { "items" });
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
                None => w.toast(&format!("Restored {done} {}", if done == 1 { "item" } else { "items" })),
            }
        });
    }

    fn empty_trash(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let dialog = adw::AlertDialog::new(Some("Empty the trash?"), Some("All items in the trash will be permanently deleted."));
        dialog.add_responses(&[("cancel", "Cancel"), ("empty", "Empty Trash")]);
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
                    None => w.toast(&format!("Trash emptied ({done} items)")),
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
            Some("This cannot be undone. The files will not go to the trash."),
        );
        dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
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
                    None => w.toast(&format!("Deleted {done} {}", if done == 1 { "item" } else { "items" })),
                }
            });
        });
    }

    // ----- archives ---------------------------------------------------------------------

    fn extract_selected(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let Some(item) = pane.selected_items().into_iter().find(|i| i.is_archive()) else { return };
        let (Some(archive), Some(into)) = (item.target_file().path(), pane.cwd().path()) else {
            self.toast_error("Archives can only be extracted on local file systems");
            return;
        };
        let this = Rc::downgrade(self);
        let name = item.display_name();
        self.toast(&format!("Extracting {name}…"));
        glib::spawn_future_local(async move {
            let result = ops::extract(&archive, &into).await;
            let Some(w) = this.upgrade() else { return };
            pane.reload();
            match result {
                Ok(()) => w.toast(&format!("Extracted {name}")),
                Err(e) => w.toast_error(&format!("Cannot extract {name}: {}", e.message())),
            }
        });
    }

    fn prompt_compress(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let items = pane.selected_items();
        let Some(cwd) = pane.cwd().path() else {
            self.toast_error("Archives can only be created on local file systems");
            return;
        };
        if items.is_empty() {
            return;
        }
        let default = if items.len() == 1 {
            format!("{}.tar.zst", items[0].display_name().trim_end_matches('/'))
        } else {
            "Archive.tar.zst".to_string()
        };
        let entry = gtk::Entry::builder().text(&default).activates_default(true).build();
        let dialog = adw::AlertDialog::new(Some("Compress"), Some("The extension picks the format: .zip, .tar.gz, .tar.xz, .tar.zst, .7z"));
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancel"), ("create", "Create")]);
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
                w.toast_error("Invalid archive name");
                return;
            }
            let names: Vec<String> = items.iter().map(|i| i.name()).collect();
            let this = Rc::downgrade(&w);
            let target = pane.cwd().child(&archive);
            w.toast(&format!("Creating {archive}…"));
            glib::spawn_future_local(async move {
                let result = ops::compress(&cwd, &archive, &names).await;
                let Some(w) = this.upgrade() else { return };
                pane.reload();
                match result {
                    Ok(()) => {
                        w.toast(&format!("Created {archive}"));
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

    fn prompt_connect(self: &Rc<Self>) {
        let entry = gtk::Entry::builder().placeholder_text("user@host[:port][/path]  or  sftp://host/").activates_default(true).build();
        let hosts = remote::ssh_hosts();
        let body = if hosts.is_empty() {
            "Connect over SSH (SFTP). Hosts from ~/.ssh/config appear under Remote in the sidebar.".to_string()
        } else {
            format!("Connect over SSH (SFTP). Known hosts: {}", hosts.iter().map(|h| h.alias.as_str()).collect::<Vec<_>>().join(", "))
        };
        let dialog = adw::AlertDialog::new(Some("Connect to Server"), Some(&body));
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancel"), ("connect", "Connect")]);
        dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("connect"));
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
            if response != "connect" {
                return;
            }
            let text = entry2.text().to_string();
            // Allow typing an alias from ~/.ssh/config.
            let uri = hosts
                .iter()
                .find(|h| h.alias == text.trim())
                .map(|h| h.uri())
                .or_else(|| remote::parse_server(&text));
            match uri {
                Some(uri) => w.connect_to(gio::File::for_uri(&uri), true),
                None => w.toast_error("Enter a host to connect to"),
            }
        });
        entry.grab_focus();
    }

    fn prompt_create(self: &Rc<Self>, folder: bool) {
        let Some(pane) = self.active_pane() else { return };
        let heading = if folder { "New Folder" } else { "New File" };
        let entry = gtk::Entry::builder().text(if folder { "Untitled Folder" } else { "Untitled.txt" }).activates_default(true).build();
        let dialog = adw::AlertDialog::new(Some(heading), None);
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancel"), ("create", "Create")]);
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
                w.toast_error("Invalid name");
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
                        w.record(Op::Create(vec![(target, folder)]), &format!("Created {name}"));
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
                    w.record(Op::Move(vec![(item.file(), new_file)]), &format!("Renamed to {name}"));
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
        let dialog = adw::AlertDialog::new(Some("Rename"), None);
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancel"), ("rename", "Rename")]);
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
            let r = adw::ActionRow::builder().title(title).subtitle(subtitle).build();
            r.add_css_class("property");
            r.set_subtitle_selectable(true);
            r
        };
        group.add(&row("Name", &item.display_name()));
        group.add(&row("Type", &item.type_description()));
        if item.is_dir() {
            let size_row = row("Size", "Calculating…");
            group.add(&size_row);
            let file = item.target_file();
            let item2 = item.clone();
            glib::spawn_future_local(async move {
                let bytes = ops::dir_size(&file).await;
                item2.set_dir_size(Some(bytes));
                size_row.set_subtitle(&format!("{} ({bytes} bytes)", ops::human_size(bytes)));
            });
        } else {
            group.add(&row("Size", &format!("{} ({} bytes)", ops::human_size(item.size()), item.size())));
        }
        if item.is_secret() {
            group.add(&row("Security", "Looks like a credential file: preview masked, copying asks first"));
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
            let hash_row = row("SHA-256", "Calculating…");
            let copy = gtk::Button::from_icon_name("edit-copy-symbolic");
            copy.add_css_class("flat");
            copy.set_valign(gtk::Align::Center);
            copy.set_tooltip_text(Some("Copy hash"));
            copy.set_sensitive(false);
            hash_row.add_suffix(&copy);
            group.add(&hash_row);
            if let Some(path) = item.target_file().path() {
                let clip = self.window.clipboard();
                glib::spawn_future_local(async move {
                    let hash = crate::checksum::sha256(path).await.unwrap_or_else(|| "sha256sum not available".into());
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
        if let Some(venv) = item.target_file().path().filter(|p| item.is_dir() && crate::pyenv::is_venv_dir(p)).and_then(|p| crate::pyenv::read(&p)) {
            let py = adw::PreferencesGroup::builder().title("Python virtual environment").build();
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
        let dialog = adw::Dialog::builder().title("Properties").content_width(520).content_height(780).child(&content).build();
        self.present_dialog(&dialog);
    }

    fn show_shortcuts(self: &Rc<Self>) {
        self.show_bindings();
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
