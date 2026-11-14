//! Drives: what a drive is, and formatting one.
//!
//! What `lsblk` knows about a device (size, file system, label, how it is attached, how
//! full it is), read as JSON. Formatting goes through `udisksctl`, the same daemon that
//! mounts drives for the desktop: it asks for the password through the polkit agent
//! (Omarchy's own) and writes nothing without it. omafile never calls `mkfs` itself and
//! never formats a whole disk that carries partitions.

use crate::json::{self, Json};
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    /// `/dev/sdb1`
    pub device: String,
    /// `/dev/sdb`, when the device is a partition.
    pub disk: Option<String>,
    pub kind: String,
    pub size: u64,
    pub filesystem: String,
    pub label: String,
    pub uuid: String,
    pub mountpoint: String,
    pub removable: bool,
    pub model: String,
    pub vendor: String,
    pub transport: String,
    pub used: Option<u64>,
    pub available: Option<u64>,
    /// The device has partitions: it is a disk to be formatted partition by partition.
    pub has_partitions: bool,
}

pub const COLUMNS: &str = "PATH,PKNAME,TYPE,SIZE,FSTYPE,LABEL,UUID,MOUNTPOINT,RM,HOTPLUG,MODEL,VENDOR,TRAN,FSUSED,FSAVAIL";

/// `lsblk -J -b -o COLUMNS <device>`, read. The first device in the answer is the one
/// asked for; its children tell whether it carries partitions.
pub fn parse_lsblk(output: &str) -> Option<Info> {
    let root = json::parse(output)?;
    let devices = match root.get("blockdevices") {
        Some(Json::Array(list)) => list,
        _ => return None,
    };
    let first = devices.first()?;
    let text = |key: &str| first.str(key).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string).unwrap_or_default();
    let number = |key: &str| match first.get(key) {
        Some(Json::Number(n)) => Some(*n as u64),
        Some(Json::String(s)) => s.trim().parse().ok(),
        _ => None,
    };
    let flag = |key: &str| matches!(first.get(key), Some(Json::Bool(true))) || first.str(key).is_some_and(|v| v == "1" || v == "true");
    let children = match first.get("children") {
        Some(Json::Array(list)) => list.len(),
        _ => 0,
    };
    let disk = text("pkname");
    Some(Info {
        device: text("path"),
        disk: (!disk.is_empty()).then(|| if disk.starts_with('/') { disk.clone() } else { format!("/dev/{disk}") }),
        kind: text("type"),
        size: number("size").unwrap_or(0),
        filesystem: text("fstype"),
        label: text("label"),
        uuid: text("uuid"),
        mountpoint: text("mountpoint"),
        removable: flag("rm") || flag("hotplug"),
        model: text("model"),
        vendor: text("vendor"),
        transport: text("tran"),
        used: number("fsused"),
        available: number("fsavail"),
        has_partitions: children > 0,
    })
}

/// Ask lsblk about a device. Blocks for the length of the command: call it off the main thread.
pub fn info(device: &str) -> Option<Info> {
    let output = std::process::Command::new("lsblk").args(["-J", "-b", "-o", COLUMNS, "--", device]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_lsblk(&String::from_utf8_lossy(&output.stdout))
}

/// The file systems that can be made here: the name udisks takes, what the dialog calls
/// it, and the program that must be installed for it.
pub const FILESYSTEMS: &[(&str, &str, &str)] = &[
    ("ext4", "ext4 (Linux)", "mkfs.ext4"),
    ("btrfs", "Btrfs (Linux, snapshots)", "mkfs.btrfs"),
    ("exfat", "exFAT (every system, large files)", "mkfs.exfat"),
    ("vfat", "FAT32 (every system, files up to 4 GB)", "mkfs.vfat"),
    ("ntfs", "NTFS (Windows)", "mkfs.ntfs"),
    ("xfs", "XFS (Linux)", "mkfs.xfs"),
];

pub fn filesystems_available() -> Vec<(&'static str, &'static str)> {
    FILESYSTEMS.iter().filter(|(_, _, program)| gtk::glib::find_program_in_path(program).is_some()).map(|(id, name, _)| (*id, *name)).collect()
}

/// A label the file system accepts: FAT keeps eleven capitals, the others their own limits.
pub fn fit_label(filesystem: &str, label: &str) -> String {
    let label = label.trim();
    match filesystem {
        "vfat" => label.to_uppercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '_' || *c == '-').take(11).collect::<String>().trim().to_string(),
        "exfat" => label.chars().take(15).collect(),
        "ext4" | "xfs" => label.chars().take(16).collect(),
        "ntfs" => label.chars().take(32).collect(),
        _ => label.chars().take(255).collect(),
    }
}

/// Whether this device may be formatted at all: a partition, or a disk without partitions
/// (a USB stick without a table, a card). Never the disk the system runs from.
pub fn can_format(info: &Info) -> Result<(), &'static str> {
    if info.has_partitions {
        return Err("This is a whole disk with partitions on it: format one of its partitions, not the disk.");
    }
    if info.mountpoint == "/" || info.mountpoint == "/boot" || info.mountpoint == "/home" || info.mountpoint.starts_with("/boot/") {
        return Err("The system runs from this: it cannot be formatted from here.");
    }
    if info.device.is_empty() || !info.device.starts_with("/dev/") {
        return Err("No device to format.");
    }
    Ok(())
}

/// The commands that format a device: unmount it when it is mounted, then make the file
/// system. Each is a `udisksctl` call; the daemon asks for the password itself.
pub fn format_commands(info: &Info, filesystem: &str, label: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    if !info.mountpoint.is_empty() {
        out.push(vec!["udisksctl".into(), "unmount".into(), "-b".into(), info.device.clone()]);
    }
    let mut format = vec!["udisksctl".into(), "format".into(), "-b".into(), info.device.clone(), "--type".into(), filesystem.to_string()];
    let label = fit_label(filesystem, label);
    if !label.is_empty() {
        format.push("--label".into());
        format.push(label);
    }
    out.push(format);
    out
}

/// `/dev/sdb1` of a mount or a volume, as GIO names it.
pub fn device_of_mount(mount: &gtk::gio::Mount) -> Option<String> {
    use gtk::gio::prelude::*;
    mount.volume().and_then(|v| v.identifier(gtk::gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE)).map(|s| s.to_string()).or_else(|| {
        // A mount without a volume (a loop device, a bind mount): findmnt knows the source.
        let path = mount.root().path()?;
        let output = std::process::Command::new("findmnt").args(["-n", "-o", "SOURCE", "-T"]).arg(&path).output().ok()?;
        let source = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (output.status.success() && source.starts_with("/dev/") && Path::new(&source).exists()).then_some(source)
    })
}

pub fn device_of_volume(volume: &gtk::gio::Volume) -> Option<String> {
    use gtk::gio::prelude::*;
    volume.identifier(gtk::gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STICK: &str = r#"{"blockdevices":[{"path":"/dev/sdb1","pkname":"sdb","type":"part","size":31029460992,"fstype":"exfat","label":"PHOTOS","uuid":"1234-ABCD","mountpoint":"/run/media/me/PHOTOS","rm":true,"hotplug":true,"model":null,"vendor":null,"tran":null,"fsused":12000000000,"fsavail":19000000000}]}"#;
    const DISK: &str = r#"{"blockdevices":[{"path":"/dev/sdb","pkname":null,"type":"disk","size":31037849600,"fstype":null,"label":null,"uuid":null,"mountpoint":null,"rm":true,"hotplug":true,"model":"Ultra Fit","vendor":"SanDisk ","tran":"usb","fsused":null,"fsavail":null,"children":[{"path":"/dev/sdb1","type":"part","size":31029460992}]}]}"#;

    #[test]
    fn lsblk_is_read() {
        let stick = parse_lsblk(STICK).unwrap();
        assert_eq!(stick.device, "/dev/sdb1");
        assert_eq!(stick.disk.as_deref(), Some("/dev/sdb"));
        assert_eq!((stick.size, stick.filesystem.as_str(), stick.label.as_str(), stick.mountpoint.as_str()), (31029460992, "exfat", "PHOTOS", "/run/media/me/PHOTOS"));
        assert!(stick.removable && !stick.has_partitions);
        assert_eq!((stick.used, stick.available), (Some(12000000000), Some(19000000000)));
        let disk = parse_lsblk(DISK).unwrap();
        assert!(disk.has_partitions && disk.disk.is_none());
        assert_eq!((disk.model.as_str(), disk.vendor.as_str(), disk.transport.as_str()), ("Ultra Fit", "SanDisk", "usb"));
        // Older lsblk writes numbers and flags as strings.
        let old = parse_lsblk(r#"{"blockdevices":[{"path":"/dev/sdc1","size":"1024","rm":"1","fsused":"512"}]}"#).unwrap();
        assert_eq!((old.size, old.removable, old.used), (1024, true, Some(512)));
        assert_eq!(parse_lsblk("not json"), None);
        assert_eq!(parse_lsblk(r#"{"blockdevices":[]}"#), None);
    }

    #[test]
    fn formatting_is_careful() {
        let stick = parse_lsblk(STICK).unwrap();
        assert_eq!(can_format(&stick), Ok(()));
        assert!(can_format(&parse_lsblk(DISK).unwrap()).is_err(), "a disk with partitions is not formatted whole");
        let root = Info { device: "/dev/vda2".into(), mountpoint: "/".into(), ..Default::default() };
        assert!(can_format(&root).is_err(), "not the system disk");
        assert!(can_format(&Info::default()).is_err());
        // Mounted: unmount first, then format, through udisks only.
        let commands = format_commands(&stick, "exfat", "Holiday 2026");
        assert_eq!(commands, vec![vec!["udisksctl", "unmount", "-b", "/dev/sdb1"], vec!["udisksctl", "format", "-b", "/dev/sdb1", "--type", "exfat", "--label", "Holiday 2026"]]);
        let bare = Info { device: "/dev/sdc".into(), ..Default::default() };
        assert_eq!(format_commands(&bare, "ext4", "  "), vec![vec!["udisksctl", "format", "-b", "/dev/sdc", "--type", "ext4"]]);
        assert!(format_commands(&bare, "vfat", "x").iter().all(|c| c[0] == "udisksctl"), "never mkfs by hand");
        // Labels the file systems take.
        assert_eq!(fit_label("vfat", "Holiday pictures 2026!"), "HOLIDAY PIC");
        assert_eq!(fit_label("ext4", "a very long label indeed"), "a very long labe");
        assert_eq!(fit_label("exfat", " Photos "), "Photos");
        assert!(!FILESYSTEMS.is_empty());
    }
}
