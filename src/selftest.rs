//! The self-test: omafile runs every one of its commands against itself.
//!
//! With `OMAFILE_SELFTEST=1` the first window walks through all window actions, five
//! times over: with nothing selected, with a text file, with a picture, with a folder and
//! with a file whose name is full of signs that markup trips over.
//! After each one it closes whatever opened (dialogs, other windows, the page over the
//! files) and goes on. Every step is announced on stderr before it runs, so when
//! something crashes the last line names it. `scripts/selftest.sh` runs this in a
//! throwaway home folder with `G_DEBUG=fatal-criticals`, which turns every GTK critical
//! into a crash: the test passes when omafile gets to the end and quits by itself.
//!
//! It is a smoke test, not a proof: it shows that nothing falls over, not that every
//! command does the right thing. What a command does is the unit tests' business.

use crate::actions::{ACCELS, PALETTE_ACTIONS};
use crate::window::Window;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// Commands that are left out, and why: they end the test, leave the sandbox, wait for
/// something outside omafile, or start other programs whose presence is not omafile's to
/// test.
const SKIPPED: &[(&str, &str)] = &[
    ("quit", "ends the test"),
    ("close-tab", "closes the window when it is the last tab"),
    ("close-window", "ends the test"),
    ("new-window", "a second window would start a second test"),
    ("trash", "the next rounds need the files"),
    ("empty-trash", "not in a test"),
    ("git-discard", "throws work away"),
    ("vault-lock-all", "unmounts the user's vaults"),
    ("backup-now", "starts a backup"),
    ("import-photos", "waits for a phone"),
    ("nearby", "opens the network"),
    ("nearby-show-received", "opens the network"),
    ("share-lan", "starts a server"),
    ("open", "starts other programs"),
    ("open-with", "asks the desktop portal"),
    ("always-open-with", "asks the desktop portal"),
    ("open-editor", "starts an editor"),
    ("open-editor-cwd", "starts an editor"),
    ("open-workspace", "starts an editor"),
    ("terminal", "starts a terminal"),
    ("terminal-selected", "starts a terminal"),
    ("play", "starts a player"),
    ("radio-toggle", "starts the radio"),
    ("check-update", "asks the network"),
    ("ebook-reader", "starts a reader"),
    ("ebook-send", "starts a mail program"),
    ("set-wallpaper", "changes the desktop"),
    ("apply-theme", "changes the desktop"),
    ("install-theme", "changes the desktop"),
    ("export-theme", "writes into Downloads"),
    ("extract", "runs for as long as the archive is big"),
    ("compress", "runs for as long as the folder is big"),
];

/// Prefixes of commands that hand files to other tools (ImageMagick, ffmpeg, poppler,
/// Python): whether those are installed is not what this test is about.
const SKIPPED_PREFIXES: &[&str] = &["image-", "pdf-", "video-", "audio-", "py-"];

fn skipped(name: &str) -> bool {
    SKIPPED.iter().any(|(n, _)| *n == name) || SKIPPED_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// Every command the window has that takes no argument, in a fixed order: the ones the
/// command palette lists, then the ones that only have a shortcut.
pub fn commands() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in PALETTE_ACTIONS.iter().map(|(name, _, _)| name.to_string()).chain(ACCELS.iter().map(|(name, _)| name.trim_start_matches("win.").to_string())) {
        // `tag::red` and the like carry an argument; they are tried separately.
        if !name.contains("::") && !name.starts_with("app.") && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

fn other_windows(w: &Rc<Window>) -> usize {
    let main: gtk::Window = w.window.clone().upcast();
    gtk::Window::list_toplevels().into_iter().filter_map(|top| top.downcast::<gtk::Window>().ok()).filter(|other| *other != main && other.is_visible()).count()
}

/// Close what a command opened, the way a person would (Esc on a dialog, closing a
/// window), so the next one starts from the files again.
async fn settle(w: &Rc<Window>) {
    let trace = std::env::var_os("OMAFILE_SELFTEST_TRACE").is_some();
    for _ in 0..4 {
        let Some(dialog) = w.window.visible_dialog() else { break };
        // A dialog slides away; while it does it still counts as the visible one. It gets
        // the time it needs, and is asked again when it did not hear the first time (a
        // large dialog that was still being put on screen): forcing a dialog shut trips an
        // assertion inside libadwaita 1.5 a moment later, which would be this test's doing
        // and nobody else's. Only one that refuses to close (it asks first) is forced.
        // First it is allowed to arrive: a dialog asked to close while it still slides in
        // (nobody is that quick) may not hear it on libadwaita 1.5, and asking twice is
        // what confuses the library.
        glib::timeout_future(Duration::from_millis(320)).await;
        let mut gone = false;
        for attempt in 0..3 {
            if trace {
                eprintln!("selftest:   closing {} \"{}\"{} (mapped {}, can close {})", dialog.type_().name(), dialog.title(), if attempt > 0 { " again" } else { "" }, dialog.is_mapped(), dialog.can_close());
            }
            let asked = std::time::Instant::now();
            dialog.close();
            for _ in 0..60 {
                glib::timeout_future(Duration::from_millis(70)).await;
                if w.window.visible_dialog().as_ref() != Some(&dialog) {
                    gone = true;
                    if trace && asked.elapsed() > Duration::from_millis(600) {
                        eprintln!("selftest:   it took {} ms to go", asked.elapsed().as_millis());
                    }
                    break;
                }
            }
            if gone || !dialog.can_close() {
                break;
            }
        }
        if gone {
            // Gone from view is not yet done closing: a breath before the next command.
            glib::timeout_future(Duration::from_millis(180)).await;
        } else {
            if trace {
                eprintln!("selftest:   forcing {} \"{}\"", dialog.type_().name(), dialog.title());
            }
            dialog.force_close();
            glib::timeout_future(Duration::from_millis(600)).await;
        }
    }
    let main: gtk::Window = w.window.clone().upcast();
    for top in gtk::Window::list_toplevels() {
        if let Ok(other) = top.downcast::<gtk::Window>() {
            if other != main && other.is_visible() {
                other.close();
            }
        }
    }
    w.show_files();
}

thread_local! {
    static STARTED: Cell<bool> = const { Cell::new(false) };
}

impl Window {
    /// Start the self-test in this window when it was asked for (once per process).
    pub fn maybe_selftest(self: &Rc<Self>) {
        if std::env::var_os("OMAFILE_SELFTEST").is_none() || STARTED.with(|s| s.replace(true)) {
            return;
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            glib::timeout_future(Duration::from_millis(1500)).await;
            // OMAFILE_SELFTEST_SIZE=760x460 runs the test in a small window, the size of a tile
            // on a busy workspace: dialogs that do not fit say so.
            if let (Some(w), Some((width, height))) = (this.upgrade(), std::env::var("OMAFILE_SELFTEST_SIZE").ok().and_then(|s| s.split_once('x').and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))))) {
                w.window.set_default_size(width, height);
                glib::timeout_future(Duration::from_millis(400)).await;
            }
            // Every action the window has, not only the ones with a name in the palette:
            // the sidebar's and the context menus' too.
            let Some(w) = this.upgrade() else { return };
            eprintln!("selftest: window {}x{}", w.window.width(), w.window.height());
            let mut names: Vec<String> = gio::prelude::ActionGroupExt::list_actions(&w.window).iter().map(|n| n.to_string()).collect();
            names.sort();
            for listed in commands() {
                if !names.contains(&listed) {
                    eprintln!("selftest: MISSING {listed}: listed, but the window has no such action");
                    std::process::exit(3);
                }
            }
            drop(w);
            // To chase one failure: OMAFILE_SELFTEST_ONLY=help,help-about runs just those,
            // OMAFILE_SELFTEST_REPEAT=50 runs the whole list that many times.
            if let Some(only) = std::env::var("OMAFILE_SELFTEST_ONLY").ok().filter(|v| !v.trim().is_empty()) {
                let wanted: Vec<&str> = only.split(',').map(str::trim).collect();
                names.retain(|n| wanted.contains(&n.as_str()));
            }
            let repeat: usize = std::env::var("OMAFILE_SELFTEST_REPEAT").ok().and_then(|n| n.parse().ok()).unwrap_or(1);
            // OMAFILE_SELFTEST_ROUND=- (nothing selected) or a file's name repeats that one round.
            let one_round = std::env::var("OMAFILE_SELFTEST_ROUND").ok().filter(|v| !v.trim().is_empty());
            let all = [None, Some("notes.txt"), Some("picture.png"), Some("Folder"), Some("R&D <draft> 100%.txt")];
            let rounds: Vec<Option<&str>> = match one_round.as_deref() {
                Some("-") => vec![None; repeat],
                Some(name) => all.into_iter().filter(|r| *r == Some(name)).cycle().take(repeat).collect(),
                None => all.into_iter().cycle().take(5 * repeat).collect(),
            };
            let mut ran = 0;
            let Some(start) = this.upgrade().and_then(|w| w.active_pane()).map(|p| p.cwd()) else { return };
            // What is selected in each round: nothing, then one item by its name. The runner
            // puts these three into the folder the test starts in.
            for selection in rounds {
                for name in &names {
                    let Some(w) = this.upgrade() else { return };
                    if skipped(name) {
                        continue;
                    }
                    let Some(action) = w.window.lookup_action(name) else {
                        eprintln!("selftest: MISSING {name}: listed, but the window has no such action");
                        std::process::exit(3);
                    };
                    if action.parameter_type().is_some() || !action.is_enabled() {
                        continue;
                    }
                    // Each command starts from the same place: the folder the test began in (a
                    // command before may have gone elsewhere), with this round's selection.
                    if w.active_pane().is_some_and(|p| !p.cwd().equal(&start)) {
                        w.navigate(start.clone());
                        glib::timeout_future(Duration::from_millis(220)).await;
                    }
                    if let Some(pane) = w.active_pane() {
                        match selection.and_then(|n| pane.cwd().path().map(|dir| dir.join(n))).filter(|p| p.exists()) {
                            Some(path) => pane.select_file(&gio::File::for_path(path), true),
                            None if selection.is_some() => continue,
                            None => {}
                        }
                    }
                    eprintln!("selftest: {} {name}", selection.unwrap_or("-"));
                    gio::prelude::ActionGroupExt::activate_action(&w.window, name, None);
                    ran += 1;
                    glib::timeout_future(Duration::from_millis(90)).await;
                    // Another window (the system's file chooser, the shelf) gets the time to
                    // come up before it is closed again, as it would with a person at the keys.
                    if other_windows(&w) > 0 {
                        glib::timeout_future(Duration::from_millis(350)).await;
                    }
                    settle(&w).await;
                    glib::timeout_future(Duration::from_millis(if other_windows(&w) > 0 { 250 } else { 30 })).await;
                }
            }
            // The commands with an argument that are always there.
            for (name, value) in [("tag", "red"), ("tag", "none"), ("image-rotate", "90")] {
                let Some(w) = this.upgrade() else { return };
                if name != "image-rotate" {
                    eprintln!("selftest: {name}::{value}");
                    gio::prelude::ActionGroupExt::activate_action(&w.window, name, Some(&value.to_variant()));
                    ran += 1;
                    glib::timeout_future(Duration::from_millis(90)).await;
                    settle(&w).await;
                }
            }
            glib::timeout_future(Duration::from_millis(400)).await;
            eprintln!("selftest: ok, {ran} runs of {} commands, {} left out on purpose", names.iter().filter(|n| !skipped(n)).count(), names.iter().filter(|n| skipped(n)).count());
            if let Some(w) = this.upgrade() {
                if let Some(app) = w.window.application() {
                    app.quit();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_of_commands_is_whole_and_the_exceptions_are_real() {
        let names = commands();
        assert!(names.len() > 120, "{} commands", names.len());
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "no command twice");
        // Everything left out is an action that exists: a typo here would silently test less.
        // (The window's and the application's actions are registered in these two files.)
        let registered = [include_str!("actions.rs"), include_str!("main.rs")].concat();
        for (name, why) in SKIPPED {
            assert!(!why.is_empty());
            assert!(registered.contains(&format!("\"{name}\"")) || ["close-window", "new-window"].contains(name), "{name} is left out but is not an action");
        }
        assert!(skipped("video-gif") && skipped("trash") && !skipped("themes") && !skipped("tour") && !skipped("backup"));
        // Most commands are run: the exceptions stay exceptions.
        assert!(names.iter().filter(|n| skipped(n)).count() * 3 < names.len());
    }

    /// A toast shows a file's name as often as not, and a name may hold `&` or `<`. A toast
    /// that reads its text as markup would show nothing then, so every one is told not to.
    /// (Rows are different: there the text is escaped, because switching markup off when a
    /// row is built does not take in every libadwaita.)
    #[test]
    fn every_toast_takes_its_text_as_it_is() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut seen = 0;
        for entry in std::fs::read_dir(src).unwrap().flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            // Assembled here so that this test does not find itself.
            let builder = format!("adw::Toast::{}", "builder()");
            for (number, line) in text.lines().enumerate().filter(|(_, l)| l.contains(&builder)) {
                seen += 1;
                assert!(line.contains(&format!("{builder}.use_markup(false)")), "{}:{}: a toast that reads its text as markup", entry.path().display(), number + 1);
            }
        }
        assert!(seen >= 8, "the toasts were not found: {seen}");
    }
}
