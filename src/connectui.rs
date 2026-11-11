//! Connecting to servers the WinSCP way: a dialog that can save the connection (and let the
//! system keyring remember its password), and local on the left / remote on the right once
//! connected, with copy and move across the divider.

use crate::connections::{self, Connection};
use crate::i18n::tr;
use crate::remote;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::Cell;
use std::rc::Rc;

/// How to connect and what to do once connected.
#[derive(Clone, Default)]
pub struct ConnectOpts {
    /// Answer the server's first password question with this.
    pub password: Option<String>,
    /// Let GVFS keep that password in the system keyring.
    pub remember: bool,
    /// List the connection in the sidebar under this name.
    pub save_as: Option<String>,
    /// Open in a plain new tab, no second pane.
    pub plain_tab: bool,
}

impl Window {
    /// Sidebar › Connect to Server…
    pub fn prompt_connect(self: &Rc<Self>) {
        self.prompt_connection(None);
    }

    /// The connect dialog; with `existing`, it edits that saved connection.
    pub fn prompt_connection(self: &Rc<Self>, existing: Option<Connection>) {
        let hosts = remote::ssh_hosts();
        let server = gtk::Entry::builder().placeholder_text(tr("user@host[:port][/path]  or  sftp://host/")).activates_default(true).build();
        let name = gtk::Entry::builder().placeholder_text(tr("Name in the sidebar (optional)")).activates_default(true).build();
        let password = gtk::PasswordEntry::builder().placeholder_text(tr("Password (leave empty for SSH keys)")).show_peek_icon(true).activates_default(true).build();
        let save = gtk::CheckButton::builder().label(tr("Save in the sidebar")).active(existing.is_some()).build();
        let keyring = connections::keyring_available();
        let remember = gtk::CheckButton::builder().label(tr("Remember the password in the system keyring")).active(keyring).sensitive(keyring).build();
        let hint = gtk::Label::builder()
            .label(if keyring {
                tr("The password is kept by the system keyring (encrypted with your login), never in omafile's own files. SSH keys work without one.")
            } else {
                tr("No keyring service is running (gnome-keyring), so the password is asked each time. SSH keys work without one.")
            })
            .wrap(true)
            .xalign(0.0)
            .css_classes(["omafile-dim", "caption"])
            .build();
        // Giving it a name means wanting it kept.
        let save2 = save.clone();
        name.connect_changed(move |n| {
            if !n.text().trim().is_empty() {
                save2.set_active(true);
            }
        });
        if let Some(c) = &existing {
            server.set_text(&connections::short(&c.uri));
            name.set_text(&c.name);
        }
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
        for w in [server.upcast_ref::<gtk::Widget>(), name.upcast_ref(), password.upcast_ref(), save.upcast_ref(), remember.upcast_ref(), hint.upcast_ref()] {
            vbox.append(w);
        }
        let body = if hosts.is_empty() {
            tr("Connect over SSH (SFTP / SCP). Local files stay on the left, the server opens on the right.")
        } else {
            format!("{} {}", tr("Connect over SSH (SFTP / SCP). Local files stay on the left, the server opens on the right. Known hosts:"), hosts.iter().map(|h| h.alias.as_str()).collect::<Vec<_>>().join(", "))
        };
        let title = if existing.is_some() { tr("Edit Connection") } else { tr("Connect to Server") };
        let dialog = adw::AlertDialog::new(Some(title.as_str()), Some(&body));
        dialog.set_extra_child(Some(&vbox));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("connect", tr("Connect").as_str())]);
        dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("connect"));
        dialog.set_close_response("cancel");
        let refocus = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            if let Some(w) = refocus.upgrade() {
                w.refocus();
            }
        });
        let this = Rc::downgrade(self);
        let server2 = server.clone();
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "connect" {
                return;
            }
            let text = server2.text().to_string();
            // An alias from ~/.ssh/config may be typed instead of an address.
            let uri = remote::resolve_alias(&hosts, &text).or_else(|| remote::parse_server(&text));
            let Some(uri) = uri else {
                w.toast_error(&tr("Enter a host to connect to"));
                return;
            };
            let typed_name = name.text().trim().to_string();
            // An edited connection that was renamed must not leave its old entry behind.
            if let Some(old) = &existing {
                if save.is_active() && !typed_name.is_empty() && typed_name != old.name {
                    connections::remove(&old.name);
                }
            }
            let pw = password.text().to_string();
            let opts = ConnectOpts {
                password: if pw.is_empty() { None } else { Some(pw) },
                remember: remember.is_active(),
                save_as: if save.is_active() { Some(if typed_name.is_empty() { connections::default_name(&uri) } else { typed_name }) } else { None },
                plain_tab: false,
            };
            w.connect_remote(gio::File::for_uri(&uri), opts);
        });
        server.grab_focus();
    }

    /// Mount a server and show it: beside the local files (Preferences), or in a tab.
    pub fn connect_remote(self: &Rc<Self>, file: gio::File, opts: ConnectOpts) {
        let op = gtk::MountOperation::new(Some(&self.window));
        if let Some(password) = opts.password.clone() {
            let used = Cell::new(false);
            let remember = opts.remember;
            op.connect_ask_password(move |op, _message, _user, _domain, flags| {
                // Only the first question is answered from the dialog. If the server says no,
                // GTK's own prompt takes over, so a typo cannot loop.
                if used.replace(true) || !flags.contains(gio::AskPasswordFlags::NEED_PASSWORD) {
                    return;
                }
                op.set_password(Some(&password));
                op.set_password_save(if remember { gio::PasswordSave::Permanently } else { gio::PasswordSave::Never });
                op.stop_signal_emission_by_name("ask-password");
                op.reply(gio::MountOperationResult::Handled);
            });
        }
        let this = Rc::downgrade(self);
        let label = connections::short(&file.uri());
        glib::spawn_future_local(async move {
            let result = remote::ensure_mounted(&file, &op).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(()) => {
                    if let Some(name) = &opts.save_as {
                        connections::add(name, &file.uri());
                    }
                    // SFTP: prefer the remote home directory when the root was given.
                    let target = if file.uri_scheme().as_deref() == Some("sftp") && file.parent().is_none() {
                        let home = file.resolve_relative_path("~");
                        if home.query_exists(gio::Cancellable::NONE) {
                            home
                        } else {
                            file
                        }
                    } else {
                        file
                    };
                    if opts.plain_tab {
                        w.open_in_new_tab(target, true);
                    } else if w.prefs.get_bool("remote-two-panes", true) {
                        w.open_beside_local(target);
                    } else {
                        w.navigate(target);
                    }
                    w.refresh_sidebar();
                }
                Err(e) if e.matches(gio::IOErrorEnum::FailedHandled) => {}
                Err(e) => w.toast_error(&format!("{} {label}: {}", tr("Cannot connect to"), e.message())),
            }
        });
    }

    /// Two panes like WinSCP: what is on show stays on the left, `remote` opens on the right.
    pub fn open_beside_local(self: &Rc<Self>, remote: gio::File) {
        if !self.is_split() {
            self.split_boxes[1].set_visible(true);
            let width = self.split_paned.width();
            if width > 0 {
                self.split_paned.set_position(width);
                crate::motion::slide_paned(&self.split_paned, width / 2, crate::motion::PANEL, None);
            }
        }
        self.active_side.set(1);
        self.split_boxes[1].add_css_class("omafile-split-active");
        self.split_boxes[0].remove_css_class("omafile-split-active");
        self.open_in_new_tab(remote, true);
        self.sync_split_tools();
        if !self.prefs.get_bool("remote-hint-shown", false) {
            self.prefs.set("remote-hint-shown", Some("true"));
            self.toast(&tr("Local on the left, server on the right: drag across, or use the two arrows in the header (Shift+F5 copies, Shift+F6 moves)"));
        }
    }

    /// Take a saved connection out of the sidebar and its password out of the keyring.
    pub fn connection_remove(self: &Rc<Self>, name: &str) {
        if let Some(c) = connections::list().into_iter().find(|c| c.name == name) {
            connections::forget_password(&c.uri);
        }
        connections::remove(name);
        self.refresh_sidebar();
        self.toast(&tr("Connection removed; its saved password is forgotten"));
    }

    /// The copy / move buttons in the header: only with two panes, arrows pointing at the other one.
    pub fn sync_split_tools(&self) {
        let split = self.is_split();
        self.split_tools.set_visible(split);
        if split {
            let arrow = if self.active_side.get() == 0 { "go-next-symbolic" } else { "go-previous-symbolic" };
            let mut child = self.split_tools.first_child();
            while let Some(button) = child {
                child = button.next_sibling();
                if let Some(image) = button.first_child().and_then(|b| b.last_child()).and_downcast::<gtk::Image>() {
                    image.set_icon_name(Some(arrow));
                }
            }
        }
    }
}
