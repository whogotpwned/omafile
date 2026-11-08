//! Preferences, custom icons and batch rename.

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
        dialog.set_title("Preferences");
        let page = adw::PreferencesPage::new();

        // Icons
        let icons = adw::PreferencesGroup::builder().title("Icons").description("Omarchy picks a Yaru variant per theme through icons.theme. Override it here if you prefer another icon set.").build();
        let themes = prefs::installed_icon_themes();
        let mut names: Vec<String> = vec![format!("Follow Omarchy theme ({})", self.theme.palette().icon_theme.unwrap_or_else(|| "system default".into()))];
        names.extend(themes.iter().cloned());
        let model = gtk::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let row = adw::ComboRow::builder().title("Icon theme").model(&model).build();
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
        let views = adw::PreferencesGroup::builder().title("Views").build();
        let thumbs = adw::SwitchRow::builder().title("Thumbnails").subtitle("Preview images and PDFs in the list and grid").active(self.prefs.get_bool("thumbnails", true)).build();
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
        let default_view = adw::ComboRow::builder().title("Default view").model(&gtk::StringList::new(&["List", "Grid"])).build();
        default_view.set_selected(if self.prefs.get("view").as_deref() == Some("grid") { 1 } else { 0 });
        let this = Rc::downgrade(self);
        default_view.connect_selected_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("view", Some(if row.selected() == 1 { "grid" } else { "list" }));
            }
        });
        views.add(&default_view);
        let hidden = adw::SwitchRow::builder().title("Show hidden files at start").active(self.prefs.get_bool("show-hidden", false)).build();
        let this = Rc::downgrade(self);
        hidden.connect_active_notify(move |row| {
            if let Some(w) = this.upgrade() {
                w.prefs.set("show-hidden", Some(if row.is_active() { "true" } else { "false" }));
            }
        });
        views.add(&hidden);
        page.add(&views);

        dialog.add(&page);
        dialog.present(Some(&self.window));
    }

    // ----- custom icons --------------------------------------------------------------

    pub fn show_icon_chooser(self: &Rc<Self>, item: &FileItem) {
        let file = item.target_file();
        if !file.is_native() {
            self.toast_error("Custom icons work on local files only");
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
            .label("Pick an icon from the current icon theme or use your own image. Nautilus shows the same icon, it is stored as file metadata.")
            .wrap(true)
            .xalign(0.0)
            .css_classes(["omafile-dim", "caption"])
            .build();
        let image_btn = gtk::Button::with_label("Choose Image…");
        let reset_btn = gtk::Button::with_label("Reset to Default");
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
            filter.set_name(Some("Images"));
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let chooser = gtk::FileDialog::builder().title("Choose an icon image").modal(true).filters(&filters).build();
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
        dialog.present(Some(&self.window));
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
                self.toast(if value.is_some() { "Icon changed" } else { "Icon reset" });
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
        let pattern = adw::EntryRow::builder().title("Name pattern").text("[name]").build();
        let find = adw::EntryRow::builder().title("Find").build();
        let replace = adw::EntryRow::builder().title("Replace with").build();
        let start = adw::SpinRow::with_range(0.0, 99_999.0, 1.0);
        start.set_title("First number");
        start.set_value(1.0);
        let group = adw::PreferencesGroup::builder()
            .title(format!("Rename {} items", items.len()))
            .description("[name] original name, [ext] extension, [N] running number, [date] modification date. Find/replace runs afterwards.")
            .build();
        group.add(&pattern);
        group.add(&find);
        group.add(&replace);
        group.add(&start);

        let preview_list = gtk::ListBox::new();
        preview_list.add_css_class("boxed-list");
        preview_list.set_selection_mode(gtk::SelectionMode::None);
        let preview_group = adw::PreferencesGroup::builder().title("Preview").build();
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
        let dialog = adw::Dialog::builder().title("Batch Rename").content_width(560).content_height(640).child(&view).build();

        let items = Rc::new(items);
        let compute = {
            let items = items.clone();
            let (pattern, find, replace, start) = (pattern.clone(), find.clone(), replace.clone(), start.clone());
            move || -> Vec<(String, String, bool)> {
                let names = batch_names(&items, &pattern.text(), &find.text(), &replace.text(), start.value() as u32);
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
                for (file, new) in staged {
                    match ops::rename(file, &new).await {
                        Ok(_) => renamed += 1,
                        Err(e) => {
                            error.get_or_insert_with(|| format!("{new}: {}", e.message()));
                        }
                    };
                }
                let Some(w) = this.upgrade() else { return };
                pane.reload();
                match error {
                    Some(e) => w.toast_error(&format!("Rename failed: {e}")),
                    None => w.toast(&format!("Renamed {renamed} items")),
                }
            });
        });
        dialog.present(Some(&self.window));
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

/// New names for `items` from a pattern, then a plain find/replace.
pub fn batch_names(items: &[FileItem], pattern: &str, find: &str, replace: &str, start: u32) -> Vec<String> {
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
            let mut name = pattern.replace("[name]", &stem).replace("[N]", &number).replace("[date]", &date);
            if name.contains("[ext]") {
                name = name.replace("[ext]", &ext);
            } else if !ext.is_empty() && pattern.contains("[name]") {
                name = format!("{name}.{ext}");
            }
            if !find.is_empty() {
                name = name.replace(find, replace);
            }
            name.trim().to_string()
        })
        .collect()
}
