//! Noticing that there is a newer omafile.
//!
//! Once a day omafile asks GitHub which version is the newest: one request for a small
//! public file, with nothing in it about the user or their files. What counts as "the
//! newest" depends on how omafile got onto the machine, because it should name the
//! version an update would really bring:
//!
//! * installed as a package (the binary is under `/usr`): the latest tagged release, which
//!   is what the AUR packages follow;
//! * installed with `install.sh` from a checkout: the version on the main branch, which
//!   is what `git pull` brings.
//!
//! omafile never updates itself. It says that a newer version exists and shows the command
//! that updates this installation; the user runs it, in a terminal, if and when they want.
//! Preferences › Updates turns the daily question off.

use std::path::{Path, PathBuf};

pub const RELEASES: &str = "https://api.github.com/repos/whogotpwned/omafile/releases/latest";
pub const MANIFEST: &str = "https://raw.githubusercontent.com/whogotpwned/omafile/main/manifest.json";
/// Not more often than this, however often omafile is started.
const EVERY: i64 = 20 * 3600;

/// `0.29.0`, `v0.29.0`, `0.29.0-2` as three numbers.
pub fn parse_version(text: &str) -> Option<[u32; 3]> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let mut parts = text.split('.');
    let mut out = [0u32; 3];
    for slot in &mut out {
        let part = parts.next()?;
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        *slot = digits.parse().ok()?;
    }
    parts.next().is_none().then_some(out)
}

/// Whether `candidate` is a later version than `current`. Anything that is not a version
/// is never newer.
pub fn newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// The version in GitHub's answer about the latest release. Drafts and pre-releases are
/// nothing to tell anyone about; a repository without a release answers "Not Found".
pub fn version_in_release(json: &str) -> Option<String> {
    let release = crate::json::parse(json)?;
    if release.boolean("draft") == Some(true) || release.boolean("prerelease") == Some(true) {
        return None;
    }
    let tag = release.str("tag_name")?;
    parse_version(tag).map(|[a, b, c]| format!("{a}.{b}.{c}"))
}

/// The version in the plugin manifest of the main branch.
pub fn version_in_manifest(json: &str) -> Option<String> {
    let manifest = crate::json::parse(json)?;
    parse_version(manifest.str("version")?).map(|[a, b, c]| format!("{a}.{b}.{c}"))
}

/// How this omafile got onto the machine.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// By the package manager; the name of the package that owns the binary.
    Package(String),
    /// With `install.sh` from this checkout.
    Checkout(PathBuf),
    Unknown,
}

/// `source` is what `install.sh` noted (the checkout it ran in), `owner` the package that
/// owns the binary according to pacman.
pub fn install_from(exe: &Path, source: Option<&Path>, owner: Option<&str>) -> Install {
    if exe.starts_with("/usr") && !exe.starts_with("/usr/local") {
        return Install::Package(owner.map(str::trim).filter(|o| !o.is_empty()).unwrap_or("omafile").to_string());
    }
    match source {
        Some(dir) if dir.join("install.sh").is_file() && dir.join(".git").exists() => Install::Checkout(dir.to_path_buf()),
        _ => Install::Unknown,
    }
}

fn source_note() -> PathBuf {
    gtk::glib::user_data_dir().join("omafile/source")
}

pub fn install() -> Install {
    let exe = std::env::current_exe().unwrap_or_default();
    let source = std::fs::read_to_string(source_note()).ok().map(|s| PathBuf::from(s.trim()));
    let owner = if exe.starts_with("/usr") {
        std::process::Command::new("pacman").arg("-Qqo").arg(&exe).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        None
    };
    install_from(&exe, source.as_deref(), owner.as_deref())
}

fn shell_quote(text: &str) -> String {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+~".contains(c)) {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// The command that brings this installation up to date. None when omafile cannot know:
/// then it only says that there is a newer version. `helper` is the AUR helper found
/// (yay on Omarchy).
pub fn update_command(install: &Install, helper: Option<&str>) -> Option<String> {
    match install {
        Install::Package(name) => helper.map(|helper| format!("{helper} -S {}", shell_quote(name))),
        Install::Checkout(dir) => Some(format!("cd {} && git pull --ff-only && ./install.sh", shell_quote(&dir.to_string_lossy()))),
        Install::Unknown => None,
    }
}

pub fn aur_helper() -> Option<&'static str> {
    ["yay", "paru"].into_iter().find(|helper| gtk::glib::find_program_in_path(helper).is_some())
}

/// What is remembered between starts: when GitHub was asked last, what it said, which
/// version the user does not want to hear about, and when they were told last.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub checked: i64,
    pub latest: String,
    pub skipped: String,
    pub told: i64,
}

impl State {
    pub fn parse(text: &str) -> State {
        let mut state = State::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "checked" => state.checked = value.parse().unwrap_or(0),
                "latest" => state.latest = value.to_string(),
                "skipped" => state.skipped = value.to_string(),
                "told" => state.told = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        state
    }

    pub fn serialize(&self) -> String {
        format!("checked={}\nlatest={}\nskipped={}\ntold={}\n", self.checked, self.latest, self.skipped, self.told)
    }

    /// Whether it is time to ask again.
    pub fn due(&self, now: i64) -> bool {
        now - self.checked >= EVERY || now < self.checked
    }

    /// The version to tell the user about now, if any: newer than the one running, not
    /// one they skipped, and not mentioned already today.
    pub fn to_tell(&self, current: &str, now: i64) -> Option<String> {
        let news = newer(&self.latest, current) && self.latest != self.skipped;
        (news && (now - self.told >= EVERY || now < self.told)).then(|| self.latest.clone())
    }

    /// The newer version that is known, whether or not it was mentioned or skipped.
    pub fn available(&self, current: &str) -> Option<String> {
        newer(&self.latest, current).then(|| self.latest.clone())
    }
}

fn state_path() -> PathBuf {
    gtk::glib::user_state_dir().join("omafile/update")
}

pub fn load_state() -> State {
    State::parse(&std::fs::read_to_string(state_path()).unwrap_or_default())
}

pub fn save_state(state: &State) {
    let path = state_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, state.serialize());
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Where the newest version of this installation is written, and how to read it there.
pub fn question(install: &Install) -> (&'static str, fn(&str) -> Option<String>) {
    match install {
        Install::Package(_) => (RELEASES, version_in_release),
        _ => (MANIFEST, version_in_manifest),
    }
}

/// Ask. `Ok(None)`: the answer names no version (no release yet). Blocks for the length of
/// one small download: call it off the main thread.
pub fn ask(install: &Install) -> Result<Option<String>, String> {
    let (url, read) = question(install);
    // For tests: another address, read as the same kind of answer.
    let url = std::env::var("OMAFILE_UPDATE_URL").unwrap_or_else(|_| url.to_string());
    let output = std::process::Command::new("curl")
        .args(["-sSL", "--max-time", "15", "--max-filesize", "2000000", "--proto", "=http,https", "-A", "omafile", "-H", "Accept: application/vnd.github+json, application/json", "--"])
        .arg(&url)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(message.strip_prefix("curl: ").unwrap_or(&message).to_string());
    }
    Ok(read(&String::from_utf8_lossy(&output.stdout)))
}

/// `omafile --check-update`: ask now and say what there is. Exit code 0 when up to date,
/// 10 when a newer version exists, 1 when GitHub could not be asked.
pub fn cli() -> u8 {
    let current = env!("CARGO_PKG_VERSION");
    let install = install();
    match ask(&install) {
        Err(e) => {
            eprintln!("omafile: cannot ask for the newest version: {e}");
            1
        }
        Ok(Some(latest)) if newer(&latest, current) => {
            println!("omafile {latest} is available (installed: {current})");
            if let Some(command) = update_command(&install, aur_helper()) {
                println!("update with: {command}");
            }
            10
        }
        Ok(_) => {
            println!("omafile {current} is the newest version");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_compared_as_numbers() {
        assert_eq!(parse_version("0.29.0"), Some([0, 29, 0]));
        assert_eq!(parse_version("v1.2.3"), Some([1, 2, 3]));
        assert_eq!(parse_version(" 0.29.0-2\n"), Some([0, 29, 0]));
        assert_eq!(parse_version("0.29"), None);
        assert_eq!(parse_version("0.29.0.1"), None);
        assert_eq!(parse_version("latest"), None);
        assert!(newer("0.30.0", "0.29.0"));
        assert!(newer("0.100.0", "0.29.9"), "not compared as text");
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.29.0", "0.29.0"));
        assert!(!newer("0.28.2", "0.29.0"), "an older answer is no news");
        assert!(!newer("garbage", "0.29.0") && !newer("<html>", "0.29.0"));
    }

    #[test]
    fn the_answers_of_github_are_read() {
        assert_eq!(version_in_release(r#"{"tag_name":"v0.30.0","draft":false,"prerelease":false,"name":"omafile 0.30.0"}"#).as_deref(), Some("0.30.0"));
        assert_eq!(version_in_release(r#"{"tag_name":"v0.31.0","prerelease":true}"#), None, "a pre-release is not announced");
        assert_eq!(version_in_release(r#"{"tag_name":"v0.31.0","draft":true}"#), None);
        assert_eq!(version_in_release(r#"{"message":"Not Found","status":"404"}"#), None, "no release yet");
        assert_eq!(version_in_release("<html>rate limited</html>"), None);
        assert_eq!(version_in_manifest(r#"{"schemaVersion":1,"id":"whogotpwned.omafile","version":"0.30.1"}"#).as_deref(), Some("0.30.1"));
        assert_eq!(version_in_manifest(r#"{"version":"soon"}"#), None);
        // The manifest of this very checkout is one such answer, and names this version.
        assert_eq!(version_in_manifest(include_str!("../manifest.json")).as_deref(), Some(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn the_update_command_fits_the_installation() {
        let dir = std::env::temp_dir().join(format!("omafile-update-{}", std::process::id()));
        let checkout = dir.join("my omafile");
        std::fs::create_dir_all(checkout.join(".git")).unwrap();
        std::fs::write(checkout.join("install.sh"), "#!/bin/sh\n").unwrap();
        // A package: whatever pacman says owns the binary, updated with the AUR helper.
        let package = install_from(Path::new("/usr/bin/omafile"), Some(&checkout), Some("omafile-bin\n"));
        assert_eq!(package, Install::Package("omafile-bin".into()));
        assert_eq!(update_command(&package, Some("yay")).as_deref(), Some("yay -S omafile-bin"));
        assert_eq!(update_command(&package, None), None, "without a helper omafile does not guess");
        assert_eq!(install_from(Path::new("/usr/bin/omafile"), None, None), Install::Package("omafile".into()));
        assert_eq!(question(&package).0, RELEASES, "a package follows the releases");
        // From a checkout: pull and install again, the path quoted.
        let built = install_from(Path::new("/home/me/.local/bin/omafile"), Some(&checkout), None);
        assert_eq!(built, Install::Checkout(checkout.clone()));
        assert_eq!(update_command(&built, Some("yay")), Some(format!("cd '{}' && git pull --ff-only && ./install.sh", checkout.display())));
        assert_eq!(question(&built).0, MANIFEST, "a checkout follows the main branch");
        // A note that points nowhere, or no note: unknown, and no command.
        assert_eq!(install_from(Path::new("/home/me/.local/bin/omafile"), Some(&dir.join("gone")), None), Install::Unknown);
        assert_eq!(install_from(Path::new("/usr/local/bin/omafile"), None, None), Install::Unknown);
        assert_eq!(update_command(&Install::Unknown, Some("yay")), None);
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn once_a_day_and_never_about_a_skipped_version() {
        let day = 24 * 3600;
        let fresh = State::default();
        assert!(fresh.due(1_000_000), "never asked: ask");
        let mut state = State { checked: 1_000_000, latest: "0.30.0".into(), ..Default::default() };
        assert!(!state.due(1_000_000 + 3600), "asked an hour ago");
        assert!(state.due(1_000_000 + day));
        assert!(state.due(999_000), "the clock went back: ask again rather than never");
        assert_eq!(state.to_tell("0.29.0", 1_000_100).as_deref(), Some("0.30.0"));
        assert_eq!(state.to_tell("0.30.0", 1_000_100), None, "already on it");
        state.told = 1_000_100;
        assert_eq!(state.to_tell("0.29.0", 1_000_100 + 3600), None, "said so today");
        assert_eq!(state.to_tell("0.29.0", 1_000_100 + day).as_deref(), Some("0.30.0"), "and again tomorrow");
        state.skipped = "0.30.0".into();
        assert_eq!(state.to_tell("0.29.0", 1_000_100 + 2 * day), None, "skipped");
        assert_eq!(state.available("0.29.0").as_deref(), Some("0.30.0"), "but still known, for Help");
        state.latest = "0.31.0".into();
        assert_eq!(state.to_tell("0.29.0", 1_000_100 + 2 * day).as_deref(), Some("0.31.0"), "the next one is news again");
        // It survives being written down.
        assert_eq!(State::parse(&state.serialize()), state);
        assert_eq!(State::parse("nonsense\nchecked=abc\n"), State::default());
    }
}
