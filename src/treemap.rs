//! Disk usage map: the folder on show as a squarified treemap. Click zooms
//! into a folder, Backspace or the Up button goes back, double click opens the
//! folder in omafile, right click offers Open / Reveal / Move to Trash.

use crate::i18n::tr;
use crate::ops;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Children kept per folder; the rest is summed into "other".
const MAX_CHILDREN: usize = 120;

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
    pub children: Vec<Node>,
}

/// Walk `root` and build the tree; sizes come from file lengths, not blocks.
pub fn scan(root: &Path, cancel: &AtomicBool) -> Node {
    fn walk(dir: &Path, cancel: &AtomicBool) -> Node {
        let mut node = Node { name: dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| dir.to_string_lossy().to_string()), path: dir.to_path_buf(), size: 0, is_dir: true, children: Vec::new() };
        let Ok(entries) = std::fs::read_dir(dir) else { return node };
        for entry in entries.flatten() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.file_type().is_symlink() {
                continue;
            }
            let child = if meta.is_dir() {
                walk(&entry.path(), cancel)
            } else {
                Node { name: entry.file_name().to_string_lossy().to_string(), path: entry.path(), size: meta.len(), is_dir: false, children: Vec::new() }
            };
            node.size += child.size;
            node.children.push(child);
        }
        node.children.sort_by_key(|c| std::cmp::Reverse(c.size));
        if node.children.len() > MAX_CHILDREN {
            let rest: Vec<Node> = node.children.split_off(MAX_CHILDREN);
            let other = rest.iter().map(|n| n.size).sum();
            node.children.push(Node { name: format!("… {} more", rest.len()), path: dir.to_path_buf(), size: other, is_dir: false, children: Vec::new() });
        }
        node
    }
    walk(root, cancel)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Squarified treemap (Bruls, Huizing, van Wijk): lay `sizes` (descending)
/// into `rect`, returning one rectangle per size in the same order.
pub fn squarify(sizes: &[u64], rect: Rect) -> Vec<Rect> {
    let total: f64 = sizes.iter().map(|s| *s as f64).sum();
    let mut out = Vec::with_capacity(sizes.len());
    if total <= 0.0 || rect.w <= 0.0 || rect.h <= 0.0 {
        return sizes.iter().map(|_| Rect { x: rect.x, y: rect.y, w: 0.0, h: 0.0 }).collect();
    }
    let scale = rect.w * rect.h / total;
    let areas: Vec<f64> = sizes.iter().map(|s| *s as f64 * scale).collect();
    let mut free = rect;
    let mut i = 0;
    while i < areas.len() {
        // Grow a row while the worst aspect ratio improves.
        let side = free.w.min(free.h);
        let mut row_end = i + 1;
        let mut best = worst(&areas[i..row_end], side);
        while row_end < areas.len() {
            let next = worst(&areas[i..row_end + 1], side);
            if next > best {
                break;
            }
            best = next;
            row_end += 1;
        }
        let row_area: f64 = areas[i..row_end].iter().sum();
        if free.w >= free.h {
            // Vertical strip on the left.
            let strip_w = if free.h > 0.0 { row_area / free.h } else { 0.0 };
            let mut y = free.y;
            for a in &areas[i..row_end] {
                let h = if strip_w > 0.0 { a / strip_w } else { 0.0 };
                out.push(Rect { x: free.x, y, w: strip_w, h });
                y += h;
            }
            free.x += strip_w;
            free.w -= strip_w;
        } else {
            let strip_h = if free.w > 0.0 { row_area / free.w } else { 0.0 };
            let mut x = free.x;
            for a in &areas[i..row_end] {
                let w = if strip_h > 0.0 { a / strip_h } else { 0.0 };
                out.push(Rect { x, y: free.y, w, h: strip_h });
                x += w;
            }
            free.y += strip_h;
            free.h -= strip_h;
        }
        i = row_end;
    }
    out
}

fn worst(row: &[f64], side: f64) -> f64 {
    let sum: f64 = row.iter().sum();
    if sum <= 0.0 || side <= 0.0 {
        return f64::MAX;
    }
    let (mut max, mut min) = (0.0f64, f64::MAX);
    for a in row {
        max = max.max(*a);
        min = min.min(*a);
    }
    let s2 = sum * sum;
    (side * side * max / s2).max(s2 / (side * side * min))
}

struct Laid {
    rect: Rect,
    /// Index path from the shown root: [child] or [child, grandchild].
    path: Vec<usize>,
}

pub struct Treemap {
    dialog: adw::Dialog,
    area: gtk::DrawingArea,
    title: adw::WindowTitle,
    up: gtk::Button,
    spinner: gtk::Spinner,
    root: RefCell<Option<Node>>,
    /// Index path into `root` for the folder on show.
    cursor: RefCell<Vec<usize>>,
    layout: RefCell<Vec<Laid>>,
    hover: Cell<Option<usize>>,
    cancel: Arc<AtomicBool>,
    owner: std::rc::Weak<Window>,
}

impl Treemap {
    pub fn open(owner: &Rc<Window>, folder: PathBuf) {
        let header = adw::HeaderBar::new();
        let title = adw::WindowTitle::new(&tr("Disk Usage Map"), &crate::rules::shorten(&folder));
        header.set_title_widget(Some(&title));
        let up = gtk::Button::builder().icon_name("go-up-symbolic").tooltip_text(tr("Up one level (Backspace)")).sensitive(false).build();
        header.pack_start(&up);
        let spinner = gtk::Spinner::new();
        spinner.start();
        header.pack_end(&spinner);
        let area = gtk::DrawingArea::new();
        area.set_vexpand(true);
        area.set_hexpand(true);
        area.set_has_tooltip(true);
        area.add_css_class("omafile-treemap");
        let hint = gtk::Label::new(Some(&tr("Click zooms into a folder  ·  double click opens it in omafile  ·  right click: Open, Reveal, Move to Trash  ·  Backspace goes up")));
        hint.add_css_class("omafile-dim");
        hint.add_css_class("caption");
        hint.set_margin_bottom(6);
        hint.set_wrap(true);
        let view = adw::ToolbarView::new();
        view.add_top_bar(&header);
        view.set_content(Some(&area));
        view.add_bottom_bar(&hint);
        let dialog = adw::Dialog::builder().title(tr("Disk Usage Map")).content_width(960).content_height(700).child(&view).build();
        let this = Rc::new(Treemap { dialog, area, title, up, spinner, root: RefCell::new(None), cursor: RefCell::new(Vec::new()), layout: RefCell::new(Vec::new()), hover: Cell::new(None), cancel: Arc::new(AtomicBool::new(false)), owner: Rc::downgrade(owner) });

        let weak = Rc::downgrade(&this);
        this.area.set_draw_func(move |_, cr, w, h| {
            if let Some(t) = weak.upgrade() {
                t.draw(cr, w as f64, h as f64);
            }
        });
        let weak = Rc::downgrade(&this);
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(move |_, x, y| {
            if let Some(t) = weak.upgrade() {
                let hit = t.hit(x, y);
                if hit != t.hover.get() {
                    t.hover.set(hit);
                    t.area.queue_draw();
                }
            }
        });
        let weak = Rc::downgrade(&this);
        motion.connect_leave(move |_| {
            if let Some(t) = weak.upgrade() {
                t.hover.set(None);
                t.area.queue_draw();
            }
        });
        this.area.add_controller(motion);
        let weak = Rc::downgrade(&this);
        this.area.connect_query_tooltip(move |_, x, y, _, tip| {
            let Some(t) = weak.upgrade() else { return false };
            let Some(node) = t.hit(x as f64, y as f64).and_then(|i| t.node_at(i)) else { return false };
            tip.set_text(Some(&format!("{}\n{}", crate::rules::shorten(&node.path), ops::human_size(node.size))));
            true
        });
        let weak = Rc::downgrade(&this);
        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_PRIMARY);
        click.connect_released(move |g, n, x, y| {
            let Some(t) = weak.upgrade() else { return };
            let Some(i) = t.hit(x, y) else { return };
            let Some(node) = t.node_at(i) else { return };
            if n >= 2 {
                if let Some(w) = t.owner.upgrade() {
                    let target = if node.is_dir { node.path.clone() } else { node.path.parent().map(Path::to_path_buf).unwrap_or(node.path.clone()) };
                    if let Some(p) = w.active_pane() {
                        p.navigate(gio::File::for_path(&target), true);
                        if !node.is_dir {
                            p.select_file_later(gio::File::for_path(&node.path));
                        }
                    }
                    t.dialog.close();
                }
                g.set_state(gtk::EventSequenceState::Claimed);
            } else if node.is_dir && !node.children.is_empty() {
                let path = t.layout.borrow().get(i).map(|l| l.path.clone()).unwrap_or_default();
                // Zoom to the top-level child under the pointer.
                if let Some(first) = path.first() {
                    t.cursor.borrow_mut().push(*first);
                    t.relayout();
                }
            }
        });
        this.area.add_controller(click);
        let weak = Rc::downgrade(&this);
        let right = gtk::GestureClick::new();
        right.set_button(gdk::BUTTON_SECONDARY);
        right.connect_pressed(move |_, _, x, y| {
            if let Some(t) = weak.upgrade() {
                t.context_menu(x, y);
            }
        });
        this.area.add_controller(right);
        let weak = Rc::downgrade(&this);
        this.up.connect_clicked(move |_| { if let Some(t) = weak.upgrade() { t.go_up(); } });
        let weak = Rc::downgrade(&this);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            match (weak.upgrade(), key) {
                (Some(t), gdk::Key::BackSpace) | (Some(t), gdk::Key::Up) => {
                    t.go_up();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        this.dialog.add_controller(keys);
        // The dialog keeps the map alive until it closes.
        let keep = RefCell::new(Some(this.clone()));
        let cancel = this.cancel.clone();
        this.dialog.connect_closed(move |_| {
            cancel.store(true, Ordering::Relaxed);
            keep.borrow_mut().take();
        });

        owner.present_dialog(&this.dialog);
        // Scan off the main thread, then show.
        let weak = Rc::downgrade(&this);
        let cancel = this.cancel.clone();
        glib::spawn_future_local(async move {
            let node = gio::spawn_blocking(move || scan(&folder, &cancel)).await.ok();
            let Some(t) = weak.upgrade() else { return };
            t.spinner.stop();
            t.spinner.set_visible(false);
            if let Some(node) = node {
                t.root.replace(Some(node));
                t.relayout();
            }
        });
    }

    fn current(&self) -> Option<Node> {
        let root = self.root.borrow();
        let mut node = root.as_ref()?;
        for i in self.cursor.borrow().iter() {
            node = node.children.get(*i)?;
        }
        Some(node.clone())
    }

    fn node_at(&self, laid_index: usize) -> Option<Node> {
        let path = self.layout.borrow().get(laid_index)?.path.clone();
        let mut node = self.current()?;
        for i in path {
            node = node.children.get(i)?.clone();
        }
        Some(node)
    }

    fn go_up(&self) {
        if self.cursor.borrow_mut().pop().is_some() {
            self.relayout();
        }
    }

    fn hit(&self, x: f64, y: f64) -> Option<usize> {
        // Prefer the deepest (last laid) rectangle under the pointer.
        self.layout.borrow().iter().enumerate().rev().find(|(_, l)| x >= l.rect.x && x <= l.rect.x + l.rect.w && y >= l.rect.y && y <= l.rect.y + l.rect.h).map(|(i, _)| i)
    }

    fn relayout(&self) {
        let Some(node) = self.current() else { return };
        self.title.set_subtitle(&format!("{}  ·  {}", crate::rules::shorten(&node.path), ops::human_size(node.size)));
        self.up.set_sensitive(!self.cursor.borrow().is_empty());
        self.hover.set(None);
        self.area.queue_draw();
    }

    fn draw(&self, cr: &gtk::cairo::Context, width: f64, height: f64) {
        let Some(node) = self.current() else { return };
        let pad = 6.0;
        let outer = Rect { x: pad, y: pad, w: (width - 2.0 * pad).max(0.0), h: (height - 2.0 * pad).max(0.0) };
        let sizes: Vec<u64> = node.children.iter().map(|c| c.size.max(1)).collect();
        let rects = squarify(&sizes, outer);
        let dark = adw::StyleManager::default().is_dark();
        let fg = self.area.color();
        let mut layout: Vec<Laid> = Vec::new();
        let hover = self.hover.get();
        cr.set_line_width(1.0);
        for (i, (child, rect)) in node.children.iter().zip(rects.iter()).enumerate() {
            if rect.w < 1.0 || rect.h < 1.0 {
                continue;
            }
            let hue = (i as f64 * 137.508) % 360.0;
            let (r, g, b) = hsl(hue, if dark { 0.45 } else { 0.55 }, if dark { 0.36 } else { 0.72 });
            layout.push(Laid { rect: *rect, path: vec![i] });
            cr.rectangle(rect.x + 0.5, rect.y + 0.5, (rect.w - 1.0).max(0.0), (rect.h - 1.0).max(0.0));
            cr.set_source_rgb(r, g, b);
            let _ = cr.fill_preserve();
            cr.set_source_rgba(0.0, 0.0, 0.0, if dark { 0.5 } else { 0.25 });
            let _ = cr.stroke();
            // Second level inside folders.
            if child.is_dir && !child.children.is_empty() && rect.w > 40.0 && rect.h > 40.0 {
                let inner = Rect { x: rect.x + 4.0, y: rect.y + 18.0, w: rect.w - 8.0, h: (rect.h - 22.0).max(0.0) };
                let sub: Vec<u64> = child.children.iter().map(|c| c.size.max(1)).collect();
                for (j, (grand, sr)) in child.children.iter().zip(squarify(&sub, inner).iter()).enumerate() {
                    if sr.w < 3.0 || sr.h < 3.0 {
                        continue;
                    }
                    let (r2, g2, b2) = hsl(hue, if dark { 0.35 } else { 0.45 }, if dark { 0.28 } else { 0.8 });
                    cr.rectangle(sr.x + 0.5, sr.y + 0.5, (sr.w - 1.0).max(0.0), (sr.h - 1.0).max(0.0));
                    cr.set_source_rgb(r2, g2, b2);
                    let _ = cr.fill_preserve();
                    cr.set_source_rgba(0.0, 0.0, 0.0, if dark { 0.35 } else { 0.15 });
                    let _ = cr.stroke();
                    if sr.w > 50.0 && sr.h > 14.0 {
                        cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.8);
                        cr.set_font_size(10.0);
                        cr.move_to(sr.x + 4.0, sr.y + 12.0);
                        let _ = cr.show_text(&clip(&grand.name, sr.w - 8.0, 5.5));
                    }
                    layout.push(Laid { rect: *sr, path: vec![i, j] });
                }
            }
            if rect.w > 36.0 && rect.h > 16.0 {
                cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.95);
                cr.set_font_size(12.0);
                cr.move_to(rect.x + 5.0, rect.y + 13.0);
                let label = format!("{}  {}", child.name, ops::human_size(child.size));
                let _ = cr.show_text(&clip(&label, rect.w - 10.0, 6.5));
            }
        }
        if let Some(h) = hover.and_then(|h| layout.get(h).map(|l| l.rect)) {
            cr.rectangle(h.x + 1.0, h.y + 1.0, (h.w - 2.0).max(0.0), (h.h - 2.0).max(0.0));
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.9);
            cr.set_line_width(2.0);
            let _ = cr.stroke();
        }
        self.layout.replace(layout);
    }

    fn context_menu(self: &Rc<Self>, x: f64, y: f64) {
        let Some(i) = self.hit(x, y) else { return };
        let Some(node) = self.node_at(i) else { return };
        let menu = gio::Menu::new();
        menu.append(Some(tr("Open").as_str()), Some("map.open"));
        menu.append(Some(tr("Reveal in omafile").as_str()), Some("map.reveal"));
        menu.append(Some(tr("Move to Trash").as_str()), Some("map.trash"));
        let group = gio::SimpleActionGroup::new();
        let path = node.path.clone();
        let weak = Rc::downgrade(self);
        let open = gio::SimpleAction::new("open", None);
        let p = path.clone();
        open.connect_activate(move |_, _| {
            if let Some(w) = weak.upgrade().and_then(|t| t.owner.upgrade()) {
                w.launch(&gio::File::for_path(&p), false);
            }
        });
        group.add_action(&open);
        let weak = Rc::downgrade(self);
        let reveal = gio::SimpleAction::new("reveal", None);
        let p = path.clone();
        reveal.connect_activate(move |_, _| {
            if let Some(t) = weak.upgrade() {
                if let Some(w) = t.owner.upgrade() {
                    w.reveal(gio::File::for_path(&p));
                }
                t.dialog.close();
            }
        });
        group.add_action(&reveal);
        let weak = Rc::downgrade(self);
        let trash = gio::SimpleAction::new("trash", None);
        let p = path.clone();
        trash.connect_activate(move |_, _| {
            let Some(t) = weak.upgrade() else { return };
            let Some(w) = t.owner.upgrade() else { return };
            let file = gio::File::for_path(&p);
            let t2 = t.clone();
            let p = p.clone();
            glib::spawn_future_local(async move {
                match ops::trash(file.clone()).await {
                    Ok(()) => {
                        w.tags.removed(&file);
                        w.record(crate::undo::Op::Trash(vec![file.clone()]), &trf!("Moved {name} to trash", name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
                        t2.remove_path(&p);
                    }
                    Err(e) => w.toast_error(e.message()),
                }
            });
        });
        group.add_action(&trash);
        self.area.insert_action_group("map", Some(&group));
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_parent(&self.area);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.set_has_arrow(false);
        popover.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    }

    /// Drop a trashed entry from the tree and fix the sizes up the chain.
    fn remove_path(&self, path: &Path) {
        fn prune(node: &mut Node, path: &Path) -> u64 {
            if let Some(pos) = node.children.iter().position(|c| c.path == path) {
                let gone = node.children.remove(pos).size;
                node.size -= gone;
                return gone;
            }
            for child in node.children.iter_mut() {
                if path.starts_with(&child.path) {
                    let gone = prune(child, path);
                    node.size -= gone;
                    return gone;
                }
            }
            0
        }
        if let Some(root) = self.root.borrow_mut().as_mut() {
            prune(root, path);
        }
        self.relayout();
    }
}

fn clip(text: &str, width: f64, char_w: f64) -> String {
    let max = (width / char_w).max(0.0) as usize;
    if text.chars().count() <= max {
        text.to_string()
    } else if max > 1 {
        text.chars().take(max - 1).collect::<String>() + "…"
    } else {
        String::new()
    }
}

fn hsl(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (r + m, g + m, b + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squarify_fills_the_rect() {
        let rect = Rect { x: 0.0, y: 0.0, w: 100.0, h: 60.0 };
        let rects = squarify(&[6, 6, 4, 3, 2, 2, 1], rect);
        assert_eq!(rects.len(), 7);
        let area: f64 = rects.iter().map(|r| r.w * r.h).sum();
        assert!((area - 6000.0).abs() < 1.0, "area {area}");
        for r in &rects {
            assert!(r.x >= -0.01 && r.y >= -0.01 && r.x + r.w <= 100.01 && r.y + r.h <= 60.01, "{r:?}");
        }
    }

    #[test]
    fn squarify_keeps_order_and_handles_empty() {
        let rects = squarify(&[10, 1], Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 });
        assert!(rects[0].w * rects[0].h > rects[1].w * rects[1].h);
        assert!(squarify(&[], Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }).is_empty());
        assert_eq!(squarify(&[0, 0], Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }).len(), 2);
    }

    #[test]
    fn scan_sums_sizes() {
        let dir = std::env::temp_dir().join(format!("omafile-treemap-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.bin"), vec![0u8; 300]).unwrap();
        std::fs::write(dir.join("sub/b.bin"), vec![0u8; 200]).unwrap();
        let node = scan(&dir, &AtomicBool::new(false));
        assert_eq!(node.size, 500);
        assert_eq!(node.children[0].name, "a.bin");
        assert!(node.children[1].is_dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
