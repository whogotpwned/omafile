//! Python venv actions: create, activate in the terminal panel, install,
//! freeze, REPL, run a script, .gitignore, recreate. Commands run in the
//! embedded terminal so their output stays visible.

use crate::pyenv;
use crate::window::Window;
use adw::prelude::*;
use gtk::glib;
use std::path::PathBuf;
use std::rc::Rc;

/// What the Python submenu should offer for the current folder.
#[derive(Default, Clone, Debug)]
pub struct PyMenu {
    pub has_venv: bool,
    pub has_requirements: bool,
    pub gitignore_missing: bool,
}

pub fn menu_state(dir: &std::path::Path, in_repo: bool) -> Option<PyMenu> {
    let venv = pyenv::find(dir);
    let root = pyenv::project_root(dir);
    let project = pyenv::is_python_project(dir) || venv.is_some();
    if !project {
        return None;
    }
    let gitignore_missing = in_repo && venv.as_ref().map(|v| {
        let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
        let name = v.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        !ignore.lines().any(|l| l.trim().trim_end_matches('/') == name || l.trim() == format!("/{name}") || l.trim() == "*")
    }).unwrap_or(false);
    Some(PyMenu { has_venv: venv.is_some(), has_requirements: root.join("requirements.txt").exists() || root.join("pyproject.toml").exists(), gitignore_missing })
}

impl Window {
    fn py_dir(&self) -> Option<PathBuf> {
        self.active_pane().and_then(|p| p.cwd().path())
    }

    /// Open the terminal panel (if needed) and type a command into it.
    pub fn run_in_panel(self: &Rc<Self>, command: &str) {
        let Some(pane) = self.active_pane() else { return };
        if !pane.terminal_visible() {
            pane.toggle_terminal();
        }
        let term = pane.term.borrow().clone();
        let Some(term) = term else { return };
        let cmd = command.to_string();
        // Give a freshly spawned shell a moment to print its prompt.
        glib::timeout_add_local_once(std::time::Duration::from_millis(if pane.terminal_visible() { 350 } else { 900 }), move || {
            term.run_line(&cmd);
        });
    }

    pub fn py_create(self: &Rc<Self>) {
        let Some(dir) = self.py_dir() else { return };
        let root = pyenv::project_root(&dir);
        let cd = if root != dir { format!("cd '{}' && ", root.to_string_lossy()) } else { String::new() };
        self.run_in_panel(&format!("{cd}{}", pyenv::create_command()));
        self.toast(&format!("Creating .venv in {}", crate::window::abbreviate_home(&root)));
    }

    pub fn py_activate(self: &Rc<Self>) {
        let Some(dir) = self.py_dir() else { return };
        let Some(venv) = pyenv::find(&dir) else {
            self.toast("No virtual environment here (Python › Create venv)");
            return;
        };
        let Some(pane) = self.active_pane() else { return };
        if !pane.terminal_visible() {
            pane.toggle_terminal();
        }
        let term = pane.term.borrow().clone();
        if let Some(t) = term {
            let v = venv.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || t.activate_venv(&v));
        }
    }

    pub fn py_install(self: &Rc<Self>) {
        let Some(dir) = self.py_dir() else { return };
        let root = pyenv::project_root(&dir);
        let Some(venv) = pyenv::find(&dir) else {
            self.toast("Create a venv first (Python › Create venv)");
            return;
        };
        let cd = if root != dir { format!("cd '{}' && ", root.to_string_lossy()) } else { String::new() };
        self.run_in_panel(&format!("{cd}{}", pyenv::install_command(&venv, &root)));
    }

    pub fn py_freeze(self: &Rc<Self>) {
        let Some(dir) = self.py_dir() else { return };
        let Some(venv) = pyenv::find(&dir) else {
            self.toast("No virtual environment here");
            return;
        };
        self.run_in_panel(&pyenv::freeze_command(&venv));
    }

    pub fn py_repl(self: &Rc<Self>) {
        let Some(dir) = self.py_dir() else { return };
        let py = pyenv::find(&dir).map(|v| v.root.join("bin/python").to_string_lossy().to_string()).unwrap_or_else(|| "python3".into());
        self.run_in_panel(&format!("'{py}'"));
    }

    /// Run the selected .py file with the venv's interpreter.
    pub fn py_run(self: &Rc<Self>) {
        let Some(item) = self.selected_items().into_iter().find(|i| i.name().ends_with(".py")) else { return };
        let Some(path) = item.target_file().path() else { return };
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let py = pyenv::find(&dir).map(|v| v.root.join("bin/python").to_string_lossy().to_string()).unwrap_or_else(|| "python3".into());
        self.run_in_panel(&format!("'{py}' '{}'", path.to_string_lossy().replace('\'', "'\\''")));
    }

    pub fn py_gitignore(self: &Rc<Self>) {
        let Some(dir) = self.py_dir() else { return };
        let Some(venv) = pyenv::find(&dir) else { return };
        let root = self.active_pane().and_then(|p| p.git.borrow().as_ref().map(|r| r.root.clone())).unwrap_or_else(|| pyenv::project_root(&dir));
        let name = venv.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(".venv".into());
        let path = root.join(".gitignore");
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("{name}/\n"));
        match std::fs::write(&path, text) {
            Ok(()) => {
                self.toast(&format!("Added {name}/ to .gitignore"));
                if let Some(p) = self.active_pane() {
                    p.reload();
                }
            }
            Err(e) => self.toast_error(&format!("Cannot write .gitignore: {e}")),
        }
    }

    pub fn py_recreate(self: &Rc<Self>) {
        let Some(dir) = self.py_dir() else { return };
        let Some(venv) = pyenv::find(&dir) else { return };
        let name = venv.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let body = format!("{} is deleted and created again with {}. Installed packages are gone until you run Install again.", crate::window::abbreviate_home(&venv.root), if pyenv::uv_available() { "uv" } else { "python -m venv" });
        let dialog = adw::AlertDialog::new(Some(&format!("Recreate {name}?")), Some(&body));
        dialog.add_responses(&[("cancel", "Cancel"), ("recreate", "Recreate")]);
        dialog.set_response_appearance("recreate", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let this = Rc::downgrade(self);
        dialog.choose(Some(&self.window), gtk::gio::Cancellable::NONE, move |r| {
            let Some(w) = this.upgrade() else { return };
            w.refocus();
            if r != "recreate" {
                return;
            }
            let root = venv.root.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let create = pyenv::create_command().replace(".venv", &name);
            w.run_in_panel(&format!("cd '{}' && rm -rf '{name}' && {create}", root.to_string_lossy()));
        });
    }
}
