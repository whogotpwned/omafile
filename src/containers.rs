//! Docker / podman volumes as sidebar places. Only volumes whose mount point
//! this user can read are listed: rootless podman volumes live under
//! `~/.local/share/containers/storage/volumes`, Docker's under `/var/lib/docker`
//! and are usually root-only (then nothing is shown).

use gtk::{gio, glib};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    pub engine: &'static str,
    pub name: String,
    pub mountpoint: PathBuf,
}

async fn list_with(engine: &'static str) -> Vec<Volume> {
    if glib::find_program_in_path(engine).is_none() {
        return Vec::new();
    }
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE);
    let argv = [engine, "volume", "ls", "--format", "{{.Name}}\t{{.Mountpoint}}"];
    let args: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
    let Ok(process) = launcher.spawn(&args) else { return Vec::new() };
    // A daemon that is down answers quickly; a hung socket must not block the UI forever.
    let output = futures_util::future::select(
        Box::pin(process.communicate_future(None)),
        Box::pin(glib::timeout_future(std::time::Duration::from_secs(4))),
    )
    .await;
    let stdout = match output {
        futures_util::future::Either::Left((Ok((Some(out), _)), _)) if process.is_successful() => String::from_utf8_lossy(&out).to_string(),
        futures_util::future::Either::Right(_) => {
            process.force_exit();
            return Vec::new();
        }
        _ => return Vec::new(),
    };
    parse(engine, &stdout).into_iter().filter(|v| std::fs::read_dir(&v.mountpoint).is_ok()).collect()
}

pub fn parse(engine: &'static str, text: &str) -> Vec<Volume> {
    text.lines()
        .filter_map(|l| {
            let (name, mount) = l.split_once('\t')?;
            if name.is_empty() || mount.is_empty() {
                return None;
            }
            // podman prints the _data directory; docker prints it too.
            Some(Volume { engine, name: name.to_string(), mountpoint: PathBuf::from(mount) })
        })
        .collect()
}

pub async fn volumes() -> Vec<Volume> {
    let (a, b) = futures_util::future::join(list_with("docker"), list_with("podman")).await;
    let mut all = a;
    all.extend(b);
    all.sort_by(|x, y| x.name.cmp(&y.name));
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_and_mountpoint() {
        let v = parse("podman", "pgdata\t/home/u/.local/share/containers/storage/volumes/pgdata/_data\nbroken\n\tnomount\n");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "pgdata");
        assert!(v[0].mountpoint.ends_with("pgdata/_data"));
    }
}
