//! Application state and key handling.

use crate::fs::{self, Entry, SortMode};
use crate::preview::{self, Preview};
use crate::theme::{Theme, ThemeWatcher};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const PREVIEW_LINES: usize = 200;

#[derive(Clone, Debug)]
pub enum InputKind {
    Rename(PathBuf),
    NewFile,
    NewDir,
}

#[derive(Clone, Debug)]
pub enum ConfirmAction {
    Delete { paths: Vec<PathBuf>, permanent: bool },
}

pub enum Mode {
    Normal,
    Filter,
    Input { kind: InputKind, buf: String },
    Confirm { prompt: String, action: ConfirmAction },
    Help,
}

/// Things the main loop must run with the terminal handed back to the user.
pub enum External {
    Editor(PathBuf),
    Shell,
}

pub struct Clipboard {
    pub paths: Vec<PathBuf>,
    pub cut: bool,
}

pub struct Message {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

pub struct App {
    pub cwd: PathBuf,
    pub all_entries: Vec<Entry>,
    pub entries: Vec<Entry>,
    pub cursor: usize,
    pub list_state: ListState,
    pub parent_entries: Vec<Entry>,
    pub parent_cursor: Option<usize>,
    pub preview: Preview,
    pub show_hidden: bool,
    pub sort: SortMode,
    pub filter: String,
    pub mode: Mode,
    pub clipboard: Option<Clipboard>,
    pub marked: BTreeSet<PathBuf>,
    pub message: Option<Message>,
    pub history: Vec<PathBuf>,
    pub theme: Theme,
    theme_watcher: ThemeWatcher,
    pub icons: bool,
    pub should_quit: bool,
    pub external: Option<External>,
    pub cwd_file: Option<PathBuf>,
    pub page_size: usize,
    pending_g: bool,
}

impl App {
    pub fn new(start: PathBuf, icons: bool, cwd_file: Option<PathBuf>) -> App {
        let mut theme_watcher = ThemeWatcher::new();
        let theme = theme_watcher.load();
        let mut app = App {
            cwd: start,
            all_entries: Vec::new(),
            entries: Vec::new(),
            cursor: 0,
            list_state: ListState::default(),
            parent_entries: Vec::new(),
            parent_cursor: None,
            preview: Preview::Empty,
            show_hidden: false,
            sort: SortMode::Name,
            filter: String::new(),
            mode: Mode::Normal,
            clipboard: None,
            marked: BTreeSet::new(),
            message: None,
            history: Vec::new(),
            theme,
            theme_watcher,
            icons,
            should_quit: false,
            external: None,
            cwd_file,
            page_size: 20,
            pending_g: false,
        };
        app.reload(None);
        app
    }

    // ----- state helpers ---------------------------------------------------

    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), error: false, at: Instant::now() });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), error: true, at: Instant::now() });
    }

    /// Called on every tick: expire messages, pick up theme switches.
    pub fn tick(&mut self) {
        if let Some(m) = &self.message {
            let ttl = if m.error { Duration::from_secs(6) } else { Duration::from_secs(3) };
            if m.at.elapsed() > ttl {
                self.message = None;
            }
        }
        if self.theme_watcher.changed() {
            self.theme = self.theme_watcher.load();
        }
    }

    /// Re-read the current directory. `select` names the entry to put the cursor on.
    pub fn reload(&mut self, select: Option<&Path>) {
        let keep = select
            .map(Path::to_path_buf)
            .or_else(|| self.current().map(|e| e.path.clone()));
        match fs::read_dir(&self.cwd, self.show_hidden, self.sort) {
            Ok(entries) => self.all_entries = entries,
            Err(e) => {
                self.all_entries.clear();
                self.error(format!("{}: {}", fs::display_path(&self.cwd), e));
            }
        }
        self.apply_filter();
        if let Some(path) = keep {
            if let Some(i) = self.entries.iter().position(|e| e.path == path) {
                self.cursor = i;
            }
        }
        self.clamp_cursor();
        self.load_parent();
        self.refresh_preview();
    }

    fn apply_filter(&mut self) {
        let needle = self.filter.to_lowercase();
        self.entries = if needle.is_empty() {
            self.all_entries.clone()
        } else {
            self.all_entries
                .iter()
                .filter(|e| e.name.to_lowercase().contains(&needle))
                .cloned()
                .collect()
        };
    }

    fn clamp_cursor(&mut self) {
        if self.entries.is_empty() {
            self.cursor = 0;
            self.list_state.select(None);
        } else {
            self.cursor = self.cursor.min(self.entries.len() - 1);
            self.list_state.select(Some(self.cursor));
        }
    }

    fn load_parent(&mut self) {
        match self.cwd.parent() {
            Some(parent) => {
                self.parent_entries =
                    fs::read_dir(parent, self.show_hidden, self.sort).unwrap_or_default();
                self.parent_cursor = self.parent_entries.iter().position(|e| e.path == self.cwd);
            }
            None => {
                self.parent_entries.clear();
                self.parent_cursor = None;
            }
        }
    }

    pub fn refresh_preview(&mut self) {
        self.preview = preview::build(self.current(), self.show_hidden, self.sort, PREVIEW_LINES);
    }

    fn set_cursor(&mut self, i: usize) {
        if self.entries.is_empty() {
            return;
        }
        self.cursor = i.min(self.entries.len() - 1);
        self.list_state.select(Some(self.cursor));
        self.refresh_preview();
    }

    // ----- navigation ------------------------------------------------------

    pub fn cd(&mut self, dir: PathBuf, select: Option<&Path>) {
        if !dir.is_dir() {
            self.error(format!("not a directory: {}", fs::display_path(&dir)));
            return;
        }
        if std::fs::read_dir(&dir).is_err() {
            self.error(format!("permission denied: {}", fs::display_path(&dir)));
            return;
        }
        if dir != self.cwd {
            self.history.push(self.cwd.clone());
            self.history.truncate(50);
        }
        self.cwd = dir;
        self.filter.clear();
        self.cursor = 0;
        self.reload(select);
        if select.is_none() {
            self.set_cursor(0);
        }
    }

    fn go_parent(&mut self) {
        if let Some(parent) = self.cwd.parent().map(Path::to_path_buf) {
            let from = self.cwd.clone();
            self.cd(parent, Some(&from));
        }
    }

    fn go_back(&mut self) {
        if let Some(prev) = self.history.pop() {
            let from = self.cwd.clone();
            self.cwd = prev;
            self.filter.clear();
            self.cursor = 0;
            self.reload(Some(&from));
        }
    }

    fn open_current(&mut self) {
        let Some(entry) = self.current().cloned() else {
            return;
        };
        if entry.is_dir {
            self.cd(entry.path, None);
        } else if preview::looks_like_text(&entry.path) {
            self.external = Some(External::Editor(entry.path));
        } else {
            self.xdg_open(&entry.path);
        }
    }

    fn xdg_open(&mut self, path: &Path) {
        match fs::spawn_detached("xdg-open", &[&path.to_string_lossy()], Some(&self.cwd)) {
            Ok(()) => self.info(format!("opened {}", fs::display_path(path))),
            Err(e) => self.error(format!("xdg-open: {e}")),
        }
    }

    fn open_terminal_here(&mut self) {
        let term = std::env::var("TERMINAL").unwrap_or_else(|_| "alacritty".into());
        let cwd = self.cwd.to_string_lossy().to_string();
        let result = if fs::has_cmd("uwsm") {
            fs::spawn_detached("uwsm", &["app", "--", &term, "--working-directory", &cwd], Some(&self.cwd))
        } else {
            fs::spawn_detached(&term, &["--working-directory", &cwd], Some(&self.cwd))
        };
        match result {
            Ok(()) => self.info(format!("{term} in {}", fs::display_path(&self.cwd))),
            Err(e) => self.error(format!("{term}: {e}")),
        }
    }

    // ----- selection & clipboard ------------------------------------------

    /// Paths an operation applies to: marked entries, else the cursor entry.
    fn targets(&self) -> Vec<PathBuf> {
        if !self.marked.is_empty() {
            self.marked.iter().cloned().collect()
        } else {
            self.current().map(|e| vec![e.path.clone()]).unwrap_or_default()
        }
    }

    fn toggle_mark(&mut self) {
        if let Some(e) = self.current() {
            let p = e.path.clone();
            if !self.marked.remove(&p) {
                self.marked.insert(p);
            }
        }
        self.move_cursor(1);
    }

    fn mark_all(&mut self) {
        if self.marked.len() == self.entries.len() {
            self.marked.clear();
        } else {
            self.marked = self.entries.iter().map(|e| e.path.clone()).collect();
        }
    }

    fn yank(&mut self, cut: bool) {
        let paths = self.targets();
        if paths.is_empty() {
            return;
        }
        let n = paths.len();
        self.clipboard = Some(Clipboard { paths, cut });
        self.marked.clear();
        self.info(format!("{} {} item{}", if cut { "cut" } else { "yanked" }, n, if n == 1 { "" } else { "s" }));
    }

    fn paste(&mut self) {
        let Some(clip) = self.clipboard.take() else {
            self.info("clipboard is empty");
            return;
        };
        let mut done = 0;
        let mut last = None;
        let mut errors = Vec::new();
        for src in &clip.paths {
            let Some(name) = src.file_name().map(|n| n.to_string_lossy().to_string()) else {
                continue;
            };
            if clip.cut && src.parent() == Some(self.cwd.as_path()) {
                continue;
            }
            let dst = fs::unique_dest(&self.cwd, &name);
            let result = if clip.cut { fs::move_path(src, &dst) } else { fs::copy_recursive(src, &dst) };
            match result {
                Ok(()) => {
                    done += 1;
                    last = Some(dst);
                }
                Err(e) => errors.push(format!("{name}: {e}")),
            }
        }
        let verb = if clip.cut { "moved" } else { "copied" };
        if !clip.cut {
            self.clipboard = Some(clip);
        }
        self.reload(last.as_deref());
        if let Some(err) = errors.first() {
            self.error(format!("{err} ({} failed)", errors.len()));
        } else {
            self.info(format!("{verb} {done} item{}", if done == 1 { "" } else { "s" }));
        }
    }

    fn ask_delete(&mut self, permanent: bool) {
        let paths = self.targets();
        if paths.is_empty() {
            return;
        }
        let permanent = permanent || !fs::trash_available();
        let what = if paths.len() == 1 {
            paths[0].file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        } else {
            format!("{} items", paths.len())
        };
        let verb = if permanent { "Permanently delete" } else { "Move to trash" };
        self.mode = Mode::Confirm {
            prompt: format!("{verb} {what}?"),
            action: ConfirmAction::Delete { paths, permanent },
        };
    }

    fn run_confirmed(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::Delete { paths, permanent } => {
                let mut errors = Vec::new();
                let mut done = 0;
                for p in &paths {
                    let r = if permanent { fs::remove(p) } else { fs::trash(p) };
                    match r {
                        Ok(()) => done += 1,
                        Err(e) => errors.push(format!("{}: {e}", p.display())),
                    }
                }
                self.marked.clear();
                self.reload(None);
                if let Some(err) = errors.first() {
                    self.error(err.clone());
                } else {
                    self.info(format!("{} {done} item{}", if permanent { "deleted" } else { "trashed" }, if done == 1 { "" } else { "s" }));
                }
            }
        }
    }

    fn copy_path_to_clipboard(&mut self) {
        let Some(entry) = self.current().cloned() else {
            return;
        };
        let text = entry.path.to_string_lossy().to_string();
        let result = std::process::Command::new("wl-copy")
            .arg(&text)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match result {
            Ok(s) if s.success() => self.info(format!("copied {}", fs::display_path(&entry.path))),
            _ => self.error("wl-copy failed (is wl-clipboard installed?)"),
        }
    }

    fn submit_input(&mut self, kind: InputKind, buf: String) {
        let name = buf.trim();
        if name.is_empty() {
            return;
        }
        if name.contains('/') && !matches!(kind, InputKind::NewDir) {
            self.error("name must not contain '/'");
            return;
        }
        match kind {
            InputKind::Rename(from) => {
                let to = self.cwd.join(name);
                if to != from && std::fs::symlink_metadata(&to).is_ok() {
                    self.error(format!("{name} already exists"));
                    return;
                }
                match std::fs::rename(&from, &to) {
                    Ok(()) => {
                        self.reload(Some(&to));
                        self.info(format!("renamed to {name}"));
                    }
                    Err(e) => self.error(format!("rename: {e}")),
                }
            }
            InputKind::NewFile => {
                let path = self.cwd.join(name);
                if std::fs::symlink_metadata(&path).is_ok() {
                    self.error(format!("{name} already exists"));
                    return;
                }
                match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                    Ok(_) => {
                        self.reload(Some(&path));
                        self.info(format!("created {name}"));
                    }
                    Err(e) => self.error(format!("create: {e}")),
                }
            }
            InputKind::NewDir => {
                let path = self.cwd.join(name);
                match std::fs::create_dir_all(&path) {
                    Ok(()) => {
                        let first = self.cwd.join(name.split('/').next().unwrap_or(name));
                        self.reload(Some(&first));
                        self.info(format!("created {name}/"));
                    }
                    Err(e) => self.error(format!("mkdir: {e}")),
                }
            }
        }
    }

    // ----- key handling ----------------------------------------------------

    fn move_cursor(&mut self, delta: i64) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len() as i64 - 1;
        let next = (self.cursor as i64 + delta).clamp(0, max);
        self.set_cursor(next as usize);
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.handle_normal(key),
            Mode::Filter => self.handle_filter(key),
            Mode::Input { kind, buf } => self.handle_input(key, kind, buf),
            Mode::Confirm { prompt, action } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => self.run_confirmed(action),
                KeyCode::Char('n') | KeyCode::Char('q') | KeyCode::Esc => {}
                _ => self.mode = Mode::Confirm { prompt, action },
            },
            Mode::Help => {}
        }
    }

    fn handle_filter(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.apply_filter();
                self.clamp_cursor();
                self.refresh_preview();
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.apply_filter();
                self.cursor = 0;
                self.clamp_cursor();
                self.refresh_preview();
                self.mode = Mode::Filter;
            }
            KeyCode::Down => {
                self.move_cursor(1);
                self.mode = Mode::Filter;
            }
            KeyCode::Up => {
                self.move_cursor(-1);
                self.mode = Mode::Filter;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.push(c);
                self.apply_filter();
                self.cursor = 0;
                self.clamp_cursor();
                self.refresh_preview();
                self.mode = Mode::Filter;
            }
            _ => self.mode = Mode::Filter,
        }
    }

    fn handle_input(&mut self, key: KeyEvent, kind: InputKind, mut buf: String) {
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Enter => self.submit_input(kind, buf),
            KeyCode::Backspace => {
                buf.pop();
                self.mode = Mode::Input { kind, buf };
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                buf.clear();
                self.mode = Mode::Input { kind, buf };
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                buf.push(c);
                self.mode = Mode::Input { kind, buf };
            }
            _ => self.mode = Mode::Input { kind, buf },
        }
    }

    fn handle_normal(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let pending_g = std::mem::replace(&mut self.pending_g, false);
        match key.code {
            KeyCode::Char('g') if !ctrl => {
                if pending_g {
                    self.set_cursor(0);
                } else {
                    self.pending_g = true;
                }
            }
            KeyCode::Char('q') if !ctrl => self.should_quit = true,
            KeyCode::Char('c') if ctrl => self.should_quit = true,

            KeyCode::Char('j') | KeyCode::Down => self.move_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_cursor(-1),
            KeyCode::Char('d') if ctrl => self.move_cursor(self.page_size as i64 / 2),
            KeyCode::Char('u') if ctrl => self.move_cursor(-(self.page_size as i64) / 2),
            KeyCode::PageDown => self.move_cursor(self.page_size as i64),
            KeyCode::PageUp => self.move_cursor(-(self.page_size as i64)),
            KeyCode::Home => self.set_cursor(0),
            KeyCode::Char('G') | KeyCode::End => self.set_cursor(usize::MAX),

            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => self.go_parent(),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => self.open_current(),
            KeyCode::Char('-') => self.go_back(),
            KeyCode::Char('~') => {
                if let Ok(home) = std::env::var("HOME") {
                    self.cd(PathBuf::from(home), None);
                }
            }

            KeyCode::Char('.') => {
                self.show_hidden = !self.show_hidden;
                self.reload(None);
            }
            KeyCode::Char('s') => {
                self.sort = self.sort.next();
                self.reload(None);
                self.info(format!("sort: {}", self.sort.label()));
            }
            KeyCode::Char('R') | KeyCode::F(5) => {
                self.reload(None);
                self.info("reloaded");
            }
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Esc => {
                self.filter.clear();
                self.marked.clear();
                self.apply_filter();
                self.clamp_cursor();
                self.refresh_preview();
            }

            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Char('a') if ctrl => self.mark_all(),
            KeyCode::Char('y') => self.yank(false),
            KeyCode::Char('x') => self.yank(true),
            KeyCode::Char('p') => self.paste(),
            KeyCode::Char('Y') => self.copy_path_to_clipboard(),
            KeyCode::Char('d') => self.ask_delete(false),
            KeyCode::Char('D') => self.ask_delete(true),

            KeyCode::Char('a') => self.mode = Mode::Input { kind: InputKind::NewFile, buf: String::new() },
            KeyCode::Char('A') => self.mode = Mode::Input { kind: InputKind::NewDir, buf: String::new() },
            KeyCode::Char('r') | KeyCode::F(2) => {
                if let Some(e) = self.current().cloned() {
                    self.mode = Mode::Input { kind: InputKind::Rename(e.path), buf: e.name };
                }
            }

            KeyCode::Char('e') => {
                if let Some(e) = self.current().cloned() {
                    if e.is_dir {
                        self.error("cannot edit a directory");
                    } else {
                        self.external = Some(External::Editor(e.path));
                    }
                }
            }
            KeyCode::Char('o') => {
                if let Some(e) = self.current().cloned() {
                    self.xdg_open(&e.path);
                }
            }
            KeyCode::Char('S') => self.external = Some(External::Shell),
            KeyCode::Char('t') => self.open_terminal_here(),

            KeyCode::Char('?') | KeyCode::F(1) => self.mode = Mode::Help,
            _ => {}
        }
    }

    /// Write the final directory for the shell `cd` wrapper.
    pub fn write_cwd_file(&self) {
        if let Some(path) = &self.cwd_file {
            let _ = std::fs::write(path, self.cwd.to_string_lossy().as_bytes());
        }
    }
}

pub const HELP: &[(&str, &str)] = &[
    ("j / k  ↓ / ↑", "move cursor"),
    ("h / l  ← / →", "parent / open"),
    ("gg / G", "first / last entry"),
    ("Ctrl-d / Ctrl-u", "half page down / up"),
    ("-", "previous directory"),
    ("~", "home directory"),
    ("Enter", "open directory, edit text, xdg-open other"),
    ("e / o", "edit in $EDITOR / open with xdg-open"),
    ("S / t", "shell here / new terminal here"),
    ("/", "filter entries (Esc clears)"),
    (".", "toggle hidden files"),
    ("s", "cycle sort: name, size, modified, ext"),
    ("Space / Ctrl-a", "mark entry / mark all"),
    ("y / x / p", "yank / cut / paste"),
    ("Y", "copy path to Wayland clipboard"),
    ("a / A", "new file / new directory"),
    ("r", "rename"),
    ("d / D", "move to trash / delete permanently"),
    ("R", "reload"),
    ("q", "quit"),
];
