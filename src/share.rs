//! Share: copy an scp command, or serve the selection on the local network
//! for a moment with a QR code (qrencode). The page also takes uploads, so a
//! phone can send photos back into the folder on show.

use crate::i18n::tr;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::path::PathBuf;
use std::rc::Rc;

pub struct LanShare {
    /// Where the page lives: on the LocalSend port (open in Omarchy's firewall) or on a server of its own.
    host: Host,
    pub url: String,
    /// Port the page is served on, and whether it is the shared LocalSend port.
    pub port: u16,
    pub shared_port: bool,
    /// Files the phone uploaded, reported to the UI.
    pub uploads: std::sync::mpsc::Receiver<PathBuf>,
}

enum Host {
    /// Dropping the server stops it.
    Standalone(#[allow(dead_code)] crate::httpd::Server),
    Mounted(std::sync::Arc<crate::localsend::Node>, String),
}

impl Drop for LanShare {
    fn drop(&mut self) {
        if let Host::Mounted(node, prefix) = &self.host {
            node.unmount(prefix);
        }
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

/// Colours for the share page, from the Omarchy palette.
#[derive(Clone)]
pub struct PageTheme {
    pub bg: String,
    pub fg: String,
    pub accent: String,
    pub card: String,
    pub dim: String,
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The page a phone sees: the shared files to download, and a drop zone / picker to send files back.
pub fn page_html(files: &[PathBuf], upload_dir: &std::path::Path, theme: &PageTheme, strings: &[(&str, String)], base: &str) -> String {
    let t = |k: &str| strings.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone()).unwrap_or_else(|| k.to_string());
    let mut rows = String::new();
    for (i, p) in files.iter().enumerate() {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let size = std::fs::metadata(p).map(|m| if m.is_dir() { t("folder") } else { crate::ops::human_size(m.len()) }).unwrap_or_default();
        let href = format!("{base}/dl/{i}/{}", crate::httpd::percent_encode(&name));
        rows.push_str(&format!("<a class=\"file\" href=\"{href}\" download><span class=\"name\">{}</span><span class=\"size\">{}</span><span class=\"get\">↓</span></a>", escape(&name), escape(&size)));
    }
    let dest = escape(&crate::rules::shorten(upload_dir));
    format!(r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover"><title>omafile</title>
<style>
:root{{color-scheme:dark light}}
body{{margin:0;font:16px/1.4 -apple-system,system-ui,sans-serif;background:{bg};color:{fg};padding:max(16px,env(safe-area-inset-top)) 16px 32px}}
h1{{font-size:20px;margin:8px 0 4px;display:flex;align-items:center;gap:10px}}
h1 small{{color:{dim};font-weight:400;font-size:13px}}
h2{{font-size:13px;text-transform:uppercase;letter-spacing:.08em;color:{dim};margin:26px 0 10px}}
.card{{background:{card};border-radius:16px;overflow:hidden}}
.file{{display:flex;align-items:center;gap:12px;padding:14px 16px;text-decoration:none;color:{fg};border-top:1px solid rgba(127,127,127,.15)}}
.file:first-child{{border-top:0}}
.name{{flex:1;word-break:break-all}} .size{{color:{dim};font-size:13px}} .get{{color:{accent};font-weight:700}}
.drop{{border:2px dashed {accent};border-radius:16px;padding:28px 16px;text-align:center;color:{dim}}}
.drop.hot{{background:rgba(127,127,127,.12)}}
button,.btn{{display:inline-block;background:{accent};color:{bg};border:0;border-radius:999px;padding:12px 22px;font:600 16px system-ui;margin-top:12px;cursor:pointer}}
input[type=file]{{display:none}}
#status{{margin-top:12px;color:{dim};font-size:14px;white-space:pre-line}}
.done{{color:{accent}}}
</style></head><body>
<h1>omafile <small>{host}</small></h1>
<h2>{download}</h2>
<div class="card">{rows}</div>
<h2>{upload}</h2>
<div class="drop" id="drop">
<div>{drop_hint}</div>
<label class="btn" for="pick">{choose}</label>
<input id="pick" type="file" multiple>
<div id="status">{into} {dest}</div>
</div>
<script>
const drop=document.getElementById('drop'),status=document.getElementById('status'),pick=document.getElementById('pick');
async function send(files){{
  let n=0;for(const f of files){{
    status.textContent='{sending} '+f.name+' ('+(++n)+'/'+files.length+')';
    const fd=new FormData();fd.append('files',f,f.name);
    const r=await fetch('{base}/upload',{{method:'POST',body:fd}});
    if(!r.ok){{status.textContent='{failed} '+f.name;return;}}
  }}
  status.textContent='✓ '+n+' {sent}';status.className='done';
}}
pick.onchange=()=>send([...pick.files]);
['dragenter','dragover'].forEach(e=>drop.addEventListener(e,ev=>{{ev.preventDefault();drop.classList.add('hot')}}));
['dragleave','drop'].forEach(e=>drop.addEventListener(e,ev=>{{ev.preventDefault();drop.classList.remove('hot')}}));
drop.addEventListener('drop',ev=>send([...ev.dataTransfer.files]));
</script></body></html>"#,
        bg = theme.bg, fg = theme.fg, accent = theme.accent, card = theme.card, dim = theme.dim,
        host = escape(&glib::host_name()), download = escape(&t("Download")), upload = escape(&t("Send to this computer")),
        drop_hint = escape(&t("Drop files here or pick them")), choose = escape(&t("Choose files")), into = escape(&t("They land in")), dest = dest,
        sending = escape(&t("Sending")), failed = escape(&t("Failed:")), sent = escape(&t("files sent")), base = base)
}

/// Serve `paths` on the LAN and accept uploads into `upload_dir` until the `LanShare` is dropped.
pub fn start(paths: &[PathBuf], upload_dir: PathBuf, theme: PageTheme, strings: Vec<(&'static str, String)>, node: Option<std::sync::Arc<crate::localsend::Node>>) -> Result<LanShare, String> {
    let ip = lan_ip().ok_or_else(|| tr("No network address found: not connected to a LAN?"))?;
    let files: Vec<PathBuf> = paths.to_vec();
    let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
    let tx = std::sync::Mutex::new(tx);
    // An unguessable path keeps the page private to whoever has the link or QR code.
    let prefix = match &node {
        Some(_) => format!("/s/{}", (0..16).map(|_| format!("{:x}", glib::random_int_range(0, 16))).collect::<String>()),
        None => String::new(),
    };
    let page = page_html(&files, &upload_dir, &theme, &strings, &prefix);
    let handler: std::sync::Arc<crate::httpd::Handler> = std::sync::Arc::new(move |req: &crate::httpd::Request, body: &mut dyn std::io::Read| {
        use crate::httpd::Response;
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/") | ("GET", "/index.html") => Response::html(page.clone()),
            ("GET", p) if p.starts_with("/dl/") => {
                let index: usize = p.trim_start_matches("/dl/").split('/').next().and_then(|i| i.parse().ok()).unwrap_or(usize::MAX);
                match files.get(index) {
                    Some(f) if f.is_file() => {
                        let (ct, _) = gio::content_type_guess(Some(f.as_path()), None);
                        let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        Response::file(f, &ct).with_header("Content-Disposition", &format!("attachment; filename*=UTF-8''{}", crate::httpd::percent_encode(&name)))
                    }
                    _ => Response::empty(404),
                }
            }
            ("POST", "/upload") => {
                let ct = req.headers.get("content-type").cloned().unwrap_or_default();
                let mut saved = 0;
                crate::httpd::read_multipart(body, &ct, |name, reader| {
                    let target = crate::localsend::unique_path(&upload_dir, &crate::localsend::safe_name(name));
                    if let Ok(mut f) = std::fs::File::create(&target) {
                        if std::io::copy(reader, &mut f).is_ok() {
                            saved += 1;
                            let _ = tx.lock().unwrap().send(target);
                        }
                    }
                });
                if saved > 0 { Response::empty(200) } else { Response::empty(400) }
            }
            _ => Response::empty(404),
        }
    });
    if let Some(node) = node {
        let port = node.port();
        node.mount(&prefix, handler);
        return Ok(LanShare { host: Host::Mounted(node, prefix.clone()), url: format!("http://{ip}:{port}{prefix}/"), port, shared_port: true, uploads: rx });
    }
    let server = crate::httpd::start(0, handler).map_err(|e| e.to_string())?;
    let port = server.port;
    Ok(LanShare { host: Host::Standalone(server), url: format!("http://{ip}:{port}/"), port, shared_port: false, uploads: rx })
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
        self.toast(&trf!("Copied: {cmd}", cmd = cmd));
    }

    pub fn share_lan_dialog(self: &Rc<Self>) {
        self.share_lan_dialog_for(self.share_paths());
    }

    /// The share page for the given files, wherever they were picked.
    pub fn share_lan_dialog_for(self: &Rc<Self>, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let upload_dir = self.active_pane().and_then(|p| p.cwd().path()).filter(|p| p.is_dir()).unwrap_or_else(crate::nearby::default_dest);
        let pal = self.theme.palette();
        let theme = PageTheme { bg: pal.background.hex(), fg: pal.foreground.hex(), accent: pal.accent.hex(), card: pal.lighter_background.hex(), dim: pal.muted.hex() };
        let strings = vec![
            ("Download", tr("Download")), ("Send to this computer", tr("Send to this computer")), ("Drop files here or pick them", tr("Drop files here or pick them")),
            ("Choose files", tr("Choose files")), ("They land in", tr("They land in")), ("Sending", tr("Sending")), ("Failed:", tr("Failed:")), ("files sent", tr("files sent")), ("folder", tr("folder")),
        ];
        // Ride on the LocalSend port when it is ours: Omarchy's firewall lets only 53317 in.
        let node = self.nearby().map(|n| n.node.clone()).filter(|n| n.port() == crate::localsend::PORT);
        let share = match start(&paths, upload_dir.clone(), theme, strings, node) {
            Ok(s) => s,
            Err(e) => {
                self.toast_error(&trf!("Cannot share: {e}", e = e));
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
        let copy = gtk::Button::with_label(&tr("Copy Link"));
        copy.add_css_class("pill");
        let mut note_text = trf!("Anyone on your local network can download this while the dialog is open, and the page lets a phone send files back into {folder}. Closing it stops the share.", folder = crate::rules::shorten(&upload_dir));
        if !share.shared_port && glib::find_program_in_path("ufw").is_some() {
            note_text.push_str(&format!("\n\n{}", trf!("If the phone cannot open the page, the firewall is blocking port {port}: sudo ufw allow {port}/tcp", port = share.port)));
        }
        let note = gtk::Label::builder().label(note_text).wrap(true).justify(gtk::Justification::Center).selectable(true).build();
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
        header.set_title_widget(Some(&adw::WindowTitle::new(&tr("Sharing on local network"), &title)));
        content.add_top_bar(&header);
        content.set_content(Some(&vbox));
        let dialog = adw::Dialog::builder().title(tr("Share")).child(&content).content_width(360).build();
        let clip = self.window.clipboard();
        let url2 = url.clone();
        let this = Rc::downgrade(self);
        copy.connect_clicked(move |_| {
            clip.set_text(&url2);
            if let Some(w) = this.upgrade() {
                w.toast(&tr("Link copied"));
            }
        });
        let share = Rc::new(std::cell::RefCell::new(Some(share)));
        // Uploads from the phone: toast and refresh the folder.
        let this = Rc::downgrade(self);
        let share_poll = share.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
            let (Some(w), Some(s)) = (this.upgrade(), share_poll.borrow().as_ref().map(|s| s.uploads.try_recv().ok())) else { return glib::ControlFlow::Break };
            if let Some(path) = s {
                w.toast_keyed("share-upload", &trf!("Received {name}", name = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()));
                for p in w.panes.borrow().clone() {
                    if p.cwd().path().as_deref() == path.parent() {
                        p.reload();
                    }
                }
            }
            glib::ControlFlow::Continue
        });
        dialog.connect_closed(move |_| {
            share.borrow_mut().take(); // Drop stops the server.
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
    #[ignore]
    fn writes_page_for_a_screenshot() {
        let Some(out) = std::env::var_os("OMAFILE_PAGE_OUT") else { return };
        let theme = PageTheme { bg: "#1a1b26".into(), fg: "#c0caf5".into(), accent: "#7aa2f7".into(), card: "#24283b".into(), dim: "#565f89".into() };
        let files = vec![PathBuf::from("/tmp/Rechnung Oktober.pdf"), PathBuf::from("/tmp/Urlaub 2026.jpg"), PathBuf::from("/tmp/omarchy-theme.tar.gz")];
        std::fs::write(out, page_html(&files, std::path::Path::new("/home/nora/Downloads"), &theme, &[], "")).unwrap();
    }

    #[test]
    fn share_page_rides_on_the_localsend_port() {
        if glib::find_program_in_path("curl").is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("omafile-share-mounted-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, b"mounted").unwrap();
        let Ok((node, _rx)) = crate::localsend::start("test".into(), dir.clone()) else { return };
        let theme = PageTheme { bg: "#000".into(), fg: "#fff".into(), accent: "#7aa2f7".into(), card: "#222".into(), dim: "#888".into() };
        let Ok(share) = start(&[file.clone()], dir.clone(), theme, Vec::new(), Some(node.clone())) else { return };
        assert!(share.shared_port && share.url.contains("/s/"));
        let base = share.url.replace(&lan_ip().unwrap(), "127.0.0.1");
        let get = |url: &str| std::process::Command::new("curl").args(["-s", "--noproxy", "*", url]).output().map(|o| o.stdout).unwrap_or_default();
        let page = String::from_utf8_lossy(&get(&base)).to_string();
        assert!(page.contains("omafile") && page.contains("/dl/0/a.txt"), "{page}");
        assert_eq!(get(&format!("{base}dl/0/a.txt")), b"mounted");
        // LocalSend's own endpoints still answer next to it.
        assert!(String::from_utf8_lossy(&get(&format!("http://127.0.0.1:{}/api/localsend/v2/info", node.port()))).contains("fingerprint"));
        let prefix_gone = base.clone();
        drop(share);
        assert!(get(&prefix_gone).is_empty());
        node.stop();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn lan_share_serves_the_file() {
        if glib::find_program_in_path("curl").is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("omafile-share-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hello world.txt");
        std::fs::write(&file, b"shared!").unwrap();
        let theme = PageTheme { bg: "#000".into(), fg: "#fff".into(), accent: "#7aa2f7".into(), card: "#222".into(), dim: "#888".into() };
        let Ok(share) = start(&[file.clone()], dir.clone(), theme, Vec::new(), None) else { return }; // no LAN in some sandboxes
        let url = share.url.replace(&lan_ip().unwrap(), "127.0.0.1") + "dl/0/hello%20world.txt";
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
        // Upload back through the page's form.
        let page_url = share.url.replace(&lan_ip().unwrap(), "127.0.0.1");
        let out = std::process::Command::new("curl").args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "--noproxy", "*", "-F", &format!("files=@{}", file.display()), &format!("{page_url}upload")]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "200");
        assert_eq!(std::fs::read(dir.join("hello world (1).txt")).unwrap(), b"shared!");
        assert!(share.uploads.recv_timeout(std::time::Duration::from_secs(2)).is_ok());
        drop(share);
        let _ = std::fs::remove_dir_all(dir);
    }
}
