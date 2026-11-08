//! Share: copy an scp command, or serve the selection on the local network
//! for a moment (python http.server) with a QR code (qrencode).

use crate::window::Window;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::path::PathBuf;
use std::rc::Rc;

pub struct LanShare {
    process: gio::Subprocess,
    dir: PathBuf,
    pub url: String,
}

impl Drop for LanShare {
    fn drop(&mut self) {
        self.process.force_exit();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The address other machines on the LAN would use (no packet is sent).
pub fn lan_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("10.255.255.255:1").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip.to_string())
    }
}

fn free_port() -> Option<u16> {
    std::net::TcpListener::bind("0.0.0.0:0").ok()?.local_addr().ok().map(|a| a.port())
}

pub fn scp_command(paths: &[PathBuf]) -> String {
    let user = glib::user_name().to_string_lossy().to_string();
    let host = lan_ip().unwrap_or_else(|| glib::host_name().to_string());
    let quote = |p: &PathBuf| {
        let s = p.to_string_lossy();
        if s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-@".contains(c)) { s.to_string() } else { format!("'{}'", s.replace('\'', "'\\''")) }
    };
    let recursive = paths.iter().any(|p| p.is_dir());
    let sources: Vec<String> = paths.iter().map(|p| format!("{user}@{host}:{}", quote(p))).collect();
    format!("scp {}{} .", if recursive { "-r " } else { "" }, sources.join(" "))
}

/// Serve `paths` from a scratch directory of symlinks so siblings stay private.
pub fn start(paths: &[PathBuf]) -> Result<LanShare, String> {
    if glib::find_program_in_path("python3").is_none() {
        return Err("python3 is not installed".into());
    }
    let ip = lan_ip().ok_or("No network address found: not connected to a LAN?")?;
    let port = free_port().ok_or("No free port")?;
    let dir = glib::user_runtime_dir().join(format!("omafile-share-{port}"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for p in paths {
        let Some(name) = p.file_name() else { continue };
        let _ = std::os::unix::fs::symlink(p, dir.join(name));
    }
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE);
    launcher.set_cwd(&dir);
    let port_s = port.to_string();
    let argv = ["python3", "-m", "http.server", &port_s, "--bind", "0.0.0.0"];
    let args: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
    let process = launcher.spawn(&args).map_err(|e| e.message().to_string())?;
    let url = match paths {
        [one] if one.is_file() => format!("http://{ip}:{port}/{}", glib::uri_escape_string(one.file_name().unwrap().to_string_lossy().as_ref(), None::<&str>, false)),
        _ => format!("http://{ip}:{port}/"),
    };
    Ok(LanShare { process, dir, url })
}

pub fn qr_texture(text: &str) -> Option<gdk::Texture> {
    glib::find_program_in_path("qrencode")?;
    let out = std::process::Command::new("qrencode").args(["-t", "PNG", "-s", "6", "-m", "1", "-o", "-", text]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    gdk::Texture::from_bytes(&glib::Bytes::from(&out.stdout)).ok()
}

impl Window {
    fn share_paths(&self) -> Vec<PathBuf> {
        self.selected_items().iter().filter_map(|i| i.target_file().path()).collect()
    }

    pub fn share_copy_scp(self: &Rc<Self>) {
        let paths = self.share_paths();
        if paths.is_empty() {
            return;
        }
        let cmd = scp_command(&paths);
        self.window.clipboard().set_text(&cmd);
        self.toast(&format!("Copied: {cmd}"));
    }

    pub fn share_lan_dialog(self: &Rc<Self>) {
        let paths = self.share_paths();
        if paths.is_empty() {
            return;
        }
        let share = match start(&paths) {
            Ok(s) => s,
            Err(e) => {
                self.toast_error(&format!("Cannot share: {e}"));
                return;
            }
        };
        let url = share.url.clone();
        let picture = gtk::Picture::new();
        picture.set_size_request(220, 220);
        picture.set_can_shrink(false);
        picture.add_css_class("omafile-qr");
        if let Some(texture) = qr_texture(&url) {
            picture.set_paintable(Some(&texture));
        } else {
            picture.set_visible(false);
        }
        let link = gtk::Label::builder().label(&url).selectable(true).wrap(true).justify(gtk::Justification::Center).build();
        link.add_css_class("omafile-share-url");
        let copy = gtk::Button::with_label("Copy Link");
        copy.add_css_class("pill");
        let note = gtk::Label::builder().label("Anyone on your local network can download this while the dialog is open. Closing it stops the share.").wrap(true).justify(gtk::Justification::Center).build();
        note.add_css_class("omafile-dim");
        note.add_css_class("caption");
        let n = paths.len();
        let title = if n == 1 { paths[0].file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default() } else { format!("{n} items") };
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 14);
        vbox.set_margin_top(6);
        vbox.set_margin_bottom(18);
        vbox.set_margin_start(24);
        vbox.set_margin_end(24);
        vbox.set_halign(gtk::Align::Center);
        vbox.append(&picture);
        vbox.append(&link);
        vbox.append(&copy);
        vbox.append(&note);
        let content = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&adw::WindowTitle::new("Sharing on local network", &title)));
        content.add_top_bar(&header);
        content.set_content(Some(&vbox));
        let dialog = adw::Dialog::builder().title("Share").child(&content).content_width(360).build();
        let clip = self.window.clipboard();
        let url2 = url.clone();
        let this = Rc::downgrade(self);
        copy.connect_clicked(move |_| {
            clip.set_text(&url2);
            if let Some(w) = this.upgrade() {
                w.toast("Link copied");
            }
        });
        let share = std::cell::RefCell::new(Some(share));
        dialog.connect_closed(move |_| {
            share.borrow_mut().take(); // Drop stops the server and removes the scratch dir.
        });
        self.present_dialog(&dialog);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scp_command_quotes_odd_names() {
        let cmd = scp_command(&[PathBuf::from("/tmp/a b.txt"), PathBuf::from("/tmp/plain.txt")]);
        assert!(cmd.starts_with("scp "));
        assert!(cmd.contains("'/tmp/a b.txt'"));
        assert!(cmd.contains(":/tmp/plain.txt"));
        assert!(cmd.ends_with(" ."));
    }

    #[test]
    fn lan_share_serves_the_file() {
        if glib::find_program_in_path("python3").is_none() || glib::find_program_in_path("curl").is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("omafile-share-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hello world.txt");
        std::fs::write(&file, b"shared!").unwrap();
        let Ok(share) = start(&[file.clone()]) else { return }; // no LAN in some sandboxes
        let url = share.url.replace(&lan_ip().unwrap(), "127.0.0.1");
        let mut body = Vec::new();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(150));
            if let Ok(out) = std::process::Command::new("curl").args(["-s", "--noproxy", "*", &url]).output() {
                if out.status.success() && !out.stdout.is_empty() {
                    body = out.stdout;
                    break;
                }
            }
        }
        assert_eq!(body, b"shared!");
        let scratch = share.dir.clone();
        drop(share);
        assert!(!scratch.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
