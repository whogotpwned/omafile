//! Captures: the screenshots and screen recordings Omarchy makes, found again.
//!
//! Omarchy's `omarchy-capture-screenshot` writes `screenshot-<date>_<time>.png` into
//! `$OMARCHY_SCREENSHOT_DIR` (else the Pictures folder), `omarchy-capture-screenrecording`
//! writes `screenrecording-<date>_<time>.mp4` into `$OMARCHY_SCREENRECORD_DIR` (else the
//! Videos folder) with a `-preview.png` beside it. This module only reads those folders
//! and names the commands Omarchy itself would run (Satty for drawing on a screenshot);
//! taking the captures stays Omarchy's job.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Screenshot,
    Recording,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    pub path: PathBuf,
    pub kind: Kind,
    /// When it was taken: the time in its name, else when the file was last written.
    pub time: i64,
    pub size: u64,
    /// The still Omarchy saves beside a recording.
    pub poster: Option<PathBuf>,
}

/// Where screenshots and recordings go, the way Omarchy's scripts decide it.
pub fn dirs_from(screenshot_env: Option<String>, recording_env: Option<String>, pictures: Option<PathBuf>, videos: Option<PathBuf>, home: &Path) -> (PathBuf, PathBuf) {
    let pick = |env: Option<String>, special: Option<PathBuf>, fallback: &str| env.filter(|v| !v.trim().is_empty()).map(PathBuf::from).or(special).unwrap_or_else(|| home.join(fallback));
    (pick(screenshot_env, pictures, "Pictures"), pick(recording_env, videos, "Videos"))
}

pub fn dirs() -> (PathBuf, PathBuf) {
    use gtk::glib;
    dirs_from(std::env::var("OMARCHY_SCREENSHOT_DIR").ok(), std::env::var("OMARCHY_SCREENRECORD_DIR").ok(), glib::user_special_dir(glib::UserDirectory::Pictures), glib::user_special_dir(glib::UserDirectory::Videos), &glib::home_dir())
}

/// What a file name is: a screenshot, a recording, or neither (the preview stills of
/// recordings are neither).
pub fn classify(name: &str) -> Option<Kind> {
    let lower = name.to_lowercase();
    let ends = |list: &[&str]| list.iter().any(|e| lower.ends_with(e));
    if lower.starts_with("screenrecording") {
        return ends(&[".mp4", ".mkv", ".webm", ".mov"]).then_some(Kind::Recording);
    }
    if lower.starts_with("screenshot") && ends(&[".png", ".jpg", ".jpeg", ".webp"]) {
        return Some(Kind::Screenshot);
    }
    None
}

/// Days since 1 January 1970 for a date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The moment in a name like `screenshot-2026-11-12_09-15-02.png`, which is local time.
pub fn time_in_name(name: &str, offset: i64) -> Option<i64> {
    let digits: Vec<i64> = name.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect();
    let [year, month, day, hour, minute, second] = digits.get(..6)?.try_into().ok()?;
    if !(1990..=2200).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86400 + hour * 3600 + minute * 60 + second - offset)
}

/// Every capture in the given folders, newest first. `offset` is local time minus UTC.
pub fn list_in(folders: &[&Path], offset: i64) -> Vec<Capture> {
    let mut out: Vec<Capture> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for folder in folders {
        if seen.iter().any(|s| s == folder) {
            continue;
        }
        seen.push(folder.to_path_buf());
        let Ok(entries) = std::fs::read_dir(folder) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(kind) = classify(&name) else { continue };
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() || meta.len() == 0 {
                continue;
            }
            let written = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
            let path = entry.path();
            let poster = (kind == Kind::Recording).then(|| path.with_file_name(format!("{}-preview.png", path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()))).filter(|p| p.is_file());
            out.push(Capture { path, kind, time: time_in_name(&name, offset).unwrap_or(written), size: meta.len(), poster });
        }
    }
    out.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| b.path.cmp(&a.path)));
    out
}

pub fn list() -> Vec<Capture> {
    let (shots, recordings) = dirs();
    list_in(&[&shots, &recordings], crate::backup::utc_offset())
}

/// Which day a capture belongs to, seen from today.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Day {
    Today,
    Yesterday,
    /// Earlier this week: the weekday, Monday = 0.
    Weekday(u8),
    /// Before that: any moment of that day, to print its date.
    Earlier(i64),
}

pub fn day_of(time: i64, now: i64, offset: i64) -> Day {
    let day = |t: i64| (t + offset).div_euclid(86400);
    match day(now) - day(time) {
        i64::MIN..=0 => Day::Today,
        1 => Day::Yesterday,
        2..=6 => Day::Weekday(((day(time) + 3).rem_euclid(7)) as u8),
        _ => Day::Earlier(time),
    }
}

/// The captures in groups by day, newest day first.
pub fn by_day(captures: &[Capture], now: i64, offset: i64) -> Vec<(Day, Vec<Capture>)> {
    let mut groups: Vec<(Day, i64, Vec<Capture>)> = Vec::new();
    for capture in captures {
        let number = (capture.time + offset).div_euclid(86400);
        match groups.iter_mut().find(|(_, n, _)| *n == number) {
            Some((_, _, list)) => list.push(capture.clone()),
            None => groups.push((day_of(capture.time, now, offset), number, vec![capture.clone()])),
        }
    }
    groups.sort_by_key(|(_, number, _)| std::cmp::Reverse(*number));
    groups.into_iter().map(|(day, _, list)| (day, list)).collect()
}

/// The program that draws on a screenshot, as Omarchy picks it.
pub fn editor() -> String {
    std::env::var("OMARCHY_SCREENSHOT_EDITOR").ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| "satty".into())
}

/// The command to draw on a screenshot. For Satty it is the one Omarchy's screenshot
/// script runs: the picture is saved over itself and copied to the clipboard on Enter.
pub fn annotate_argv(editor: &str, picture: &Path) -> Vec<String> {
    let file = picture.to_string_lossy().to_string();
    if editor == "satty" {
        vec!["satty".into(), "--filename".into(), file.clone(), "--output-filename".into(), file, "--actions-on-enter".into(), "save-to-clipboard".into(), "--save-after-copy".into(), "--copy-command".into(), "wl-copy".into()]
    } else {
        vec![editor.to_string(), file]
    }
}

/// A few stills of a recording, spread over its length, to flip through like a film strip.
pub fn frames_argv(video: &Path, into: &Path, seconds: f64, count: u32) -> Vec<String> {
    // Without a known length: one frame a second.
    let rate = if seconds > 0.5 { format!("{count}/{seconds:.3}") } else { "1".to_string() };
    vec![
        "ffmpeg".into(),
        "-v".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        video.to_string_lossy().to_string(),
        "-vf".into(),
        format!("fps={rate},scale=480:-2"),
        "-frames:v".into(),
        count.to_string(),
        "-q:v".into(),
        "5".into(),
        into.join("%02d.jpg").to_string_lossy().to_string(),
    ]
}

/// How long a recording is, in seconds (ffprobe).
pub fn duration(video: &Path) -> Option<f64> {
    let out = std::process::Command::new("ffprobe").args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"]).arg(video).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().ok().filter(|d| *d > 0.0)
}

/// `1:05` for 65 seconds.
pub fn clock(seconds: f64) -> String {
    let s = seconds.round().max(0.0) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// Where the stills of a recording are kept: a folder per file and state, so a recording
/// that was cut gets new ones.
pub fn frames_dir(cache: &Path, capture: &Capture) -> PathBuf {
    cache.join("captures").join(crate::feeds::hash(&format!("{}:{}:{}", capture.path.display(), capture.size, capture.time)))
}

/// Make the stills (when they are not there yet) and return them in order.
pub fn prepare_frames(cache: &Path, capture: &Capture, count: u32) -> Vec<PathBuf> {
    let dir = frames_dir(cache, capture);
    let existing = |dir: &Path| {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "jpg")).collect()).unwrap_or_default();
        files.sort();
        files
    };
    let have = existing(&dir);
    if !have.is_empty() {
        return have;
    }
    if std::fs::create_dir_all(&dir).is_err() {
        return Vec::new();
    }
    let argv = frames_argv(&capture.path, &dir, duration(&capture.path).unwrap_or(0.0), count);
    let _ = std::process::Command::new(&argv[0]).args(&argv[1..]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    existing(&dir)
}

/// `omafile --copy-capture`: the newest capture onto the clipboard, for the bar's menu and
/// for key bindings. A screenshot goes as a picture, a recording as a file. Returns the
/// exit code.
pub fn copy_latest() -> u8 {
    let Some(latest) = list().into_iter().next() else {
        eprintln!("omafile: no screenshot or recording found");
        return 1;
    };
    let ok = match latest.kind {
        Kind::Screenshot => std::fs::File::open(&latest.path).ok().and_then(|file| std::process::Command::new("wl-copy").args(["--type", "image/png"]).stdin(file).status().ok()).is_some_and(|s| s.success()),
        Kind::Recording => std::process::Command::new("wl-copy").args(["--type", "text/uri-list"]).arg(format!("file://{}", latest.path.display())).status().is_ok_and(|s| s.success()),
    };
    let name = latest.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if ok {
        let _ = std::process::Command::new("notify-send").args(["-a", "omafile", "-i", "edit-copy-symbolic", &crate::i18n::tr("Copied to the clipboard"), &name]).status();
        0
    } else {
        eprintln!("omafile: cannot copy {name} (is wl-copy installed?)");
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omafile-captures-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn folders_and_names_follow_omarchy() {
        let home = Path::new("/home/me");
        assert_eq!(dirs_from(None, None, None, None, home), (home.join("Pictures"), home.join("Videos")));
        assert_eq!(dirs_from(None, None, Some("/home/me/Bilder".into()), Some("/home/me/Filme".into()), home), (PathBuf::from("/home/me/Bilder"), PathBuf::from("/home/me/Filme")));
        assert_eq!(dirs_from(Some("/data/shots".into()), Some(" ".into()), Some("/home/me/Bilder".into()), None, home), (PathBuf::from("/data/shots"), home.join("Videos")));
        assert_eq!(classify("screenshot-2026-11-12_09-15-02.png"), Some(Kind::Screenshot));
        assert_eq!(classify("Screenshot from 2026-11-12.jpg"), Some(Kind::Screenshot));
        assert_eq!(classify("screenrecording-2026-11-12_09-15-02.mp4"), Some(Kind::Recording));
        assert_eq!(classify("screenrecording-2026-11-12_09-15-02-preview.png"), None, "the still beside a recording is not a capture");
        assert_eq!(classify("holiday.png"), None);
        assert_eq!(classify("screenshot-notes.txt"), None);
        // The name carries local time: 09:15:02 at UTC+1 is 08:15:02 UTC.
        let at = time_in_name("screenshot-2026-11-12_09-15-02.png", 3600).unwrap();
        assert_eq!(at, 1_794_471_302);
        assert_eq!(time_in_name("screenshot-notes.png", 0), None);
        assert_eq!(time_in_name("screenshot-2026-13-40_09-15-02.png", 0), None);
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
    }

    #[test]
    fn lists_newest_first_and_groups_by_day() {
        let root = scratch("list");
        let (shots, videos) = (root.join("Pictures"), root.join("Videos"));
        std::fs::create_dir_all(&shots).unwrap();
        std::fs::create_dir_all(&videos).unwrap();
        for name in ["screenshot-2026-11-12_09-15-02.png", "screenshot-2026-11-12_08-00-00.png", "screenshot-2026-11-11_23-59-59.png", "screenshot-2026-11-08_12-00-00.png", "screenshot-2026-10-01_12-00-00.png", "holiday.png"] {
            std::fs::write(shots.join(name), "png").unwrap();
        }
        std::fs::write(shots.join("screenshot-2026-11-12_10-00-00.png"), "").unwrap();
        std::fs::write(videos.join("screenrecording-2026-11-12_09-00-00.mp4"), "mp4").unwrap();
        std::fs::write(videos.join("screenrecording-2026-11-12_09-00-00-preview.png"), "png").unwrap();
        std::fs::write(videos.join("screenrecording-2026-11-10_18-30-00.mp4"), "mp4").unwrap();

        let all = list_in(&[&shots, &videos, &shots], 0);
        let names: Vec<String> = all.iter().map(|c| c.path.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names[..3], ["screenshot-2026-11-12_09-15-02.png", "screenrecording-2026-11-12_09-00-00.mp4", "screenshot-2026-11-12_08-00-00.png"]);
        assert_eq!(all.len(), 7, "no other pictures, no empty files, no folder twice");
        assert_eq!(all[1].poster.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).as_deref(), Some("screenrecording-2026-11-12_09-00-00-preview.png"));
        assert!(all.iter().filter(|c| c.kind == Kind::Recording).count() == 2 && all.last().unwrap().poster.is_none());

        // Seen on 12 November 2026 (a Thursday) at noon.
        let now = time_in_name("x-2026-11-12_12-00-00", 0).unwrap();
        let groups = by_day(&all, now, 0);
        assert_eq!(groups.iter().map(|(d, list)| (std::mem::discriminant(d), list.len())).collect::<Vec<_>>().len(), 5);
        assert_eq!(groups[0].0, Day::Today);
        assert_eq!(groups[0].1.len(), 3);
        assert_eq!(groups[1].0, Day::Yesterday);
        assert_eq!(groups[2].0, Day::Weekday(1), "10 November 2026 is a Tuesday");
        assert_eq!(groups[3].0, Day::Weekday(6), "8 November 2026 is a Sunday");
        assert!(matches!(groups[4].0, Day::Earlier(_)));
        // Just after midnight, yesterday evening's capture is "yesterday", not "today".
        let late = time_in_name("x-2026-11-11_23-59-59", 0).unwrap();
        assert_eq!(day_of(late, late + 2, 0), Day::Yesterday);
        assert_eq!(day_of(late, late + 2, -3600), Day::Today, "one time zone further west it is still the same evening");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn commands_are_omarchy_s_own() {
        let shot = Path::new("/home/me/Pictures/screenshot-2026-11-12_09-15-02.png");
        assert_eq!(
            annotate_argv("satty", shot).join(" "),
            "satty --filename /home/me/Pictures/screenshot-2026-11-12_09-15-02.png --output-filename /home/me/Pictures/screenshot-2026-11-12_09-15-02.png --actions-on-enter save-to-clipboard --save-after-copy --copy-command wl-copy"
        );
        assert_eq!(annotate_argv("swappy", shot), vec!["swappy".to_string(), shot.to_string_lossy().to_string()]);
        let frames = frames_argv(Path::new("/v/a b.mp4"), Path::new("/cache/x"), 24.0, 12);
        assert!(frames.contains(&"fps=12/24.000,scale=480:-2".to_string()) && frames.contains(&"/v/a b.mp4".to_string()) && frames.last().unwrap() == "/cache/x/%02d.jpg");
        assert!(frames_argv(Path::new("/v/a.mp4"), Path::new("/c"), 0.0, 12).contains(&"fps=1,scale=480:-2".to_string()));
        assert_eq!((clock(65.2), clock(4.0), clock(3725.0)), ("1:05".to_string(), "0:04".to_string(), "1:02:05".to_string()));
        let capture = Capture { path: "/v/a.mp4".into(), kind: Kind::Recording, time: 10, size: 20, poster: None };
        let cut = Capture { size: 12, ..capture.clone() };
        assert_ne!(frames_dir(Path::new("/cache"), &capture), frames_dir(Path::new("/cache"), &cut), "a recording that was cut gets new stills");
    }
}
