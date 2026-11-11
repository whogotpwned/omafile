//! SSH hosts, GTK bookmarks and mounting remote locations (SFTP via GVFS).

use gtk::{gio, glib};
use gtk::prelude::*;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct SshHost {
    pub alias: String,
    pub hostname: String,
    pub user: Option<String>,
    pub port: Option<u16>,
}

impl SshHost {
    pub fn uri(&self) -> String {
        let mut uri = String::from("sftp://");
        if let Some(user) = &self.user {
            uri.push_str(user);
            uri.push('@');
        }
        uri.push_str(&self.hostname);
        if let Some(port) = self.port {
            uri.push_str(&format!(":{port}"));
        }
        uri.push('/');
        uri
    }
}

/// Hosts from `~/.ssh/config`, ignoring wildcard patterns.
pub fn ssh_hosts() -> Vec<SshHost> {
    let path = glib::home_dir().join(".ssh/config");
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    parse_ssh_config(&text)
}

pub fn parse_ssh_config(text: &str) -> Vec<SshHost> {
    let mut hosts: Vec<SshHost> = Vec::new();
    let mut current: Option<SshHost> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once([' ', '=', '\t']) {
            Some((k, v)) => (k.trim().to_ascii_lowercase(), v.trim().trim_start_matches('=').trim().to_string()),
            None => continue,
        };
        match key.as_str() {
            "host" => {
                if let Some(h) = current.take() {
                    hosts.push(h);
                }
                let alias = value.split_whitespace().next().unwrap_or("").to_string();
                if alias.is_empty() || alias.contains(['*', '?', '!']) {
                    current = None;
                } else {
                    current = Some(SshHost { alias: alias.clone(), hostname: alias, user: None, port: None });
                }
            }
            "match" => {
                if let Some(h) = current.take() {
                    hosts.push(h);
                }
            }
            "hostname" => {
                if let Some(h) = current.as_mut() {
                    h.hostname = value;
                }
            }
            "user" => {
                if let Some(h) = current.as_mut() {
                    h.user = Some(value);
                }
            }
            "port" => {
                if let Some(h) = current.as_mut() {
                    h.port = value.parse().ok();
                }
            }
            _ => {}
        }
    }
    if let Some(h) = current {
        hosts.push(h);
    }
    hosts
}

/// An alias from `~/.ssh/config`, alone or followed by a path (`web/srv/www`), as a URI.
pub fn resolve_alias(hosts: &[SshHost], input: &str) -> Option<String> {
    let input = input.trim();
    let (alias, path) = match input.find('/') {
        Some(i) => (&input[..i], &input[i..]),
        None => (input, "/"),
    };
    let host = hosts.iter().find(|h| h.alias == alias)?;
    Some(format!("{}{}", host.uri().trim_end_matches('/'), path))
}

/// Turn user input like `user@host:2222/srv` or `sftp://host/` into a URI.
pub fn parse_server(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if input.contains("://") {
        return Some(input.to_string());
    }
    let (userhost, path) = match input.find('/') {
        Some(i) => (&input[..i], &input[i..]),
        None => (input, "/"),
    };
    let (user, hostport) = match userhost.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, userhost),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => (h, Some(p)),
        _ => (hostport, None),
    };
    if host.is_empty() {
        return None;
    }
    let mut uri = String::from("sftp://");
    if let Some(u) = user {
        uri.push_str(u);
        uri.push('@');
    }
    uri.push_str(host);
    if let Some(p) = port {
        uri.push(':');
        uri.push_str(p);
    }
    uri.push_str(path);
    Some(uri)
}

/// Mount the volume enclosing `file` if needed, prompting through `op`.
pub async fn ensure_mounted(file: &gio::File, op: &gtk::MountOperation) -> Result<(), glib::Error> {
    match file.mount_enclosing_volume_future(gio::MountMountFlags::NONE, Some(op)).await {
        Ok(()) => Ok(()),
        Err(e) if e.matches(gio::IOErrorEnum::AlreadyMounted) => Ok(()),
        Err(e) => Err(e),
    }
}

// ----- bookmarks (shared with the GTK file chooser) -----------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Bookmark {
    pub file: gio::File,
    pub name: String,
}

fn bookmarks_path() -> PathBuf {
    glib::user_config_dir().join("gtk-3.0/bookmarks")
}

pub fn bookmarks() -> Vec<Bookmark> {
    let Ok(text) = std::fs::read_to_string(bookmarks_path()) else { return Vec::new() };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let (uri, name) = line.split_once(' ').unwrap_or((line, ""));
            let file = gio::File::for_uri(uri);
            let name = if name.is_empty() { crate::window::display_name(&file) } else { name.to_string() };
            Bookmark { file, name }
        })
        .collect()
}

fn write_bookmarks(list: &[Bookmark]) -> std::io::Result<()> {
    let path = bookmarks_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text: String = list.iter().map(|b| format!("{} {}\n", b.file.uri(), b.name)).collect();
    std::fs::write(path, text)
}

pub fn add_bookmark(file: &gio::File) -> std::io::Result<bool> {
    let mut list = bookmarks();
    if list.iter().any(|b| b.file.equal(file)) {
        return Ok(false);
    }
    list.push(Bookmark { file: file.clone(), name: crate::window::display_name(file) });
    write_bookmarks(&list).map(|_| true)
}

pub fn remove_bookmark(file: &gio::File) -> std::io::Result<()> {
    let list: Vec<Bookmark> = bookmarks().into_iter().filter(|b| !b.file.equal(file)).collect();
    write_bookmarks(&list)
}

pub fn is_bookmarked(file: &gio::File) -> bool {
    bookmarks().iter().any(|b| b.file.equal(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_config() {
        let cfg = "Host *\n  ForwardAgent yes\n\nHost web\n  HostName web.example.org\n  User deploy\n  Port 2222\n\nHost db backup\n  User root\nMatch all\n";
        let hosts = parse_ssh_config(cfg);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].uri(), "sftp://deploy@web.example.org:2222/");
        assert_eq!(hosts[1].alias, "db");
        assert_eq!(hosts[1].uri(), "sftp://root@db/");
    }

    #[test]
    fn aliases_take_a_path() {
        let hosts = vec![SshHost { alias: "web".into(), hostname: "web.example.org".into(), user: Some("deploy".into()), port: Some(2222) }];
        assert_eq!(resolve_alias(&hosts, "web"), Some("sftp://deploy@web.example.org:2222/".into()));
        assert_eq!(resolve_alias(&hosts, " web/srv/www "), Some("sftp://deploy@web.example.org:2222/srv/www".into()));
        assert_eq!(resolve_alias(&hosts, "other/srv"), None);
        assert_eq!(resolve_alias(&hosts, "deploy@web"), None);
    }

    #[test]
    fn server_strings() {
        assert_eq!(parse_server("user@host").as_deref(), Some("sftp://user@host/"));
        assert_eq!(parse_server("host:2222/srv").as_deref(), Some("sftp://host:2222/srv"));
        assert_eq!(parse_server("ssh://x/").as_deref(), Some("ssh://x/"));
        assert_eq!(parse_server(""), None);
    }
}
