//! Encrypted folders with gocryptfs.
//!
//! Layout: `Name.gocryptfs/` holds the ciphertext (safe to copy, sync or move
//! to another machine) and `Name/` is where it appears decrypted while
//! unlocked. gocryptfs encrypts file by file with AES-256-GCM and scrypt-derived
//! keys, so the ciphertext folder is the one thing to back up or migrate.

use gtk::gio;
use gtk::glib;
use std::path::{Path, PathBuf};

pub const SUFFIX: &str = ".gocryptfs";

pub fn available() -> bool {
    glib::find_program_in_path("gocryptfs").is_some()
}

/// A ciphertext folder is recognised by gocryptfs.conf inside it.
pub fn is_vault(dir: &Path) -> bool {
    dir.join("gocryptfs.conf").is_file()
}

/// Mount point that belongs to a vault: strip the suffix.
pub fn mount_point_for(vault: &Path) -> PathBuf {
    let name = vault.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let base = name.strip_suffix(SUFFIX).unwrap_or(&name).to_string();
    vault.with_file_name(base)
}

/// Vault that belongs to a mount point, if there is one next to it.
pub fn vault_for(dir: &Path) -> Option<PathBuf> {
    let name = dir.file_name()?.to_string_lossy().to_string();
    let vault = dir.with_file_name(format!("{name}{SUFFIX}"));
    if is_vault(&vault) {
        Some(vault)
    } else {
        None
    }
}

pub fn is_mounted(mount: &Path) -> bool {
    let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else { return false };
    let target = mount.to_string_lossy().replace(' ', "\\040");
    mounts.lines().any(|l| {
        let mut parts = l.split_whitespace();
        let _dev = parts.next();
        let point = parts.next();
        let fstype = parts.next();
        point == Some(target.as_str()) && fstype.map(|t| t.contains("gocryptfs")).unwrap_or(false)
    })
}

async fn run_with_password(args: &[&str], password: &str) -> Result<String, glib::Error> {
    let mut argv: Vec<&std::ffi::OsStr> = vec![std::ffi::OsStr::new("gocryptfs")];
    argv.extend(args.iter().map(std::ffi::OsStr::new));
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDIN_PIPE | gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_PIPE);
    let process = launcher.spawn(&argv)?;
    // The password goes through a pipe only; it never touches the disk or the command line.
    let (stdout, stderr) = process.communicate_utf8_future(Some(format!("{password}\n"))).await?;
    if process.is_successful() {
        Ok(stdout.map(|s| s.to_string()).unwrap_or_default())
    } else {
        let msg = stderr.map(|s| s.to_string()).unwrap_or_default();
        let msg = msg.lines().rfind(|l| !l.contains("syslog")).unwrap_or("gocryptfs failed").to_string();
        Err(glib::Error::new(gio::IOErrorEnum::Failed, &msg))
    }
}

/// Create a vault; returns the master key gocryptfs prints for emergency recovery.
///
/// gocryptfs only prints the master key when its stdout is a terminal, so the
/// init runs under `script` (util-linux) with a pseudo terminal. The password
/// still arrives through a private socket pair as `/dev/fd/3`, never through
/// the command line or a file.
pub async fn create(vault: &Path, password: &str) -> Result<String, glib::Error> {
    let io_err = |e: std::io::Error| glib::Error::new(gio::IOErrorEnum::Failed, &e.to_string());
    std::fs::create_dir_all(vault).map_err(io_err)?;
    std::fs::create_dir_all(mount_point_for(vault)).map_err(io_err)?;
    let vault_str = vault.to_string_lossy().to_string();
    if glib::find_program_in_path("script").is_none() {
        run_with_password(&["-init", "-q", "-passfile", "/dev/stdin", &vault_str], password).await?;
        return Ok(String::new());
    }
    // A pipe, not a socket pair: /dev/fd/N can be opened for a pipe but not for a socket.
    let (reader, mut writer) = std::io::pipe().map_err(io_err)?;
    let command = format!("gocryptfs -init -passfile /dev/fd/3 {}", shell_quote(&vault_str));
    let args = ["script", "-qefc", &command, "/dev/null"];
    let argv: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE);
    // SAFETY: 3 is only used as a number for the child; it is not read in this process.
    launcher.take_fd(std::os::fd::OwnedFd::from(reader), unsafe { std::os::fd::BorrowedFd::borrow_raw(3) });
    let process = launcher.spawn(&argv)?;
    {
        use std::io::Write;
        writer.write_all(format!("{password}\n").as_bytes()).map_err(io_err)?;
        drop(writer);
    }
    let (stdout, _) = process.communicate_future(None).await?;
    let text = stdout.map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
    if !process.is_successful() {
        let plain = strip_ansi(&text);
        let msg = plain.lines().rfind(|l| !l.trim().is_empty() && !l.contains("syslog")).unwrap_or("gocryptfs init failed").trim().to_string();
        // Leave nothing half-made behind.
        let _ = std::fs::remove_dir(mount_point_for(vault));
        let _ = std::fs::remove_dir(vault);
        return Err(glib::Error::new(gio::IOErrorEnum::Failed, &msg));
    }
    Ok(master_key_from(&text))
}

/// The master key gocryptfs prints: hex groups separated by dashes, possibly
/// wrapped over several lines and wrapped in terminal colour codes.
pub fn master_key_from(text: &str) -> String {
    let plain = strip_ansi(text);
    let mut key = String::new();
    let mut in_key = false;
    for line in plain.lines() {
        let l = line.trim();
        if l.starts_with("Your master key is") {
            in_key = true;
            continue;
        }
        if in_key {
            if !l.is_empty() && l.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
                key.push_str(l);
            } else if !key.is_empty() {
                break;
            }
        }
    }
    key
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Skip CSI sequences: ESC [ ... final byte in 0x40..=0x7e
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            continue;
        }
        if c != '\r' {
            out.push(c);
        }
    }
    out
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub async fn unlock(vault: &Path, password: &str) -> Result<PathBuf, glib::Error> {
    let mount = mount_point_for(vault);
    std::fs::create_dir_all(&mount).map_err(|e| glib::Error::new(gio::IOErrorEnum::Failed, &e.to_string()))?;
    run_with_password(&["-q", "-passfile", "/dev/stdin", &vault.to_string_lossy(), &mount.to_string_lossy()], password).await?;
    Ok(mount)
}

pub async fn lock(mount: &Path) -> Result<(), glib::Error> {
    let tool = if glib::find_program_in_path("fusermount3").is_some() { "fusermount3" } else { "fusermount" };
    crate::ops::run_command(&[tool, "-u", &mount.to_string_lossy()], None).await
}

pub async fn change_password(vault: &Path, old: &str, new: &str) -> Result<(), glib::Error> {
    // gocryptfs -passwd reads the old password, then the new one twice.
    let path = vault.to_string_lossy().to_string();
    let args = ["gocryptfs", "-passwd", "-q", "-passfile", "/dev/stdin", &path];
    let argv: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDIN_PIPE | gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_PIPE);
    let process = launcher.spawn(&argv)?;
    let (_, stderr) = process.communicate_utf8_future(Some(format!("{old}\n{new}\n{new}\n"))).await?;
    if process.is_successful() {
        Ok(())
    } else {
        let msg = stderr.map(|s| s.to_string()).unwrap_or_default();
        Err(glib::Error::new(gio::IOErrorEnum::Failed, msg.lines().rfind(|l| !l.contains("syslog")).unwrap_or("gocryptfs failed")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_master_key() {
        let out = "Choose a password.\r\nYour master key is:\r\n\r\n    \u{1b}[2m1a2b3c4d-5e6f7a8b-9c0d1e2f-3a4b5c6d-\r\n    7e8f9a0b-1c2d3e4f-5a6b7c8d-9e0f1a2b\u{1b}[0m\r\n\r\nThe gocryptfs filesystem has been created successfully.\n";
        assert_eq!(master_key_from(out), "1a2b3c4d-5e6f7a8b-9c0d1e2f-3a4b5c6d-7e8f9a0b-1c2d3e4f-5a6b7c8d-9e0f1a2b");
        assert_eq!(master_key_from("nothing here"), "");
    }

    #[test]
    fn names() {
        let v = PathBuf::from("/home/u/Secrets.gocryptfs");
        assert_eq!(mount_point_for(&v), PathBuf::from("/home/u/Secrets"));
    }
}
