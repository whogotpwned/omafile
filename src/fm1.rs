//! Being the system's file manager.
//!
//! `org.freedesktop.FileManager1` is the D-Bus interface browsers, download managers and
//! editors call for "Show in folder": omafile answers it by opening the folder and
//! selecting the file. While omafile runs it owns the name; a small service file in the
//! user's own D-Bus directory (written by "Make default" in Preferences) lets D-Bus start
//! omafile for such a call when it is not running, instead of another file manager.

use gtk::prelude::*;
use gtk::{gio, glib};
use std::path::PathBuf;

pub const NAME: &str = "org.freedesktop.FileManager1";
const PATH: &str = "/org/freedesktop/FileManager1";
const DESKTOP_ID: &str = "io.github.whogotpwned.Omafile.desktop";
const XML: &str = r#"<node>
  <interface name="org.freedesktop.FileManager1">
    <method name="ShowFolders"><arg type="as" name="URIs" direction="in"/><arg type="s" name="StartupId" direction="in"/></method>
    <method name="ShowItems"><arg type="as" name="URIs" direction="in"/><arg type="s" name="StartupId" direction="in"/></method>
    <method name="ShowItemProperties"><arg type="as" name="URIs" direction="in"/><arg type="s" name="StartupId" direction="in"/></method>
  </interface>
</node>"#;

#[derive(Debug, PartialEq)]
pub enum Request {
    /// Open these folders.
    Folders(Vec<String>),
    /// Open the folders these items are in and select them.
    Items(Vec<String>),
    /// Show the properties of these items.
    Properties(Vec<String>),
}

/// A method call as a request; None for a method this interface does not have.
pub fn request(method: &str, params: &glib::Variant) -> Option<Request> {
    let uris: Vec<String> = params.try_child_value(0)?.get::<Vec<String>>()?;
    match method {
        "ShowFolders" => Some(Request::Folders(uris)),
        "ShowItems" => Some(Request::Items(uris)),
        "ShowItemProperties" => Some(Request::Properties(uris)),
        _ => None,
    }
}

/// Own the name on the session bus and hand every call to `handler`.
pub fn serve(handler: impl Fn(Request) + 'static) {
    let handler = std::rc::Rc::new(handler);
    gio::bus_own_name(
        gio::BusType::Session,
        NAME,
        gio::BusNameOwnerFlags::ALLOW_REPLACEMENT | gio::BusNameOwnerFlags::REPLACE,
        move |conn, _| {
            let Some(interface) = gio::DBusNodeInfo::for_xml(XML).ok().and_then(|node| node.lookup_interface(NAME)) else { return };
            let handler = handler.clone();
            let registered = conn
                .register_object(PATH, &interface)
                .method_call(move |_, _, _, _, method, params, invocation| match request(method, &params) {
                    Some(req) => {
                        handler(req);
                        invocation.return_value(None);
                    }
                    None => invocation.return_error(gio::DBusError::UnknownMethod, "No such method"),
                })
                .build();
            if let Err(e) = registered {
                eprintln!("omafile: cannot offer {NAME}: {}", e.message());
            }
        },
        |_, _| {},
        |_, _| {},
    );
}

// ----- being the default ----------------------------------------------------------------

fn service_file() -> PathBuf {
    glib::user_data_dir().join("dbus-1/services").join(format!("{NAME}.service"))
}

/// What the D-Bus service file says: start this very program, waiting for calls.
pub fn service_text(exe: &str) -> String {
    format!("[D-BUS Service]\nName={NAME}\nExec={exe} --gapplication-service\n")
}

fn our_app() -> Option<gio::AppInfo> {
    gio::AppInfo::all().into_iter().find(|a| a.id().as_deref() == Some(DESKTOP_ID))
}

/// The desktop file that opens folders right now.
pub fn current_default() -> Option<String> {
    gio::AppInfo::default_for_type("inode/directory", false).and_then(|a| a.id()).map(|id| id.to_string())
}

pub fn is_default() -> bool {
    current_default().as_deref() == Some(DESKTOP_ID) && service_file().exists()
}

/// Folders open in omafile from now on, and "Show in folder" starts it. Returns the
/// desktop file that was the default before, so it can be put back.
pub fn make_default() -> Result<Option<String>, String> {
    let before = current_default().filter(|d| d != DESKTOP_ID);
    let app = our_app().ok_or("omafile's desktop file is not installed (run the installer or the package first)")?;
    app.set_as_default_for_type("inode/directory").map_err(|e| e.message().to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = service_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, service_text(&exe.to_string_lossy())).map_err(|e| e.to_string())?;
    reload_bus();
    Ok(before)
}

/// Undo `make_default`: remove the service file and give folders back to `previous`.
pub fn undo_default(previous: Option<&str>) -> Result<(), String> {
    let _ = std::fs::remove_file(service_file());
    // Forget our claim first; then the previous one (or the system's choice) applies again.
    gio::AppInfo::reset_type_associations("inode/directory");
    if let Some(previous) = previous.filter(|p| !p.is_empty()) {
        if let Some(app) = gio::AppInfo::all().into_iter().find(|a| a.id().as_deref() == Some(previous)) {
            app.set_as_default_for_type("inode/directory").map_err(|e| e.message().to_string())?;
        }
    }
    reload_bus();
    Ok(())
}

/// Tell the bus to read its service files again; it does not watch them everywhere.
fn reload_bus() {
    if let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        let _ = conn.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "ReloadConfig", None, None, gio::DBusCallFlags::NONE, 2000, gio::Cancellable::NONE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_become_requests() {
        let params = (vec!["file:///home/me/a.txt".to_string(), "file:///tmp/b".to_string()], "startup-1").to_variant();
        assert_eq!(request("ShowItems", &params), Some(Request::Items(vec!["file:///home/me/a.txt".into(), "file:///tmp/b".into()])));
        assert_eq!(request("ShowFolders", &params), Some(Request::Folders(vec!["file:///home/me/a.txt".into(), "file:///tmp/b".into()])));
        assert!(matches!(request("ShowItemProperties", &params), Some(Request::Properties(v)) if v.len() == 2));
        assert_eq!(request("Delete", &params), None);
        assert_eq!(request("ShowItems", &("not a list", "x").to_variant()), None);
    }

    #[test]
    fn the_interface_description_is_valid() {
        let node = gio::DBusNodeInfo::for_xml(XML).unwrap();
        let interface = node.lookup_interface(NAME).unwrap();
        for method in ["ShowFolders", "ShowItems", "ShowItemProperties"] {
            assert!(interface.lookup_method(method).is_some(), "{method}");
        }
    }

    #[test]
    fn the_service_file_starts_this_program() {
        assert_eq!(service_text("/usr/bin/omafile"), "[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec=/usr/bin/omafile --gapplication-service\n");
    }
}
