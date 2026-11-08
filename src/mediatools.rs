//! Quick actions on documents and media: images → PDF, merge / split PDFs,
//! extract pages; trim video, video → GIF, extract audio, convert audio.
//! Outputs are new files next to the originals, recorded for undo.

use crate::item::FileItem;
use crate::ops;
use crate::undo::Op;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn magick() -> Option<&'static str> {
    crate::imagetools::magick()
}

fn has(tool: &str) -> bool {
    glib::find_program_in_path(tool).is_some()
}

fn out_path(dir: &Path, name: &str) -> PathBuf {
    ops::unique_child(&gio::File::for_path(dir), name).path().unwrap()
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "output".into())
}

impl Window {
    /// Run `argv` (first element the program), then record `outputs` as created.
    fn run_tool(self: &Rc<Self>, argv: Vec<String>, outputs: Vec<PathBuf>, verb: &'static str) {
        let Some(pane) = self.active_pane() else { return };
        let this = Rc::downgrade(self);
        self.toast(&format!("{verb}…"));
        glib::spawn_future_local(async move {
            let args: Vec<&str> = argv.iter().map(String::as_str).collect();
            let result = ops::run_command(&args, None).await;
            let Some(w) = this.upgrade() else { return };
            pane.reload();
            match result {
                Ok(()) => {
                    let files: Vec<gio::File> = outputs.iter().filter(|p| p.exists()).map(gio::File::for_path).collect();
                    if let Some(last) = files.last() {
                        pane.select_file_later(last.clone());
                    }
                    let n = files.len();
                    w.record(Op::Derive(files), &format!("{verb}: {n} {}", if n == 1 { "file" } else { "files" }));
                }
                Err(e) => w.toast_error(&format!("{verb} failed: {}", e.message())),
            }
        });
    }

    fn selected_paths_where(&self, pred: impl Fn(&FileItem) -> bool) -> Vec<PathBuf> {
        self.selected_items().iter().filter(|i| pred(i)).filter_map(|i| i.target_file().path()).collect()
    }

    // ----- PDF -------------------------------------------------------------------

    /// Selected images → one PDF, in selection (sorted) order.
    pub fn pdf_from_images(self: &Rc<Self>) {
        let mut imgs = self.selected_paths_where(|i| i.is_image());
        imgs.sort();
        let (Some(first), Some(tool)) = (imgs.first().cloned(), magick()) else {
            self.toast_error("ImageMagick is needed to build PDFs");
            return;
        };
        let dir = first.parent().unwrap_or(Path::new(".")).to_path_buf();
        let name = if imgs.len() == 1 { format!("{}.pdf", stem(&first)) } else { format!("{}.pdf", dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or("images".into())) };
        let out = out_path(&dir, &name);
        let mut argv = vec![tool.to_string()];
        argv.extend(imgs.iter().map(|p| p.to_string_lossy().to_string()));
        argv.extend(["-auto-orient".into(), "-units".into(), "PixelsPerInch".into(), "-density".into(), "150".into(), out.to_string_lossy().to_string()]);
        self.run_tool(argv, vec![out], "Create PDF");
    }

    pub fn pdf_merge(self: &Rc<Self>) {
        let mut pdfs = self.selected_paths_where(|i| i.is_pdf());
        pdfs.sort();
        if pdfs.len() < 2 {
            self.toast("Select two or more PDFs to merge");
            return;
        }
        if !has("pdfunite") {
            self.toast_error("pdfunite (poppler) is not installed");
            return;
        }
        let dir = pdfs[0].parent().unwrap_or(Path::new(".")).to_path_buf();
        let out = out_path(&dir, &format!("{} (merged).pdf", stem(&pdfs[0])));
        let mut argv = vec!["pdfunite".to_string()];
        argv.extend(pdfs.iter().map(|p| p.to_string_lossy().to_string()));
        argv.push(out.to_string_lossy().to_string());
        self.run_tool(argv, vec![out], "Merge PDFs");
    }

    /// Every page of the selected PDF as its own file (`name-1.pdf`, …).
    pub fn pdf_split(self: &Rc<Self>) {
        let Some(pdf) = self.selected_paths_where(|i| i.is_pdf()).into_iter().next() else { return };
        if !has("pdfseparate") {
            self.toast_error("pdfseparate (poppler) is not installed");
            return;
        }
        let dir = pdf.parent().unwrap_or(Path::new(".")).to_path_buf();
        let folder = out_path(&dir, &format!("{} pages", stem(&pdf)));
        let _ = std::fs::create_dir_all(&folder);
        let pattern = folder.join(format!("{}-%d.pdf", stem(&pdf)));
        self.run_tool(vec!["pdfseparate".into(), pdf.to_string_lossy().to_string(), pattern.to_string_lossy().to_string()], vec![folder], "Split PDF");
    }

    /// Ask for a page range, write it as a new PDF.
    pub fn pdf_extract_dialog(self: &Rc<Self>) {
        let Some(pdf) = self.selected_paths_where(|i| i.is_pdf()).into_iter().next() else { return };
        if !has("pdfseparate") || !has("pdfunite") {
            self.toast_error("poppler (pdfseparate, pdfunite) is not installed");
            return;
        }
        let pages = crate::thumbs::pdf_pages(&pdf).unwrap_or(0);
        let entry = gtk::Entry::builder().text("1-3").activates_default(true).build();
        let dialog = adw::AlertDialog::new(Some("Extract pages"), Some(&format!("Pages to keep, e.g. 2-5 or 1,4,7{}", if pages > 0 { format!(" (the document has {pages})") } else { String::new() })));
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancel"), ("extract", "Extract")]);
        dialog.set_response_appearance("extract", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("extract"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let entry2 = entry.clone();
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |r| {
            let Some(w) = this.upgrade() else { return };
            w.refocus();
            if r != "extract" {
                return;
            }
            let Some(list) = parse_pages(&entry2.text(), pages as usize) else {
                w.toast_error("Could not read that page range");
                return;
            };
            let dir = pdf.parent().unwrap_or(Path::new(".")).to_path_buf();
            let out = out_path(&dir, &format!("{} (pages {}).pdf", stem(&pdf), entry2.text().trim()));
            let tmp = std::env::temp_dir().join(format!("omafile-pages-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&tmp);
            // One shell line: separate the wanted pages, unite them, clean up.
            let parts: Vec<String> = list.iter().map(|p| tmp.join(format!("p{p}.pdf")).to_string_lossy().to_string()).collect();
            let mut script = String::new();
            for p in &list {
                script.push_str(&format!("pdfseparate -f {p} -l {p} '{}' '{}' && ", pdf.to_string_lossy(), tmp.join(format!("p{p}.pdf")).to_string_lossy()));
            }
            script.push_str(&format!("pdfunite {} '{}'; rm -rf '{}'", parts.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(" "), out.to_string_lossy(), tmp.to_string_lossy()));
            w.run_tool(vec!["bash".into(), "-c".into(), script], vec![out], "Extract pages");
        });
        entry.grab_focus();
    }

    // ----- video / audio ----------------------------------------------------------

    pub fn video_trim_dialog(self: &Rc<Self>) {
        let Some(path) = self.selected_paths_where(|i| i.is_media()).into_iter().next() else { return };
        if !has("ffmpeg") {
            self.toast_error("ffmpeg is not installed");
            return;
        }
        let start = gtk::Entry::builder().text("00:00:00").activates_default(true).build();
        let end = gtk::Entry::builder().text("00:00:10").activates_default(true).build();
        let grid = gtk::Grid::builder().row_spacing(8).column_spacing(12).build();
        grid.attach(&gtk::Label::builder().label("From").xalign(0.0).build(), 0, 0, 1, 1);
        grid.attach(&start, 1, 0, 1, 1);
        grid.attach(&gtk::Label::builder().label("To").xalign(0.0).build(), 0, 1, 1, 1);
        grid.attach(&end, 1, 1, 1, 1);
        let dialog = adw::AlertDialog::new(Some("Trim"), Some("hh:mm:ss or seconds. The streams are copied, so it is instant and lossless; cuts land on the nearest keyframe."));
        dialog.set_extra_child(Some(&grid));
        dialog.add_responses(&[("cancel", "Cancel"), ("trim", "Trim")]);
        dialog.set_response_appearance("trim", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("trim"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        let (start2, end2) = (start.clone(), end.clone());
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |r| {
            let Some(w) = this.upgrade() else { return };
            w.refocus();
            if r != "trim" {
                return;
            }
            let (from, to) = (start2.text().trim().to_string(), end2.text().trim().to_string());
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or("mp4".into());
            let out = out_path(&dir, &format!("{} (trimmed).{ext}", stem(&path)));
            w.run_tool(vec!["ffmpeg".into(), "-v".into(), "error".into(), "-ss".into(), from, "-to".into(), to, "-i".into(), path.to_string_lossy().to_string(), "-c".into(), "copy".into(), out.to_string_lossy().to_string()], vec![out], "Trim");
        });
        start.grab_focus();
    }

    /// First 10 s as a palette-optimised GIF at 480 px.
    pub fn video_to_gif(self: &Rc<Self>) {
        let Some(path) = self.selected_paths_where(|i| i.is_video()).into_iter().next() else { return };
        if !has("ffmpeg") {
            self.toast_error("ffmpeg is not installed");
            return;
        }
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let out = out_path(&dir, &format!("{}.gif", stem(&path)));
        let filter = "fps=12,scale=480:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse";
        self.run_tool(vec!["ffmpeg".into(), "-v".into(), "error".into(), "-t".into(), "10".into(), "-i".into(), path.to_string_lossy().to_string(), "-vf".into(), filter.into(), out.to_string_lossy().to_string()], vec![out], "Make GIF");
    }

    /// Audio track out of a video, container-matched (m4a for AAC, else the codec's extension).
    pub fn video_extract_audio(self: &Rc<Self>) {
        let Some(path) = self.selected_paths_where(|i| i.is_video()).into_iter().next() else { return };
        if !has("ffmpeg") {
            self.toast_error("ffmpeg is not installed");
            return;
        }
        let codec = std::process::Command::new("ffprobe").args(["-v", "quiet", "-select_streams", "a:0", "-show_entries", "stream=codec_name", "-of", "csv=p=0"]).arg(&path).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        let ext = match codec.as_str() {
            "aac" => "m4a",
            "mp3" => "mp3",
            "opus" => "opus",
            "vorbis" => "ogg",
            "flac" => "flac",
            _ => "mka",
        };
        let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let out = out_path(&dir, &format!("{}.{ext}", stem(&path)));
        self.run_tool(vec!["ffmpeg".into(), "-v".into(), "error".into(), "-i".into(), path.to_string_lossy().to_string(), "-vn".into(), "-c:a".into(), "copy".into(), out.to_string_lossy().to_string()], vec![out], "Extract audio");
    }

    /// Convert selected audio (or the audio of videos) to mp3 / opus / flac.
    pub fn audio_convert(self: &Rc<Self>, format: &str) {
        let paths = self.selected_paths_where(|i| i.is_media());
        if paths.is_empty() || !has("ffmpeg") {
            self.toast_error("ffmpeg is not installed");
            return;
        }
        let (ext, codec_args): (&str, Vec<&str>) = match format {
            "mp3" => ("mp3", vec!["-c:a", "libmp3lame", "-q:a", "2"]),
            "opus" => ("opus", vec!["-c:a", "libopus", "-b:a", "128k"]),
            _ => ("flac", vec!["-c:a", "flac"]),
        };
        let mut script = String::new();
        let mut outs = Vec::new();
        for p in &paths {
            let dir = p.parent().unwrap_or(Path::new(".")).to_path_buf();
            let out = out_path(&dir, &format!("{}.{ext}", stem(p)));
            script.push_str(&format!("ffmpeg -v error -i '{}' -vn {} '{}' && ", p.to_string_lossy().replace('\'', "'\\''"), codec_args.join(" "), out.to_string_lossy()));
            outs.push(out);
        }
        script.push_str("true");
        self.run_tool(vec!["bash".into(), "-c".into(), script], outs, "Convert audio");
    }
}

/// `1-3,7` → [1,2,3,7]; `0` or beyond `max` (when known) is rejected.
pub fn parse_pages(text: &str, max: usize) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b): (usize, usize) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
                if a == 0 || b < a {
                    return None;
                }
                out.extend(a..=b);
            }
            None => {
                let n: usize = part.parse().ok()?;
                if n == 0 {
                    return None;
                }
                out.push(n);
            }
        }
    }
    if out.is_empty() || (max > 0 && out.iter().any(|p| *p > max)) {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ranges() {
        assert_eq!(parse_pages("1-3,7", 10), Some(vec![1, 2, 3, 7]));
        assert_eq!(parse_pages("0", 10), None);
        assert_eq!(parse_pages("12", 10), None);
        assert_eq!(parse_pages("2", 0), Some(vec![2]));
    }
}
