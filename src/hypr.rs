//! Hyprland integration through `hyprctl`: a drop-down omafile on a special
//! workspace (`omafile --toggle`) and "Open in New Workspace".

use std::process::Command;

pub const CLASS: &str = "io.github.whogotpwned.Omafile";
pub const SPECIAL: &str = "files";

pub fn available() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() && gtk::glib::find_program_in_path("hyprctl").is_some()
}

/// Is an omafile window already parked on the special workspace? Reads the
/// JSON of `hyprctl clients -j` without a JSON library: one client per
/// `"address"` block, class and workspace name inside it.
pub fn has_dropdown(clients_json: &str) -> bool {
    clients_json.split("\"address\"").skip(1).any(|block| block.contains(&format!("\"class\": \"{CLASS}\"")) && block.contains(&format!("\"name\": \"special:{SPECIAL}\"")))
}

fn hyprctl(args: &[&str]) -> Option<String> {
    let out = Command::new("hyprctl").args(args).output().ok()?;
    if out.status.success() { Some(String::from_utf8_lossy(&out.stdout).to_string()) } else { None }
}

/// `omafile --toggle`: show or hide the drop-down file manager.
pub fn toggle() -> i32 {
    if !available() {
        eprintln!("omafile --toggle needs a running Hyprland session with hyprctl");
        return 1;
    }
    let clients = hyprctl(&["clients", "-j"]).unwrap_or_default();
    if !has_dropdown(&clients) {
        // Start it on the special workspace; the exec rule keeps it there.
        let launch = format!("[workspace special:{SPECIAL}] {}", launcher_command());
        if hyprctl(&["dispatch", "exec", &launch]).is_none() {
            eprintln!("hyprctl dispatch exec failed");
            return 1;
        }
    }
    match hyprctl(&["dispatch", "togglespecialworkspace", SPECIAL]) {
        Some(_) => 0,
        None => 1,
    }
}

/// Open `dir` in a fresh omafile window on an empty workspace.
pub fn open_in_new_workspace(dir: &std::path::Path) -> bool {
    if !available() {
        return false;
    }
    let cmd = format!("[workspace empty] {} '{}'", launcher_command(), dir.to_string_lossy().replace('\'', "'\\''"));
    hyprctl(&["dispatch", "exec", &cmd]).is_some()
}

fn launcher_command() -> String {
    if gtk::glib::find_program_in_path("uwsm-app").is_some() { "uwsm-app -- omafile".into() } else { "omafile".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_dropdown_client() {
        let json = r#"[{"address": "0x1", "class": "foot", "workspace": {"id": 1, "name": "1"}}, {"address": "0x2", "class": "io.github.whogotpwned.Omafile", "workspace": {"id": -98, "name": "special:files"}}]"#;
        assert!(has_dropdown(json));
        let json = r#"[{"address": "0x2", "class": "io.github.whogotpwned.Omafile", "workspace": {"id": 2, "name": "2"}}]"#;
        assert!(!has_dropdown(json));
    }
}
