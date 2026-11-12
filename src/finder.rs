//! Small Finder habits: New Folder with Selection, aliases and Show Original,
//! Always Open With…, Properties for several items.

use crate::i18n::tr;
use crate::ops;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;

impl Window {
    /// Finder's "New Folder with Selection": asks for a name, moves the items in.
    pub fn new_folder_with_selection(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let items = pane.selected_items();
        if items.is_empty() {
            return;
        }
        let n = items.len();
        let entry = gtk::Entry::builder().text(if n == 1 { tr("New Folder With 1 Item") } else { trf!("New Folder With {n} Items", n = n) }).activates_default(true).build();
        let dialog = adw::AlertDialog::new(Some(tr("New folder with selection").as_str()), Some(tr("The selected items move into the new folder.").as_str()));
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
            let dest = ops::unique_child(&pane.cwd(), &name);
            let files: Vec<gio::File> = items.iter().map(|i| i.target_file()).collect();
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let Some(w) = this.upgrade() else { return };
                if let Err(e) = ops::make_dir(dest.clone()).await {
                    w.toast_error(&format!("Cannot create folder: {}", e.message()));
                    return;
                }
                w.record(Op::Create(vec![(dest.clone(), true)]), &format!("Created {}", crate::window::display_name(&dest)));
                w.transfer(files, dest, crate::transfer::Mode::Move, pane, false);
            });
        });
        entry.grab_focus();
        entry.select_region(0, -1);
    }

    /// A symlink `name alias` next to the original (Finder's Make Alias).
    pub fn make_alias(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let items = pane.selected_items();
        if items.is_empty() {
            return;
        }
        let mut created = Vec::new();
        for item in &items {
            let Some(target) = item.target_file().path() else { continue };
            let (stem, ext) = match target.file_name().and_then(|n| n.to_str()).and_then(|n| n.rsplit_once('.')).filter(|_| !item.is_dir()) {
                Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
                _ => (target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), String::new()),
            };
            let link = ops::unique_child(&pane.cwd(), &format!("{stem} alias{ext}"));
            let Some(link_path) = link.path() else { continue };
            match std::os::unix::fs::symlink(&target, &link_path) {
                Ok(()) => created.push(link),
                Err(e) => {
                    self.toast_error(&trf!("Cannot create alias: {e}", e = e));
                    break;
                }
            }
        }
        pane.reload();
        let n = created.len();
        if n > 0 {
            if let Some(last) = created.last() {
                pane.select_file_later(last.clone());
            }
            self.record(Op::Derive(created), &format!("Created {n} {}", if n == 1 { tr("alias") } else { tr("aliases") }));
        }
    }

    /// For a symlink: go to the folder of its target and select it.
    pub fn show_original(self: &Rc<Self>) {
        let Some(item) = self.selected_items().into_iter().find(|i| i.is_symlink()) else {
            self.toast(&tr("Select an alias (symbolic link) first"));
            return;
        };
        let Some(path) = item.file().path() else { return };
        match std::fs::canonicalize(&path) {
            Ok(real) => self.reveal(gio::File::for_path(real)),
            Err(e) => self.toast_error(&trf!("The original is gone: {e}", e = e)),
        }
    }

    /// Pick an application for this file type and remember it as the default.
    pub fn always_open_with(self: &Rc<Self>) {
        let Some(item) = self.selected_items().into_iter().next() else { return };
        let Some(ct) = item.content_type() else { return };
        let apps = gio::AppInfo::all_for_type(&ct);
        let all = gio::AppInfo::all();
        let default = gio::AppInfo::default_for_type(&ct, false);
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::None);
        let mut shown: Vec<gio::AppInfo> = apps.clone();
        for a in all {
            if a.should_show() && !shown.iter().any(|s| s.id() == a.id()) {
                shown.push(a);
            }
        }
        let description = gio::content_type_get_description(&ct);
        let dialog = adw::Dialog::builder().title(tr("Always Open With")).content_width(420).content_height(560).build();
        let remember = gtk::CheckButton::with_label(&trf!("Always use it for {description}", description = description));
        remember.set_active(true);
        remember.set_margin_start(12);
        remember.set_margin_bottom(6);
        for (i, app) in shown.iter().enumerate() {
            if i == apps.len() && i > 0 {
                let sep = adw::ActionRow::builder().title(tr("Other applications")).build();
                sep.add_css_class("dim-label");
                sep.set_activatable(false);
                list.append(&sep);
            }
            let row = adw::ActionRow::builder().title(glib::markup_escape_text(&app.display_name())).activatable(true).build();
            if let Some(icon) = app.icon() {
                row.add_prefix(&gtk::Image::from_gicon(&icon));
            }
            if default.as_ref().map(|d| d.id() == app.id()).unwrap_or(false) {
                row.set_subtitle(&tr("current default"));
            }
            let (app, ct, this, dialog, remember, file) = (app.clone(), ct.clone(), Rc::downgrade(self), dialog.clone(), remember.clone(), item.target_file());
            row.connect_activated(move |_| {
                let Some(w) = this.upgrade() else { return };
                if remember.is_active() {
                    match app.set_as_default_for_type(&ct) {
                        Ok(()) => w.toast(&format!("{} now opens {}", app.display_name(), gio::content_type_get_description(&ct))),
                        Err(e) => w.toast_error(&format!("Cannot set default: {}", e.message())),
                    }
                }
                let ctx = gio::AppLaunchContext::new();
                if let Err(e) = app.launch(std::slice::from_ref(&file), Some(&ctx)) {
                    w.toast_error(&format!("Cannot launch: {}", e.message()));
                }
                dialog.close();
            });
            list.append(&row);
        }
        let scroll = gtk::ScrolledWindow::builder().child(&list).vexpand(true).build();
        scroll.set_margin_start(12);
        scroll.set_margin_end(12);
        scroll.set_margin_bottom(6);
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&adw::WindowTitle::new(&tr("Open With"), &item.display_name())));
        let content = adw::ToolbarView::new();
        content.add_top_bar(&header);
        content.set_content(Some(&scroll));
        content.add_bottom_bar(&remember);
        dialog.set_child(Some(&content));
        self.present_dialog(&dialog);
    }

    /// Properties for several items: count, total size, kinds, shared tag.
    pub fn show_multi_properties(self: &Rc<Self>, items: Vec<crate::item::FileItem>) {
        let n = items.len();
        let folders = items.iter().filter(|i| i.is_dir()).count();
        let files = n - folders;
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::new();
        let row = |title: &str, subtitle: &str| {
            let r = adw::ActionRow::builder().title(glib::markup_escape_text(title)).subtitle(glib::markup_escape_text(subtitle)).build();
            r.add_css_class("property");
            r
        };
        group.add(&row("Selection", &format!("{n} items: {folders} {}, {files} {}", if folders == 1 { tr("folder") } else { tr("folders") }, if files == 1 { tr("file") } else { tr("files") })));
        let size_row = row("Total size", &tr("Calculating…"));
        group.add(&size_row);
        let mut kinds: std::collections::BTreeMap<String, usize> = Default::default();
        for i in &items {
            *kinds.entry(i.type_description()).or_default() += 1;
        }
        let kinds_text: Vec<String> = kinds.iter().map(|(k, c)| format!("{c} × {k}")).collect();
        group.add(&row("Kinds", &kinds_text.join(", ")));
        let tags: std::collections::BTreeSet<String> = items.iter().filter_map(|i| self.tags.get(&i.file())).collect();
        if !tags.is_empty() {
            group.add(&row("Tags", &tags.iter().map(|t| crate::tags::label(t)).collect::<Vec<_>>().join(", ")));
        }
        if let Some(parent) = items[0].target_file().parent() {
            group.add(&row("Location", &crate::window::location_text(&parent)));
        }
        page.add(&group);
        let files_for_size: Vec<gio::File> = items.iter().map(|i| i.target_file()).collect();
        let dirs: Vec<bool> = items.iter().map(|i| i.is_dir()).collect();
        let sizes: Vec<u64> = items.iter().map(|i| i.size()).collect();
        glib::spawn_future_local(async move {
            let mut total = 0u64;
            for ((f, is_dir), size) in files_for_size.iter().zip(dirs).zip(sizes) {
                total += if is_dir { ops::dir_size(f).await } else { size };
            }
            size_row.set_subtitle(&format!("{} ({total} bytes)", ops::human_size(total)));
        });
        let content = adw::ToolbarView::new();
        content.add_top_bar(&adw::HeaderBar::new());
        content.set_content(Some(&page));
        let dialog = adw::Dialog::builder().title(format!("{n} items")).content_width(460).content_height(420).child(&content).build();
        self.present_dialog(&dialog);
    }
}
