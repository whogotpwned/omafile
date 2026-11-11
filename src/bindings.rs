//! Every binding in one place: a searchable overlay (`Ctrl+?` / `F1`), the
//! `--print-bindings` command line, and the test that keeps the list complete.
//! Keys for actions come from `actions::ACCELS`, so the overlay cannot drift
//! from what the window actually reacts to.

use crate::i18n::tr;
use crate::actions::ACCELS;
use adw::prelude::*;
use gtk::glib;

/// (group, action name without `win.` or empty, keys when not an action, what it does)
pub const TABLE: &[(&str, &str, &str, &str)] = &[
    ("Navigate", "back", "", "Back"),
    ("Navigate", "forward", "", "Forward"),
    ("Navigate", "up", "", "Parent folder"),
    ("Navigate", "", "BackSpace", "Parent folder"),
    ("Navigate", "home", "", "Home"),
    ("Navigate", "location", "", "Edit the path; Tab completes, ↑ ↓ pick, Enter opens"),
    ("Navigate", "reload", "", "Reload"),
    ("Navigate", "open", "", "Open the selection (Enter; with macOS keys Enter renames instead)"),
    ("Navigate", "open-new-tab", "", "Open folder in a new tab (also middle click)"),
    ("Navigate", "", "Double click", "Open; on a folder in the sidebar: go there"),
    ("Navigate", "", "Type a name", "Jump to the matching item (type-ahead)"),
    ("Search", "search", "", "Filter this folder"),
    ("Search", "deep-search", "", "Search all subfolders (fd, streamed)"),
    ("Search", "palette", "", "Command palette: actions, folders, project commands; > runs a shell command"),
    ("Search", "", "Escape", "Close the search, the palette or a dialog"),
    ("Tabs & panes", "new-tab", "", "New tab"),
    ("Tabs & panes", "close-tab", "", "Close tab"),
    ("Tabs & panes", "next-tab", "", "Next tab"),
    ("Tabs & panes", "prev-tab", "", "Previous tab"),
    ("Tabs & panes", "split-view", "", "Split view: two sets of tabs side by side"),
    ("Tabs & panes", "show-preview", "", "Preview pane"),
    ("Tabs & panes", "show-sidebar", "", "Sidebar"),
    ("Tabs & panes", "terminal-panel", "", "Terminal panel below the files"),
    ("Tabs & panes", "", "Ctrl+Shift+C / Ctrl+Shift+V", "Copy / paste inside the terminal panel"),
    ("Tabs & panes", "", "Drag a tab", "Reorder, or move it to the other side of the split"),
    ("View", "view::list", "", "List view"),
    ("View", "view::grid", "", "Grid view"),
    ("View", "view::columns", "", "Column view (Miller columns); ← → move between columns"),
    ("View", "view::gallery", "", "Gallery view: big preview with a filmstrip"),
    ("View", "zoom-in", "", "Bigger icons in the grid (also Ctrl+scroll)"),
    ("View", "zoom-out", "", "Smaller icons in the grid"),
    ("View", "zoom-reset", "", "Default icon size"),
    ("View", "", "Menu › Group By", "Sections by kind, date, size or tag"),
    ("Navigate", "up-alt", "", "Parent folder (macOS habit)"),
    ("Files", "new-folder-with-selection", "", "New folder with the selected items inside"),
    ("Files", "", "Context menu › Make Alias / Show Original", "Symbolic links and where they point"),
    ("Files", "", "Context menu › Always Open With…", "Pick the default app for this file type"),
    ("Files", "", "Context menu › PDF / Video / Audio", "Merge, split, extract pages; trim, GIF, extract or convert audio"),
    ("Files", "", "Drag over a folder", "Spring-loaded: it opens after a moment (also sidebar places)"),
    ("Search", "", "Content toggle in the search bar", "Full-text search with ripgrep"),
    ("View", "show-hidden", "", "Show or hide hidden files (eye button)"),
    ("View", "folder-sizes", "", "Measure folder sizes and sort by disk usage"),
    ("View", "", "Click a column header", "Sort by that column"),
    ("View", "", "Space", "Quick Look; on music it plays. ← → walk the folder, ↑ ↓ turn the pages of a PDF, Space or Esc closes"),
    ("View", "", "Alt+PgDn / Alt+PgUp", "Next / previous page of a PDF in the preview pane (also the wheel over the page, or the ‹ › buttons)"),
    ("View", "", "Click the play badge", "Play a song or video"),
    ("Files", "new-folder", "", "New folder"),
    ("Files", "new-file", "", "New file"),
    ("Files", "rename", "", "Rename in place; batch rename with several selected"),
    ("Files", "rename-dialog", "", "Rename in a dialog"),
    ("Files", "copy", "", "Copy"),
    ("Files", "cut", "", "Cut"),
    ("Files", "paste", "", "Paste (conflicts ask: replace, skip, keep both)"),
    ("Files", "select-all", "", "Select all"),
    ("Files", "copy-path", "", "Copy path"),
    ("Files", "duplicate", "", "Duplicate"),
    ("Files", "undo", "", "Undo the last file operation"),
    ("Files", "redo", "", "Redo"),
    ("Files", "", "Delete", "Move to trash"),
    ("Files", "delete", "", "Delete permanently"),
    ("Files", "restore", "", "Restore from the trash"),
    ("Files", "", "Drag & drop", "Move within the same disk, copy elsewhere; Ctrl forces copy"),
    ("Files", "", "Right click", "Context menu: Git, Image, Share, Tag, Compress, Change Icon…"),
    ("Files", "copy-other", "", "Copy the selection to the other side of the split"),
    ("Files", "move-other", "", "Move the selection to the other side of the split"),
    ("Files", "compare", "", "Compare two files or folders (or one on each side of the split)"),
    ("Files", "edit-note", "", "Note on the file (searchable in the palette)"),
    ("Files", "", "Context menu › Verify Checksum", "SHA-256 against a .sha256 file or the clipboard"),
    ("Files", "", "Context menu › New from Template", "~/Templates entries and built-in scaffolds"),
    ("Files", "", "Enter on an archive", "Browse inside it (GVFS); Extract Here stays in the menu"),
    ("Search", "save-smart", "", "Save the current search as a smart folder in the sidebar"),
    ("Search", "", "Ctrl+K then rename *.jpeg *.jpg", "Bulk commands with preview: rename, chmod, touch, mkdir"),
    ("Files", "properties", "", "Properties (size, SHA-256, note)"),
    ("Files", "open-with", "", "Open with…"),
    ("Files", "change-icon", "", "Change icon"),
    ("Files", "bookmark", "", "Bookmark this folder"),
    ("Tags", "tag::red", "", "Tag red"),
    ("Tags", "tag::orange", "", "Tag orange"),
    ("Tags", "tag::yellow", "", "Tag yellow"),
    ("Tags", "tag::green", "", "Tag green"),
    ("Tags", "tag::blue", "", "Tag blue"),
    ("Tags", "tag::purple", "", "Tag purple"),
    ("Tags", "tag::gray", "", "Tag gray"),
    ("Tags", "tag::none", "", "Remove tag"),
    ("Developer", "open-editor", "", "Open selection in the editor"),
    ("Developer", "open-editor-cwd", "", "Open folder in the editor"),
    ("Developer", "terminal", "", "Open a terminal window here"),
    ("Developer", "git-commit", "", "Git: commit"),
    ("Developer", "git-branch", "", "Git: switch or create a branch (also the branch button)"),
    ("Developer", "", "Click a git badge", "Stage or unstage that file"),
    ("Developer", "", "Context menu › Project", "cargo, npm, make, docker compose… in a terminal"),
    ("Developer", "", "Context menu › Git", "Stage, unstage, discard, commit"),
    ("Developer", "py-activate", "", "Python: activate the project's venv in the terminal panel"),
    ("Developer", "", "Context menu › Python", "Create venv (uv), install requirements, freeze, REPL, .gitignore, recreate"),
    ("Developer", "", "Run in venv", "Context menu on a .py file: runs it with the venv's interpreter"),
    ("Developer", "", "Context menu › New Encrypted Folder", "gocryptfs vault; Unlock, Lock, Change Password on the folder"),
    ("Tools", "versions", "", "Earlier versions of the selected file or folder from Btrfs snapshots: compare, restore a copy or in place, bring back deleted files; turns snapshots on for the home folder"),
    ("Tools", "feeds", "", "Feeds: read RSS and Atom news where the files are (Add Feed… in the sidebar); in the reader: ↑ ↓ or J K next and previous article, O opens the page in the browser, M marks unread, R fetches, Space scrolls, Esc back"),
    ("Remote", "", "Sidebar › Remote", "Saved connections open beside the local files (local left, server right); copy or move across with the header arrows, Shift+F5 / Shift+F6 or by dragging; passwords stay in the system keyring"),
    ("Developer", "", "Sidebar › Vaults", "Encrypted folders with their own master password: Add Vault… creates or lists one, a click shows the padlock and asks for the password, the lock button closes it"),
    ("Remote", "connect", "", "Connect to a server (SFTP, SMB, …)"),
    ("Remote", "", "Sidebar › Remote", "Hosts from ~/.ssh/config; right click to disconnect"),
    ("Tools", "treemap", "", "Disk usage map of this folder: click zooms, double click opens, right click trashes"),
    ("Tools", "find-duplicates", "", "Find duplicates below this folder (same size and SHA-256), trash the extra copies"),
    ("Tools", "rules", "", "Folder rules for this folder: move, copy, tag, trash, sort by month, run a command"),
    ("Tools", "shelf-add", "", "Put the selection on the shelf (a floating window that collects files; drag out or copy / move them in one go)"),
    ("Tools", "shelf-show", "", "Show or hide the shelf"),
    ("Tools", "nearby", "", "Nearby devices (LocalSend): send the selection to a phone or computer, receive files; also drop files on a device in the sidebar"),
    ("Tools", "now-playing", "", "Now Playing: whatever plays on this computer (browser, Spotify, mpv, …) with cover, live spectrum and full controls; in the view: Space play / pause, ← → seek, ↑ ↓ volume, N / P next / previous, Esc back"),
    ("Tools", "", "Menu › Import Photos from iPhone", "Import the camera roll of a connected iPhone / iPad into Pictures/iPhone/YYYY-MM (HEIC → JPEG); also omafile --import-photos"),
    ("Tools", "", "Context menu › eBook", "Open in Reader, Rename from Metadata (Author - Title), Send to eReader, Convert to EPUB / MOBI / AZW3 / PDF (Calibre)"),
    ("Vim keys", "", "h  j  k  l", "Parent folder, down, up, open (Preferences › Keys › Vim keys)"),
    ("Vim keys", "", "gg  G", "First and last item"),
    ("Vim keys", "", "/  :", "Filter this folder, command palette"),
    ("Vim keys", "", "dd  yy  p  x  u", "Trash, copy, paste, cut, undo; each group can be switched off"),
    ("App", "preferences", "", "Preferences"),
    ("App", "help", "", "Help: what omafile can do, every shortcut, the version and what is new in it"),
    ("App", "shortcuts", "", "Keyboard shortcuts (this list)"),
    ("App", "", "Ctrl+Q", "Quit"),
];

#[derive(Clone, Debug)]
pub struct Binding {
    pub group: String,
    pub keys: String,
    pub what: String,
}

/// Human-readable shortcut, e.g. `Ctrl+Shift+N`.
pub fn pretty(accel: &str) -> String {
    let mut out = accel
        .replace("<Control>", "Ctrl+")
        .replace("<Shift>", "Shift+")
        .replace("<Alt>", "Alt+")
        .replace("<Super>", "Super+");
    let key = out.rsplit('+').next().unwrap_or("").to_string();
    let nice = match key.as_str() {
        "comma" => ",".to_string(),
        "question" => "?".to_string(),
        "grave" => "`".to_string(),
        "Return" => tr("Enter"),
        "Page_Down" => tr("PgDn"),
        "Page_Up" => tr("PgUp"),
        "Left" => "←".to_string(),
        "Right" => "→".to_string(),
        "Up" => "↑".to_string(),
        "Down" => "↓".to_string(),
        k if k.len() == 1 => k.to_uppercase(),
        k => k.to_string(),
    };
    let cut = out.len() - key.len();
    out.truncate(cut);
    out.push_str(&nice);
    out
}

pub fn accels_for(action: &str) -> Vec<String> {
    let full = format!("win.{action}");
    ACCELS.iter().find(|(a, _)| *a == full).map(|(_, keys)| keys.iter().map(|k| pretty(k)).collect()).unwrap_or_default()
}

pub fn all() -> Vec<Binding> {
    let mut list: Vec<Binding> = TABLE
        .iter()
        .map(|(group, action, keys, what)| {
            // Key names stay as they are; gestures and menu paths are sentences and get translated.
            let keys = if action.is_empty() { if keys.contains(' ') { tr(keys) } else { keys.to_string() } } else { accels_for(action).join("  /  ") };
            Binding { group: tr(group), keys, what: tr(what) }
        })
        .collect();
    if let Some(launch) = omarchy_launch_binding() {
        list.insert(0, Binding { group: tr("Omarchy"), keys: launch, what: tr("Open omafile (from your Hyprland bindings)") });
    }
    list
}

/// The Hyprland binding that launches omafile, read from Omarchy's bindings files.
pub fn omarchy_launch_binding() -> Option<String> {
    let hypr = glib::home_dir().join(".config/hypr");
    for name in ["bindings.lua", "bindings.conf"] {
        let Ok(text) = std::fs::read_to_string(hypr.join(name)) else { continue };
        for line in text.lines().filter(|l| l.contains("omafile") && !l.trim_start().starts_with('#') && !l.trim_start().starts_with("--")) {
            if let Some(keys) = parse_binding_line(line) {
                return Some(keys);
            }
        }
    }
    None
}

/// `o.rebind("SUPER + SHIFT + F", …)` or `bindd = SUPER SHIFT, F, …` → `Super+Shift+F`.
pub fn parse_binding_line(line: &str) -> Option<String> {
    let title = |s: &str| {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let mut c = s.chars();
        Some(c.next()?.to_uppercase().collect::<String>() + &c.as_str().to_lowercase())
    };
    if let Some(start) = line.find("rebind(\"").or_else(|| line.find("bind(\"")) {
        let rest = &line[start..];
        let open = rest.find('"')? + 1;
        let close = rest[open..].find('"')? + open;
        let combo = &rest[open..close];
        let parts: Vec<String> = combo.split('+').filter_map(title).collect();
        return if parts.is_empty() { None } else { Some(parts.join("+")) };
    }
    if line.trim_start().starts_with("bind") {
        let (_, rhs) = line.split_once('=')?;
        let mut fields = rhs.split(',');
        let mods: Vec<String> = fields.next()?.split_whitespace().filter_map(title).collect();
        let key = title(fields.next()?)?;
        let mut parts = mods;
        parts.push(key);
        return Some(parts.join("+"));
    }
    None
}

/// `omafile --print-bindings`: one line per binding, tab separated, for launchers.
pub fn print() {
    for b in all() {
        if !b.keys.is_empty() {
            println!("{}\t{}\t{}", b.keys, b.what, b.group);
        }
    }
}

/// The list of every binding, grouped, ready to be filtered with `filter_rows`.
pub fn bindings_list() -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.add_css_class("omafile-bindings");
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    for b in all() {
        let row = gtk::ListBoxRow::new();
        row.set_activatable(false);
        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        hbox.set_margin_top(7);
        hbox.set_margin_bottom(7);
        hbox.set_margin_start(12);
        hbox.set_margin_end(12);
        let what = gtk::Label::builder().label(&b.what).xalign(0.0).hexpand(true).wrap(true).build();
        hbox.append(&what);
        let keys = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        keys.set_valign(gtk::Align::Center);
        for alt in b.keys.split("  /  ") {
            if alt.is_empty() {
                continue;
            }
            let cap = gtk::Label::new(Some(alt));
            cap.add_css_class("omafile-keycap");
            keys.append(&cap);
        }
        hbox.append(&keys);
        row.set_child(Some(&hbox));
        // Searchable text and group live on the row for the filter and header functions.
        unsafe {
            row.set_data("group", Box::leak(b.group.clone().into_boxed_str()) as &'static str);
            row.set_data("haystack", format!("{} {} {}", b.group, b.keys, b.what).to_lowercase());
        }
        list.append(&row);
    }
    list.set_header_func(|row, before| {
        let group: &str = unsafe { row.data::<&'static str>("group").map(|p| *p.as_ref()).unwrap_or("") };
        let prev: Option<&str> = before.map(|b| unsafe { b.data::<&'static str>("group").map(|p| *p.as_ref()).unwrap_or("") });
        if prev == Some(group) {
            row.set_header(None::<&gtk::Widget>);
        } else {
            let label = gtk::Label::builder().label(group).xalign(0.0).margin_start(12).margin_top(14).margin_bottom(4).build();
            label.add_css_class("caption-heading");
            label.add_css_class("omafile-dim");
            row.set_header(Some(&label));
        }
    });
    list
}

/// Show only the rows of a list built here (or in `help`) that contain every word of `needle`.
pub fn filter_rows(list: &gtk::ListBox, needle: &str) {
    let words: Vec<String> = needle.to_lowercase().split_whitespace().map(str::to_string).collect();
    list.set_filter_func(move |row| {
        if words.is_empty() {
            return true;
        }
        let hay: String = unsafe { row.data::<String>("haystack").map(|p| p.as_ref().clone()).unwrap_or_default() };
        words.iter().all(|w| hay.contains(w))
    });
    list.invalidate_headers();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_accelerator_is_listed() {
        for (action, _) in ACCELS {
            let short = action.trim_start_matches("win.");
            assert!(TABLE.iter().any(|(_, a, _, _)| *a == short), "{action} has a shortcut but no entry in the bindings list");
        }
    }

    #[test]
    fn listed_actions_have_shortcuts() {
        for (_, action, keys, _) in TABLE {
            if !action.is_empty() {
                assert!(!accels_for(action).is_empty(), "{action} is listed as an action but has no accelerator");
            } else {
                assert!(!keys.is_empty());
            }
        }
    }

    #[test]
    fn pretty_accels() {
        assert_eq!(pretty("<Control><Shift>n"), "Ctrl+Shift+N");
        assert_eq!(pretty("<Control>grave"), "Ctrl+`");
        assert_eq!(pretty("<Alt>Left"), "Alt+←");
        assert_eq!(pretty("F5"), "F5");
        assert_eq!(pretty("<Control>question"), "Ctrl+?");
    }

    #[test]
    fn parses_omarchy_binding_lines() {
        assert_eq!(parse_binding_line(r#"o.rebind("SUPER + SHIFT + F", "File manager", { launch = "omafile" })"#).as_deref(), Some("Super+Shift+F"));
        assert_eq!(parse_binding_line("bindd = SUPER SHIFT, F, File manager, exec, uwsm-app -- omafile").as_deref(), Some("Super+Shift+F"));
        assert_eq!(parse_binding_line("# bindd = SUPER, F, …"), None);
    }
}
