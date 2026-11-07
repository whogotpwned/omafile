mod app;
mod fs;
mod icons;
mod preview;
mod theme;
mod ui;

use app::{App, External};
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui::crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::crossterm::execute;
use ratatui::prelude::*;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const USAGE: &str = "\
omafile - a keyboard-driven file manager for Omarchy

USAGE:
    omafile [OPTIONS] [PATH]

ARGS:
    PATH                 directory to open, or a file to select (default: cwd)

OPTIONS:
    --cwd-file <FILE>    write the last visited directory to FILE on exit
                         (used by the shell `cd` wrapper, see README)
    --no-icons           use ASCII instead of Nerd Font glyphs
    -h, --help           print this help
    -V, --version        print the version

ENVIRONMENT:
    EDITOR / VISUAL      editor for text files (default: nvim)
    TERMINAL             terminal spawned with `t` (default: alacritty)
    OMAFILE_THEME_DIR    override ~/.config/omarchy/current/theme
    OMAFILE_ICONS=0      same as --no-icons
";

struct Args {
    path: Option<PathBuf>,
    cwd_file: Option<PathBuf>,
    icons: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        path: None,
        cwd_file: None,
        icons: std::env::var("OMAFILE_ICONS").map(|v| v != "0").unwrap_or(true),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("omafile {VERSION}");
                std::process::exit(0);
            }
            "--no-icons" => args.icons = false,
            "--cwd-file" => {
                args.cwd_file = Some(PathBuf::from(it.next().ok_or("--cwd-file needs a value")?));
            }
            s if s.starts_with("--cwd-file=") => {
                args.cwd_file = Some(PathBuf::from(&s["--cwd-file=".len()..]));
            }
            s if s.starts_with('-') && s != "-" => return Err(format!("unknown option: {s}")),
            _ => args.path = Some(PathBuf::from(arg)),
        }
    }
    Ok(args)
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(TerminalGuard)
    }

    fn leave() {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        TerminalGuard::leave();
    }
}

/// Hand the terminal to another program and take it back afterwards.
fn run_external(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &App, ext: External) -> io::Result<()> {
    TerminalGuard::leave();
    let status = match ext {
        External::Editor(path) => {
            let editor = std::env::var("VISUAL")
                .or_else(|_| std::env::var("EDITOR"))
                .unwrap_or_else(|_| "nvim".into());
            Command::new("sh")
                .arg("-c")
                .arg(format!("{editor} \"$1\""))
                .arg("omafile")
                .arg(&path)
                .current_dir(&app.cwd)
                .status()
        }
        External::Shell => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".into());
            println!("omafile: shell in {} (exit to return)", fs::display_path(&app.cwd));
            let _ = io::stdout().flush();
            Command::new(&shell).current_dir(&app.cwd).status()
        }
    };
    let _guard = TerminalGuard::enter()?;
    std::mem::forget(_guard); // the outer guard in main owns teardown
    terminal.clear()?;
    status.map(|_| ())
}

fn main() -> io::Result<()> {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("omafile: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let mut start = args.path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
    let mut select: Option<PathBuf> = None;
    if start.is_file() {
        select = std::fs::canonicalize(&start).ok();
        start = start.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/"));
    }
    let start = std::fs::canonicalize(&start).unwrap_or(start);
    if !start.is_dir() {
        eprintln!("omafile: {}: not a directory", start.display());
        std::process::exit(1);
    }

    let mut app = App::new(start, args.icons, args.cwd_file);
    if let Some(sel) = select {
        app.reload(Some(&sel));
    }

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        TerminalGuard::leave();
        default_hook(info);
    }));

    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    loop {
        terminal.draw(|f| ui::draw(f, &mut app))?;
        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => app.handle_key(key),
                _ => {}
            }
        }
        app.tick();
        if let Some(ext) = app.external.take() {
            if let Err(e) = run_external(&mut terminal, &app, ext) {
                app.error(format!("{e}"));
            }
            app.reload(None);
        }
        if app.should_quit {
            break;
        }
    }

    drop(_guard);
    app.write_cwd_file();
    Ok(())
}
