//! The LocalSend protocol (v2): discover devices on the LAN by UDP
//! multicast, receive files over HTTP, send files to a device. The free
//! LocalSend app on an iPhone or iPad speaks the same protocol, so files go
//! both ways without a cloud. Plain HTTP: the peer's "Encryption" setting
//! must be off, since omafile ships no TLS stack.

use crate::httpd::{self, Body, Request, Response};
use crate::json::{self, Json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const MULTICAST: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 167);
pub const PORT: u16 = 53317;
const API: &str = "/api/localsend/v2";

#[derive(Clone, Debug, PartialEq)]
pub struct Peer {
    pub alias: String,
    pub ip: String,
    pub port: u16,
    pub protocol: String,
    pub device_model: String,
    pub device_type: String,
    pub fingerprint: String,
    pub last_seen: Instant,
}

impl Peer {
    pub fn is_mobile(&self) -> bool {
        self.device_type == "mobile"
    }

    pub fn is_apple(&self) -> bool {
        let m = self.device_model.to_lowercase();
        m.contains("iphone") || m.contains("ipad") || m.contains("ios") || m.contains("mac")
    }

    pub fn icon(&self) -> &'static str {
        match self.device_type.as_str() {
            "mobile" => "phone-symbolic",
            "web" => "web-browser-symbolic",
            "headless" | "server" => "network-server-symbolic",
            _ => "computer-symbolic",
        }
    }
}

#[derive(Clone, Debug)]
pub struct IncomingFile {
    pub id: String,
    pub name: String,
    pub size: u64,
}

/// What the network thread tells the UI.
pub enum Event {
    PeersChanged,
    /// A device wants to send; answer on the channel (true = accept).
    Incoming { alias: String, files: Vec<IncomingFile>, reply: Sender<bool> },
    Received(PathBuf),
    SessionDone { alias: String, files: Vec<PathBuf> },
    SendProgress { alias: String, done: usize, total: usize },
    SendDone { alias: String, sent: usize, error: Option<String> },
}

struct Session {
    alias: String,
    tokens: HashMap<String, (String, IncomingFile)>,
    received: Vec<PathBuf>,
}

pub struct Node {
    pub alias: String,
    pub fingerprint: String,
    port: AtomicU16,
    peers: Mutex<Vec<Peer>>,
    sessions: Mutex<HashMap<String, Session>>,
    dest: Mutex<PathBuf>,
    accept_all: AtomicBool,
    events: Mutex<Sender<Event>>,
    stop: Arc<AtomicBool>,
    server: Mutex<Option<httpd::Server>>,
    /// Another handler served under a path prefix on the same port (the share
    /// page): Omarchy's firewall only lets 53317 in, so everything rides on it.
    mounted: Mutex<Option<(String, Arc<httpd::Handler>)>>,
}

impl Node {
    pub fn info(&self, announce: bool) -> Json {
        Json::obj(vec![
            ("alias", Json::s(self.alias.clone())),
            ("version", Json::s("2.0")),
            ("deviceModel", Json::s("Linux")),
            ("deviceType", Json::s("desktop")),
            ("fingerprint", Json::s(self.fingerprint.clone())),
            ("port", Json::Number(self.port() as f64)),
            ("protocol", Json::s("http")),
            ("download", Json::Bool(false)),
            ("announce", Json::Bool(announce)),
        ])
    }

    pub fn port(&self) -> u16 {
        self.port.load(Ordering::Relaxed)
    }

    pub fn peers(&self) -> Vec<Peer> {
        let cutoff = Instant::now() - Duration::from_secs(120);
        let mut peers = self.peers.lock().unwrap();
        peers.retain(|p| p.last_seen > cutoff);
        peers.clone()
    }

    pub fn set_dest(&self, dest: PathBuf) {
        *self.dest.lock().unwrap() = dest;
    }

    pub fn dest(&self) -> PathBuf {
        self.dest.lock().unwrap().clone()
    }

    pub fn set_accept_all(&self, on: bool) {
        self.accept_all.store(on, Ordering::Relaxed);
    }

    fn emit(&self, e: Event) {
        let _ = self.events.lock().unwrap().send(e);
    }

    fn remember(&self, peer: Peer) {
        if peer.fingerprint == self.fingerprint {
            return;
        }
        let mut peers = self.peers.lock().unwrap();
        let changed = match peers.iter_mut().find(|p| p.fingerprint == peer.fingerprint) {
            Some(existing) => {
                let changed = existing.alias != peer.alias || existing.ip != peer.ip || existing.port != peer.port;
                *existing = peer;
                changed
            }
            None => {
                peers.push(peer);
                true
            }
        };
        drop(peers);
        if changed {
            self.emit(Event::PeersChanged);
        }
    }

    /// Send our announcement to the multicast group.
    pub fn announce(&self) {
        if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
            let _ = sock.set_multicast_ttl_v4(1);
            let _ = sock.send_to(self.info(true).to_string().as_bytes(), SocketAddrV4::new(MULTICAST, PORT));
        }
    }

    /// Serve `handler` for every path below `prefix` (e.g. `/s/ab12`).
    pub fn mount(&self, prefix: &str, handler: Arc<httpd::Handler>) {
        *self.mounted.lock().unwrap() = Some((prefix.trim_end_matches('/').to_string(), handler));
    }

    pub fn unmount(&self, prefix: &str) {
        let mut m = self.mounted.lock().unwrap();
        if m.as_ref().map(|(p, _)| p == prefix.trim_end_matches('/')).unwrap_or(false) {
            *m = None;
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.server.lock().unwrap().take();
    }
}

/// Parse a device announcement / register body into a Peer.
pub fn parse_peer(body: &str, ip: &str) -> Option<Peer> {
    let j = json::parse(body)?;
    Some(Peer {
        alias: j.str("alias")?.to_string(),
        ip: ip.to_string(),
        port: j.num("port").map(|p| p as u16).unwrap_or(PORT),
        protocol: j.str("protocol").unwrap_or("https").to_string(),
        device_model: j.str("deviceModel").unwrap_or("").to_string(),
        device_type: j.str("deviceType").unwrap_or("desktop").to_string(),
        fingerprint: j.str("fingerprint").unwrap_or("").to_string(),
        last_seen: Instant::now(),
    })
}

fn random_hex(n: usize) -> String {
    (0..n).map(|_| format!("{:x}", gtk::glib::random_int_range(0, 16))).collect()
}

/// Start listening (HTTP on 53317 or the next free port, UDP multicast on 53317) and announce.
pub fn start(alias: String, dest: PathBuf) -> Result<(Arc<Node>, Receiver<Event>), String> {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let node = Arc::new(Node {
        alias,
        fingerprint: random_hex(32),
        port: AtomicU16::new(0),
        peers: Mutex::new(Vec::new()),
        sessions: Mutex::new(HashMap::new()),
        dest: Mutex::new(dest),
        accept_all: AtomicBool::new(false),
        events: Mutex::new(tx),
        stop: stop.clone(),
        server: Mutex::new(None),
        mounted: Mutex::new(None),
    });
    let weak = Arc::downgrade(&node);
    let handler: Arc<httpd::Handler> = Arc::new(move |req: &Request, body: &mut dyn Read| match weak.upgrade() {
        Some(n) => handle(&n, req, body),
        None => Response::empty(500),
    });
    let server = httpd::start(PORT, handler.clone()).or_else(|_| httpd::start(0, handler)).map_err(|e| e.to_string())?;
    node.port.store(server.port, Ordering::Relaxed);
    *node.server.lock().unwrap() = Some(server);
    listen_udp(node.clone());
    node.announce();
    Ok((node, rx))
}

fn listen_udp(node: Arc<Node>) {
    std::thread::Builder::new().name("omafile-localsend-udp".into()).spawn(move || {
        let Ok(sock) = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, PORT)) else { return };
        let _ = sock.join_multicast_v4(&MULTICAST, &Ipv4Addr::UNSPECIFIED);
        let _ = sock.set_read_timeout(Some(Duration::from_secs(1)));
        let mut buf = [0u8; 8192];
        let mut last_announce = Instant::now();
        while !node.stop.load(Ordering::Relaxed) {
            if let Ok((n, from)) = sock.recv_from(&mut buf) {
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                if let Some(peer) = parse_peer(&text, &from.ip().to_string()) {
                    let announcing = json::parse(&text).and_then(|j| j.boolean("announce")).unwrap_or(false);
                    let n2 = node.clone();
                    if peer.fingerprint != n2.fingerprint {
                        n2.remember(peer.clone());
                        if announcing {
                            // Answer so the device learns about us: HTTP register when it speaks http, else a UDP reply.
                            let body = n2.info(false).to_string();
                            let registered = peer.protocol == "http" && httpd::request("POST", &peer.ip, peer.port, &format!("{API}/register"), "application/json", Body::Bytes(body.clone().into_bytes()), Duration::from_secs(3)).map(|(s, _)| s == 200).unwrap_or(false);
                            if !registered {
                                let _ = sock.send_to(body.as_bytes(), from);
                            }
                        }
                    }
                }
            }
            if last_announce.elapsed() > Duration::from_secs(20) {
                node.announce();
                last_announce = Instant::now();
            }
        }
    }).ok();
}

fn handle(node: &Arc<Node>, req: &Request, body: &mut dyn Read) -> Response {
    let mounted = node.mounted.lock().unwrap().clone();
    if let Some((prefix, handler)) = mounted {
        if let Some(rest) = req.path.strip_prefix(&prefix).filter(|r| r.is_empty() || r.starts_with('/')) {
            let inner = Request { method: req.method.clone(), path: if rest.is_empty() { "/".to_string() } else { rest.to_string() }, query: req.query.clone(), headers: req.headers.clone(), content_length: req.content_length, peer: req.peer.clone() };
            return handler(&inner, body);
        }
    }
    let path = req.path.trim_end_matches('/');
    match (req.method.as_str(), path) {
        ("GET", p) if p == format!("{API}/info") || p == "/api/localsend/v1/info" => Response::json(200, &node.info(false)),
        ("POST", p) if p == format!("{API}/register") || p == "/api/localsend/v1/register" => {
            let mut text = String::new();
            let _ = body.take(64 << 10).read_to_string(&mut text);
            if let Some(peer) = parse_peer(&text, &req.peer) {
                node.remember(peer);
            }
            Response::json(200, &node.info(false))
        }
        ("POST", p) if p == format!("{API}/prepare-upload") => {
            let mut text = String::new();
            let _ = body.take(4 << 20).read_to_string(&mut text);
            let Some(j) = json::parse(&text) else { return Response::empty(400) };
            let alias = j.get("info").and_then(|i| i.str("alias")).unwrap_or("?").to_string();
            if let Some(peer) = j.get("info").and_then(|i| parse_peer(&i.to_string(), &req.peer)) {
                node.remember(peer);
            }
            let files: Vec<IncomingFile> = j.get("files").map(|f| f.entries().into_iter().filter_map(|(id, v)| Some(IncomingFile { id: id.clone(), name: v.str("fileName")?.to_string(), size: v.num("size").unwrap_or(0.0) as u64 })).collect()).unwrap_or_default();
            if files.is_empty() {
                return Response::empty(400);
            }
            let accepted = if node.accept_all.load(Ordering::Relaxed) {
                true
            } else {
                let (tx, rx) = mpsc::channel();
                node.emit(Event::Incoming { alias: alias.clone(), files: files.clone(), reply: tx });
                rx.recv_timeout(Duration::from_secs(90)).unwrap_or(false)
            };
            if !accepted {
                return Response::empty(403);
            }
            let session_id = random_hex(24);
            let mut tokens = HashMap::new();
            let mut map = std::collections::BTreeMap::new();
            for f in files {
                let token = random_hex(24);
                map.insert(f.id.clone(), Json::s(token.clone()));
                tokens.insert(f.id.clone(), (token, f));
            }
            node.sessions.lock().unwrap().insert(session_id.clone(), Session { alias, tokens, received: Vec::new() });
            Response::json(200, &Json::obj(vec![("sessionId", Json::s(session_id)), ("files", Json::Object(map))]))
        }
        ("POST", p) if p == format!("{API}/upload") => {
            let (Some(sid), Some(fid), Some(token)) = (req.query.get("sessionId"), req.query.get("fileId"), req.query.get("token")) else { return Response::empty(400) };
            let (name, done_after) = {
                let sessions = node.sessions.lock().unwrap();
                let Some(session) = sessions.get(sid) else { return Response::empty(403) };
                let Some((t, file)) = session.tokens.get(fid) else { return Response::empty(403) };
                if t != token {
                    return Response::empty(403);
                }
                (file.name.clone(), session.tokens.len())
            };
            let dest = node.dest();
            let _ = std::fs::create_dir_all(&dest);
            let target = unique_path(&dest, &safe_name(&name));
            let tmp = target.with_extension(format!("{}.part", target.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default()));
            let written = std::fs::File::create(&tmp).and_then(|mut f| {
                let mut limited = body.take(req.content_length);
                std::io::copy(&mut limited, &mut f)?;
                f.flush()
            });
            if written.is_err() || std::fs::rename(&tmp, &target).is_err() {
                let _ = std::fs::remove_file(&tmp);
                return Response::empty(500);
            }
            node.emit(Event::Received(target.clone()));
            let finished = {
                let mut sessions = node.sessions.lock().unwrap();
                if let Some(session) = sessions.get_mut(sid) {
                    session.received.push(target);
                    if session.received.len() >= done_after {
                        sessions.remove(sid)
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(s) = finished {
                node.emit(Event::SessionDone { alias: s.alias, files: s.received });
            }
            Response::empty(200)
        }
        ("POST", p) if p == format!("{API}/cancel") => {
            if let Some(sid) = req.query.get("sessionId") {
                node.sessions.lock().unwrap().remove(sid);
            }
            Response::empty(200)
        }
        _ => Response::empty(404),
    }
}

pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base.chars().filter(|c| *c != '\0').collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() { "file".to_string() } else { cleaned }
}

pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    (1..).map(|i| dir.join(format!("{stem} ({i}){ext}"))).find(|p| !p.exists()).unwrap()
}

/// Send `files` to `peer`; progress and the result arrive as events.
pub fn send(node: Arc<Node>, peer: Peer, files: Vec<PathBuf>) {
    std::thread::spawn(move || {
        let result = send_blocking(&node, &peer, &files);
        let sent = match &result {
            Ok(n) => *n,
            Err(_) => 0,
        };
        node.emit(Event::SendDone { alias: peer.alias.clone(), sent, error: result.err() });
    });
}

fn send_blocking(node: &Arc<Node>, peer: &Peer, files: &[PathBuf]) -> Result<usize, String> {
    if peer.protocol != "http" {
        return Err(format!("{} only accepts encrypted connections. Turn off \"Encryption\" in LocalSend's settings on that device, then try again.", peer.alias));
    }
    let mut list = std::collections::BTreeMap::new();
    let mut by_id: Vec<(String, PathBuf)> = Vec::new();
    for (i, path) in files.iter().enumerate() {
        if path.is_dir() {
            continue;
        }
        let Ok(meta) = std::fs::metadata(path) else { continue };
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let (ct, _) = gtk::gio::content_type_guess(Some(path), None);
        let id = format!("f{i}");
        list.insert(id.clone(), Json::obj(vec![("id", Json::s(id.clone())), ("fileName", Json::s(name)), ("size", Json::Number(meta.len() as f64)), ("fileType", Json::s(ct.to_string()))]));
        by_id.push((id, path.clone()));
    }
    if by_id.is_empty() {
        return Err("nothing to send".into());
    }
    let body = Json::obj(vec![("info", node.info(false)), ("files", Json::Object(list))]).to_string();
    let (status, resp) = httpd::request("POST", &peer.ip, peer.port, &format!("{API}/prepare-upload"), "application/json", Body::Bytes(body.into_bytes()), Duration::from_secs(120)).map_err(|e| e.to_string())?;
    match status {
        200 => {}
        403 => return Err("declined".into()),
        409 => return Err("busy with another transfer".into()),
        s => return Err(format!("HTTP {s}")),
    }
    let j = json::parse(&String::from_utf8_lossy(&resp)).ok_or("bad reply")?;
    let session = j.str("sessionId").ok_or("bad reply")?.to_string();
    let tokens = j.get("files").ok_or("bad reply")?;
    let total = by_id.len();
    for (done, (id, path)) in by_id.iter().enumerate() {
        let Some(token) = tokens.str(id) else { continue };
        let (ct, _) = gtk::gio::content_type_guess(Some(path), None);
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let path_q = format!("{API}/upload?sessionId={}&fileId={}&token={}", httpd::percent_encode(&session), httpd::percent_encode(id), httpd::percent_encode(token));
        let (status, _) = httpd::request("POST", &peer.ip, peer.port, &path_q, &ct, Body::File(file, len), Duration::from_secs(3600)).map_err(|e| e.to_string())?;
        if status != 200 {
            let _ = httpd::request("POST", &peer.ip, peer.port, &format!("{API}/cancel?sessionId={}", httpd::percent_encode(&session)), "text/plain", Body::Bytes(Vec::new()), Duration::from_secs(5));
            return Err(format!("upload failed (HTTP {status})"));
        }
        node.emit(Event::SendProgress { alias: peer.alias.clone(), done: done + 1, total });
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_announcements_and_names() {
        let p = parse_peer(r#"{"alias":"Nora's iPhone","version":"2.0","deviceModel":"iPhone 15","deviceType":"mobile","fingerprint":"abc","port":53317,"protocol":"http","download":false,"announce":true}"#, "192.168.1.7").unwrap();
        assert!(p.is_mobile() && p.is_apple());
        assert_eq!(p.icon(), "phone-symbolic");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("  .hidden  "), "hidden");
        let dir = std::env::temp_dir().join(format!("omafile-ls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a.txt"), dir.join("a (1).txt"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn receive_round_trip_over_http() {
        let dest = std::env::temp_dir().join(format!("omafile-ls-recv-{}", std::process::id()));
        std::fs::create_dir_all(&dest).unwrap();
        let Ok((node, rx)) = start("test".into(), dest.clone()) else { return }; // no network in some sandboxes
        node.set_accept_all(true);
        let info = node.info(false).to_string();
        let prepare = format!(r#"{{"info":{info},"files":{{"f1":{{"id":"f1","fileName":"hello.txt","size":5,"fileType":"text/plain"}}}}}}"#);
        let (status, body) = httpd::request("POST", "127.0.0.1", node.port(), &format!("{API}/prepare-upload"), "application/json", Body::Bytes(prepare.into_bytes()), Duration::from_secs(5)).unwrap();
        assert_eq!(status, 200);
        let j = json::parse(&String::from_utf8_lossy(&body)).unwrap();
        let sid = j.str("sessionId").unwrap();
        let token = j.get("files").unwrap().str("f1").unwrap();
        let (status, _) = httpd::request("POST", "127.0.0.1", node.port(), &format!("{API}/upload?sessionId={sid}&fileId=f1&token={token}"), "text/plain", Body::Bytes(b"hello".to_vec()), Duration::from_secs(5)).unwrap();
        assert_eq!(status, 200);
        assert_eq!(std::fs::read(dest.join("hello.txt")).unwrap(), b"hello");
        let mut done = false;
        while let Ok(e) = rx.recv_timeout(Duration::from_secs(2)) {
            if matches!(e, Event::SessionDone { .. }) {
                done = true;
                break;
            }
        }
        assert!(done);
        node.stop();
        std::fs::remove_dir_all(&dest).unwrap();
    }
}
