//! A drive's properties, and formatting it: from the context menu of a drive in the sidebar.

use crate::drives::{self, Info};
use crate::i18n::tr;
use crate::window::{PlaceKind, Window};
use adw::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;

fn row(title: String, value: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(glib::markup_escape_text(&title)).subtitle(glib::markup_escape_text(if value.is_empty() { "–" } else { value })).subtitle_selectable(true).build();
    row.add_css_class("property");
    row
}

fn attached(info: &Info) -> String {
    let how = match info.transport.as_str() {
        "usb" => tr("USB"),
        "nvme" => "NVMe".to_string(),
        "sata" => "SATA".to_string(),
        "mmc" => tr("SD card slot"),
        "virtio" => tr("virtual"),
        other if !other.is_empty() => other.to_uppercase(),
        _ => String::new(),
    };
    let removable = if info.removable { tr("removable") } else { String::new() };
    [how, removable].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ")
}

impl Window {
    /// The device behind the sidebar row the menu was opened on.
    fn sidebar_device(self: &Rc<Self>) -> Option<(String, String)> {
        // For the tests and the screenshots: a device named outright, no sidebar needed.
        if let Ok(device) = std::env::var("OMAFILE_DEMO_DRIVE") {
            return Some((device.rsplit('/').next().unwrap_or("drive").to_string(), device));
        }
        match self.sidebar_target_place()? {
            (_, PlaceKind::Mount(m)) => drives::device_of_mount(&m).map(|d| (m.name().to_string(), d)),
            (_, PlaceKind::Volume(v)) => drives::device_of_volume(&v).map(|d| (v.name().to_string(), d)),
            _ => None,
        }
    }

    /// Sidebar › a drive › Properties.
    pub fn drive_properties(self: &Rc<Self>) {
        let Some((name, device)) = self.sidebar_device() else {
            self.toast_error(&tr("No device is known for this drive"));
            return;
        };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let asked = device.clone();
            let info = gio::spawn_blocking(move || drives::info(&asked)).await.ok().flatten();
            let Some(w) = this.upgrade() else { return };
            match info {
                Some(info) => w.drive_properties_dialog(&name, info),
                None => w.toast_error(&crate::trf!("lsblk knows nothing about {device}", device = device)),
            }
        });
    }

    fn drive_properties_dialog(self: &Rc<Self>, name: &str, info: Info) {
        let group = adw::PreferencesGroup::new();
        group.add(&row(tr("Device"), &info.device));
        if let Some(disk) = &info.disk {
            group.add(&row(tr("Disk"), disk));
        }
        let hardware = [info.vendor.as_str(), info.model.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
        if !hardware.is_empty() {
            group.add(&row(tr("Hardware"), &hardware));
        }
        group.add(&row(tr("Attached"), &attached(&info)));
        group.add(&row(tr("Size"), &crate::ops::human_size(info.size)));
        if let (Some(used), Some(available)) = (info.used, info.available) {
            let total = used + available;
            let percent = (used * 100).checked_div(total).unwrap_or(0);
            group.add(&row(tr("Used"), &crate::trf!("{used} of {total} ({percent} %)", used = crate::ops::human_size(used), total = crate::ops::human_size(total), percent = percent)));
            group.add(&row(tr("Free"), &crate::ops::human_size(available)));
        }
        group.add(&row(tr("File system"), &info.filesystem));
        group.add(&row(tr("Label"), &info.label));
        group.add(&row(tr("UUID"), &info.uuid));
        group.add(&row(tr("Mounted at"), &info.mountpoint));
        let page = adw::PreferencesPage::new();
        page.add(&group);
        let format = gtk::Button::builder().label(tr("Format…")).halign(gtk::Align::End).margin_end(12).margin_bottom(12).build();
        format.add_css_class("destructive-action");
        format.add_css_class("pill");
        format.set_sensitive(drives::can_format(&info).is_ok());
        if let Err(why) = drives::can_format(&info) {
            format.set_tooltip_text(Some(&tr(why)));
        }
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.append(&page);
        column.append(&format);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        view.set_content(Some(&column));
        let dialog = adw::Dialog::builder().title(name).content_width(480).child(&view).build();
        let (this, weak, name) = (Rc::downgrade(self), dialog.downgrade(), name.to_string());
        format.connect_clicked(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.close();
            }
            if let Some(w) = this.upgrade() {
                w.drive_format_dialog(&name, info.clone());
            }
        });
        dialog.present(Some(&self.window));
    }

    /// Sidebar › a drive › Format…
    pub fn drive_format(self: &Rc<Self>) {
        let Some((name, device)) = self.sidebar_device() else {
            self.toast_error(&tr("No device is known for this drive"));
            return;
        };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let asked = device.clone();
            let info = gio::spawn_blocking(move || drives::info(&asked)).await.ok().flatten();
            let Some(w) = this.upgrade() else { return };
            match info {
                Some(info) => match drives::can_format(&info) {
                    Ok(()) => w.drive_format_dialog(&name, info),
                    Err(why) => w.toast_error(&tr(why)),
                },
                None => w.toast_error(&crate::trf!("lsblk knows nothing about {device}", device = device)),
            }
        });
    }

    fn drive_format_dialog(self: &Rc<Self>, name: &str, info: Info) {
        let systems = drives::filesystems_available();
        if systems.is_empty() {
            self.toast_error(&tr("No program to make a file system was found (install e2fsprogs, exfatprogs or dosfstools)"));
            return;
        }
        if glib::find_program_in_path("udisksctl").is_none() {
            self.toast_error(&tr("udisksctl is needed to format a drive, and it is not installed (package udisks2)"));
            return;
        }
        let names: Vec<String> = systems.iter().map(|(_, n)| tr(n)).collect();
        let model = gtk::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let choice = adw::ComboRow::builder().title(tr("File system")).model(&model).build();
        // What it has now, when that can be made again; else exFAT for a removable drive.
        let preselect = systems.iter().position(|(id, _)| *id == info.filesystem).or_else(|| info.removable.then(|| systems.iter().position(|(id, _)| *id == "exfat")).flatten()).unwrap_or(0);
        choice.set_selected(preselect as u32);
        let label = adw::EntryRow::builder().title(tr("Name of the drive")).text(&info.label).build();
        let sure = adw::SwitchRow::builder().title(tr("Erase everything on it")).subtitle(crate::trf!("{size} on {device}; nothing on it can be brought back afterwards", size = crate::ops::human_size(info.size), device = info.device)).build();
        let rows = gtk::ListBox::new();
        rows.set_selection_mode(gtk::SelectionMode::None);
        rows.add_css_class("boxed-list");
        rows.set_width_request(420);
        rows.append(&choice);
        rows.append(&label);
        rows.append(&sure);
        let dialog = adw::AlertDialog::new(Some(crate::trf!("Format {name}?", name = name).as_str()), Some(tr("The drive is unmounted, everything on it is erased, and a new, empty file system is written. The system asks for your password first.").as_str()));
        dialog.set_extra_child(Some(&rows));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("format", tr("Format").as_str())]);
        dialog.set_response_appearance("format", adw::ResponseAppearance::Destructive);
        dialog.set_response_enabled("format", false);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let weak = dialog.downgrade();
        sure.connect_active_notify(move |row| {
            if let Some(dialog) = weak.upgrade() {
                dialog.set_response_enabled("format", row.is_active());
            }
        });
        let this = Rc::downgrade(self);
        let (choice2, label2, name) = (choice.clone(), label.clone(), name.to_string());
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            if response != "format" || !sure.is_active() {
                w.refocus();
                return;
            }
            let Some((filesystem, _)) = systems.get(choice2.selected() as usize) else { return };
            w.drive_format_run(&name, &info, filesystem, &label2.text());
        });
    }

    fn drive_format_run(self: &Rc<Self>, name: &str, info: &Info, filesystem: &str, label: &str) {
        let commands = drives::format_commands(info, filesystem, label);
        self.toast(&crate::trf!("Formatting {name}…", name = name));
        let (this, name) = (Rc::downgrade(self), name.to_string());
        glib::spawn_future_local(async move {
            let mut failed = None;
            for argv in commands {
                let args: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
                let process = match gio::Subprocess::newv(&args, gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_PIPE) {
                    Ok(p) => p,
                    Err(e) => {
                        failed = Some(e.message().to_string());
                        break;
                    }
                };
                let (_, err) = process.communicate_utf8_future(None).await.unwrap_or((None, None));
                if !process.is_successful() {
                    let text = err.map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).unwrap_or_else(|| argv.join(" "));
                    // An unmount that fails because nothing is mounted is no failure.
                    if argv.get(1).map(|s| s.as_str()) == Some("unmount") && text.contains("not mounted") {
                        continue;
                    }
                    failed = Some(text);
                    break;
                }
            }
            let Some(w) = this.upgrade() else { return };
            match failed {
                None => w.toast(&crate::trf!("{name} is formatted", name = name)),
                Some(text) => w.toast_error(&crate::trf!("Formatting {name} failed: {error}", name = name, error = text.lines().last().unwrap_or(&text))),
            }
            w.refresh_sidebar();
        });
    }
}
