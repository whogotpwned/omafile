//! Preferences, custom icons and batch rename.

use crate::i18n::tr;
use crate::item::FileItem;
use crate::ops;
use crate::pane::Pane;
use crate::prefs;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;

pub const ICON_CHOICES: &[&str] = &[
    "folder",
    "folder-documents",
    "folder-download",
    "folder-music",
    "folder-pictures",
    "folder-videos",
    "folder-templates",
    "folder-publicshare",
    "folder-remote",
    "folder-saved-search",
    "user-home",
    "user-desktop",
    "network-server",
    "drive-harddisk",
    "applications-development",
    "applications-games",
    "applications-graphics",
    "applications-internet",
    "applications-multimedia",
    "applications-office",
    "applications-science",
    "applications-system",
    "applications-utilities",
    "emblem-favorite",
    "emblem-important",
    "emblem-default",
    "security-high",
    "text-x-generic",
    "package-x-generic",
    "x-office-document",
];

impl Window {
    // ----- preferences --------------------------------------------------------------

    pub fn show_preferences(self: &Rc<Self>) {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title(&tr("Preferences"));
        let page = adw::PreferencesPage::new();

        // Icons
        let icons = adw::PreferencesGroup::builder().title(tr("Icons")).description(tr("Omarchy picks a Yaru variant per theme through icons.theme. Override it here if you prefer another icon set.")).build();
        let themes = prefs::installed_icon_themes();
        let mut names: Vec<String> = vec![format!("Follow Omarchy theme ({})", self.theme.palette().icon_theme.unwrap_or_else(|| tr("system default")))];
        names.extend(themes.iter().cloned());
        let model = gtk::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let row = adw::ComboRow::builder().title(tr("Icon theme")).model(&model).build();
        let current = self.prefs.get("icon-theme");
        if let Some(cur) = &current {
            if let Some(i) = themes.iter().position(|t| t == cur) {
                row.set_selected(i as u32 + 1);
            }
        }
        let this = Rc::downgrade(self);
        row.connect_selected_notify(move |row| {
            let Some(w) = this.upgrade() else { return };
            let selected = row.selected() as usize;
            let value = if selected == 0 { None } else { themes.get(selected - 1).cloned() };
            w.prefs.set("icon-theme", value.as_deref());
            w.theme.set_icon_override(value);
        });
        icons.add(&row);
        page.add(&icons);

        // Views
        let views = adw::PreferencesGroup::builder().title(tr("Views")).build();
        let thumbs = adw::SwitchRow::builder().title(tr("Thumbnails")).subtitle(tr("Preview images and PDFs in the list and grid")).active(self.prefs.get_bool("thumbnails", true)).build();
        let this = Rc::downgrade(self);
        thumbs.connect_active_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("thumbnails", Some(if row.is_active() { "true" } else { "false" }));
                w.thumbnails_enabled.set(row.is_active());
                for p in w.all_panes() {
                    p.reload();
                }
            }
        });
        views.add(&thumbs);
        let default_view = adw::ComboRow::builder().title(tr("Default view")).model(&gtk::StringList::new(&[tr("List").as_str(), tr("Grid").as_str()])).build();
        default_view.set_selected(if self.prefs.get("view").as_deref() == Some("grid") { 1 } else { 0 });
        let this = Rc::downgrade(self);
        default_view.connect_selected_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("view", Some(if row.selected() == 1 { "grid" } else { "list" }));
            }
        });
        views.add(&default_view);
        let hidden = adw::SwitchRow::builder().title(tr("Show hidden files at start")).active(self.prefs.get_bool("show-hidden", false)).build();
        let this = Rc::downgrade(self);
        hidden.connect_active_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("show-hidden", Some(if row.is_active() { "true" } else { "false" }));
            }
        });
        views.add(&hidden);
        let session = adw::SwitchRow::builder().title(tr("Restore tabs on start")).subtitle(tr("Reopen the last tabs, split view and terminal panels")).active(self.prefs.get_bool("restore-session", true)).build();
        let this = Rc::downgrade(self);
        session.connect_active_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("restore-session", Some(if row.is_active() { "true" } else { "false" }));
                if !row.is_active() {
                    let _ = std::fs::remove_file(crate::session::path());
                }
            }
        });
        views.add(&session);
        let archives = adw::SwitchRow::builder().title(tr("Open archives in place")).subtitle(tr("Enter on a .zip / .tar browses inside it instead of launching an app")).active(self.prefs.get_bool("browse-archives", true)).build();
        let this = Rc::downgrade(self);
        archives.connect_active_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("browse-archives", Some(if row.is_active() { "true" } else { "false" }));
            }
        });
        views.add(&archives);
        let venv = adw::SwitchRow::builder().title(tr("Activate Python venvs in the terminal panel")).subtitle(tr("Entering a project with a .venv sources it, leaving runs deactivate")).active(self.prefs.get_bool("venv-auto-activate", true)).build();
        let this = Rc::downgrade(self);
        venv.connect_active_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("venv-auto-activate", Some(if row.is_active() { "true" } else { "false" }));
            }
        });
        views.add(&venv);
        let remember = adw::SwitchRow::builder().title(tr("Remember view per folder")).subtitle(tr("List or grid, sort order, grouping and icon size come back when you return to a folder")).active(self.prefs.get_bool("remember-views", true)).build();
        let this = Rc::downgrade(self);
        remember.connect_active_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("remember-views", Some(if row.is_active() { "true" } else { "false" }));
            }
        });
        views.add(&remember);
        page.add(&views);

        let keys = adw::PreferencesGroup::builder().title(tr("Keys")).build();
        let mac = adw::SwitchRow::builder().title(tr("macOS keys")).subtitle(tr("Enter renames, Ctrl+O or Ctrl+↓ opens, Ctrl+↑ goes up")).active(self.prefs.get_bool("mac-keys", false)).build();
        let this = Rc::downgrade(self);
        mac.connect_active_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("mac-keys", Some(if row.is_active() { "true" } else { "false" }));
            }
        });
        keys.add(&mac);
        let vim = adw::ExpanderRow::builder().title(tr("Vim keys")).subtitle(tr("h j k l move and open, gg / G jump, / filters, : opens the palette; only while the file list has focus")).show_enable_switch(true).enable_expansion(crate::vimkeys::enabled(&self.prefs)).build();
        let this = Rc::downgrade(self);
        vim.connect_enable_expansion_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("vim-keys", Some(if row.enables_expansion() { "true" } else { "false" }));
            }
        });
        for (id, keys_text, what) in crate::vimkeys::GROUPS {
            let row = adw::SwitchRow::builder().title(keys_text.to_string()).subtitle(tr(what)).active(!crate::vimkeys::disabled(&self.prefs).iter().any(|g| g == id)).build();
            row.add_css_class("monospace");
            let this = Rc::downgrade(self);
            row.connect_active_notify(move |r| {
                if let Some(w) = this.upgrade() {
                    crate::vimkeys::set_group(&w.prefs, id, r.is_active());
                }
            });
            vim.add_row(&row);
        }
        keys.add(&vim);
        page.add(&keys);

        let trash = adw::PreferencesGroup::builder().title(tr("Trash")).build();
        let auto = adw::SpinRow::with_range(0.0, 365.0, 1.0);
        auto.set_title(&tr("Delete trashed items after … days"));
        auto.set_subtitle(&tr("0 keeps everything until you empty the trash; runs when omafile starts"));
        auto.set_value(self.prefs.get("trash-auto-days").and_then(|v| v.parse().ok()).unwrap_or(0.0));
        let this = Rc::downgrade(self);
        auto.connect_value_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("trash-auto-days", Some(&(row.value() as u32).to_string()));
            }
        });
        trash.add(&auto);
        page.add(&trash);

        dialog.add(&page);
        self.present_dialog(dialog.upcast_ref());
    }

    // ----- custom icons --------------------------------------------------------------

    pub fn show_icon_chooser(self: &Rc<Self>, item: &FileItem) {
        let file = item.target_file();
        if !file.is_native() {
            self.toast_error(&tr("Custom icons work on local files only"));
            return;
        }
        let flow = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .max_children_per_line(6)
            .min_children_per_line(4)
            .column_spacing(6)
            .row_spacing(6)
            .homogeneous(true)
            .build();
        for name in ICON_CHOICES {
            let image = gtk::Image::from_icon_name(name);
            image.set_pixel_size(40);
            let child = gtk::FlowBoxChild::new();
            child.set_child(Some(&image));
            child.set_tooltip_text(Some(name));
            child.add_css_class("omafile-icon-choice");
            flow.insert(&child, -1);
        }
        let hint = gtk::Label::builder()
            .label(tr("Pick an icon from the current icon theme or use your own image. Nautilus shows the same icon, it is stored as file metadata."))
            .wrap(true)
            .xalign(0.0)
            .css_classes(["omafile-dim", "caption"])
            .build();
        let image_btn = gtk::Button::with_label(&tr("Choose Image…"));
        let reset_btn = gtk::Button::with_label(&tr("Reset to Default"));
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        buttons.append(&reset_btn);
        buttons.append(&image_btn);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(6);
        content.set_margin_bottom(14);
        content.set_margin_start(14);
        content.set_margin_end(14);
        content.append(&hint);
        content.append(&flow);
        content.append(&buttons);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        view.set_content(Some(&content));
        let dialog = adw::Dialog::builder().title(format!("Icon for {}", item.display_name())).content_width(420).child(&view).build();

        let this = Rc::downgrade(self);
        let file2 = file.clone();
        let dialog2 = dialog.clone();
        flow.connect_child_activated(move |_, child| {
            let name = ICON_CHOICES[child.index() as usize];
            if let Some(w) = this.upgrade() {
                w.set_custom_icon(&file2, Some(("metadata::custom-icon-name", name.to_string())));
            }
            dialog2.close();
        });
        let this = Rc::downgrade(self);
        let file2 = file.clone();
        let dialog2 = dialog.clone();
        reset_btn.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.set_custom_icon(&file2, None);
            }
            dialog2.close();
        });
        let this = Rc::downgrade(self);
        let file2 = file.clone();
        let dialog2 = dialog.clone();
        image_btn.connect_clicked(move |_| {
            let Some(w) = this.upgrade() else { return };
            let filter = gtk::FileFilter::new();
            filter.add_mime_type("image/*");
            filter.set_name(Some(tr("Images").as_str()));
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let chooser = gtk::FileDialog::builder().title(tr("Choose an icon image")).modal(true).filters(&filters).build();
            let this = Rc::downgrade(&w);
            let file2 = file2.clone();
            let dialog2 = dialog2.clone();
            chooser.open(Some(&w.window), gio::Cancellable::NONE, move |result| {
                if let (Ok(picked), Some(w)) = (result, this.upgrade()) {
                    w.set_custom_icon(&file2, Some(("metadata::custom-icon", picked.uri().to_string())));
                    dialog2.close();
                }
            });
        });
        self.present_dialog(&dialog);
    }

    fn set_custom_icon(self: &Rc<Self>, file: &gio::File, value: Option<(&str, String)>) {
        let result = match &value {
            Some((attr, v)) => {
                let other = if *attr == "metadata::custom-icon" { "metadata::custom-icon-name" } else { "metadata::custom-icon" };
                unset_attribute(file, other).and_then(|_| file.set_attribute_string(attr, v, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE))
            }
            None => unset_attribute(file, "metadata::custom-icon").and_then(|_| unset_attribute(file, "metadata::custom-icon-name")),
        };
        match result {
            Ok(()) => {
                self.toast(&if value.is_some() { tr("Icon changed") } else { tr("Icon reset") });
                for p in self.all_panes() {
                    p.reload();
                }
                self.refresh_sidebar();
            }
            Err(e) => self.toast_error(&format!("Cannot change icon: {}", e.message())),
        }
    }

    // ----- batch rename ---------------------------------------------------------------

    pub fn show_batch_rename(self: &Rc<Self>, pane: Rc<Pane>, items: Vec<FileItem>) {
        let pattern = adw::EntryRow::builder().title(tr("Name pattern")).text("[name]").build();
        let find = adw::EntryRow::builder().title(tr("Find")).build();
        let replace = adw::EntryRow::builder().title(tr("Replace with")).build();
        let regex = adw::SwitchRow::builder().title(tr("Find is a regular expression")).subtitle(tr("Capture groups: $1, $2 … in Replace")).build();
        let start = adw::SpinRow::with_range(0.0, 99_999.0, 1.0);
        start.set_title(&tr("First number"));
        start.set_value(1.0);
        let group = adw::PreferencesGroup::builder()
            .title(format!("Rename {} items", items.len()))
            .description("[name] original name, [ext] extension, [N] running number ([N:3] zero-padded), [date] modification date, [exif] photo date, [artist] [title] [album] [track] from audio tags. Find/replace runs afterwards.")
            .build();
        group.add(&pattern);
        group.add(&find);
        group.add(&replace);
        group.add(&regex);
        group.add(&start);

        let preview_list = gtk::ListBox::new();
        preview_list.add_css_class("boxed-list");
        preview_list.set_selection_mode(gtk::SelectionMode::None);
        let preview_group = adw::PreferencesGroup::builder().title(tr("Preview")).build();
        preview_group.add(&preview_list);

        let page = adw::PreferencesPage::new();
        page.add(&group);
        page.add(&preview_group);

        let apply = gtk::Button::with_label("Rename");
        apply.add_css_class("suggested-action");
        let header = adw::HeaderBar::new();
        header.pack_end(&apply);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&page));
        let dialog = adw::Dialog::builder().title(tr("Batch Rename")).content_width(560).content_height(640).child(&view).build();

        let items = Rc::new(items);
        let compute = {
            let items = items.clone();
            let (pattern, find, replace, start, regex) = (pattern.clone(), find.clone(), replace.clone(), start.clone(), regex.clone());
            let meta = Rc::new(std::cell::RefCell::new(std::collections::HashMap::<String, Metadata>::new()));
            move || -> Vec<(String, String, bool)> {
                let wants_meta = ["[exif]", "[artist]", "[title]", "[album]", "[track]"].iter().any(|t| pattern.text().contains(t));
                if wants_meta {
                    let mut cache = meta.borrow_mut();
                    for item in items.iter() {
                        if let Some(path) = item.target_file().path() {
                            cache.entry(path.to_string_lossy().to_string()).or_insert_with(|| read_metadata(&path));
                        }
                    }
                }
                let names = batch_names_ext(&items, &pattern.text(), &find.text(), &replace.text(), start.value() as u32, regex.is_active(), &meta.borrow());
                let mut seen = std::collections::HashSet::new();
                names
                    .into_iter()
                    .zip(items.iter())
                    .map(|(new, item)| {
                        let old = item.display_name();
                        let invalid = new.is_empty() || new.contains('/') || !seen.insert(new.clone());
                        (old, new, invalid)
                    })
                    .collect()
            }
        };
        let refresh = {
            let compute = compute.clone();
            let preview_list = preview_list.clone();
            let apply = apply.clone();
            move || {
                while let Some(row) = preview_list.row_at_index(0) {
                    preview_list.remove(&row);
                }
                let rows = compute();
                let mut ok = true;
                let mut changes = 0;
                for (old, new, invalid) in rows.iter().take(200) {
                    let row = adw::ActionRow::builder().title(glib::markup_escape_text(new)).subtitle(glib::markup_escape_text(old)).build();
                    if *invalid {
                        row.add_css_class("error");
                        row.set_title(&format!("{} (conflict)", glib::markup_escape_text(new)));
                        ok = false;
                    }
                    if old != new {
                        changes += 1;
                    }
                    preview_list.append(&row);
                }
                apply.set_sensitive(ok && changes > 0);
            }
        };
        refresh();
        for entry in [&pattern, &find, &replace] {
            let refresh = refresh.clone();
            entry.connect_changed(move |_| refresh());
        }
        let refresh2 = refresh.clone();
        start.connect_value_notify(move |_| refresh2());
        let refresh3 = refresh.clone();
        regex.connect_active_notify(move |_| refresh3());

        let this = Rc::downgrade(self);
        let dialog2 = dialog.clone();
        apply.connect_clicked(move |_| {
            let Some(w) = this.upgrade() else { return };
            let plan: Vec<(gio::File, String)> = compute().into_iter().zip(items.iter()).filter(|((old, new, _), _)| old != new).map(|((_, new, _), item)| (item.file(), new)).collect();
            dialog2.close();
            let pane = pane.clone();
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let mut error = None;
                // Two passes through temporary names avoid A→B, B→A collisions.
                let mut staged = Vec::new();
                for (index, (file, new)) in plan.iter().enumerate() {
                    let tmp = format!(".omafile-rename-{}-{index}", std::process::id());
                    match ops::rename(file.clone(), &tmp).await {
                        Ok(f) => staged.push((f, new.clone())),
                        Err(e) => {
                            error = Some(e.message().to_string());
                            break;
                        }
                    }
                }
                let mut renamed = 0;
                let mut pairs = Vec::new();
                let originals: Vec<gio::File> = plan.iter().map(|(f, _)| f.clone()).collect();
                for ((file, new), original) in staged.into_iter().zip(originals) {
                    match ops::rename(file, &new).await {
                        Ok(renamed_file) => {
                            if let Some(w) = this.upgrade() {
                                w.tags.moved(&original, &renamed_file);
                            }
                            pairs.push((original, renamed_file));
                            renamed += 1;
                        }
                        Err(e) => {
                            error.get_or_insert_with(|| format!("{new}: {}", e.message()));
                        }
                    };
                }
                let Some(w) = this.upgrade() else { return };
                pane.reload();
                match error {
                    Some(e) => w.toast_error(&trf!("Rename failed: {e}", e = e)),
                    None => w.record(crate::undo::Op::Move(pairs), &trf!("Renamed {renamed} items", renamed = renamed)),
                }
            });
        });
        self.present_dialog(&dialog);
    }
}

fn unset_attribute(file: &gio::File, attribute: &str) -> Result<(), glib::Error> {
    use glib::translate::*;
    unsafe {
        let mut error = std::ptr::null_mut();
        let ok = gio::ffi::g_file_set_attribute(
            file.to_glib_none().0,
            attribute.to_glib_none().0,
            gio::ffi::G_FILE_ATTRIBUTE_TYPE_INVALID,
            std::ptr::null_mut(),
            gio::ffi::G_FILE_QUERY_INFO_NONE,
            std::ptr::null_mut(),
            &mut error,
        );
        if ok == glib::ffi::GFALSE {
            Err(from_glib_full(error))
        } else {
            Ok(())
        }
    }
}

/// Tags read from a photo (EXIF date) or an audio file (ffprobe), for rename patterns.
#[derive(Default, Clone, Debug)]
pub struct Metadata {
    pub exif_date: String,
    pub artist: String,
    pub title: String,
    pub album: String,
    pub track: String,
}

pub fn read_metadata(path: &std::path::Path) -> Metadata {
    let mut m = Metadata::default();
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    if matches!(ext.as_str(), "jpg" | "jpeg" | "tif" | "tiff" | "heic" | "png" | "webp" | "dng" | "cr2" | "nef" | "arw") {
        for tool in ["magick", "identify"] {
            if glib::find_program_in_path(tool).is_none() {
                continue;
            }
            let mut cmd = std::process::Command::new(tool);
            if tool == "magick" {
                cmd.arg("identify");
            }
            if let Ok(out) = cmd.args(["-format", "%[EXIF:DateTimeOriginal]"]).arg(path).stderr(std::process::Stdio::null()).output() {
                let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
                // 2024:06:01 12:30:00 → 2024-06-01
                if raw.len() >= 10 {
                    m.exif_date = raw[..10].replace(':', "-");
                }
                break;
            }
        }
    } else if glib::find_program_in_path("ffprobe").is_some() {
        if let Ok(out) = std::process::Command::new("ffprobe").args(["-v", "quiet", "-show_entries", "format_tags=artist,title,album,track", "-of", "default=noprint_wrappers=1"]).arg(path).output() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                let Some((k, v)) = line.split_once('=') else { continue };
                let k = k.trim_start_matches("TAG:").to_ascii_lowercase();
                let v = v.trim().replace('/', "-");
                match k.as_str() {
                    "artist" => m.artist = v,
                    "title" => m.title = v,
                    "album" => m.album = v,
                    "track" => m.track = v.split('/').next().unwrap_or("").to_string(),
                    _ => {}
                }
            }
        }
    }
    m
}

/// New names for `items` from a pattern, then a plain find/replace.
#[cfg(test)]
pub fn batch_names(items: &[FileItem], pattern: &str, find: &str, replace: &str, start: u32) -> Vec<String> {
    batch_names_ext(items, pattern, find, replace, start, false, &std::collections::HashMap::new())
}

/// Full variant: `[N:3]` padding, metadata tokens and an optional regex find.
pub fn batch_names_ext(items: &[FileItem], pattern: &str, find: &str, replace: &str, start: u32, use_regex: bool, meta: &std::collections::HashMap<String, Metadata>) -> Vec<String> {
    let re = if use_regex && !find.is_empty() { regex::Regex::new(find).ok() } else { None };
    let pad_re = regex::Regex::new(r"\[N:(\d+)\]").unwrap();
    let width = ((start as usize + items.len()).max(1)).to_string().len().max(2);
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let full = item.display_name();
            let (stem, ext) = if item.is_dir() {
                (full.clone(), String::new())
            } else {
                match full.rsplit_once('.') {
                    Some((s, e)) if !s.is_empty() => (s.to_string(), e.to_string()),
                    _ => (full.clone(), String::new()),
                }
            };
            let date = item.modified().and_then(|d| d.format("%Y-%m-%d").ok()).map(|s| s.to_string()).unwrap_or_default();
            let number = format!("{:0width$}", start as usize + i, width = width);
            let md = item.target_file().path().and_then(|p| meta.get(p.to_string_lossy().as_ref()).cloned()).unwrap_or_default();
            let mut name = pattern.replace("[name]", &stem).replace("[N]", &number).replace("[date]", &date);
            name = pad_re.replace_all(&name, |c: &regex::Captures| {
                let w: usize = c[1].parse().unwrap_or(2);
                format!("{:0w$}", start as usize + i, w = w)
            }).to_string();
            let exif = if md.exif_date.is_empty() { date.clone() } else { md.exif_date.clone() };
            name = name.replace("[exif]", &exif).replace("[artist]", &md.artist).replace("[title]", &md.title).replace("[album]", &md.album).replace("[track]", &md.track);
            if name.contains("[ext]") {
                name = name.replace("[ext]", &ext);
            } else if !ext.is_empty() && pattern.contains("[name]") {
                name = format!("{name}.{ext}");
            }
            if let Some(re) = &re {
                name = re.replace_all(&name, replace).to_string();
            } else if !find.is_empty() {
                name = name.replace(find, replace);
            }
            name.trim().to_string()
        })
        .collect()
}
