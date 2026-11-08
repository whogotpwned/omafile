//! Quick Look renderers and media metadata: Markdown with structure, CSV as a
//! table, JSON pretty-printed with jq, source code highlighted with bat (ANSI
//! → text tags), font files as a specimen, plus image / audio / video facts
//! from gdk-pixbuf, ImageMagick and ffprobe.

use gtk::glib;
use std::path::Path;

pub fn bat_binary() -> Option<&'static str> {
    ["bat", "batcat"].into_iter().find(|b| glib::find_program_in_path(b).is_some())
}

pub fn is_markdown(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".md") || n.ends_with(".markdown") || n == "readme"
}

pub fn is_csv(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".csv") || n.ends_with(".tsv")
}

pub fn is_json(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".json") || n.ends_with(".jsonc") || n.ends_with(".geojson")
}

pub fn is_font(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".ttf") || n.ends_with(".otf") || n.ends_with(".woff") || n.ends_with(".woff2") || n.ends_with(".ttc")
}

/// Source files bat knows how to colour (anything text that is not prose).
pub fn is_code(name: &str) -> bool {
    let n = name.to_lowercase();
    let exts = ["rs", "py", "js", "ts", "tsx", "jsx", "go", "c", "h", "cpp", "hpp", "cc", "java", "kt", "rb", "sh", "bash", "zsh", "fish", "lua", "toml", "yaml", "yml", "css", "scss", "html", "xml", "qml", "nix", "conf", "ini", "sql", "swift", "php", "pl", "r", "dart", "zig", "vue", "svelte", "dockerfile", "makefile", "cmake", "tf", "hcl", "proto", "graphql"];
    exts.iter().any(|e| n.ends_with(&format!(".{e}"))) || n == "dockerfile" || n == "makefile" || n == "justfile" || n == "pkgbuild"
}

/// One styled piece of text for the TextView: (text, tag name or "").
pub type Span = (String, &'static str);

/// Markdown → spans with heading / bold / italic / code / list structure.
pub fn markdown(text: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut in_code = false;
    for line in text.lines().take(400) {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push((format!("    {line}\n"), "code"));
            continue;
        }
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("### ") {
            out.push((format!("{rest}\n"), "h3"));
        } else if let Some(rest) = trimmed.strip_prefix("## ") {
            out.push((format!("{rest}\n"), "h2"));
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            out.push((format!("{rest}\n"), "h1"));
        } else if let Some(rest) = trimmed.strip_prefix("> ") {
            out.push((format!("│ {rest}\n"), "quote"));
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            let indent = line.len() - trimmed.len();
            out.push((format!("{}• ", " ".repeat(indent)), ""));
            inline(&trimmed[2..], &mut out);
            out.push(("\n".into(), ""));
        } else if trimmed.starts_with("---") || trimmed.starts_with("***") {
            out.push(("────────────\n".into(), "dim"));
        } else if trimmed.starts_with('|') {
            out.push((format!("{}\n", trimmed.replace('|', " │ ")), "code"));
        } else {
            inline(line, &mut out);
            out.push(("\n".into(), ""));
        }
    }
    out
}

/// `**bold**`, `*italic*`, `` `code` ``, `[text](url)` inside one line.
fn inline(line: &str, out: &mut Vec<Span>) {
    let mut rest = line;
    while !rest.is_empty() {
        let next = ["**", "`", "*", "["].iter().filter_map(|m| rest.find(m).map(|i| (i, *m))).min_by_key(|(i, _)| *i);
        let Some((i, marker)) = next else {
            out.push((rest.to_string(), ""));
            break;
        };
        out.push((rest[..i].to_string(), ""));
        let after = &rest[i + marker.len()..];
        let close = match marker {
            "[" => after.find("](").map(|c| (c, after[c..].find(')').map(|e| c + e + 1).unwrap_or(c + 2))),
            _ => after.find(marker).map(|c| (c, c + marker.len())),
        };
        match close {
            Some((c, end)) => {
                let tag = match marker {
                    "**" => "bold",
                    "`" => "code",
                    "*" => "italic",
                    _ => "link",
                };
                out.push((after[..c].to_string(), tag));
                rest = &after[end.min(after.len())..];
            }
            None => {
                out.push((marker.to_string(), ""));
                rest = after;
            }
        }
    }
}

/// CSV / TSV rows (quoted fields respected), limited for the preview.
pub fn csv_rows(text: &str, max_rows: usize) -> Vec<Vec<String>> {
    let delim = if text.lines().next().map(|l| l.matches('\t').count() > l.matches(',').count()).unwrap_or(false) { '\t' } else if text.lines().next().map(|l| l.matches(';').count() > l.matches(',').count()).unwrap_or(false) { ';' } else { ',' };
    let mut rows = Vec::new();
    for line in text.lines().take(max_rows) {
        let mut row = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '"' if quoted && chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = !quoted,
                c if c == delim && !quoted => row.push(std::mem::take(&mut field)),
                c => field.push(c),
            }
        }
        row.push(field);
        rows.push(row);
    }
    rows
}

/// JSON through jq; None when jq is missing or the text is not JSON.
pub fn pretty_json(path: &Path) -> Option<String> {
    glib::find_program_in_path("jq")?;
    let out = std::process::Command::new("jq").args(["--indent", "2", "."]).arg(path).stderr(std::process::Stdio::null()).output().ok()?;
    if out.status.success() { Some(String::from_utf8_lossy(&out.stdout).chars().take(60_000).collect()) } else { None }
}

/// bat's coloured output for a source file, truncated to `max_lines`.
pub fn highlight(path: &Path, max_lines: usize, dark: bool) -> Option<String> {
    let bat = bat_binary()?;
    // "ansi" maps colours onto the terminal palette, which our tag colours follow in both modes.
    let _ = dark;
    let out = std::process::Command::new(bat)
        .args(["--color=always", "--style=plain", "--paging=never", "--wrap=never", "--theme", "ansi", "--line-range", &format!(":{max_lines}")])
        .arg(path)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if out.status.success() { Some(String::from_utf8_lossy(&out.stdout).to_string()) } else { None }
}

/// ANSI SGR → spans. Supports bold, italic, the 16 colours, 256 and truecolor.
pub fn ansi_spans(text: &str) -> Vec<(String, AnsiStyle)> {
    let mut out = Vec::new();
    let mut style = AnsiStyle::default();
    let mut buf = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            let mut params = String::new();
            while let Some(&n) = chars.peek() {
                chars.next();
                if n == 'm' {
                    break;
                }
                params.push(n);
            }
            if !buf.is_empty() {
                out.push((std::mem::take(&mut buf), style.clone()));
            }
            style.apply(&params);
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        out.push((buf, style));
    }
    out
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct AnsiStyle {
    pub bold: bool,
    pub italic: bool,
    pub fg: Option<(u8, u8, u8)>,
}

const ANSI16: [(u8, u8, u8); 16] = [
    (0x1d, 0x1f, 0x21), (0xcc, 0x66, 0x66), (0xb5, 0xbd, 0x68), (0xf0, 0xc6, 0x74), (0x81, 0xa2, 0xbe), (0xb2, 0x94, 0xbb), (0x8a, 0xbe, 0xb7), (0xc5, 0xc8, 0xc6),
    (0x96, 0x98, 0x96), (0xd5, 0x4e, 0x53), (0xb9, 0xca, 0x4a), (0xe7, 0xc5, 0x47), (0x7a, 0xa6, 0xda), (0xc3, 0x97, 0xd8), (0x70, 0xc0, 0xb1), (0xea, 0xea, 0xea),
];

impl AnsiStyle {
    fn apply(&mut self, params: &str) {
        let codes: Vec<u32> = params.split(';').filter_map(|p| p.parse().ok()).collect();
        let mut i = 0;
        if codes.is_empty() {
            *self = AnsiStyle::default();
        }
        while i < codes.len() {
            match codes[i] {
                0 => *self = AnsiStyle::default(),
                1 => self.bold = true,
                3 => self.italic = true,
                22 => self.bold = false,
                23 => self.italic = false,
                39 => self.fg = None,
                30..=37 => self.fg = Some(ANSI16[(codes[i] - 30) as usize]),
                90..=97 => self.fg = Some(ANSI16[(codes[i] - 90 + 8) as usize]),
                38 if codes.get(i + 1) == Some(&5) => {
                    if let Some(&n) = codes.get(i + 2) {
                        self.fg = Some(xterm256(n as u8));
                    }
                    i += 2;
                }
                38 if codes.get(i + 1) == Some(&2) => {
                    if let (Some(&r), Some(&g), Some(&b)) = (codes.get(i + 2), codes.get(i + 3), codes.get(i + 4)) {
                        self.fg = Some((r as u8, g as u8, b as u8));
                    }
                    i += 4;
                }
                _ => {}
            }
            i += 1;
        }
    }
}

fn xterm256(n: u8) -> (u8, u8, u8) {
    match n {
        0..=15 => ANSI16[n as usize],
        16..=231 => {
            let n = n - 16;
            let c = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (c(n / 36), c((n / 6) % 6), c(n % 6))
        }
        _ => {
            let g = 8 + (n - 232) * 10;
            (g, g, g)
        }
    }
}

/// Font specimen rendered by ImageMagick straight from the font file.
pub fn font_specimen(path: &Path, dark: bool) -> Option<Vec<u8>> {
    let tool = if glib::find_program_in_path("magick").is_some() { "magick" } else if glib::find_program_in_path("convert").is_some() { "convert" } else { return None };
    let (bg, fg) = if dark { ("#1e1e2e", "#e0e0e0") } else { ("white", "#202020") };
    let text = "AaBbCcDdEeFfGg 0123456789\nThe quick brown fox jumps\nover the lazy dog. {}[]<>&@";
    let out = std::process::Command::new(tool)
        .args(["-background", bg, "-fill", fg, "-font"])
        .arg(path)
        .args(["-pointsize", "40", "-size", "900x", "-gravity", "west", &format!("caption:{text}"), "-bordercolor", bg, "-border", "30", "png:-"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if out.status.success() && !out.stdout.is_empty() { Some(out.stdout) } else { None }
}

// ----- metadata ------------------------------------------------------------------------

/// Facts worth a line in the preview: dimensions, camera, duration, codec…
pub fn media_facts(path: &Path, content_type: &str) -> Vec<(String, String)> {
    let mut facts = Vec::new();
    if content_type.starts_with("image/") {
        if let Some((format, w, h)) = gtk::gdk_pixbuf::Pixbuf::file_info(path) {
            facts.push(("Dimensions".into(), format!("{w} × {h} px")));
            facts.push(("Format".into(), format.name().map(|n| n.to_uppercase()).unwrap_or_default()));
        }
        if let Some(exif) = exif_facts(path) {
            facts.extend(exif);
        }
    } else if content_type.starts_with("audio/") || content_type.starts_with("video/") {
        facts.extend(ffprobe_facts(path));
    }
    facts
}

fn exif_facts(path: &Path) -> Option<Vec<(String, String)>> {
    let tool = if glib::find_program_in_path("magick").is_some() { "magick" } else if glib::find_program_in_path("identify").is_some() { "identify" } else { return None };
    let mut cmd = std::process::Command::new(tool);
    if tool == "magick" {
        cmd.arg("identify");
    }
    let out = cmd.args(["-format", "%[EXIF:Model]|%[EXIF:DateTimeOriginal]|%[EXIF:FNumber]|%[EXIF:ExposureTime]|%[EXIF:ISOSpeedRatings]|%[EXIF:FocalLength]"]).arg(path).stderr(std::process::Stdio::null()).output().ok()?;
    let raw = String::from_utf8_lossy(&out.stdout).to_string();
    let parts: Vec<&str> = raw.split('|').collect();
    if parts.len() < 6 || parts.iter().all(|p| p.trim().is_empty()) {
        return None;
    }
    let mut facts = Vec::new();
    let frac = |s: &str| -> String {
        match s.split_once('/') {
            Some((a, b)) => {
                let (a, b): (f64, f64) = (a.parse().unwrap_or(0.0), b.parse().unwrap_or(1.0));
                if b == 0.0 { s.to_string() } else { format!("{:.1}", a / b).trim_end_matches(".0").to_string() }
            }
            None => s.to_string(),
        }
    };
    if !parts[0].trim().is_empty() {
        facts.push(("Camera".into(), parts[0].trim().to_string()));
    }
    if parts[1].len() >= 19 {
        facts.push(("Taken".into(), parts[1][..19].replacen(':', "-", 2)));
    }
    let mut exposure = Vec::new();
    if !parts[2].trim().is_empty() {
        exposure.push(format!("f/{}", frac(parts[2].trim())));
    }
    if !parts[3].trim().is_empty() {
        exposure.push(format!("{} s", parts[3].trim()));
    }
    if !parts[4].trim().is_empty() {
        exposure.push(format!("ISO {}", parts[4].trim()));
    }
    if !parts[5].trim().is_empty() {
        exposure.push(format!("{} mm", frac(parts[5].trim())));
    }
    if !exposure.is_empty() {
        facts.push(("Exposure".into(), exposure.join("  ·  ")));
    }
    Some(facts)
}

fn path_is_audio(path: &Path) -> bool {
    let (ct, _) = gtk::gio::content_type_guess(Some(path), None);
    ct.starts_with("audio/")
}

fn ffprobe_facts(path: &Path) -> Vec<(String, String)> {
    let mut facts = Vec::new();
    if glib::find_program_in_path("ffprobe").is_none() {
        return facts;
    }
    let Ok(out) = std::process::Command::new("ffprobe")
        .args(["-v", "quiet", "-show_entries", "format=duration,bit_rate:stream=codec_type,codec_name,width,height,sample_rate,channels,r_frame_rate:format_tags=artist,title,album", "-of", "default"])
        .arg(path)
        .output()
    else {
        return facts;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut video: Option<(String, String, String, String)> = None;
    let mut audio: Option<(String, String, String)> = None;
    let mut cur_type = String::new();
    let mut cur: std::collections::HashMap<String, String> = Default::default();
    let flush = |t: &str, m: &std::collections::HashMap<String, String>, video: &mut Option<(String, String, String, String)>, audio: &mut Option<(String, String, String)>| {
        let g = |k: &str| m.get(k).cloned().unwrap_or_default();
        if t == "video" && video.is_none() {
            *video = Some((g("codec_name"), g("width"), g("height"), g("r_frame_rate")));
        } else if t == "audio" && audio.is_none() {
            *audio = Some((g("codec_name"), g("sample_rate"), g("channels")));
        }
    };
    let mut duration = String::new();
    let mut bitrate = String::new();
    let mut tags = Vec::new();
    for line in text.lines() {
        if line == "[STREAM]" {
            cur_type.clear();
            cur.clear();
            continue;
        }
        if line == "[/STREAM]" {
            if !cur_type.is_empty() {
                flush(&cur_type, &cur, &mut video, &mut audio);
            }
            cur_type.clear();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match k {
            "codec_type" => cur_type = v.to_string(),
            "duration" => duration = v.to_string(),
            "bit_rate" => bitrate = v.to_string(),
            k if k.starts_with("TAG:") => tags.push((k[4..].to_string(), v.to_string())),
            _ => {
                cur.insert(k.to_string(), v.to_string());
            }
        }
    }
    if let Ok(secs) = duration.parse::<f64>() {
        let s = secs.round() as u64;
        facts.push(("Duration".into(), if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }));
    }
    // Cover art inside audio files shows up as a video stream; not a fact worth a line.
    if let Some((codec, w, h, fps)) = video.filter(|_| !path_is_audio(path)) {
        let fps = fps.split_once('/').map(|(a, b)| { let (a, b): (f64, f64) = (a.parse().unwrap_or(0.0), b.parse().unwrap_or(1.0)); if b > 0.0 { format!("{:.0} fps", a / b) } else { String::new() } }).unwrap_or_default();
        facts.push(("Video".into(), format!("{w} × {h}  ·  {}  ·  {fps}", codec.to_uppercase()).trim_end_matches("  ·  ").to_string()));
    }
    if let Some((codec, rate, ch)) = audio {
        let khz = rate.parse::<f64>().map(|r| format!("{:.1} kHz", r / 1000.0)).unwrap_or_default();
        let chs = match ch.as_str() { "1" => "mono", "2" => "stereo", other => other };
        facts.push(("Audio".into(), format!("{}  ·  {khz}  ·  {chs}", codec.to_uppercase())));
    }
    if let Ok(b) = bitrate.parse::<u64>() {
        facts.push(("Bitrate".into(), format!("{} kbit/s", b / 1000)));
    }
    for (k, v) in tags {
        let k = k.to_lowercase();
        if ["artist", "title", "album"].contains(&k.as_str()) {
            let mut c = k.chars();
            facts.push((c.next().unwrap().to_uppercase().collect::<String>() + c.as_str(), v));
        }
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_structure() {
        let spans = markdown("# Title\n\n- **bold** item\n```\ncode\n```\n");
        assert_eq!(spans[0], ("Title\n".to_string(), "h1"));
        assert!(spans.iter().any(|(t, s)| t == "bold" && *s == "bold"));
        assert!(spans.iter().any(|(t, s)| t.contains("code") && *s == "code"));
    }

    #[test]
    fn csv_and_ansi() {
        let rows = csv_rows("a,b\n\"x, y\",2\n", 10);
        assert_eq!(rows[1], vec!["x, y", "2"]);
        let spans = ansi_spans("\u{1b}[1;31mred\u{1b}[0m plain \u{1b}[38;5;82mgreen\u{1b}[m");
        assert_eq!(spans[0].0, "red");
        assert!(spans[0].1.bold);
        assert_eq!(spans[0].1.fg, Some(ANSI16[1]));
        assert_eq!(spans[1].1, AnsiStyle::default());
        assert_eq!(spans[2].1.fg, Some(xterm256(82)));
    }
}
