//! A very small HTTP/1.1 server on a thread, for the local-network share
//! page and the LocalSend endpoints. One thread per connection, bodies
//! streamed to the handler, no TLS.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub content_length: u64,
    pub peer: String,
}

pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Body,
    pub extra_headers: Vec<(String, String)>,
}

pub enum Body {
    Bytes(Vec<u8>),
    File(std::fs::File, u64),
}

impl Response {
    pub fn text(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Response {
        Response { status, content_type: content_type.to_string(), body: Body::Bytes(body.into()), extra_headers: Vec::new() }
    }

    pub fn json(status: u16, json: &crate::json::Json) -> Response {
        Response::text(status, "application/json", json.to_string())
    }

    pub fn html(body: String) -> Response {
        Response::text(200, "text/html; charset=utf-8", body)
    }

    pub fn empty(status: u16) -> Response {
        Response::text(status, "text/plain", Vec::new())
    }

    pub fn file(path: &std::path::Path, content_type: &str) -> Response {
        match std::fs::File::open(path).and_then(|f| f.metadata().map(|m| (f, m.len()))) {
            Ok((f, len)) => Response { status: 200, content_type: content_type.to_string(), body: Body::File(f, len), extra_headers: Vec::new() },
            Err(_) => Response::empty(404),
        }
    }

    pub fn with_header(mut self, k: &str, v: &str) -> Response {
        self.extra_headers.push((k.to_string(), v.to_string()));
        self
    }
}

pub type Handler = dyn Fn(&Request, &mut dyn Read) -> Response + Send + Sync;

pub struct Server {
    pub port: u16,
    stop: Arc<AtomicBool>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the accept loop.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// Bind `port` (0 = any) on all interfaces and serve until the `Server` is dropped.
pub fn start(port: u16, handler: Arc<Handler>) -> std::io::Result<Server> {
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    let port = listener.local_addr()?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::Builder::new().name("omafile-httpd".into()).spawn(move || {
        for stream in listener.incoming() {
            if stop2.load(Ordering::Relaxed) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let handler = handler.clone();
            std::thread::spawn(move || {
                let _ = handle(stream, &handler);
            });
        }
    })?;
    Ok(Server { port, stop })
}

fn handle(stream: TcpStream, handler: &Arc<Handler>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(120)))?;
    let peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }
    let mut parts = line.trim_end().splitn(3, ' ');
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let content_length: u64 = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), parse_query(q)),
        None => (target.clone(), HashMap::new()),
    };
    let request = Request { method, path: percent_decode(&path), query, headers, content_length, peer };
    let mut body = (&mut reader).take(content_length);
    let response = handler(&request, &mut body);
    // Drain what the handler left so the client sees our status.
    let mut sink = Vec::new();
    let _ = body.take(1 << 20).read_to_end(&mut sink);
    let mut out = stream;
    let len = match &response.body {
        Body::Bytes(b) => b.len() as u64,
        Body::File(_, n) => *n,
    };
    let reason = match response.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let mut head = format!("HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {len}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n", response.status, response.content_type);
    for (k, v) in &response.extra_headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    out.write_all(head.as_bytes())?;
    match response.body {
        Body::Bytes(b) => out.write_all(&b)?,
        Body::File(mut f, _) => {
            std::io::copy(&mut f, &mut out)?;
        }
    }
    out.flush()
}

pub fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&').filter(|p| !p.is_empty()).map(|p| match p.split_once('=') {
        Some((k, v)) => (percent_decode(k), percent_decode(v)),
        None => (percent_decode(p), String::new()),
    }).collect()
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(b) {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Minimal HTTP client for JSON APIs and file uploads: returns (status, body).
pub fn request(method: &str, host: &str, port: u16, path: &str, content_type: &str, body: Body, timeout: std::time::Duration) -> std::io::Result<(u16, Vec<u8>)> {
    let addr = format!("{host}:{port}");
    let mut stream = TcpStream::connect_timeout(&addr.parse().map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad address"))?, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let len = match &body {
        Body::Bytes(b) => b.len() as u64,
        Body::File(_, n) => *n,
    };
    stream.write_all(format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nUser-Agent: omafile\r\nAccept: */*\r\nContent-Type: {content_type}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n").as_bytes())?;
    match body {
        Body::Bytes(b) => stream.write_all(&b)?,
        Body::File(mut f, _) => {
            std::io::copy(&mut f, &mut stream)?;
        }
    }
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;
    let status: u16 = status_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut content_length: Option<usize> = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse().ok();
            }
        }
    }
    let mut out = Vec::new();
    match content_length {
        Some(n) => {
            out.resize(n, 0);
            reader.read_exact(&mut out)?;
        }
        None => {
            reader.take(8 << 20).read_to_end(&mut out)?;
        }
    }
    Ok((status, out))
}

/// Stream a multipart/form-data body, calling `on_file(filename, reader)` for every file part.
/// Returns the number of files handed out.
pub fn read_multipart(body: &mut dyn Read, content_type: &str, mut on_file: impl FnMut(&str, &mut dyn Read)) -> usize {
    let Some(boundary) = content_type.split(';').map(str::trim).find_map(|p| p.strip_prefix("boundary=")) else { return 0 };
    let boundary = boundary.trim_matches('"');
    let delimiter = format!("\r\n--{boundary}").into_bytes();
    let mut data = Vec::new();
    // Whole body in memory is fine for photos and documents; the share page caps uploads anyway.
    if body.read_to_end(&mut data).is_err() {
        return 0;
    }
    let first = format!("--{boundary}").into_bytes();
    let mut pos = match find(&data, &first, 0) {
        Some(p) => p + first.len(),
        None => return 0,
    };
    let mut count = 0;
    loop {
        if data[pos..].starts_with(b"--") {
            break;
        }
        // Skip CRLF after the boundary, then headers until a blank line.
        if data[pos..].starts_with(b"\r\n") {
            pos += 2;
        }
        let Some(header_end) = find(&data, b"\r\n\r\n", pos) else { break };
        let headers = String::from_utf8_lossy(&data[pos..header_end]).to_string();
        let content_start = header_end + 4;
        let Some(next) = find(&data, &delimiter, content_start) else { break };
        let filename = headers.lines().find_map(|l| l.to_ascii_lowercase().starts_with("content-disposition").then(|| l.split(';').map(str::trim).find_map(|p| p.strip_prefix("filename=").map(|f| f.trim_matches('"').to_string()))).flatten());
        if let Some(name) = filename.filter(|n| !n.is_empty()) {
            let mut part = &data[content_start..next];
            on_file(&name, &mut part);
            count += 1;
        }
        pos = next + delimiter.len();
    }
    count
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_and_decodes() {
        let server = start(0, Arc::new(|req: &Request, body: &mut dyn Read| {
            let mut b = String::new();
            let _ = body.read_to_string(&mut b);
            Response::text(200, "text/plain", format!("{} {} q={} body={b}", req.method, req.path, req.query.get("x").cloned().unwrap_or_default()))
        })).unwrap();
        let (status, out) = request("POST", "127.0.0.1", server.port, "/a%20b?x=1%202", "text/plain", Body::Bytes(b"hi".to_vec()), std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(status, 200);
        assert_eq!(String::from_utf8_lossy(&out), "POST /a b q=1 2 body=hi");
    }

    #[test]
    fn parses_multipart() {
        let body = b"--XX\r\nContent-Disposition: form-data; name=\"files\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--XX\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nignored\r\n--XX\r\nContent-Disposition: form-data; name=\"files\"; filename=\"b.bin\"\r\n\r\n\x00\x01\r\n--XX--\r\n".to_vec();
        let mut got = Vec::new();
        let n = read_multipart(&mut body.as_slice(), "multipart/form-data; boundary=XX", |name, r| {
            let mut v = Vec::new();
            r.read_to_end(&mut v).unwrap();
            got.push((name.to_string(), v));
        });
        assert_eq!(n, 2);
        assert_eq!(got[0], ("a.txt".to_string(), b"hello".to_vec()));
        assert_eq!(got[1], ("b.bin".to_string(), vec![0, 1]));
        assert_eq!(percent_encode("a b/ü"), "a%20b%2F%C3%BC");
        assert_eq!(percent_decode("a%20b%2F%C3%BC+c"), "a b/ü c");
    }
}
