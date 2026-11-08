//! Status file for the bar widget: `~/.local/state/omafile/status`
//! (`busy=0|1`, `vaults=<count of unlocked gocryptfs mounts>`).

use crate::window::Window;
use gtk::glib;

pub fn path() -> std::path::PathBuf {
    glib::user_state_dir().join("omafile/status")
}

pub fn unlocked_vaults() -> Vec<std::path::PathBuf> {
    let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else { return Vec::new() };
    mounts
        .lines()
        .filter_map(|l| {
            let mut parts = l.split_whitespace();
            let _dev = parts.next()?;
            let point = parts.next()?;
            let fstype = parts.next()?;
            if fstype.contains("gocryptfs") {
                Some(std::path::PathBuf::from(point.replace("\\040", " ")))
            } else {
                None
            }
        })
        .collect()
}

pub fn write(window: &Window) {
    let busy = window.queue_busy();
    let vaults = unlocked_vaults().len();
    let p = path();
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(p, format!("busy={}\nvaults={vaults}\n", busy as u8));
}

/// `omafile --lock-all`: unmount every gocryptfs mount of this user.
pub fn lock_all() -> i32 {
    let mut failed = 0;
    for mount in unlocked_vaults() {
        let tool = if glib::find_program_in_path("fusermount3").is_some() { "fusermount3" } else { "fusermount" };
        let ok = std::process::Command::new(tool).arg("-u").arg(&mount).status().map(|s| s.success()).unwrap_or(false);
        if ok {
            println!("locked {}", mount.display());
        } else {
            eprintln!("could not lock {} (still in use?)", mount.display());
            failed += 1;
        }
    }
    let p = path();
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(p, format!("busy=0\nvaults={}\n", unlocked_vaults().len()));
    failed
}

// ----- recent folders (shared with `omafile --print-recent`) -----------------------------

pub fn recent_path() -> std::path::PathBuf {
    glib::user_state_dir().join("omafile/recent")
}

pub fn load_recent() -> Vec<String> {
    std::fs::read_to_string(recent_path()).map(|s| s.lines().filter(|l| !l.is_empty()).map(str::to_string).collect()).unwrap_or_default()
}

pub fn save_recent(entries: &[String]) {
    let p = recent_path();
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(p, entries.join("\n") + "\n");
}

/// `omafile --print-recent`: one line per recent folder, newest first, for launchers.
pub fn print_recent() {
    for line in load_recent() {
        println!("{line}");
    }
}
