//! Embedded terminal below the file view (Ctrl+`): VTE running the user's
//! shell in the current folder, coloured like every other Omarchy terminal.

use crate::pane::Pane;
use crate::theme::{Palette, Rgb};
use crate::window::Window;
use adw::prelude::*;
use gtk::{gdk, gio, glib, pango};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use vte4::prelude::*;

pub struct TermPanel {
    pub root: gtk::Box,
    pub terminal: vte4::Terminal,
    title: gtk::Label,
    follow: gtk::ToggleButton,
    pane: Weak<Pane>,
    window: Weak<Window>,
    /// Folder the shell is in as far as we know (from `cd`s we sent or OSC 7).
    shell_dir: RefCell<Option<PathBuf>>,
    syncing: Cell<bool>,
}

fn rgba(c: Rgb) -> gdk::RGBA {
    gdk::RGBA::new(c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0, 1.0)
}

/// The same mapping Omarchy's alacritty template uses.
pub fn vte_palette(p: &Palette) -> (gdk::RGBA, gdk::RGBA, Vec<gdk::RGBA>) {
    let normal = [p.background, p.red, p.green, p.yellow, p.blue, p.magenta, p.cyan, p.foreground];
    let bright = [p.muted, p.bright_red, p.bright_green, p.bright_yellow, p.bright_blue, p.bright_magenta, p.bright_cyan, p.bright_foreground];
    let colors = normal.iter().chain(bright.iter()).map(|c| rgba(*c)).collect();
    (rgba(p.foreground), rgba(p.dark_background), colors)
}

impl TermPanel {
    pub fn new(window: &Rc<Window>, pane: &Rc<Pane>) -> Rc<Self> {
        let terminal = vte4::Terminal::new();
        terminal.set_scrollback_lines(10_000);
        terminal.set_bold_is_bright(true);
        terminal.set_mouse_autohide(true);
        terminal.set_scroll_on_keystroke(true);
        terminal.set_scroll_on_output(false);
        terminal.set_enable_sixel(true);
        terminal.set_font(Some(&pango::FontDescription::from_string("monospace 10.5")));
        terminal.set_vexpand(true);
        terminal.set_hexpand(true);
        terminal.add_css_class("omafile-terminal");

        let title = gtk::Label::builder().xalign(0.0).ellipsize(pango::EllipsizeMode::Middle).hexpand(true).css_classes(["caption", "omafile-dim"]).build();
        let follow = gtk::ToggleButton::builder().icon_name("folder-symbolic").active(true).tooltip_text("Follow the folder shown above (sends cd)").build();
        follow.add_css_class("flat");
        let clear = gtk::Button::builder().icon_name("edit-clear-all-symbolic").tooltip_text("Clear").build();
        clear.add_css_class("flat");
        let close = gtk::Button::builder().icon_name("window-close-symbolic").tooltip_text("Hide terminal (Ctrl+`)").action_name("win.terminal-panel").build();
        close.add_css_class("flat");
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        header.add_css_class("omafile-terminal-header");
        let icon = gtk::Image::from_icon_name("utilities-terminal-symbolic");
        icon.add_css_class("omafile-dim");
        header.append(&icon);
        header.append(&title);
        header.append(&follow);
        header.append(&clear);
        header.append(&close);

        let scroll = gtk::ScrolledWindow::builder().child(&terminal).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("omafile-terminal-panel");
        root.append(&header);
        root.append(&scroll);

        let this = Rc::new(TermPanel {
            root,
            terminal: terminal.clone(),
            title,
            follow,
            pane: Rc::downgrade(pane),
            window: Rc::downgrade(window),
            shell_dir: RefCell::new(None),
            syncing: Cell::new(false),
        });

        // Colours follow the Omarchy theme, live.
        let weak = Rc::downgrade(&this);
        window.theme.connect_changed(move |palette| {
            if let Some(t) = weak.upgrade() {
                t.apply_palette(palette);
            }
        });

        let t2 = terminal.clone();
        clear.connect_clicked(move |_| t2.reset(true, true));

        // Ctrl+Shift+C / V like every terminal; the rest goes to the shell.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let t2 = terminal.clone();
        keys.connect_key_pressed(move |_, key, _, state| {
            if state.contains(gdk::ModifierType::CONTROL_MASK) && state.contains(gdk::ModifierType::SHIFT_MASK) {
                match key {
                    gdk::Key::C | gdk::Key::c => {
                        t2.copy_clipboard_format(vte4::Format::Text);
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::V | gdk::Key::v => {
                        t2.paste_clipboard();
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            glib::Propagation::Proceed
        });
        terminal.add_controller(keys);

        // Shells that report their folder (OSC 7: fish, zsh and bash with a
        // precmd hook) steer the file view; `cd` in the terminal navigates above.
        let weak = Rc::downgrade(&this);
        terminal.connect_current_directory_uri_notify(move |t| {
            let Some(panel) = weak.upgrade() else { return };
            let Some(uri) = t.current_directory_uri() else { return };
            let file = gio::File::for_uri(&uri);
            let Some(path) = file.path() else { return };
            *panel.shell_dir.borrow_mut() = Some(path.clone());
            if !panel.follow.is_active() || panel.syncing.get() {
                return;
            }
            if let Some(pane) = panel.pane.upgrade() {
                if pane.cwd().path().as_deref() != Some(path.as_path()) {
                    panel.syncing.set(true);
                    pane.navigate(file, true);
                    panel.syncing.set(false);
                }
            }
        });

        // Respawn the shell when it exits (typing `exit` gives a fresh prompt).
        let weak = Rc::downgrade(&this);
        terminal.connect_child_exited(move |_, _| {
            if let Some(panel) = weak.upgrade() {
                let dir = panel.pane.upgrade().and_then(|p| p.cwd().path());
                panel.spawn(dir);
            }
        });

        // Focus in the terminal: its pane becomes the active side, and the
        // search bar must stop capturing keystrokes (it would eat the spaces).
        let weak = Rc::downgrade(&this);
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| {
            if let Some(panel) = weak.upgrade() {
                if let (Some(w), Some(p)) = (panel.window.upgrade(), panel.pane.upgrade()) {
                    w.set_active_side(p.side.get());
                    p.search_bar.set_key_capture_widget(None::<&gtk::Widget>);
                }
            }
        });
        let weak = Rc::downgrade(&this);
        focus.connect_leave(move |_| {
            if let Some(panel) = weak.upgrade() {
                if let (Some(w), Some(p)) = (panel.window.upgrade(), panel.pane.upgrade()) {
                    p.search_bar.set_key_capture_widget(Some(&w.window));
                }
            }
        });
        terminal.add_controller(focus);

        this.spawn(pane.cwd().path());
        this
    }

    fn apply_palette(&self, palette: &Palette) {
        let (fg, bg, colors) = vte_palette(palette);
        let refs: Vec<&gdk::RGBA> = colors.iter().collect();
        self.terminal.set_colors(Some(&fg), Some(&bg), &refs);
        self.terminal.set_color_cursor(Some(&rgba(palette.bright_foreground)));
        self.terminal.set_color_cursor_foreground(Some(&rgba(palette.background)));
    }

    fn spawn(self: &Rc<Self>, dir: Option<PathBuf>) {
        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/bash".into());
        let dir = dir.unwrap_or_else(glib::home_dir);
        let cwd = dir.to_string_lossy().to_string();
        *self.shell_dir.borrow_mut() = Some(dir.clone());
        self.set_title(&dir);
        let env: Vec<String> = vec!["TERM=xterm-256color".into(), "COLORTERM=truecolor".into(), "OMAFILE_TERMINAL=1".into()];
        let env: Vec<&str> = env.iter().map(String::as_str).collect();
        let this = Rc::downgrade(self);
        let shell_name = shell.clone();
        self.terminal.spawn_async(
            vte4::PtyFlags::DEFAULT,
            Some(&cwd),
            &[&shell],
            &env,
            glib::SpawnFlags::DEFAULT,
            || {},
            -1,
            gio::Cancellable::NONE,
            move |result| {
                if let (Err(e), Some(panel)) = (result, this.upgrade()) {
                    if let Some(w) = panel.window.upgrade() {
                        w.toast_error(&format!("Cannot start {shell_name}: {}", e.message()));
                    }
                }
            },
        );
    }

    fn set_title(&self, dir: &std::path::Path) {
        self.title.set_text(&format!("Terminal · {}", crate::window::abbreviate_home(dir)));
    }

    /// The file view moved: send the shell there too (leading space keeps it out of history).
    pub fn follow_to(&self, dir: &std::path::Path) {
        if !self.follow.is_active() || self.syncing.get() {
            return;
        }
        if self.shell_dir.borrow().as_deref() == Some(dir) {
            return;
        }
        *self.shell_dir.borrow_mut() = Some(dir.to_path_buf());
        self.set_title(dir);
        let quoted = format!("'{}'", dir.to_string_lossy().replace('\'', "'\\''"));
        self.terminal.feed_child(format!(" cd {quoted}\n").as_bytes());
    }

    pub fn focus(&self) {
        self.terminal.grab_focus();
    }
}
