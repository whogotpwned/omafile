//! Permissions and ownership in plain words, editable: who may read, write
//! and run, shown as check boxes, a sentence, `rwxr-xr-x` and the octal
//! number; owner and group as names. Changes apply at once and can be undone.

use crate::i18n::tr;
use crate::item::FileItem;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;

/// A user or group from /etc/passwd or /etc/group.
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub id: u32,
    pub name: String,
}

fn parse_accounts(text: &str) -> Vec<Account> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?.to_string();
            let _ = f.next()?;
            let id: u32 = f.next()?.parse().ok()?;
            Some(Account { id, name })
        })
        .collect()
}

pub fn users() -> Vec<Account> {
    std::fs::read_to_string("/etc/passwd").map(|t| parse_accounts(&t)).unwrap_or_default()
}

pub fn groups() -> Vec<Account> {
    std::fs::read_to_string("/etc/group").map(|t| parse_accounts(&t)).unwrap_or_default()
}

/// Groups the current user may hand files to: their own groups (root: all).
pub fn my_groups() -> Vec<Account> {
    let all = groups();
    if libc_getuid() == 0 {
        return all;
    }
    let mine: Vec<String> = std::process::Command::new("id").arg("-Gn").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().map(str::to_string).collect()).unwrap_or_default();
    all.into_iter().filter(|g| mine.contains(&g.name)).collect()
}

fn libc_getuid() -> u32 {
    // glib exposes the real user id through the credentials API; this avoids a libc dependency.
    gio::Credentials::new().unix_user().unwrap_or(u32::MAX)
}

/// "Owner can read and write, group and others can read."
pub fn sentence(mode: u32, is_dir: bool) -> String {
    let who = |shift: u32| -> String {
        let bits = (mode >> shift) & 0o7;
        let mut parts = Vec::new();
        if bits & 4 != 0 {
            parts.push(if is_dir { tr("list") } else { tr("read") });
        }
        if bits & 2 != 0 {
            parts.push(if is_dir { tr("add and delete") } else { tr("write") });
        }
        if bits & 1 != 0 {
            parts.push(if is_dir { tr("enter") } else { tr("run") });
        }
        match parts.len() {
            0 => tr("do nothing"),
            1 => parts[0].clone(),
            n => trf!("{a} and {b}", a = parts[..n - 1].join(", "), b = parts[n - 1]),
        }
    };
    let (u, g, o) = (who(6), who(3), who(0));
    let mut out = trf!("Owner can {u}", u = u);
    if g == o {
        out.push_str(&trf!(", group and others can {g}.", g = g));
    } else {
        out.push_str(&trf!(", group can {g}, others can {o}.", g = g, o = o));
    }
    let mut special = Vec::new();
    if mode & 0o4000 != 0 {
        special.push(tr("set-UID"));
    }
    if mode & 0o2000 != 0 {
        special.push(if is_dir { tr("set-GID (new files inherit the group)") } else { tr("set-GID") });
    }
    if mode & 0o1000 != 0 {
        special.push(tr("sticky (only owners delete)"));
    }
    if !special.is_empty() {
        out.push_str(&trf!(" Special: {list}.", list = special.join(", ")));
    }
    out
}

pub fn octal(mode: u32) -> String {
    format!("{:o}", mode & 0o7777)
}

impl Window {
    /// The Permissions group for the Properties dialog.
    pub fn permissions_group(self: &Rc<Self>, item: &FileItem) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::builder().title(tr("Permissions")).build();
        let file = item.target_file();
        let is_dir = item.is_dir();
        let native = file.is_native();
        let mode0 = item.mode();
        if mode0 == 0 {
            group.add(&adw::ActionRow::builder().title(tr("Not available for this location")).build());
            return group;
        }
        let summary = adw::ActionRow::builder().title(sentence(mode0, is_dir)).subtitle(format!("{}  ·  {}", crate::ops::perm_string(mode0, is_dir, item.is_symlink()), octal(mode0))).build();
        summary.add_css_class("property");
        summary.set_title_lines(3);
        group.add(&summary);

        // Grid of check boxes: rows owner / group / others, columns read / write / execute.
        let grid = gtk::Grid::builder().row_spacing(6).column_spacing(18).margin_top(10).margin_bottom(10).margin_start(14).margin_end(14).build();
        for (col, title) in [(1, if is_dir { tr("List folder") } else { tr("Read") }), (2, tr("Write")), (3, if is_dir { tr("Enter folder") } else { tr("Run") })] {
            let l = gtk::Label::new(Some(&title));
            l.add_css_class("caption-heading");
            l.add_css_class("omafile-dim");
            grid.attach(&l, col, 0, 1, 1);
        }
        let mode = Rc::new(std::cell::Cell::new(mode0));
        let checks: Rc<std::cell::RefCell<Vec<(gtk::CheckButton, u32)>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
        let this = Rc::downgrade(self);
        let apply: Rc<dyn Fn(u32)> = {
            let file = file.clone();
            let summary = summary.clone();
            let mode = mode.clone();
            let is_symlink = item.is_symlink();
            Rc::new(move |new: u32| {
                let Some(w) = this.upgrade() else { return };
                let old = mode.get();
                if new == old {
                    return;
                }
                match file.set_attribute_uint32("unix::mode", new, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE) {
                    Ok(()) => {
                        mode.set(new);
                        summary.set_title(&sentence(new, is_dir));
                        summary.set_subtitle(&format!("{}  ·  {}", crate::ops::perm_string(new, is_dir, is_symlink), octal(new)));
                        w.record(Op::Mode(vec![(file.clone(), old, new)]), &format!("Permissions: {}", octal(new)));
                        if let Some(p) = w.active_pane() {
                            p.reload();
                        }
                    }
                    Err(e) => w.toast_error(&format!("Cannot change permissions: {}", e.message())),
                }
            })
        };
        for (row, (label, shift)) in [("Owner", 6u32), ("Group", 3), ("Others", 0)].iter().enumerate() {
            let l = gtk::Label::builder().label(tr(label)).xalign(0.0).build();
            grid.attach(&l, 0, row as i32 + 1, 1, 1);
            for (col, bit) in [(1, 4u32), (2, 2), (3, 1)] {
                let mask = bit << shift;
                let check = gtk::CheckButton::new();
                check.set_active(mode0 & mask != 0);
                check.set_halign(gtk::Align::Center);
                check.set_sensitive(native);
                let (apply, mode, checks2) = (apply.clone(), mode.clone(), checks.clone());
                check.connect_toggled(move |c| {
                    let current = mode.get();
                    let new = if c.is_active() { current | mask } else { current & !mask };
                    apply(new);
                    // Roll the box back when the change was refused.
                    if mode.get() != new {
                        for (other, m) in checks2.borrow().iter() {
                            other.set_active(mode.get() & m != 0);
                        }
                    }
                });
                checks.borrow_mut().push((check.clone(), mask));
                grid.attach(&check, col, row as i32 + 1, 1, 1);
            }
        }
        let grid_row = adw::PreferencesRow::builder().activatable(false).child(&grid).build();
        group.add(&grid_row);

        // Octal entry for people who think in numbers.
        let octal_row = adw::EntryRow::builder().title(tr("Octal (e.g. 755)")).text(octal(mode0)).show_apply_button(true).build();
        octal_row.set_sensitive(native);
        {
            let (apply, checks, mode) = (apply.clone(), checks.clone(), mode.clone());
            octal_row.connect_apply(move |e| {
                if let Ok(bits) = u32::from_str_radix(e.text().trim(), 8) {
                    apply((mode.get() & !0o7777) | (bits & 0o7777));
                    for (c, m) in checks.borrow().iter() {
                        c.set_active(mode.get() & m != 0);
                    }
                }
            });
        }
        group.add(&octal_row);

        // Special bits in an expander, out of the way.
        let special = adw::ExpanderRow::builder().title(tr("Special bits")).subtitle(tr("set-UID, set-GID, sticky")).build();
        for (label, mask, hint) in [("Set-UID", 0o4000u32, tr("runs as the owner")), ("Set-GID", 0o2000, tr("runs as the group / new files inherit the group")), ("Sticky", 0o1000, tr("only owners may delete inside"))] {
            let row = adw::SwitchRow::builder().title(label).subtitle(hint).active(mode0 & mask != 0).build();
            row.set_sensitive(native);
            let (apply, mode) = (apply.clone(), mode.clone());
            row.connect_active_notify(move |r| {
                let current = mode.get();
                apply(if r.is_active() { current | mask } else { current & !mask });
            });
            special.add_row(&row);
        }
        group.add(&special);

        // Owner and group as names.
        let info = &item.info();
        let uid = info.attribute_uint32("unix::uid");
        let gid = info.attribute_uint32("unix::gid");
        let owner_name = users().into_iter().find(|u| u.id == uid).map(|u| u.name).unwrap_or_else(|| uid.to_string());
        let owner_row = adw::ActionRow::builder().title(tr("Owner")).subtitle(glib::markup_escape_text(&format!("{owner_name} (uid {uid})"))).build();
        owner_row.add_css_class("property");
        if libc_getuid() == 0 && native {
            // Only root can give files away; offer the list then.
            let names: Vec<String> = users().into_iter().filter(|u| u.id == uid || u.id >= 1000 || u.id == 0).map(|u| u.name).collect();
            let drop = gtk::DropDown::from_strings(&names.iter().map(String::as_str).collect::<Vec<_>>());
            drop.set_valign(gtk::Align::Center);
            if let Some(i) = names.iter().position(|n| *n == owner_name) {
                drop.set_selected(i as u32);
            }
            let (file2, this, row2) = (file.clone(), Rc::downgrade(self), owner_row.clone());
            drop.connect_selected_notify(move |d| {
                let Some(w) = this.upgrade() else { return };
                let Some(name) = names.get(d.selected() as usize) else { return };
                let Some(acc) = users().into_iter().find(|u| u.name == *name) else { return };
                match file2.set_attribute_uint32("unix::uid", acc.id, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE) {
                    Ok(()) => {
                        row2.set_subtitle(&glib::markup_escape_text(&format!("{} (uid {})", acc.name, acc.id)));
                        w.record(Op::Owner(vec![(file2.clone(), (uid, gid), (acc.id, gid))]), &format!("Owner: {}", acc.name));
                    }
                    Err(e) => w.toast_error(&format!("Cannot change owner: {}", e.message())),
                }
            });
            owner_row.add_suffix(&drop);
        }
        group.add(&owner_row);

        let group_name = groups().into_iter().find(|g| g.id == gid).map(|g| g.name).unwrap_or_else(|| gid.to_string());
        let group_row = adw::ActionRow::builder().title(tr("Group")).subtitle(glib::markup_escape_text(&format!("{group_name} (gid {gid})"))).build();
        group_row.add_css_class("property");
        let mine = my_groups();
        if native && mine.len() > 1 {
            let names: Vec<String> = mine.iter().map(|g| g.name.clone()).collect();
            let drop = gtk::DropDown::from_strings(&names.iter().map(String::as_str).collect::<Vec<_>>());
            drop.set_valign(gtk::Align::Center);
            match names.iter().position(|n| *n == group_name) {
                Some(i) => drop.set_selected(i as u32),
                None => drop.set_selected(gtk::INVALID_LIST_POSITION),
            }
            let (file2, this, row2) = (file.clone(), Rc::downgrade(self), group_row.clone());
            drop.connect_selected_notify(move |d| {
                let Some(w) = this.upgrade() else { return };
                let Some(acc) = names.get(d.selected() as usize).and_then(|n| mine.iter().find(|g| g.name == *n)) else { return };
                if acc.id == gid {
                    return;
                }
                match file2.set_attribute_uint32("unix::gid", acc.id, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE) {
                    Ok(()) => {
                        row2.set_subtitle(&glib::markup_escape_text(&format!("{} (gid {})", acc.name, acc.id)));
                        w.record(Op::Owner(vec![(file2.clone(), (uid, gid), (uid, acc.id))]), &format!("Group: {}", acc.name));
                    }
                    Err(e) => w.toast_error(&format!("Cannot change group: {}", e.message())),
                }
            });
            group_row.add_suffix(&drop);
            group_row.set_subtitle(&tr("groups you belong to"));
        }
        group.add(&group_row);

        // What *you* can do, from the access attributes GIO already queried.
        let mut you = Vec::new();
        if info.boolean("access::can-read") {
            you.push(if is_dir { tr("list it") } else { tr("read it") });
        }
        if info.boolean("access::can-write") {
            you.push(if is_dir { tr("add and delete inside") } else { tr("change it") });
        }
        if info.boolean("access::can-execute") {
            you.push(if is_dir { tr("enter it") } else { tr("run it") });
        }
        let you_row = adw::ActionRow::builder().title(tr("You can")).subtitle(if you.is_empty() { tr("nothing (no access)") } else { you.join(", ") }).build();
        you_row.add_css_class("property");
        group.add(&you_row);

        if is_dir && native {
            let recurse = gtk::Button::with_label(&tr("Apply to Enclosed Files…"));
            recurse.set_halign(gtk::Align::Start);
            recurse.set_margin_top(6);
            recurse.set_margin_start(14);
            recurse.set_margin_bottom(10);
            let (this, file2, mode) = (Rc::downgrade(self), file.clone(), mode.clone());
            recurse.connect_clicked(move |_| {
                if let Some(w) = this.upgrade() {
                    w.apply_mode_recursively(file2.clone(), mode.get());
                }
            });
            let r = adw::PreferencesRow::builder().activatable(false).child(&recurse).build();
            group.add(&r);
        }
        group
    }

    /// Give every file below `dir` the folder's read/write bits; folders also
    /// get the execute (enter) bits. Asks first, undoable.
    fn apply_mode_recursively(self: &Rc<Self>, dir: gio::File, mode: u32) {
        let dialog = adw::AlertDialog::new(Some(tr("Apply to enclosed files?").as_str()), Some(&format!("Files inside get {} (without execute), folders get {}. This walks the whole tree.", octal(mode & !0o111), octal(mode))));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("apply", tr("Apply").as_str())]);
        dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("apply"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |r| {
            if r != "apply" {
                return;
            }
            let Some(w) = this.upgrade() else { return };
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let Some(root) = dir.path() else { return };
                let changes = gio::spawn_blocking(move || {
                    use std::os::unix::fs::PermissionsExt;
                    let mut out = Vec::new();
                    let mut stack = vec![root];
                    while let Some(d) = stack.pop() {
                        let Ok(entries) = std::fs::read_dir(&d) else { continue };
                        for e in entries.flatten() {
                            let Ok(meta) = e.metadata() else { continue };
                            if meta.file_type().is_symlink() {
                                continue;
                            }
                            let old = meta.permissions().mode();
                            let new = if meta.is_dir() { (old & !0o7777) | (mode & 0o7777) } else { (old & !0o7777) | (mode & 0o7666) };
                            if new != old && std::fs::set_permissions(e.path(), std::fs::Permissions::from_mode(new)).is_ok() {
                                out.push((gio::File::for_path(e.path()), old, new));
                            }
                            if meta.is_dir() {
                                stack.push(e.path());
                            }
                        }
                    }
                    out
                })
                .await
                .unwrap_or_default();
                let Some(w) = this.upgrade() else { return };
                let n = changes.len();
                if n > 0 {
                    w.record(Op::Mode(changes), &format!("Permissions applied to {n} {}", if n == 1 { tr("item") } else { tr("items") }));
                } else {
                    w.toast(&tr("Nothing to change"));
                }
                if let Some(p) = w.active_pane() {
                    p.reload();
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences() {
        assert_eq!(sentence(0o644, false), "Owner can read and write, group and others can read.");
        assert_eq!(sentence(0o750, true), "Owner can list, add and delete and enter, group can list and enter, others can do nothing.");
        assert!(sentence(0o1777, true).contains("sticky"));
        assert_eq!(octal(0o100644), "644");
        let accounts = parse_accounts("root:x:0:0:root:/root:/bin/bash\nuser:x:1000:1000::/home/user:/bin/fish\n");
        assert_eq!(accounts[1], Account { id: 1000, name: "user".into() });
    }
}
