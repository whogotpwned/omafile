//! Actions and dialogs for: smart folders, dual-pane copy/move, compare,
//! bulk palette commands, checksums, notes, templates, theme export/install,
//! archive browsing and Hyprland workspaces.

use crate::i18n::tr;
use crate::item::FileItem;
use crate::ops;
use crate::pane::Pane;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::path::Path;
use std::rc::Rc;

impl Window {
    // ----- smart folders -----------------------------------------------------------

    pub fn save_smart_dialog(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let query = pane.search_entry.text().to_string();
        let root = pane.cwd().path().unwrap_or_else(glib::home_dir);
        let name = adw::EntryRow::builder().title(tr("Name")).text(if query.is_empty() { tr("New smart folder") } else { format!("{query} in {}", root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()) }).build();
        let term = adw::EntryRow::builder().title(tr("Name contains")).text(&query).build();
        let glob = adw::EntryRow::builder().title(tr("Name matches (glob, e.g. *.pdf)")).build();
        let age = adw::SpinRow::with_range(0.0, 3650.0, 1.0);
        age.set_title(&tr("Modified within the last … days (0 = any)"));
        let root_row = adw::ActionRow::builder().title(tr("Search below")).subtitle(glib::markup_escape_text(&crate::window::abbreviate_home(&root))).build();
        root_row.add_css_class("property");
        let group = adw::PreferencesGroup::builder().title(tr("Smart folder")).description(tr("A saved search that lives in the sidebar and lists its matches whenever you open it.")).build();
        for r in [name.clone().upcast::<gtk::Widget>(), term.clone().upcast(), glob.clone().upcast(), age.clone().upcast(), root_row.upcast()] {
            group.add(&r);
        }
        let existing = adw::PreferencesGroup::builder().title(tr("Existing smart folders")).build();
        let list = crate::smart::load();
        if list.is_empty() {
            existing.set_visible(false);
        }
        for s in &list {
            let row = adw::ActionRow::builder().title(glib::markup_escape_text(&s.name)).subtitle(glib::markup_escape_text(&format!("{}  ·  {}{}{}", crate::window::abbreviate_home(&s.root), s.query, if s.glob.is_empty() { String::new() } else { format!("  ·  {}", s.glob) }, if s.max_age_days > 0 { format!("  ·  {} days", s.max_age_days) } else { String::new() }))).build();
            let del = gtk::Button::from_icon_name("user-trash-symbolic");
            del.add_css_class("flat");
            del.set_valign(gtk::Align::Center);
            let this = Rc::downgrade(self);
            let n = s.name.clone();
            let row2 = row.clone();
            del.connect_clicked(move |_| {
                let _ = crate::smart::remove(&n);
                row2.set_visible(false);
                if let Some(w) = this.upgrade() {
                    w.refresh_sidebar();
                }
            });
            row.add_suffix(&del);
            existing.add(&row);
        }
        let page = adw::PreferencesPage::new();
        page.add(&group);
        page.add(&existing);
        let save = gtk::Button::with_label("Save");
        save.add_css_class("suggested-action");
        let header = adw::HeaderBar::new();
        header.pack_end(&save);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&page));
        let dialog = adw::Dialog::builder().title(tr("Smart Folder")).content_width(520).content_height(560).child(&view).build();
        let this = Rc::downgrade(self);
        let d2 = dialog.clone();
        save.connect_clicked(move |_| {
            let Some(w) = this.upgrade() else { return };
            let spec = crate::smart::SmartFolder { name: name.text().trim().to_string(), root: root.clone(), query: term.text().trim().to_string(), max_age_days: age.value() as u32, glob: glob.text().trim().to_string() };
            if spec.name.is_empty() || (spec.query.is_empty() && spec.glob.is_empty() && spec.max_age_days == 0) {
                w.toast_error(&tr("Give it a name and at least one condition"));
                return;
            }
            match crate::smart::add(spec.clone()) {
                Ok(()) => {
                    d2.close();
                    w.refresh_sidebar();
                    w.toast(&format!("Smart folder “{}” saved", spec.name));
                    if let Some(p) = w.active_pane() {
                        p.show_smart(&spec);
                    }
                }
                Err(e) => w.toast_error(&trf!("Cannot save: {e}", e = e)),
            }
        });
        self.present_dialog(&dialog);
    }

    pub fn open_smart(self: &Rc<Self>, name: &str) {
        if let (Some(p), Some(spec)) = (self.active_pane(), crate::smart::load().into_iter().find(|s| s.name == name)) {
            p.show_smart(&spec);
        }
    }

    // ----- dual pane ---------------------------------------------------------------

    pub fn transfer_to_other_pane(self: &Rc<Self>, mode: crate::transfer::Mode) {
        let (Some(pane), Some(other)) = (self.active_pane(), self.other_pane()) else {
            self.toast(&tr("Split the view first (F6)"));
            return;
        };
        let files: Vec<gio::File> = pane.selected_items().iter().map(|i| i.target_file()).collect();
        if files.is_empty() {
            return;
        }
        self.transfer(files, other.cwd(), mode, other, false);
    }

    // ----- compare -----------------------------------------------------------------

    pub fn compare_selected(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let items = pane.selected_items();
        let (a, b) = match items.as_slice() {
            [a, b] => (a.target_file(), b.target_file()),
            [a] => match self.other_pane().and_then(|o| o.selected_items().into_iter().next()) {
                Some(b) => (a.target_file(), b.target_file()),
                None => {
                    self.toast(&tr("Select two items, or one on each side of the split"));
                    return;
                }
            },
            _ => {
                self.toast(&tr("Select two items to compare"));
                return;
            }
        };
        let (Some(pa), Some(pb)) = (a.path(), b.path()) else {
            self.toast_error(&tr("Compare works on local files"));
            return;
        };
        if !crate::compare::available() {
            self.toast_error(&tr("diff (diffutils) is not installed"));
            return;
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Some(w) = this.upgrade() else { return };
            if pa.is_dir() && pb.is_dir() {
                let Some(d) = crate::compare::dirs(&pa, &pb).await else { return };
                w.show_dir_compare(&pa, &pb, d);
            } else if pa.is_dir() || pb.is_dir() {
                w.toast_error(&tr("Compare two files or two folders, not a mix"));
            } else {
                let Some(diff) = crate::compare::files(&pa, &pb).await else { return };
                w.show_file_compare(&pa, &pb, &diff);
            }
        });
    }

    fn compare_dialog(self: &Rc<Self>, title: &str, subtitle: &str, body: &gtk::Widget) {
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&adw::WindowTitle::new(title, subtitle)));
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(body));
        let dialog = adw::Dialog::builder().title(tr("Compare")).content_width(900).content_height(680).child(&view).build();
        self.present_dialog(&dialog);
    }

    pub(crate) fn show_file_compare(self: &Rc<Self>, a: &Path, b: &Path, diff: &str) {
        let text = gtk::TextView::builder().editable(false).cursor_visible(false).monospace(true).left_margin(10).right_margin(10).top_margin(10).build();
        text.add_css_class("omafile-preview-text");
        let (adds, dels) = crate::preview::render_diff_into(&text, diff);
        let scroll = gtk::ScrolledWindow::builder().child(&text).vexpand(true).build();
        scroll.add_css_class("omafile-preview-card");
        scroll.set_margin_start(12);
        scroll.set_margin_end(12);
        scroll.set_margin_bottom(12);
        let subtitle = if diff.trim().is_empty() { "identical".to_string() } else { format!("+{adds} −{dels}") };
        if diff.trim().is_empty() {
            text.buffer().set_text(&tr("The files are identical."));
        }
        self.compare_dialog(&format!("{}  ↔  {}", name_of(a), name_of(b)), &subtitle, scroll.upcast_ref());
    }

    pub(crate) fn show_dir_compare(self: &Rc<Self>, a: &Path, b: &Path, d: crate::compare::DirDiff) {
        let page = adw::PreferencesPage::new();
        let total = d.only_left.len() + d.only_right.len() + d.differ.len();
        let section = |title: String, entries: &[String], icon: &str, css: &str| {
            if entries.is_empty() {
                return;
            }
            let group = adw::PreferencesGroup::builder().title(glib::markup_escape_text(&title)).build();
            for e in entries.iter().take(300) {
                let row = adw::ActionRow::builder().title(glib::markup_escape_text(e)).build();
                let img = gtk::Image::from_icon_name(icon);
                img.add_css_class(css);
                row.add_prefix(&img);
                group.add(&row);
            }
            if entries.len() > 300 {
                group.add(&adw::ActionRow::builder().title(format!("… and {} more", entries.len() - 300)).build());
            }
            page.add(&group);
        };
        section(format!("Only in {} ({})", name_of(a), d.only_left.len()), &d.only_left, "list-remove-symbolic", "omafile-git-deleted");
        section(format!("Only in {} ({})", name_of(b), d.only_right.len()), &d.only_right, "list-add-symbolic", "omafile-git-added");
        section(format!("Different ({})", d.differ.len()), &d.differ, "view-refresh-symbolic", "omafile-git-modified");
        if total == 0 {
            let status = adw::StatusPage::builder().icon_name("object-select-symbolic").title(tr("Identical")).description(tr("Both folders have the same files with the same contents.")).build();
            self.compare_dialog(&format!("{}  ↔  {}", name_of(a), name_of(b)), "identical", status.upcast_ref());
            return;
        }
        self.compare_dialog(&format!("{}  ↔  {}", name_of(a), name_of(b)), &format!("{total} differences"), page.upcast_ref());
    }

    // ----- bulk palette commands ---------------------------------------------------

    /// Preview (title, detail) for a bulk command typed into the palette.
    pub fn bulk_preview(&self, text: &str) -> Option<(String, String)> {
        let bulk = crate::bulk::parse(text)?;
        let pane = self.active_pane()?;
        let items = pane.selected_items();
        let scope: Vec<FileItem> = if items.is_empty() { (0..pane.selection.n_items()).filter_map(|i| pane.item_at(i)).collect() } else { items };
        match &bulk {
            crate::bulk::Bulk::Rename(p, r) => {
                let plan = crate::bulk::rename_plan(&scope, p, r);
                if plan.is_empty() {
                    return Some((format!("Rename {p} → {r}: nothing matches here"), tr("Patterns use * as a wildcard")));
                }
                let sample: Vec<(String, String)> = plan.iter().take(2).map(|(i, n)| (i.display_name(), n.clone())).collect();
                Some(crate::bulk::describe(&bulk, plan.len(), &sample))
            }
            crate::bulk::Bulk::Chmod(_) => {
                if pane.selected_items().is_empty() {
                    return Some((tr("chmod: select the files first"), String::new()));
                }
                Some(crate::bulk::describe(&bulk, pane.selected_items().len(), &[]))
            }
            _ => Some(crate::bulk::describe(&bulk, 0, &[])),
        }
    }

    pub fn run_bulk(self: &Rc<Self>, text: &str) {
        let Some(bulk) = crate::bulk::parse(text) else { return };
        let Some(pane) = self.active_pane() else { return };
        let cwd = pane.cwd();
        let selected = pane.selected_items();
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Some(w) = this.upgrade() else { return };
            match bulk.clone() {
                crate::bulk::Bulk::Rename(p, r) => {
                    let scope: Vec<FileItem> = if selected.is_empty() { (0..pane.selection.n_items()).filter_map(|i| pane.item_at(i)).collect() } else { selected };
                    let plan = crate::bulk::rename_plan(&scope, &p, &r);
                    let mut pairs = Vec::new();
                    let mut error = None;
                    for (item, new) in plan {
                        match ops::rename(item.file(), &new).await {
                            Ok(f) => {
                                w.tags.moved(&item.file(), &f);
                                w.notes.moved(&item.file(), &f);
                                pairs.push((item.file(), f));
                            }
                            Err(e) => {
                                error = Some(format!("{new}: {}", e.message()));
                                break;
                            }
                        }
                    }
                    pane.reload();
                    let n = pairs.len();
                    if n > 0 {
                        w.record(Op::Move(pairs), &format!("Renamed {n} {}", if n == 1 { tr("item") } else { tr("items") }));
                    }
                    if let Some(e) = error {
                        w.toast_error(&trf!("Rename stopped: {e}", e = e));
                    }
                }
                crate::bulk::Bulk::Chmod(mode) => {
                    let mut changes = Vec::new();
                    for item in &selected {
                        let old = item.mode();
                        let Some(new) = crate::bulk::apply_mode(old, &mode) else {
                            w.toast_error(&trf!("Cannot parse mode {mode}", mode = mode));
                            return;
                        };
                        if new == old {
                            continue;
                        }
                        if let Err(e) = item.target_file().set_attribute_uint32("unix::mode", new, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE) {
                            w.toast_error(&format!("{}: {}", item.display_name(), e.message()));
                            break;
                        }
                        changes.push((item.target_file(), old, new));
                    }
                    pane.reload();
                    let n = changes.len();
                    if n > 0 {
                        w.record(Op::Mode(changes), &format!("Changed mode on {n} {}", if n == 1 { tr("item") } else { tr("items") }));
                    }
                }
                crate::bulk::Bulk::Touch(names) | crate::bulk::Bulk::Mkdir(names) => {
                    let dirs = matches!(bulk, crate::bulk::Bulk::Mkdir(_));
                    let mut created = Vec::new();
                    for name in &names {
                        let f = ops::unique_child(&cwd, name);
                        let result = if dirs { ops::make_dir(f.clone()).await } else { ops::create_file(f.clone()).await };
                        match result {
                            Ok(()) => created.push((f, dirs)),
                            Err(e) => {
                                w.toast_error(&format!("{name}: {}", e.message()));
                                break;
                            }
                        }
                    }
                    pane.reload();
                    if let Some((last, _)) = created.last() {
                        pane.select_file_later(last.clone());
                    }
                    let n = created.len();
                    if n > 0 {
                        w.record(Op::Create(created), &format!("Created {n} {}", if dirs { if n == 1 { tr("folder") } else { tr("folders") } } else if n == 1 { tr("file") } else { tr("files") }));
                    }
                }
            }
        });
    }

    // ----- checksums ---------------------------------------------------------------

    pub fn verify_checksum(self: &Rc<Self>) {
        let Some(item) = self.selected_items().into_iter().next() else { return };
        let Some(path) = item.target_file().path() else { return };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Some(w) = this.upgrade() else { return };
            w.toast(&format!("Hashing {}…", item.display_name()));
            let Some(actual) = crate::checksum::sha256(path.clone()).await else {
                if let Some(w) = this.upgrade() {
                    w.toast_error(&tr("sha256sum failed"));
                }
                return;
            };
            let Some(w) = this.upgrade() else { return };
            let name = item.display_name();
            // 1. sidecar file, 2. clipboard
            for side in crate::checksum::sidecars(&path) {
                if let Ok(text) = std::fs::read_to_string(&side) {
                    if let Some(expected) = crate::checksum::expected_from(&text, &name) {
                        let ok = expected == actual;
                        w.checksum_result(&name, &actual, Some((&expected, &format!("from {}", name_of(&side)))), ok);
                        return;
                    }
                }
            }
            let clip = w.window.clipboard().read_text_future().await.ok().flatten().map(|t| t.to_string()).unwrap_or_default();
            match crate::checksum::looks_like_sha256(&clip) {
                Some(expected) => {
                    let ok = expected == actual;
                    w.checksum_result(&name, &actual, Some((&expected, "from the clipboard")), ok);
                }
                None => w.checksum_result(&name, &actual, None, true),
            }
        });
    }

    fn checksum_result(self: &Rc<Self>, name: &str, actual: &str, expected: Option<(&str, &str)>, ok: bool) {
        let (title, body) = match expected {
            Some((_, source)) if ok => (format!("✓ {name} verified"), format!("SHA-256 matches the value {source}.\n\n{actual}")),
            Some((exp, source)) => (format!("✗ {name} does NOT match"), format!("Expected ({source}):\n{exp}\n\nActual:\n{actual}")),
            None => (format!("SHA-256 of {name}"), format!("{actual}\n\nNo .sha256 file beside it and no hash on the clipboard to compare with.")),
        };
        let dialog = adw::AlertDialog::new(Some(&title), Some(&body));
        dialog.add_responses(&[("copy", tr("Copy Hash").as_str()), ("ok", tr("Close").as_str())]);
        dialog.set_default_response(Some("ok"));
        dialog.set_close_response("ok");
        if expected.is_some() && !ok {
            dialog.set_response_appearance("ok", adw::ResponseAppearance::Destructive);
        }
        let clip = self.window.clipboard();
        let actual = actual.to_string();
        let this = Rc::downgrade(self);
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |r| {
            if r == "copy" {
                clip.set_text(&actual);
            }
            if let Some(w) = this.upgrade() {
                w.refocus();
            }
        });
    }

    // ----- notes -------------------------------------------------------------------

    pub fn edit_note(self: &Rc<Self>) {
        let Some(item) = self.selected_items().into_iter().next() else { return };
        let file = item.target_file();
        let current = self.notes.get(&file).unwrap_or_default();
        let view = gtk::TextView::builder().wrap_mode(gtk::WrapMode::WordChar).top_margin(8).bottom_margin(8).left_margin(10).right_margin(10).build();
        view.buffer().set_text(&current);
        let scroll = gtk::ScrolledWindow::builder().child(&view).min_content_height(140).has_frame(true).build();
        scroll.set_width_request(420);
        let dialog = adw::AlertDialog::new(Some(&format!("Note on {}", item.display_name())), Some(tr("Stored with the file (GIO metadata, the same note Nautilus shows) and searchable in the palette. Ctrl+S saves.").as_str()));
        dialog.set_extra_child(Some(&scroll));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("clear", tr("Remove Note").as_str()), ("save", tr("Save").as_str())]);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        dialog.set_response_enabled("clear", !current.is_empty());
        let pane = self.active_pane();
        type Save = Rc<dyn Fn(&Rc<Window>, String)>;
        let save: Save = Rc::new(move |w: &Rc<Window>, text: String| match w.notes.set(&file, &text) {
            Ok(()) => {
                w.toast(&if text.trim().is_empty() { tr("Note removed") } else { tr("Note saved") });
                if let Some(p) = &pane {
                    p.reload();
                }
            }
            Err(e) => w.toast_error(&format!("Cannot save note: {}", e.message())),
        });
        // Ctrl+S saves from inside the text view (Enter makes a new line).
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let (this, d2, save2, view2) = (Rc::downgrade(self), dialog.clone(), save.clone(), view.clone());
        let saved = Rc::new(std::cell::Cell::new(false));
        let saved2 = saved.clone();
        keys.connect_key_pressed(move |_, key, _, state| {
            // Ctrl+Enter is the window's "open in new tab" accelerator, so Ctrl+S it is.
            if (key == gtk::gdk::Key::s || key == gtk::gdk::Key::S) && state.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
                if let Some(w) = this.upgrade() {
                    let b = view2.buffer();
                    saved2.set(true);
                    save2(&w, b.text(&b.start_iter(), &b.end_iter(), false).to_string());
                    d2.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        dialog.add_controller(keys);
        let this = Rc::downgrade(self);
        let view3 = view.clone();
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |r| {
            let Some(w) = this.upgrade() else { return };
            w.refocus();
            if saved.get() {
                return;
            }
            let text = match r.as_str() {
                "save" => {
                    let b = view3.buffer();
                    b.text(&b.start_iter(), &b.end_iter(), false).to_string()
                }
                "clear" => String::new(),
                _ => return,
            };
            save(&w, text);
        });
        view.grab_focus();
    }

    // ----- templates ---------------------------------------------------------------

    pub fn new_from_template(self: &Rc<Self>, index: usize) {
        let Some(pane) = self.active_pane() else { return };
        let Some(template) = crate::templates::list().into_iter().nth(index) else { return };
        let Some(dir) = pane.cwd().path() else {
            self.toast_error(&tr("Templates work in local folders"));
            return;
        };
        let entry = gtk::Entry::builder().text(crate::templates::default_name(&template)).activates_default(true).build();
        let dialog = adw::AlertDialog::new(Some(&format!("New from “{}”", crate::templates::label(&template))), Some(tr("Name for the new file or folder").as_str()));
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("create", tr("Create").as_str())]);
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("create"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let entry2 = entry.clone();
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |r| {
            let Some(w) = this.upgrade() else { return };
            w.refocus();
            if r != "create" {
                return;
            }
            let name = entry2.text().trim().to_string();
            if name.is_empty() || name.contains('/') {
                return;
            }
            let dest = ops::unique_child(&gio::File::for_path(&dir), &name).path().unwrap();
            match crate::templates::instantiate(&template, &dest) {
                Ok(()) => {
                    pane.reload();
                    let f = gio::File::for_path(&dest);
                    pane.select_file_later(f.clone());
                    w.record(Op::Derive(vec![f]), &format!("Created {}", name_of(&dest)));
                }
                Err(e) => w.toast_error(&trf!("Cannot create: {e}", e = e)),
            }
        });
        entry.grab_focus();
        let stem = entry.text().rfind('.').filter(|&i| i > 0).map(|i| i as i32).unwrap_or(-1);
        entry.select_region(0, stem);
    }

    pub fn open_templates(self: &Rc<Self>) {
        let dir = crate::templates::user_dir();
        let _ = std::fs::create_dir_all(&dir);
        self.open_in_new_tab(gio::File::for_path(dir), true);
    }

    // ----- omarchy theme export / install -----------------------------------------

    pub fn export_theme(self: &Rc<Self>) {
        let Some(item) = self.selected_items().into_iter().find(|i| i.is_dir()) else { return };
        let Some(dir) = item.target_file().path() else { return };
        let name = name_of(&dir);
        let downloads = glib::user_special_dir(glib::UserDirectory::Downloads).unwrap_or_else(glib::home_dir);
        let archive = ops::unique_child(&gio::File::for_path(&downloads), &format!("{name}.tar.gz")).path().unwrap();
        let this = Rc::downgrade(self);
        let parent = dir.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        glib::spawn_future_local(async move {
            let result = ops::run_command(&["bsdtar", "--exclude", ".git", "-czf", &archive.to_string_lossy(), "-C", &parent.to_string_lossy(), &name], None).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(()) => {
                    let f = gio::File::for_path(&archive);
                    w.record(Op::Derive(vec![f.clone()]), &format!("Exported theme to {}", crate::window::abbreviate_home(&archive)));
                    w.reveal(f);
                }
                Err(e) => w.toast_error(&format!("Export failed: {}", e.message())),
            }
        });
    }

    pub fn install_theme(self: &Rc<Self>) {
        let Some(item) = self.selected_items().into_iter().next() else { return };
        let Some(archive) = item.target_file().path() else { return };
        let name = crate::omarchy::archive_theme_name(&archive);
        let target = crate::omarchy::install_dir().join(&name);
        if target.exists() {
            self.toast_error(&trf!("A theme named {name} is already installed", name = name));
            return;
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Some(w) = this.upgrade() else { return };
            let into = crate::omarchy::install_dir();
            if let Err(e) = std::fs::create_dir_all(&into) {
                w.toast_error(&format!("Cannot create {}: {e}", into.display()));
                return;
            }
            // Archives with a single top folder land as that folder; flat ones get the archive's name.
            let flat = !archive_has_top_dir(&archive);
            let dest_dir = if flat { target.clone() } else { into.clone() };
            if flat {
                let _ = std::fs::create_dir_all(&dest_dir);
            }
            let result = ops::extract(&archive, &dest_dir).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(()) if target.is_dir() => {
                    w.record(Op::Derive(vec![gio::File::for_path(&target)]), &trf!("Installed theme {name}: omarchy-theme-set {name}", name = name));
                    w.refresh_sidebar();
                }
                Ok(()) => w.toast_error(&tr("Extracted, but no theme folder appeared")),
                Err(e) => w.toast_error(&format!("Install failed: {}", e.message())),
            }
        });
    }

    // ----- archives ----------------------------------------------------------------

    /// Mount an archive with GVFS and show it like a folder.
    pub fn browse_archive(self: &Rc<Self>, archive: &gio::File, pane: &Rc<Pane>) {
        // gvfsd-archive wants the file URI escaped twice (the host part of a URI).
        let once = glib::uri_escape_string(archive.uri().as_str(), None::<&str>, false);
        let escaped = glib::uri_escape_string(once.as_str(), None::<&str>, false);
        let mount_uri = format!("archive://{escaped}/");
        let root = gio::File::for_uri(&mount_uri);
        let op = gtk::MountOperation::new(Some(&self.window));
        let this = Rc::downgrade(self);
        let pane = pane.clone();
        let fallback = archive.clone();
        glib::spawn_future_local(async move {
            let result = crate::remote::ensure_mounted(&root, &op).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(()) => pane.navigate(root, true),
                Err(e) => {
                    // No gvfsd-archive: fall back to the default app.
                    if std::env::var_os("OMAFILE_TRACE").is_some() {
                        eprintln!("archive mount failed: {}", e.message());
                    }
                    w.launch(&fallback, false);
                }
            }
        });
    }

    pub fn browse_archive_selected(self: &Rc<Self>) {
        let (Some(pane), Some(item)) = (self.active_pane(), self.selected_items().into_iter().next()) else { return };
        self.browse_archive(&item.target_file(), &pane);
    }

    // ----- hyprland ----------------------------------------------------------------

    pub fn open_in_workspace(self: &Rc<Self>) {
        let dir = self.selected_items().into_iter().find(|i| i.is_dir()).and_then(|i| i.target_file().path()).or_else(|| self.active_pane().and_then(|p| p.cwd().path()));
        let Some(dir) = dir else { return };
        if crate::hypr::open_in_new_workspace(&dir) {
            self.toast(&tr("Opened on a new workspace"));
        } else {
            self.toast_error(&tr("Needs a running Hyprland session (hyprctl)"));
        }
    }
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string())
}

fn archive_has_top_dir(archive: &Path) -> bool {
    let Ok(out) = std::process::Command::new("bsdtar").arg("-tf").arg(archive).output() else { return false };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut tops = std::collections::BTreeSet::new();
    for l in text.lines() {
        match l.split_once('/') {
            Some((top, _)) => {
                tops.insert(top.to_string());
            }
            None => return false,
        }
    }
    tops.len() == 1
}
