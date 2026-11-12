//! Vaults: encrypted folders (gocryptfs, see `crypt`) with a home in the sidebar.
//!
//! Any number of vaults can be listed there, each with its own master password.
//! A locked vault shows a page with a large padlock and the password field where
//! the files would be; the right password swings the shackle open and the files
//! take the padlock's place. The list itself is a plain text file with one
//! ciphertext folder per line; no password and no key is ever stored.

use crate::crypt;
use crate::i18n::tr;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

// ----- the list ------------------------------------------------------------------------

fn registry() -> PathBuf {
    glib::user_config_dir().join("omafile/vaults")
}

/// One ciphertext folder per line; blank lines and `#` comments are skipped, repeats dropped.
pub fn parse(text: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let path = PathBuf::from(line);
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

pub fn list() -> Vec<PathBuf> {
    std::fs::read_to_string(registry()).map(|t| parse(&t)).unwrap_or_default()
}

fn save(vaults: &[PathBuf]) {
    let path = registry();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = vaults.iter().map(|v| format!("{}\n", v.display())).collect();
    let _ = std::fs::write(path, text);
}

/// Put a vault into the sidebar; false when it was there already.
pub fn add(vault: &Path) -> bool {
    let mut vaults = list();
    if vaults.iter().any(|v| v == vault) {
        return false;
    }
    vaults.push(vault.to_path_buf());
    save(&vaults);
    true
}

pub fn remove(vault: &Path) {
    let vaults: Vec<PathBuf> = list().into_iter().filter(|v| v != vault).collect();
    save(&vaults);
}

/// The name a vault goes by: its folder without the `.gocryptfs` suffix.
pub fn display_name(vault: &Path) -> String {
    crypt::mount_point_for(vault).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| vault.display().to_string())
}

/// Where new vaults go unless another place is chosen.
pub fn default_dir() -> PathBuf {
    glib::home_dir().join("Vaults")
}

/// The ciphertext folder behind a folder somebody picked: the folder itself, or the vault
/// that belongs to it when they picked the place where it appears unlocked.
pub fn resolve(picked: &Path) -> Option<PathBuf> {
    if crypt::is_vault(picked) {
        Some(picked.to_path_buf())
    } else {
        crypt::vault_for(picked)
    }
}

// ----- the padlock ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Anim {
    None,
    Appear,
    Shake,
    Open,
}

fn ease_out_back(t: f64) -> f64 {
    let (c1, c3) = (1.70158, 2.70158);
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let pi = std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
    cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
    cr.arc(x + r, y + r, r, pi, 1.5 * pi);
    cr.close_path();
}

/// What the padlock looks like at one moment.
struct Look {
    /// 0 closed … 1 open (a little more while it overshoots).
    open: f64,
    /// Sideways offset as a fraction of the size (the "no" shake).
    shake: f64,
    scale: f64,
    body: (f64, f64, f64),
    shackle: (f64, f64, f64),
    keyhole: (f64, f64, f64),
}

fn draw_padlock(cr: &gtk::cairo::Context, w: f64, h: f64, look: &Look) {
    // Smaller than the area: the lifted shackle, the halo and the bounce need room.
    let s = w.min(h) * 0.8;
    let pi = std::f64::consts::PI;
    cr.translate(w / 2.0, h / 2.0);
    cr.scale(look.scale, look.scale);
    cr.translate(-w / 2.0 + look.shake * s, -h / 2.0);
    let cx = w / 2.0;
    let (bw, bh) = (0.56 * s, 0.42 * s);
    let by = h / 2.0 + 0.03 * s;
    let (r, g, b) = look.body;
    // A halo in the body's colour.
    for i in (1..=10).rev() {
        let grow = i as f64 * 0.012 * s;
        rounded(cr, cx - bw / 2.0 - grow, by - grow, bw + 2.0 * grow, bh + 2.0 * grow, 0.09 * s + grow);
        cr.set_source_rgba(r, g, b, 0.028);
        let _ = cr.fill();
    }
    // The shackle: a long leg on the left that stays in the body, a short one that leaves it.
    let leg = 0.17 * s;
    let lift = look.open * 0.13 * s;
    let arc_y = by - 0.15 * s - lift;
    let (sr, sg, sb) = look.shackle;
    cr.set_line_width(0.078 * s);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.move_to(cx - leg, by + 0.08 * s);
    cr.line_to(cx - leg, arc_y);
    cr.arc(cx, arc_y, leg, pi, 2.0 * pi);
    cr.line_to(cx + leg, by + 0.08 * s - lift * 1.75);
    cr.set_source_rgb(sr, sg, sb);
    let _ = cr.stroke();
    // The body, lighter at the top.
    let shade = gtk::cairo::LinearGradient::new(0.0, by, 0.0, by + bh);
    shade.add_color_stop_rgb(0.0, (r + 0.14).min(1.0), (g + 0.14).min(1.0), (b + 0.14).min(1.0));
    shade.add_color_stop_rgb(1.0, r * 0.82, g * 0.82, b * 0.82);
    rounded(cr, cx - bw / 2.0, by, bw, bh, 0.09 * s);
    let _ = cr.set_source(&shade);
    let _ = cr.fill();
    // A soft highlight along the top edge.
    rounded(cr, cx - bw / 2.0 + 0.03 * s, by + 0.02 * s, bw - 0.06 * s, 0.05 * s, 0.025 * s);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.16);
    let _ = cr.fill();
    // The keyhole.
    let (kr, kg, kb) = look.keyhole;
    let ky = by + 0.17 * s;
    cr.arc(cx, ky, 0.05 * s, 0.0, 2.0 * pi);
    cr.move_to(cx - 0.022 * s, ky + 0.02 * s);
    cr.line_to(cx + 0.022 * s, ky + 0.02 * s);
    cr.line_to(cx + 0.036 * s, ky + 0.13 * s);
    cr.line_to(cx - 0.036 * s, ky + 0.13 * s);
    cr.close_path();
    cr.set_source_rgba(kr, kg, kb, 0.9);
    let _ = cr.fill();
}

// ----- the page ------------------------------------------------------------------------

pub struct VaultPage {
    pub root: gtk::Box,
    lock: gtk::DrawingArea,
    title: gtk::Label,
    subtitle: gtk::Label,
    form: gtk::Box,
    entry: gtk::PasswordEntry,
    unlock: gtk::Button,
    error: gtk::Label,
    links: gtk::Box,
    vault: RefCell<PathBuf>,
    anim: Cell<(Anim, Instant)>,
    /// The padlock stays open once the shackle has swung up.
    opened: Cell<bool>,
    busy: Cell<bool>,
    ticking: Cell<bool>,
    colours: RefCell<[(f64, f64, f64); 5]>,
}

impl VaultPage {
    fn new(window: &Rc<Window>) -> Rc<Self> {
        let lock = gtk::DrawingArea::new();
        lock.set_content_width(340);
        lock.set_content_height(300);
        lock.set_halign(gtk::Align::Center);
        let title = gtk::Label::builder().css_classes(["omafile-vault-title"]).ellipsize(gtk::pango::EllipsizeMode::Middle).max_width_chars(40).build();
        let subtitle = gtk::Label::builder().css_classes(["omafile-dim"]).ellipsize(gtk::pango::EllipsizeMode::Middle).max_width_chars(60).build();
        let entry = gtk::PasswordEntry::builder().placeholder_text(tr("Master password")).show_peek_icon(true).width_request(300).build();
        entry.add_css_class("omafile-vault-entry");
        let unlock = gtk::Button::builder().label(tr("Unlock")).build();
        unlock.add_css_class("suggested-action");
        unlock.add_css_class("pill");
        let form = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        form.set_halign(gtk::Align::Center);
        form.set_margin_top(14);
        form.append(&entry);
        form.append(&unlock);
        let error = gtk::Label::builder().css_classes(["error", "caption"]).build();
        let passwd = gtk::Button::builder().label(tr("Change Password…")).build();
        let reveal = gtk::Button::builder().label(tr("Show Encrypted Data")).build();
        let export = gtk::Button::builder().label(tr("Export…")).tooltip_text(tr("Save the vault as one file to take it to another computer; it stays protected by its master password")).build();
        let forget = gtk::Button::builder().label(tr("Remove from Sidebar")).build();
        let links = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        links.set_halign(gtk::Align::Center);
        links.set_margin_top(18);
        for b in [&passwd, &export, &reveal, &forget] {
            b.add_css_class("flat");
            b.add_css_class("omafile-dim");
            links.append(b);
        }
        let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
        column.set_valign(gtk::Align::Center);
        column.set_vexpand(true);
        column.append(&lock);
        column.append(&title);
        column.append(&subtitle);
        column.append(&form);
        column.append(&error);
        column.append(&links);
        let back = gtk::Button::builder()
            .child(&{
                let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                b.append(&gtk::Image::from_icon_name("go-previous-symbolic"));
                b.append(&gtk::Label::new(Some(&tr("Files"))));
                b
            })
            .tooltip_text(tr("Back to the files (Esc)"))
            .halign(gtk::Align::Start)
            .margin_top(10)
            .margin_start(12)
            .build();
        back.add_css_class("flat");
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("omafile-vault-page");
        root.append(&back);
        root.append(&column);

        let now = Instant::now();
        let page = Rc::new(VaultPage {
            root,
            lock,
            title,
            subtitle,
            form,
            entry,
            unlock,
            error,
            links,
            vault: RefCell::new(PathBuf::new()),
            anim: Cell::new((Anim::None, now)),
            opened: Cell::new(false),
            busy: Cell::new(false),
            ticking: Cell::new(false),
            colours: RefCell::new([(0.5, 0.6, 1.0); 5]),
        });

        let weak = Rc::downgrade(&page);
        page.lock.set_draw_func(move |_, cr, w, h| {
            if let Some(p) = weak.upgrade() {
                p.draw(cr, w as f64, h as f64);
            }
        });
        let (weak, wweak) = (Rc::downgrade(&page), Rc::downgrade(window));
        page.entry.connect_activate(move |_| {
            if let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) {
                p.submit(&w);
            }
        });
        let (weak, wweak) = (Rc::downgrade(&page), Rc::downgrade(window));
        page.unlock.connect_clicked(move |_| {
            if let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) {
                p.submit(&w);
            }
        });
        let weak = Rc::downgrade(&page);
        page.entry.connect_changed(move |_| {
            if let Some(p) = weak.upgrade() {
                p.error.set_text("");
            }
        });
        let (weak, wweak) = (Rc::downgrade(&page), Rc::downgrade(window));
        passwd.connect_clicked(move |_| {
            if let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) {
                w.vault_passwd_for(p.vault.borrow().clone());
            }
        });
        let (weak, wweak) = (Rc::downgrade(&page), Rc::downgrade(window));
        reveal.connect_clicked(move |_| {
            if let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) {
                let vault = p.vault.borrow().clone();
                w.navigate(gio::File::for_path(vault));
            }
        });
        let (weak, wweak) = (Rc::downgrade(&page), Rc::downgrade(window));
        export.connect_clicked(move |_| {
            if let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) {
                w.vault_export(p.vault.borrow().clone());
            }
        });
        let (weak, wweak) = (Rc::downgrade(&page), Rc::downgrade(window));
        forget.connect_clicked(move |_| {
            if let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) {
                let vault = p.vault.borrow().clone();
                w.vault_forget(&vault);
            }
        });
        let wweak = Rc::downgrade(window);
        back.connect_clicked(move |_| {
            if let Some(w) = wweak.upgrade() {
                w.show_files();
                w.refocus();
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let wweak = Rc::downgrade(window);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gdk::Key::Escape {
                if let Some(w) = wweak.upgrade() {
                    w.show_files();
                    w.refocus();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        page.root.add_controller(keys);
        page
    }

    /// Show the padlock for `vault`.
    fn present(self: &Rc<Self>, window: &Rc<Window>, vault: &Path) {
        self.vault.replace(vault.to_path_buf());
        self.opened.set(false);
        self.busy.set(false);
        self.entry.set_text("");
        self.error.set_text("");
        self.unlock.set_sensitive(true);
        self.form.set_opacity(1.0);
        self.links.set_opacity(1.0);
        self.title.set_text(&display_name(vault));
        let present = crypt::is_vault(vault);
        let tool = crypt::available();
        self.form.set_visible(present && tool);
        self.links.set_visible(true);
        self.subtitle.set_text(&if !present {
            format!("{}  ·  {}", tr("Not found: is its drive connected?"), crate::window::abbreviate_home(vault))
        } else if !tool {
            tr("gocryptfs is not installed (sudo pacman -S gocryptfs)")
        } else {
            format!("{}  ·  {}", tr("Locked"), crate::window::abbreviate_home(vault))
        });
        let p = window.theme.palette();
        let c = |c: crate::theme::Rgb| (c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0);
        self.colours.replace([c(p.accent), c(p.green), c(p.red), c(p.foreground), c(p.dark_background)]);
        self.start(Anim::Appear);
    }

    fn start(self: &Rc<Self>, anim: Anim) {
        self.anim.set((anim, Instant::now()));
        self.lock.queue_draw();
        if self.ticking.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        self.lock.add_tick_callback(move |area, _| {
            let Some(p) = weak.upgrade() else { return glib::ControlFlow::Break };
            area.queue_draw();
            let (anim, since) = p.anim.get();
            let length = match anim {
                Anim::None => 0,
                Anim::Appear => 460,
                Anim::Shake => 520,
                Anim::Open => 640,
            };
            if since.elapsed() >= Duration::from_millis(length) {
                if anim == Anim::Open {
                    p.opened.set(true);
                }
                p.anim.set((Anim::None, Instant::now()));
                p.ticking.set(false);
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }

    fn draw(&self, cr: &gtk::cairo::Context, w: f64, h: f64) {
        let [accent, green, red, fg, bg] = *self.colours.borrow();
        let (anim, since) = self.anim.get();
        let ms = since.elapsed().as_secs_f64() * 1000.0;
        let mix = |a: (f64, f64, f64), b: (f64, f64, f64), t: f64| (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t);
        let mut look = Look { open: if self.opened.get() { 1.0 } else { 0.0 }, shake: 0.0, scale: 1.0, body: if self.opened.get() { green } else { accent }, shackle: mix(fg, bg, 0.25), keyhole: bg };
        match anim {
            Anim::Appear => {
                let t = (ms / 460.0).min(1.0);
                look.scale = 0.55 + 0.45 * ease_out_back(t);
            }
            Anim::Shake => {
                let t = (ms / 520.0).min(1.0);
                look.shake = (t * std::f64::consts::PI * 7.0).sin() * (1.0 - t) * 0.07;
                look.body = mix(red, accent, t.powi(3));
            }
            Anim::Open => {
                let t = (ms / 640.0).min(1.0);
                look.open = ease_out_back((t / 0.7).min(1.0));
                look.body = mix(accent, green, (t * 1.6).min(1.0));
                look.scale = 1.0 + 0.05 * (t * std::f64::consts::PI).sin();
            }
            Anim::None => {}
        }
        draw_padlock(cr, w, h, &look);
    }

    fn fail(self: &Rc<Self>, message: &str) {
        self.busy.set(false);
        self.unlock.set_sensitive(true);
        self.error.set_text(message);
        self.start(Anim::Shake);
        self.entry.grab_focus();
    }

    fn submit(self: &Rc<Self>, window: &Rc<Window>) {
        if self.busy.get() {
            return;
        }
        let password = self.entry.text().to_string();
        if password.is_empty() {
            self.fail(&tr("Enter the master password"));
            return;
        }
        self.busy.set(true);
        self.unlock.set_sensitive(false);
        self.error.set_text("");
        let vault = self.vault.borrow().clone();
        let (weak, wweak) = (Rc::downgrade(self), Rc::downgrade(window));
        glib::spawn_future_local(async move {
            let result = crypt::unlock(&vault, &password).await;
            let (Some(p), Some(w)) = (weak.upgrade(), wweak.upgrade()) else { return };
            match result {
                Ok(mount) => {
                    p.entry.set_text("");
                    p.subtitle.set_text(&tr("Unlocked"));
                    // The form has done its job: only the padlock is left while it opens.
                    crate::motion::property(&p.form, "opacity", 1.0, 0.0, 220, None);
                    crate::motion::property(&p.links, "opacity", 1.0, 0.0, 220, None);
                    p.start(Anim::Open);
                    // Let the shackle swing open before the files take over.
                    glib::timeout_future(Duration::from_millis(if crate::motion::animations_enabled(&p.root) { 720 } else { 0 })).await;
                    p.busy.set(false);
                    // Somebody may have walked away from the page in the meantime.
                    let still_here = w.content_stack.visible_child_name().as_deref() == Some("vault") && *p.vault.borrow() == vault;
                    w.vault_unlocked(&mount, still_here);
                }
                Err(e) => {
                    let wrong = e.message().to_lowercase().contains("password incorrect");
                    p.fail(&if wrong { tr("Wrong password") } else { e.message().to_string() });
                }
            }
        });
    }
}

// ----- the window's side ---------------------------------------------------------------

impl Window {
    /// Back to the file panes from whatever lies over them (Now Playing, a locked vault).
    pub fn show_files(self: &Rc<Self>) {
        match self.content_stack.visible_child_name().as_deref() {
            Some("nowplaying") => self.show_now_playing(false),
            Some("vault") | Some("feeds") | Some("backup") | Some("themes") | Some("captures") => self.content_stack.set_visible_child_name("files"),
            _ => {}
        }
    }

    /// A click on a vault: straight into it when it is unlocked, the padlock otherwise.
    pub fn open_vault(self: &Rc<Self>, vault: &Path) {
        let mount = crypt::mount_point_for(vault);
        if crypt::is_vault(vault) && crypt::is_mounted(&mount) {
            self.navigate(gio::File::for_path(mount));
            return;
        }
        // Cloned out first: the borrow must not live through the arm that fills the slot.
        let existing = self.vault_page.borrow().clone();
        let page = match existing {
            Some(p) => p,
            None => {
                let p = VaultPage::new(self);
                self.content_stack.add_named(&p.root, Some("vault"));
                self.vault_page.replace(Some(p.clone()));
                p
            }
        };
        if self.content_stack.visible_child_name().as_deref() == Some("nowplaying") {
            self.show_now_playing(false);
        }
        page.present(self, vault);
        self.content_stack.set_visible_child_name("vault");
        if page.form.is_visible() {
            page.entry.grab_focus();
        }
    }

    fn vault_unlocked(self: &Rc<Self>, mount: &Path, open: bool) {
        crate::status::write(self);
        self.refresh_sidebar();
        self.toast(&tr("Unlocked"));
        if open {
            self.navigate(gio::File::for_path(mount));
            self.refocus();
        }
    }

    /// Lock a vault: every tab inside it steps out first, a busy mount cannot be unmounted.
    pub fn lock_vault(self: &Rc<Self>, vault: PathBuf) {
        let mount = crypt::mount_point_for(&vault);
        let mut moved = false;
        for pane in self.all_panes() {
            if pane.cwd().path().map(|p| p.starts_with(&mount)).unwrap_or(false) {
                let out = mount.parent().map(Path::to_path_buf).unwrap_or_else(glib::home_dir);
                pane.navigate(gio::File::for_path(out), true);
                moved = true;
            }
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            if moved {
                glib::timeout_future(Duration::from_millis(300)).await;
            }
            let result = crypt::lock(&mount).await;
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(()) => {
                    crate::status::write(&w);
                    w.refresh_sidebar();
                    w.toast(&tr("Locked"));
                    if let Some(p) = w.active_pane() {
                        p.reload();
                    }
                }
                Err(e) => w.toast_error(&format!("{}: {}", tr("Cannot lock (still in use?)"), e.message())),
            }
        });
    }

    /// Take a vault out of the sidebar. Its data stays where it is.
    pub fn vault_forget(self: &Rc<Self>, vault: &Path) {
        remove(vault);
        self.refresh_sidebar();
        if self.content_stack.visible_child_name().as_deref() == Some("vault") {
            self.show_files();
            self.refocus();
        }
        self.toast(&format!("{}  {}", tr("Removed from the sidebar. The encrypted data stays at"), crate::window::abbreviate_home(vault)));
    }

    /// The sidebar's "Add Vault…": make a new one or list one that exists already.
    pub fn vault_add(self: &Rc<Self>) {
        if !self.require_gocryptfs() {
            return;
        }
        let dialog = adw::AlertDialog::new(Some(tr("Add Vault").as_str()), Some(tr("A vault is an encrypted folder with its own master password. It stays in the sidebar; a click asks for the password and shows the files, the lock button closes it again.").as_str()));
        dialog.add_responses(&[("cancel", tr("Cancel").as_str()), ("import", tr("Import from File…").as_str()), ("existing", tr("Add Existing…").as_str()), ("new", tr("Create New…").as_str())]);
        dialog.set_response_appearance("new", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("new"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |response| {
            let Some(w) = this.upgrade() else { return };
            match response.as_str() {
                "new" => w.vault_create_dialog(default_dir(), true),
                "existing" => w.vault_add_existing(),
                "import" => w.vault_import(None),
                _ => w.refocus(),
            }
        });
    }

    fn vault_add_existing(self: &Rc<Self>) {
        let chooser = gtk::FileDialog::builder().title(tr("Choose an encrypted folder (Name.gocryptfs)")).modal(true).build();
        let start = default_dir();
        chooser.set_initial_folder(Some(&gio::File::for_path(if start.is_dir() { start } else { glib::home_dir() })));
        let this = Rc::downgrade(self);
        chooser.select_folder(Some(&self.window), gio::Cancellable::NONE, move |result| {
            let Some(w) = this.upgrade() else { return };
            let Some(picked) = result.ok().and_then(|f| f.path()) else { return };
            w.vault_register(&picked);
        });
    }

    /// Save a vault as one file to take along. The file is the encrypted folder in a tar
    /// archive: nothing is decrypted, the master password protects it as it protects the vault.
    pub fn vault_export(self: &Rc<Self>, vault: PathBuf) {
        if !crypt::is_vault(&vault) {
            self.toast_error(&tr("That folder is not a vault (no gocryptfs.conf inside)"));
            return;
        }
        let name = display_name(&vault);
        let chooser = gtk::FileDialog::builder().title(tr("Export Vault")).modal(true).initial_name(format!("{name}{}", crypt::ARCHIVE_SUFFIX)).build();
        chooser.set_initial_folder(Some(&gio::File::for_path(glib::home_dir())));
        let this = Rc::downgrade(self);
        chooser.save(Some(&self.window), gio::Cancellable::NONE, move |result| {
            let Some(w) = this.upgrade() else { return };
            let Some(target) = result.ok().and_then(|f| f.path()) else { return };
            w.toast(&tr("Exporting the vault…"));
            let this = Rc::downgrade(&w);
            glib::spawn_future_local(async move {
                let (v, t) = (vault.clone(), target.clone());
                let result = gio::spawn_blocking(move || crypt::export(&v, &t)).await.unwrap_or_else(|_| Err("export stopped".into()));
                let Some(w) = this.upgrade() else { return };
                match result {
                    Ok(_) => {
                        let toast = adw::Toast::builder().use_markup(false).title(format!("{}: {}", tr("Exported, still protected by its master password"), crate::window::abbreviate_home(&target))).timeout(8).button_label(tr("Show")).build();
                        let weak = Rc::downgrade(&w);
                        toast.connect_button_clicked(move |_| {
                            let (Some(w), Some(dir)) = (weak.upgrade(), target.parent()) else { return };
                            w.navigate(gio::File::for_path(dir));
                            if let Some(p) = w.active_pane() {
                                p.select_file_later(gio::File::for_path(&target));
                            }
                        });
                        w.toast.add_toast(toast);
                        if let Some(p) = w.active_pane() {
                            p.reload();
                        }
                    }
                    Err(e) => w.toast_error(&format!("{}: {e}", tr("Cannot export the vault"))),
                }
            });
        });
    }

    /// Bring in an exported vault (`Name.gocryptfs.tar`): unpack it into ~/Vaults, list it in
    /// the sidebar and show its padlock. `archive`: the file, or None to ask for one.
    pub fn vault_import(self: &Rc<Self>, archive: Option<PathBuf>) {
        let Some(archive) = archive else {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some(&tr("Exported vaults (*.gocryptfs.tar)")));
            filter.add_pattern("*.tar");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let chooser = gtk::FileDialog::builder().title(tr("Import Vault")).modal(true).filters(&filters).build();
            chooser.set_initial_folder(Some(&gio::File::for_path(glib::home_dir())));
            let this = Rc::downgrade(self);
            chooser.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
                if let (Some(w), Some(path)) = (this.upgrade(), result.ok().and_then(|f| f.path())) {
                    w.vault_import(Some(path));
                }
            });
            return;
        };
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || crypt::import(&archive, &default_dir())).await.unwrap_or_else(|_| Err("import stopped".into()));
            let Some(w) = this.upgrade() else { return };
            match result {
                Ok(vault) => {
                    add(&vault);
                    w.refresh_sidebar();
                    w.toast(&format!("{}: {}", tr("Imported; it opens with the password it had"), crate::window::abbreviate_home(&vault)));
                    w.open_vault(&vault);
                }
                Err(e) => w.toast_error(&format!("{}: {e}", tr("Cannot import the vault"))),
            }
        });
    }

    /// List the vault behind `picked` in the sidebar and show its padlock (or its files).
    pub fn vault_register(self: &Rc<Self>, picked: &Path) {
        match resolve(picked) {
            Some(vault) => {
                if !add(&vault) {
                    self.toast(&tr("This vault is in the sidebar already"));
                }
                self.refresh_sidebar();
                self.open_vault(&vault);
            }
            None => self.toast_error(&tr("That folder is not a vault (no gocryptfs.conf inside)")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_list() {
        let v = parse("# my vaults\n/home/me/Vaults/Private.gocryptfs\n\n  /mnt/usb/Work.gocryptfs  \n/home/me/Vaults/Private.gocryptfs\n");
        assert_eq!(v, vec![PathBuf::from("/home/me/Vaults/Private.gocryptfs"), PathBuf::from("/mnt/usb/Work.gocryptfs")]);
        assert!(parse("").is_empty());
    }

    #[test]
    fn names_come_from_the_folder() {
        assert_eq!(display_name(Path::new("/home/me/Vaults/Tax 2026.gocryptfs")), "Tax 2026");
        assert_eq!(display_name(Path::new("/home/me/Plain")), "Plain");
    }

    #[test]
    fn resolves_vault_and_mount_point() {
        let dir = std::env::temp_dir().join(format!("omafile-vaults-test-{}", std::process::id()));
        let vault = dir.join("Docs.gocryptfs");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::create_dir_all(dir.join("Docs")).unwrap();
        assert_eq!(resolve(&vault), None, "a folder without gocryptfs.conf is no vault");
        std::fs::write(vault.join("gocryptfs.conf"), "{}").unwrap();
        assert_eq!(resolve(&vault), Some(vault.clone()));
        assert_eq!(resolve(&dir.join("Docs")), Some(vault.clone()));
        assert_eq!(resolve(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_shackle_overshoots_and_settles() {
        assert!(ease_out_back(0.0).abs() < 1e-9);
        assert!((ease_out_back(1.0) - 1.0).abs() < 1e-9);
        assert!(ease_out_back(0.7) > 1.0);
    }
}
