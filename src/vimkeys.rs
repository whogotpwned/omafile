//! Vim-style keys on the file views. Off by default; Preferences has one
//! master switch and one switch per group, so `hjkl` can stay on while `dd`
//! stays off. They only act while a file view has focus, never in text fields,
//! and they sit in the capture phase so type-ahead does not swallow them.

use crate::pane::Pane;
use crate::prefs::Prefs;
use crate::window::Window;
use adw::prelude::*;
use gtk::{gdk, gio};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// (id, keys as shown, what the group does). Ids are stored in `vim-keys-off`.
pub const GROUPS: &[(&str, &str, &str)] = &[
    ("hjkl", "h  j  k  l", "Parent folder, down, up, open"),
    ("gG", "gg  G", "First and last item"),
    ("search", "/", "Filter this folder"),
    ("colon", ":", "Command palette"),
    ("dd", "dd", "Move to trash"),
    ("yank", "yy  p  x", "Copy, paste, cut"),
    ("undo", "u", "Undo"),
];

const CHORD_TIMEOUT: Duration = Duration::from_millis(900);

thread_local! {
    /// First key of a two-key chord (`g`, `d`, `y`) and when it was pressed.
    static PENDING: Cell<Option<(char, Instant)>> = const { Cell::new(None) };
}

pub fn enabled(prefs: &Prefs) -> bool {
    prefs.get_bool("vim-keys", false)
}

/// Ids switched off while the master switch is on.
pub fn disabled(prefs: &Prefs) -> Vec<String> {
    parse_off(&prefs.get("vim-keys-off").unwrap_or_default())
}

pub fn parse_off(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect()
}

pub fn group_enabled(prefs: &Prefs, id: &str) -> bool {
    enabled(prefs) && !disabled(prefs).iter().any(|g| g == id)
}

pub fn set_group(prefs: &Prefs, id: &str, on: bool) {
    let mut off = disabled(prefs);
    off.retain(|g| g != id);
    if !on {
        off.push(id.to_string());
    }
    prefs.set("vim-keys-off", Some(&off.join(",")));
}

/// Handle one key press on a file view. Returns true when the key was consumed.
pub fn handle(pane: &Rc<Pane>, w: &Rc<Window>, key: gdk::Key, state: gdk::ModifierType) -> bool {
    if state.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK) || !enabled(&w.prefs) {
        return false;
    }
    let Some(ch) = key.to_unicode() else { return false };
    let pending = PENDING.with(|p| p.take()).filter(|(_, at)| at.elapsed() < CHORD_TIMEOUT).map(|(c, _)| c);
    let on = |id: &str| group_enabled(&w.prefs, id);
    let act = |name: &str| gio::prelude::ActionGroupExt::activate_action(&w.window, name, None);
    match (pending, ch) {
        (Some('g'), 'g') if on("gG") => move_to(pane, Target::First),
        (Some('d'), 'd') if on("dd") => act("trash"),
        (Some('y'), 'y') if on("yank") => act("copy"),
        (None, 'g') if on("gG") => return arm('g'),
        (None, 'd') if on("dd") => return arm('d'),
        (None, 'y') if on("yank") => return arm('y'),
        (_, 'G') if on("gG") => move_to(pane, Target::Last),
        (_, 'j') if on("hjkl") => move_to(pane, Target::Step(1)),
        (_, 'k') if on("hjkl") => move_to(pane, Target::Step(-1)),
        (_, 'h') if on("hjkl") => pane.go_up(),
        (_, 'l') if on("hjkl") => {
            if let Some(item) = pane.selected_items().into_iter().next() {
                pane.open_item(&item);
            }
        }
        (_, '/') if on("search") => act("search"),
        (_, ':') if on("colon") => act("palette"),
        (_, 'p') if on("yank") => act("paste"),
        (_, 'x') if on("yank") => act("cut"),
        (_, 'u') if on("undo") => act("undo"),
        _ => return false,
    }
    true
}

fn arm(c: char) -> bool {
    PENDING.with(|p| p.set(Some((c, Instant::now()))));
    true
}

enum Target {
    First,
    Last,
    Step(i64),
}

/// Move the single selection and the keyboard cursor together.
fn move_to(pane: &Rc<Pane>, target: Target) {
    let n = pane.selection.n_items();
    if n == 0 {
        return;
    }
    let current = (0..n).find(|i| pane.selection.is_selected(*i));
    let next = match target {
        Target::First => 0,
        Target::Last => n - 1,
        Target::Step(d) => match current {
            Some(i) => (i as i64 + d).clamp(0, n as i64 - 1) as u32,
            None if d > 0 => 0,
            None => n - 1,
        },
    };
    pane.selection.select_item(next, true);
    let flags = gtk::ListScrollFlags::FOCUS;
    if pane.is_grid() {
        pane.grid_view.scroll_to(next, flags, None);
    } else {
        pane.column_view.scroll_to(next, None, flags, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_disabled_list() {
        assert_eq!(parse_off(""), Vec::<String>::new());
        assert_eq!(parse_off("dd, yank,,"), vec!["dd".to_string(), "yank".to_string()]);
    }

    #[test]
    fn every_group_has_a_unique_id() {
        let mut ids: Vec<&str> = GROUPS.iter().map(|(id, _, _)| *id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), GROUPS.len());
    }
}
