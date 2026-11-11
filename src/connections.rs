//! Saved remote connections (SFTP / SCP over SSH) for the sidebar.
//!
//! The list is a plain text file with a name and an address per line. Passwords
//! are never written there: when asked to remember one, omafile hands it to GVFS
//! with "save permanently", and GVFS keeps it in the system keyring (the Secret
//! Service, e.g. GNOME Keyring), encrypted with the login password. The next
//! connect finds it there and does not ask.

use gtk::prelude::*;
use gtk::{gio, glib};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub name: String,
    pub uri: String,
}

fn path() -> PathBuf {
    glib::user_config_dir().join("omafile/connections")
}

/// `name<TAB>uri` per line; blank lines and `#` comments are skipped.
pub fn parse(text: &str) -> Vec<Connection> {
    let mut out: Vec<Connection> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        if let Some((name, uri)) = line.split_once('\t') {
            let (name, uri) = (name.trim(), uri.trim());
            if !name.is_empty() && uri.contains("://") && !out.iter().any(|c| c.name == name) {
                out.push(Connection { name: name.into(), uri: uri.into() });
            }
        }
    }
    out
}

pub fn serialize(list: &[Connection]) -> String {
    list.iter().map(|c| format!("{}\t{}\n", c.name.replace(['\t', '\n'], " "), c.uri)).collect()
}

pub fn list() -> Vec<Connection> {
    std::fs::read_to_string(path()).map(|t| parse(&t)).unwrap_or_default()
}

fn save(list: &[Connection]) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, serialize(list));
}

/// Add a connection, or change the address of the one that has this name.
pub fn add(name: &str, uri: &str) {
    let mut all = list();
    match all.iter_mut().find(|c| c.name == name) {
        Some(c) => c.uri = uri.to_string(),
        None => all.push(Connection { name: name.to_string(), uri: uri.to_string() }),
    }
    save(&all);
}

pub fn remove(name: &str) {
    let all: Vec<Connection> = list().into_iter().filter(|c| c.name != name).collect();
    save(&all);
}

/// The parts of an address like `sftp://user@host:2222/srv`.
#[derive(Debug, PartialEq, Default)]
pub struct Parts {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<String>,
    pub path: String,
}

pub fn parts(uri: &str) -> Option<Parts> {
    let rest = uri.split_once("://")?.1;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (user, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u.to_string()), h),
        None => (None, authority),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (h, Some(p.to_string())),
        _ => (hostport, None),
    };
    if host.is_empty() {
        return None;
    }
    Some(Parts { user, host: host.to_string(), port, path: path.to_string() })
}

/// What a connection is called when no name was given: `user@host`.
pub fn default_name(uri: &str) -> String {
    match parts(uri) {
        Some(p) => match p.user {
            Some(u) => format!("{u}@{}", p.host),
            None => p.host,
        },
        None => uri.to_string(),
    }
}

/// The short form shown in the connect dialog: `user@host:port/path` without the scheme.
pub fn short(uri: &str) -> String {
    match parts(uri) {
        Some(p) => {
            let mut s = String::new();
            if let Some(u) = &p.user {
                s.push_str(u);
                s.push('@');
            }
            s.push_str(&p.host);
            if let Some(port) = &p.port {
                s.push(':');
                s.push_str(port);
            }
            if p.path != "/" {
                s.push_str(&p.path);
            }
            s
        }
        None => uri.to_string(),
    }
}

// ----- the system keyring ---------------------------------------------------------------

const SECRETS: &str = "org.freedesktop.secrets";

/// Whether a Secret Service (GNOME Keyring, KeePassXC, …) is running or can be started.
/// Without one GVFS has nowhere safe to keep a password and asks each time.
pub fn keyring_available() -> bool {
    let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return false };
    let dbus = |method: &str, args: Option<&glib::Variant>| conn.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", method, args, None, gio::DBusCallFlags::NONE, 1500, gio::Cancellable::NONE).ok();
    if dbus("NameHasOwner", Some(&(SECRETS,).to_variant())).and_then(|v| v.child_value(0).get::<bool>()).unwrap_or(false) {
        return true;
    }
    dbus("ListActivatableNames", None).map(|v| v.child_value(0).iter().any(|n| n.str() == Some(SECRETS))).unwrap_or(false)
}

/// Remove the password GVFS saved for this server from the keyring. Best effort: without a
/// keyring, or with nothing saved, this does nothing.
pub fn forget_password(uri: &str) {
    let Some(p) = parts(uri) else { return };
    let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else { return };
    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert("server".into(), p.host);
    attributes.insert("protocol".into(), "sftp".into());
    if let Some(user) = p.user {
        attributes.insert("user".into(), user);
    }
    let found = conn.call_sync(Some(SECRETS), "/org/freedesktop/secrets", "org.freedesktop.Secret.Service", "SearchItems", Some(&(attributes,).to_variant()), None, gio::DBusCallFlags::NO_AUTO_START, 3000, gio::Cancellable::NONE);
    let Ok(found) = found else { return };
    for group in 0..found.n_children().min(2) {
        for item in found.child_value(group).iter() {
            if let Some(path) = item.str() {
                let _ = conn.call_sync(Some(SECRETS), path, "org.freedesktop.Secret.Item", "Delete", None, None, gio::DBusCallFlags::NO_AUTO_START, 3000, gio::Cancellable::NONE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_round_trips_and_ignores_junk() {
        let text = "# saved\nWeb server\tsftp://deploy@web.example.org:2222/srv\n\nbroken line\nNAS\tsftp://nas.local/\nWeb server\tsftp://other/\nno scheme\thost\n";
        let list = parse(text);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], Connection { name: "Web server".into(), uri: "sftp://deploy@web.example.org:2222/srv".into() });
        assert_eq!(parse(&serialize(&list)), list);
        // A tab or a line break in a name cannot break the file.
        let odd = vec![Connection { name: "a\tb\nc".into(), uri: "sftp://h/".into() }];
        assert_eq!(parse(&serialize(&odd))[0].name, "a b c");
    }

    #[test]
    fn takes_addresses_apart() {
        assert_eq!(parts("sftp://deploy@web.example.org:2222/srv/www"), Some(Parts { user: Some("deploy".into()), host: "web.example.org".into(), port: Some("2222".into()), path: "/srv/www".into() }));
        assert_eq!(parts("sftp://nas.local"), Some(Parts { user: None, host: "nas.local".into(), port: None, path: "/".into() }));
        assert_eq!(parts("sftp://"), None);
        assert_eq!(parts("no scheme"), None);
        assert_eq!(default_name("sftp://deploy@web.example.org:2222/srv"), "deploy@web.example.org");
        assert_eq!(default_name("sftp://nas.local/"), "nas.local");
        assert_eq!(short("sftp://deploy@web.example.org:2222/srv"), "deploy@web.example.org:2222/srv");
        assert_eq!(short("sftp://nas.local/"), "nas.local");
    }
}
