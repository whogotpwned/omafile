//! Dialogs for encrypted folders: create, unlock, lock, change password.

use crate::i18n::tr;
use crate::crypt;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::path::PathBuf;
use std::rc::Rc;

fn password_row(title: &str) -> gtk::PasswordEntry {
    gtk::PasswordEntry::builder().placeholder_text(title).show_peek_icon(true).activates_default(true).build()
}

impl Window {
    fn require_gocryptfs(&self) -> bool {
        if crypt::available() {
            true
        } else {
            self.toast_error(&tr("gocryptfs is not installed (sudo pacman -S gocryptfs)"));
            false
        }
    }

    pub fn vault_new(self: &Rc<Self>) {
        if !self.require_gocryptfs() {
            return;
        }
        let Some(pane) = self.active_pane() else { return };
        let Some(dir) = pane.cwd().path() else {
            self.toast_error(&tr("Encrypted folders can only be created on local file systems"));
            return;
        };
        let name = gtk::Entry::builder().placeholder_text(tr("Folder name")).text(tr("Private")).activates_default(true).build();
        let pass = password_row(&tr("Password"));
        let confirm = password_row(&tr("Repeat password"));
        let strength = gtk::Label::builder().xalign(0.0).css_classes(["omafile-dim", "caption"]).build();
        let s2 = strength.clone();
        pass.connect_changed(move |p| {
            let n = p.text().len();
            s2.set_text(&match n {
                0 => String::new(),
                1..=7 => tr("Short: use at least 12 characters, a sentence works well"),
                8..=11 => tr("Okay, longer is better"),
                _ => tr("Good length"),
            });
        });
        let hint = gtk::Label::builder()
            .label(tr("Creates Name.gocryptfs (the encrypted data, safe to copy or sync anywhere) and Name, where the files appear while unlocked. AES-256-GCM per file via gocryptfs."))
            .wrap(true)
            .xalign(0.0)
            .css_classes(["omafile-dim", "caption"])
            .build();
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
        vbox.append(&name);
        vbox.append(&pass);
        vbox.append(&confirm);
        vbox.append(&strength);
        vbox.append(&hint);
        let dialog = adw::AlertDialog::new(Some(tr("New Encrypted Folder").as_str()), None);
        dialog.set_extra_child(Some(&vbox));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("create", tr("Create").as_str())]);
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("create"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        dialog.connect_closed({
            let this = this.clone();
            move |_| {
                if let Some(w) = this.upgrade() {
                    w.refocus();
                }
            }
        });
        let name_entry = name.clone();
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "create" {
                return;
            }
            let name = name_entry.text().trim().to_string();
            let (p1, p2) = (pass.text().to_string(), confirm.text().to_string());
            if name.is_empty() || name.contains('/') {
                w.toast_error(&tr("Invalid folder name"));
                return;
            }
            if p1.len() < 8 {
                w.toast_error(&tr("Use a password of at least 8 characters"));
                return;
            }
            if p1 != p2 {
                w.toast_error(&tr("The passwords do not match"));
                return;
            }
            let vault = dir.join(format!("{name}{}", crypt::SUFFIX));
            if vault.exists() || dir.join(&name).exists() {
                w.toast_error(&tr("A folder with that name exists already"));
                return;
            }
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let result = crypt::create(&vault, &p1).await;
                let Some(w) = this.upgrade() else { return };
                match result {
                    Ok(key) => {
                        // Unlock right away so the folder is ready to use.
                        let mount = crypt::unlock(&vault, &p1).await;
                        crate::status::write(&w);
                        pane.reload();
                        w.show_master_key(&name, &key);
                        if let Ok(m) = mount {
                            pane.select_file_later(gio::File::for_path(m));
                        }
                    }
                    Err(e) => w.toast_error(&format!("Cannot create encrypted folder: {}", e.message())),
                }
            });
        });
        name.grab_focus();
        name.select_region(0, -1);
    }

    fn show_master_key(self: &Rc<Self>, name: &str, key: &str) {
        let key_label = gtk::Label::builder().label(key).selectable(true).wrap(true).css_classes(["omafile-master-key"]).build();
        let copy = gtk::Button::with_label(&tr("Copy key"));
        let this = Rc::downgrade(self);
        let key2 = key.to_string();
        copy.connect_clicked(move |_| {
            if let Some(w) = this.upgrade() {
                w.window.clipboard().set_text(&key2);
                w.toast(&tr("Master key copied"));
            }
        });
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 10);
        vbox.append(&key_label);
        vbox.append(&copy);
        let dialog = adw::AlertDialog::new(
            Some(&format!("{name} is ready")),
            Some(tr("This master key opens the folder if you ever forget the password. Store it somewhere safe (a password manager), it is shown only once. Back up or move the folder by copying Name.gocryptfs; it stays encrypted.").as_str()),
        );
        dialog.set_extra_child(Some(&vbox));
        dialog.add_responses(&[("ok", tr("I saved it").as_str())]);
        dialog.set_default_response(Some("ok"));
        let this = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            if let Some(w) = this.upgrade() {
                w.refocus();
            }
        });
        dialog.present(Some(&self.window));
    }

    /// The vault behind the selection: a ciphertext folder, or the mount point next to one.
    fn selected_vault(&self) -> Option<PathBuf> {
        let item = self.selected_items().into_iter().next()?;
        let path = item.file().path()?;
        if crypt::is_vault(&path) {
            Some(path)
        } else {
            crypt::vault_for(&path)
        }
    }

    pub fn vault_unlock(self: &Rc<Self>) {
        if !self.require_gocryptfs() {
            return;
        }
        let Some(pane) = self.active_pane() else { return };
        let Some(vault) = self.selected_vault() else { return };
        let pass = password_row("Password");
        let dialog = adw::AlertDialog::new(Some(tr("Unlock Encrypted Folder").as_str()), Some(&format!("Enter the password for {}", crate::window::abbreviate_home(&vault))));
        dialog.set_extra_child(Some(&pass));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("unlock", tr("Unlock").as_str())]);
        dialog.set_response_appearance("unlock", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("unlock"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        dialog.connect_closed({
            let this = this.clone();
            move |_| {
                if let Some(w) = this.upgrade() {
                    w.refocus();
                }
            }
        });
        let pass_entry = pass.clone();
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "unlock" {
                return;
            }
            let password = pass_entry.text().to_string();
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let result = crypt::unlock(&vault, &password).await;
                let Some(w) = this.upgrade() else { return };
                match result {
                    Ok(mount) => {
                        crate::status::write(&w);
                        w.toast(&tr("Unlocked"));
                        pane.navigate(gio::File::for_path(mount), true);
                    }
                    Err(e) => w.toast_error(&format!("Cannot unlock: {}", e.message())),
                }
            });
        });
        pass.grab_focus();
    }

    pub fn vault_lock(self: &Rc<Self>) {
        let Some(pane) = self.active_pane() else { return };
        let Some(vault) = self.selected_vault().or_else(|| pane.cwd().path().and_then(|p| crypt::vault_for(&p))) else { return };
        let mount = crypt::mount_point_for(&vault);
        let inside = pane.cwd().path().map(|p| p.starts_with(&mount)).unwrap_or(false);
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            if inside {
                // Leave the mount first, a busy mount cannot be unmounted.
                if let Some(parent) = mount.parent() {
                    pane.navigate(gio::File::for_path(parent), true);
                }
                glib::timeout_future(std::time::Duration::from_millis(300)).await;
            }
            let result = crypt::lock(&mount).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(()) => {
                    crate::status::write(&w);
                    w.toast(&tr("Locked"));
                    pane.reload();
                }
                Err(e) => w.toast_error(&format!("Cannot lock (still in use?): {}", e.message())),
            }
        });
    }

    pub fn vault_passwd(self: &Rc<Self>) {
        if !self.require_gocryptfs() {
            return;
        }
        let Some(vault) = self.selected_vault() else { return };
        let old = password_row(&tr("Current password"));
        let new = password_row(&tr("New password"));
        let again = password_row(&tr("Repeat new password"));
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
        vbox.append(&old);
        vbox.append(&new);
        vbox.append(&again);
        let dialog = adw::AlertDialog::new(Some(tr("Change Vault Password").as_str()), Some(tr("The master key stays the same, so existing backups of the folder keep working with the new password.").as_str()));
        dialog.set_extra_child(Some(&vbox));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("change", tr("Change").as_str())]);
        dialog.set_response_appearance("change", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("change"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        dialog.connect_closed({
            let this = this.clone();
            move |_| {
                if let Some(w) = this.upgrade() {
                    w.refocus();
                }
            }
        });
        let old_entry = old.clone();
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "change" {
                return;
            }
            let (o, n, a) = (old_entry.text().to_string(), new.text().to_string(), again.text().to_string());
            if n.len() < 8 || n != a {
                w.toast_error(&tr("New passwords must match and have at least 8 characters"));
                return;
            }
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let result = crypt::change_password(&vault, &o, &n).await;
                if let Some(w) = this.upgrade() {
                    match result {
                        Ok(()) => w.toast(&tr("Password changed")),
                        Err(e) => w.toast_error(&format!("Cannot change password: {}", e.message())),
                    }
                }
            });
        });
        old.grab_focus();
    }
}
