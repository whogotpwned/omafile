//! Nearby devices (LocalSend): a sidebar section with every device on the
//! LAN, drop files on one to send them, a dialog for sending and for the
//! receive settings, and the incoming-transfer prompt.

use crate::i18n::tr;
use crate::localsend::{self, Event, Node, Peer};
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

pub struct Nearby {
    pub node: Arc<Node>,
    pub peers: RefCell<Vec<Peer>>,
    /// Folder where received files land.
    pub dest: RefCell<PathBuf>,
    pub last_received: RefCell<Option<PathBuf>>,
    /// Rows of the open devices dialog, rebuilt on changes.
    list: RefCell<Option<gtk::ListBox>>,
}

impl Drop for Nearby {
    fn drop(&mut self) {
        self.node.stop();
    }
}

pub fn default_dest() -> PathBuf {
    glib::user_special_dir(glib::UserDirectory::Downloads).unwrap_or_else(|| glib::home_dir().join("Downloads"))
}

pub fn alias() -> String {
    format!("omafile on {}", glib::host_name())
}

impl Window {
    /// Start listening on the LAN (once) and pump events to the UI.
    pub fn nearby(self: &Rc<Self>) -> Option<Rc<Nearby>> {
        if let Some(n) = self.nearby.borrow().as_ref() {
            return Some(n.clone());
        }
        if !self.prefs.get_bool("nearby-enabled", true) {
            return None;
        }
        let dest = self.prefs.get("nearby-folder").map(|p| crate::rules::expand(&p)).unwrap_or_else(default_dest);
        let (node, rx) = match localsend::start(alias(), dest.clone()) {
            Ok(x) => x,
            Err(e) => {
                if std::env::var_os("OMAFILE_TRACE").is_some() {
                    eprintln!("nearby: {e}");
                }
                return None;
            }
        };
        let nearby = Rc::new(Nearby { node, peers: RefCell::new(Vec::new()), dest: RefCell::new(dest), last_received: RefCell::new(None), list: RefCell::new(None) });
        self.nearby.replace(Some(nearby.clone()));
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            let Some(w) = weak.upgrade() else { return glib::ControlFlow::Break };
            let Some(n) = w.nearby.borrow().clone() else { return glib::ControlFlow::Break };
            while let Ok(event) = rx.try_recv() {
                w.nearby_event(&n, event);
            }
            glib::ControlFlow::Continue
        });
        Some(nearby)
    }

    fn nearby_event(self: &Rc<Self>, n: &Rc<Nearby>, event: Event) {
        match event {
            Event::PeersChanged => {
                let peers = n.node.peers();
                let changed = *n.peers.borrow() != peers;
                n.peers.replace(peers);
                if changed {
                    self.refresh_sidebar();
                    self.nearby_fill_list(n);
                    crate::status::write(self);
                }
            }
            Event::Incoming { alias, files, reply } => {
                let total: u64 = files.iter().map(|f| f.size).sum();
                let names: Vec<String> = files.iter().take(5).map(|f| f.name.clone()).collect();
                let mut body = names.join("\n");
                if files.len() > 5 {
                    body.push_str(&format!("\n… +{}", files.len() - 5));
                }
                let heading = if files.len() == 1 { trf!("{device} wants to send a file ({size})", device = alias.clone(), size = crate::ops::human_size(total)) } else { trf!("{device} wants to send {n} files ({size})", device = alias.clone(), n = files.len(), size = crate::ops::human_size(total)) };
                let dialog = adw::AlertDialog::builder().heading(heading).body(format!("{body}\n\n→ {}", crate::rules::shorten(&n.dest.borrow()))).build();
                dialog.add_response("decline", &tr("Decline"));
                dialog.add_response("accept", &tr("Accept"));
                dialog.set_response_appearance("accept", adw::ResponseAppearance::Suggested);
                dialog.set_default_response(Some("accept"));
                dialog.set_close_response("decline");
                let reply = RefCell::new(Some(reply));
                dialog.connect_response(None, move |_, r| {
                    if let Some(tx) = reply.borrow_mut().take() {
                        let _ = tx.send(r == "accept");
                    }
                });
                dialog.present(Some(&self.window));
                self.window.present();
            }
            Event::Received(path) => {
                n.last_received.replace(Some(path.clone()));
                self.toast_keyed("nearby-recv", &trf!("Received {name}", name = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()));
            }
            Event::SessionDone { alias, files } => {
                let dest = n.dest.borrow().clone();
                for p in self.panes.borrow().clone() {
                    if p.cwd().path().as_deref() == Some(dest.as_path()) {
                        p.reload();
                    }
                }
                let msg = trf!("{n} files from {device} are in {folder}", n = files.len(), device = alias, folder = crate::rules::shorten(&dest));
                let toast = adw::Toast::builder().use_markup(false).title(&msg).timeout(8).button_label(tr("Show")).action_name("win.nearby-show-received").build();
                self.toast.add_toast(toast);
                if let Some(first) = files.first() {
                    n.last_received.replace(Some(first.clone()));
                }
            }
            Event::SendProgress { alias, done, total } => {
                self.toast_keyed("nearby-send", &trf!("Sending to {device}: {done} / {total}", device = alias, done = done, total = total));
            }
            Event::SendDone { alias, sent, error } => match error {
                None => self.toast_keyed("nearby-send", &trf!("Sent {n} to {device}", n = sent, device = alias)),
                Some(e) if e == "declined" => self.toast_keyed("nearby-send", &trf!("{device} declined", device = alias)),
                Some(e) => self.toast_error(&trf!("Sending to {device} failed: {e}", device = alias, e = e)),
            },
        }
    }

    pub fn nearby_peers(&self) -> Vec<Peer> {
        self.nearby.borrow().as_ref().map(|n| n.peers.borrow().clone()).unwrap_or_default()
    }

    /// Send the selection (or `files`) to the peer with this fingerprint.
    pub fn nearby_send(self: &Rc<Self>, fingerprint: &str, files: Vec<gio::File>) {
        let Some(n) = self.nearby() else { return };
        let Some(peer) = n.peers.borrow().iter().find(|p| p.fingerprint == fingerprint).cloned() else { return };
        let paths: Vec<PathBuf> = files.iter().filter_map(|f| f.path()).filter(|p| p.is_file()).collect();
        if paths.is_empty() {
            self.toast_error(&tr("Select files to send (folders are not sent)"));
            return;
        }
        self.toast_keyed("nearby-send", &trf!("Sending {n} to {device}…", n = paths.len(), device = peer.alias.clone()));
        localsend::send(n.node.clone(), peer, paths);
    }

    pub fn nearby_show_received(self: &Rc<Self>) {
        let Some(n) = self.nearby.borrow().clone() else { return };
        let dest = n.dest.borrow().clone();
        let last = n.last_received.borrow().clone();
        self.navigate(gio::File::for_path(&dest));
        if let (Some(last), Some(p)) = (last, self.active_pane()) {
            p.select_file_later(gio::File::for_path(last));
        }
    }

    /// The devices dialog: list, send, receive settings.
    pub fn open_nearby(self: &Rc<Self>) {
        let Some(n) = self.nearby() else {
            self.toast_error(&tr("Nearby devices are off (Preferences › Nearby)"));
            return;
        };
        n.node.announce();
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&adw::WindowTitle::new(&tr("Nearby Devices"), &trf!("visible as “{alias}”", alias = n.node.alias.clone()))));
        let refresh = gtk::Button::builder().icon_name("view-refresh-symbolic").tooltip_text(tr("Look again")).build();
        header.pack_end(&refresh);
        let page = adw::PreferencesPage::new();
        let devices = adw::PreferencesGroup::builder().title(tr("Devices")).description(tr("Phones and computers running LocalSend on this network. Drop files on a device in the sidebar, or pick one here to send the selection.")).build();
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        devices.add(&list);
        page.add(&devices);
        n.list.replace(Some(list.clone()));
        self.nearby_fill_list(&n);

        let receive = adw::PreferencesGroup::builder().title(tr("Receiving")).build();
        let folder = adw::ActionRow::builder().title(tr("Save received files in")).subtitle(glib::markup_escape_text(&crate::rules::shorten(&n.dest.borrow()))).activatable(true).build();
        folder.add_suffix(&gtk::Image::from_icon_name("folder-open-symbolic"));
        let this = Rc::downgrade(self);
        let (n2, f2) = (n.clone(), folder.clone());
        folder.connect_activated(move |_| {
            let (Some(w), n3, f3) = (this.upgrade(), n2.clone(), f2.clone()) else { return };
            let chooser = gtk::FileDialog::builder().title(tr("Choose a folder")).modal(true).build();
            let initial = n3.dest.borrow().clone();
            chooser.set_initial_folder(Some(&gio::File::for_path(&initial)));
            let parent = w.window.clone();
            chooser.select_folder(Some(&parent), gio::Cancellable::NONE, move |r| {
                if let Ok(Some(p)) = r.map(|f| f.path()) {
                    n3.node.set_dest(p.clone());
                    n3.dest.replace(p.clone());
                    w.prefs.set("nearby-folder", Some(&p.to_string_lossy()));
                    f3.set_subtitle(&glib::markup_escape_text(&crate::rules::shorten(&p)));
                }
            });
        });
        receive.add(&folder);
        let auto = adw::SwitchRow::builder().title(tr("Accept without asking")).subtitle(tr("Files from any device on this network land in the folder straight away")).active(self.prefs.get_bool("nearby-accept-all", false)).build();
        n.node.set_accept_all(auto.is_active());
        let this = Rc::downgrade(self);
        let n2 = n.clone();
        auto.connect_active_notify(move |row| {
            n2.node.set_accept_all(row.is_active());
            if let Some(w) = this.upgrade() {
                w.prefs.set("nearby-accept-all", Some(if row.is_active() { "true" } else { "false" }));
            }
        });
        receive.add(&auto);
        let info = adw::ActionRow::builder().title(tr("On the iPhone")).subtitle(tr("Install the free LocalSend app, open it, and in Settings turn “Encryption” off: omafile speaks plain HTTP on this network only. Then this computer shows up there and the iPhone shows up here.")).build();
        receive.add(&info);
        page.add(&receive);
        let n2 = n.clone();
        refresh.connect_clicked(move |_| n2.node.announce());

        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&page));
        let dialog = adw::Dialog::builder().title(tr("Nearby Devices")).content_width(560).content_height(560).child(&view).build();
        let n2 = n.clone();
        dialog.connect_closed(move |_| {
            n2.list.replace(None);
        });
        self.present_dialog(&dialog);
    }

    fn nearby_fill_list(self: &Rc<Self>, n: &Rc<Nearby>) {
        let Some(list) = n.list.borrow().clone() else { return };
        while let Some(row) = list.row_at_index(0) {
            list.remove(&row);
        }
        let peers = n.peers.borrow().clone();
        if peers.is_empty() {
            let row = adw::ActionRow::builder().title(tr("No devices yet")).subtitle(tr("Open LocalSend on the other device; it appears within a few seconds.")).build();
            row.add_prefix(&gtk::Spinner::builder().spinning(true).build());
            list.append(&row);
            return;
        }
        for peer in peers {
            let detail = format!("{}  ·  {}:{}{}", peer.device_model, peer.ip, peer.port, if peer.protocol == "http" { String::new() } else { format!("  ·  {}", tr("encryption on (cannot send)")) });
            let row = adw::ActionRow::builder().title(glib::markup_escape_text(&peer.alias)).subtitle(glib::markup_escape_text(&detail)).build();
            row.add_prefix(&gtk::Image::from_icon_name(peer.icon()));
            let send = gtk::Button::with_label(&tr("Send Selection"));
            send.set_valign(gtk::Align::Center);
            send.add_css_class("suggested-action");
            send.set_sensitive(peer.protocol == "http");
            let this = Rc::downgrade(self);
            let fp = peer.fingerprint.clone();
            send.connect_clicked(move |_| {
                if let Some(w) = this.upgrade() {
                    let files: Vec<gio::File> = w.selected_items().iter().map(|i| i.target_file()).collect();
                    w.nearby_send(&fp, files);
                }
            });
            row.add_suffix(&send);
            list.append(&row);
        }
    }
}
