//! The Folder Rules dialog: rules for the folder on show, add / edit / delete,
//! enable switches and Run Now.

use crate::i18n::tr;
use crate::rules::{self, Action, Rule};
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::path::PathBuf;
use std::rc::Rc;

impl Window {
    pub fn open_rules(self: &Rc<Self>) {
        let Some(folder) = self.active_pane().and_then(|p| p.cwd().path()) else {
            self.toast_error(&tr("Rules work on local folders"));
            return;
        };
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&adw::WindowTitle::new(&tr("Folder Rules"), &rules::shorten(&folder))));
        let add = gtk::Button::builder().icon_name("list-add-symbolic").tooltip_text(tr("Add a rule")).build();
        header.pack_start(&add);
        let run = gtk::Button::with_label(&tr("Run Now"));
        header.pack_end(&run);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        list.set_margin_start(12);
        list.set_margin_end(12);
        list.set_margin_top(12);
        list.set_margin_bottom(12);
        let empty = adw::StatusPage::builder().icon_name("emblem-system-symbolic").title(tr("No rules for this folder")).description(tr("Add one with +: a name pattern, an age, and what should happen.")).vexpand(true).build();
        empty.add_css_class("compact");
        let scroll = gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&scroll, Some("list"));
        let hint = gtk::Label::builder().label(tr("Rules run when omafile starts and a few seconds after something lands in the folder. Moves, copies, tags and trashing can be undone.")).css_classes(["omafile-dim", "caption"]).wrap(true).xalign(0.0).margin_start(12).margin_end(12).margin_bottom(10).build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&stack);
        content.append(&hint);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&content));
        let dialog = adw::Dialog::builder().title(tr("Folder Rules")).content_width(620).content_height(520).child(&view).build();

        let this = Rc::downgrade(self);
        let (f2, l2, d2) = (folder.clone(), list.clone(), dialog.clone());
        let refresh: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(w) = this.upgrade() {
                w.fill_rules(&l2, &f2, &d2);
            }
        });
        refresh();
        let this = Rc::downgrade(self);
        let (f2, r2) = (folder.clone(), refresh.clone());
        add.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.edit_rule(None, Rule { folder: f2.clone(), glob: "*.pdf".into(), older_days: 0, action: Action::MoveTo(glib::user_special_dir(glib::UserDirectory::Documents).unwrap_or_else(glib::home_dir)), enabled: true }, r2.clone());
            }
        });
        let this = Rc::downgrade(self);
        let f2 = folder.clone();
        run.connect_clicked(move |_| {
            let Some(w) = this.upgrade() else { return };
            let folder = f2.clone();
            glib::spawn_future_local(async move {
                let summary = rules::run(&w, Some(&folder)).await;
                w.toast(&summary.unwrap_or_else(|| tr("Nothing matched the rules")));
                for p in w.panes.borrow().clone() {
                    p.reload();
                }
            });
        });
        self.present_dialog(&dialog);
    }

    fn fill_rules(self: &Rc<Self>, list: &gtk::ListBox, folder: &PathBuf, dialog: &adw::Dialog) {
        while let Some(row) = list.row_at_index(0) {
            list.remove(&row);
        }
        let all = rules::load();
        let mine: Vec<(usize, Rule)> = all.iter().cloned().enumerate().filter(|(_, r)| &r.folder == folder).collect();
        let mut up = list.parent();
        while let Some(w) = up {
            if let Ok(stack) = w.clone().downcast::<gtk::Stack>() {
                stack.set_visible_child_name(if mine.is_empty() { "empty" } else { "list" });
                break;
            }
            up = w.parent();
        }
        for (index, rule) in mine {
            let title = if rule.glob.is_empty() { tr("Everything") } else { rule.glob.clone() };
            let age = if rule.older_days > 0 { trf!(", older than {n} days", n = rule.older_days) } else { String::new() };
            let row = adw::ActionRow::builder().title(glib::markup_escape_text(&title)).subtitle(glib::markup_escape_text(&format!("{}{}", rule.action.describe(), age))).build();
            let switch = gtk::Switch::builder().valign(gtk::Align::Center).active(rule.enabled).build();
            let this = Rc::downgrade(self);
            switch.connect_active_notify(move |s| {
                let mut all = rules::load();
                if let Some(r) = all.get_mut(index) {
                    r.enabled = s.is_active();
                    let _ = rules::save(&all);
                }
                if let Some(w) = this.upgrade() {
                    rules::watch(&w);
                }
            });
            row.add_prefix(&switch);
            let edit = gtk::Button::builder().icon_name("document-edit-symbolic").valign(gtk::Align::Center).tooltip_text(tr("Edit")).build();
            edit.add_css_class("flat");
            let this = Rc::downgrade(self);
            let (l2, f2, d2, r2) = (list.clone(), folder.clone(), dialog.clone(), rule.clone());
            edit.connect_clicked(move |_| {
                let Some(w) = this.upgrade() else { return };
                let (this2, l3, f3, d3) = (Rc::downgrade(&w), l2.clone(), f2.clone(), d2.clone());
                let refresh: Rc<dyn Fn()> = Rc::new(move || { if let Some(w) = this2.upgrade() { w.fill_rules(&l3, &f3, &d3); } });
                w.edit_rule(Some(index), r2.clone(), refresh);
            });
            row.add_suffix(&edit);
            let delete = gtk::Button::builder().icon_name("user-trash-symbolic").valign(gtk::Align::Center).tooltip_text(tr("Delete rule")).build();
            delete.add_css_class("flat");
            let this = Rc::downgrade(self);
            let (l2, f2, d2) = (list.clone(), folder.clone(), dialog.clone());
            delete.connect_clicked(move |_| {
                let mut all = rules::load();
                if index < all.len() {
                    all.remove(index);
                    let _ = rules::save(&all);
                }
                if let Some(w) = this.upgrade() {
                    rules::watch(&w);
                    w.fill_rules(&l2, &f2, &d2);
                }
            });
            row.add_suffix(&delete);
            list.append(&row);
        }
    }

    /// Editor for one rule; `index` is its position in the file, None adds.
    fn edit_rule(self: &Rc<Self>, index: Option<usize>, rule: Rule, refresh: Rc<dyn Fn()>) {
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::builder().title(tr("When a file in this folder…")).description(rules::shorten(&rule.folder)).build();
        let glob = adw::EntryRow::builder().title(tr("Name matches (glob, e.g. *.pdf; empty = everything)")).text(&rule.glob).build();
        let age = adw::SpinRow::with_range(0.0, 3650.0, 1.0);
        age.set_title(&tr("…and is older than (days, 0 = any)"));
        age.set_value(rule.older_days as f64);
        group.add(&glob);
        group.add(&age);
        page.add(&group);

        let what = adw::PreferencesGroup::builder().title(tr("Then")).build();
        let actions = gtk::StringList::new(&[tr("Move to a folder").as_str(), tr("Copy to a folder").as_str(), tr("Give it a tag").as_str(), tr("Move it to the trash").as_str(), tr("Sort into YYYY-MM folders").as_str(), tr("Run a command ($FILE)").as_str()]);
        let action = adw::ComboRow::builder().title(tr("Action")).model(&actions).build();
        action.set_selected(rules::ACTION_IDS.iter().position(|id| *id == rule.action.id()).unwrap_or(0) as u32);
        let target = adw::EntryRow::builder().title(tr("Folder")).text(match &rule.action { Action::MoveTo(p) | Action::CopyTo(p) | Action::ByMonth(p) => rules::shorten(p), _ => String::new() }).build();
        let pick = gtk::Button::builder().icon_name("folder-open-symbolic").valign(gtk::Align::Center).tooltip_text(tr("Choose a folder")).build();
        pick.add_css_class("flat");
        target.add_suffix(&pick);
        let colors = gtk::StringList::new(&crate::tags::COLORS.iter().map(|(n, _, _)| crate::tags::label(n)).collect::<Vec<_>>().iter().map(String::as_str).collect::<Vec<_>>());
        let tag = adw::ComboRow::builder().title(tr("Tag")).model(&colors).build();
        if let Action::Tag(t) = &rule.action {
            tag.set_selected(crate::tags::COLORS.iter().position(|(n, _, _)| n == t).unwrap_or(0) as u32);
        }
        let command = adw::EntryRow::builder().title(tr("Command (runs in the folder, $FILE is the file)")).text(if let Action::Run(c) = &rule.action { c.clone() } else { String::new() }).build();
        what.add(&action);
        what.add(&target);
        what.add(&tag);
        what.add(&command);
        page.add(&what);
        let sync = {
            let (target, tag, command) = (target.clone(), tag.clone(), command.clone());
            move |selected: u32| {
                target.set_visible(matches!(selected, 0 | 1 | 4));
                target.set_title(&if selected == 4 { tr("Base folder (empty = this folder)") } else { tr("Folder") });
                tag.set_visible(selected == 2);
                command.set_visible(selected == 5);
            }
        };
        sync(action.selected());
        action.connect_selected_notify(move |a| sync(a.selected()));
        let window = self.window.clone();
        let t2 = target.clone();
        pick.connect_clicked(move |_| {
            let chooser = gtk::FileDialog::builder().title(tr("Choose a folder")).modal(true).build();
            let t3 = t2.clone();
            chooser.select_folder(Some(&window), gio::Cancellable::NONE, move |r| {
                if let Ok(f) = r {
                    if let Some(p) = f.path() {
                        t3.set_text(&rules::shorten(&p));
                    }
                }
            });
        });

        let save = gtk::Button::with_label(&tr("Save"));
        save.add_css_class("suggested-action");
        let header = adw::HeaderBar::new();
        header.pack_end(&save);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&page));
        let dialog = adw::Dialog::builder().title(if index.is_some() { tr("Edit Rule") } else { tr("New Rule") }).content_width(560).content_height(600).child(&view).build();
        let this = Rc::downgrade(self);
        let d2 = dialog.clone();
        let folder = rule.folder.clone();
        save.connect_clicked(move |_| {
            let Some(w) = this.upgrade() else { return };
            let selected = action.selected() as usize;
            let id = rules::ACTION_IDS.get(selected).copied().unwrap_or("move");
            let arg = match id {
                "move" | "copy" | "month" => target.text().trim().to_string(),
                "tag" => crate::tags::COLORS.get(tag.selected() as usize).map(|(n, _, _)| n.to_string()).unwrap_or_else(|| "red".into()),
                "run" => command.text().trim().to_string(),
                _ => String::new(),
            };
            if matches!(id, "move" | "copy") && arg.is_empty() {
                w.toast_error(&tr("Pick a target folder"));
                return;
            }
            if id == "run" && arg.is_empty() {
                w.toast_error(&tr("Enter a command"));
                return;
            }
            let Some(new_action) = Action::from_parts(id, &arg) else { return };
            let new_rule = Rule { folder: folder.clone(), glob: glob.text().trim().to_string(), older_days: age.value() as u32, action: new_action, enabled: true };
            let mut all = rules::load();
            match index {
                Some(i) if i < all.len() => {
                    let enabled = all[i].enabled;
                    all[i] = Rule { enabled, ..new_rule };
                }
                _ => all.push(new_rule),
            }
            match rules::save(&all) {
                Ok(()) => {
                    rules::watch(&w);
                    refresh();
                    d2.close();
                }
                Err(e) => w.toast_error(&trf!("Cannot save: {e}", e = e)),
            }
        });
        self.present_dialog(&dialog);
    }
}
