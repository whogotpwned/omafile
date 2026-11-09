# Changelog

All notable changes to omafile. The version here matches `Cargo.toml`, `PKGBUILD`, `manifest.json` and the README badge; `plugin/validate.sh` checks they agree.

## 0.12.0 — 2026-11-09

### Added
- **Shelf** (`Ctrl+Shift+S`, `Ctrl+Shift+B`, context menu › Add to Shelf, or drop files onto it): a floating window that collects files from anywhere; drag them out together or *Copy Here* / *Move Here* into the folder on show. Persists across restarts.
- **Disk usage map** (`Ctrl+Alt+U`): squarified treemap of the folder, click to zoom, double click to open, right click to reveal or trash. Scans off the main thread.
- **Folder rules** (`Ctrl+Alt+R`): Hazel-style rules per folder (glob, age; move, copy, tag, trash, sort into YYYY-MM, run a command with `$FILE`), with switches and *Run Now*; run at start and when a watched folder changes; undoable.
- **Find duplicates** (`Ctrl+Alt+F`): same size and SHA-256 below a folder, grouped, keep-policy, *Move Ticked to Trash* in one undoable step.
- A *Tools* section in the bindings overlay, palette entries and menu items for all four.

## 0.11.0 — 2026-11-09

### Added
- **Page turning for PDFs** in the preview pane and in Quick Look: a `‹ 2 / 3 ›` turner under the page, the wheel over the page, `↑` / `↓` (and `PgUp` / `PgDn`, `j` / `k`) in Quick Look, `Alt+PgUp` / `Alt+PgDn` in the preview pane. Pages are rendered on demand by poppler off the main thread.

## 0.10.0 — 2026-11-09

### Added
- **Vim keys**, off by default: `h` parent, `l` open, `j` / `k` move, `gg` / `G` first and last, `/` filter, `:` palette, `dd` trash, `yy` / `p` / `x` copy, paste, cut, `u` undo. Preferences › Keys has a master switch and one switch per group (`vim-keys`, `vim-keys-off` in `omafile.conf`); the keys act only while the file list has focus. Listed in the bindings overlay.
- Preferences: the macOS keys switch moved into the new *Keys* group.

## 0.9.0 — 2026-11-09

### Added
- **Languages.** The UI follows the Omarchy language (`~/.config/omarchy/language`, *Setup › Language*), falling back to `LC_ALL` / `LC_MESSAGES` / `LANG`; `OMAFILE_LANG` forces one. Complete catalogs for German, Spanish, French, Italian, Portuguese and Dutch in `data/i18n/`; missing strings stay English; `~/.config/omafile/i18n/<code>.json` overrides key by key.
- The language is handed to gettext before GTK starts, so GTK and GIO strings (file dialog, file-type names) follow as well.
- Bar widget translated through the shell's `Commons.Translations` with catalogs in `plugin/i18n/`; desktop entry with translated names and keywords.
- README section *Languages* with detection order and how to add a language.

## 0.8.0 — 2026-11-08

### Added
- **Finder views:** column view (Miller columns, `Ctrl+3`), gallery view with filmstrip (`Ctrl+4`), groups by kind / date / size / tag with section headers, `Ctrl+scroll` zoom with remembered icon size.
- Full-text search with ripgrep (content toggle in the search bar), spring-loaded folders and sidebar places on drag, type-ahead selection.
- Renderers in the preview pane: Markdown, CSV, JSON via jq, syntax highlighting via bat, font specimens; media facts (dimensions, camera, exposure, duration, bitrate) via pixbuf, ImageMagick and ffprobe.
- PDF tools (merge, split, extract pages, create from images), video tools (trim, GIF, extract audio), audio conversion (FLAC, Opus, MP3).
- Finder habits: New Folder with Selection, Make Alias / Show Original, Always Open With, multi-item properties with total size and kinds, `Enter` renames with macOS keys.

## 0.7.0 — 2026-11-08

### Added
- Permissions and ownership editor in Properties: plain-language sentence, check boxes, octal, owner and group, special bits, apply to enclosed files; undoable.
- Smart folders (saved searches in the sidebar), compare two files or folders, dual-pane copy and move (`F5` / `F6`), bulk palette commands with preview (rename, chmod, touch, mkdir), SHA-256 checksum verification.
- Notes on files (GIO annotations, searchable in the palette), templates (`~/Templates` and built-in scaffolds), browsing inside archives, trash auto-cleanup after N days.
- Omarchy theme export and install from the file manager, `omafile --toggle` for a Hyprland show / hide binding, bindings overlay (`F1`, `Ctrl+?`) and `--print-bindings`.
- Python: venv detection badge, requirements check, create / install / freeze / REPL / recreate, auto-activation in the terminal panel.

## 0.6.0 — 2026-11-08

### Added
- Transfer queue with progress banner and conflict dialog (replace, skip, keep both, apply to all).
- Folder sizes on demand, trash columns (deleted, original location), inline rename.
- Git actions: stage, unstage, discard, commit, switch branch; coloured diff preview.
- Secrets guard: credential-looking files are masked in the preview and copying asks first.
- Container volumes in the sidebar, Omarchy Themes place, `--print-recent`, bar status badge (busy, unlocked vaults, lock all).
- Image tools (rotate, resize, convert), sharing on the local network and scp command, session restore.
- Icon previews for text, video and music; play badge on media icons; hidden-files toggle with dimmed items; in-place media player; panel and view animations.

## 0.5.0 — 2026-11-08

### Added
- App icon: a folder with a prompt on a Tokyo Night tile; installed as the window and launcher icon.
- Requirements documented: what stock Omarchy ships and what to install.

## 0.4.0 — 2026-11-08

### Added
- Follows current Omarchy themes (`colors.toml`, live re-skin on theme switch), PDF previews, custom folder icons, search, batch rename, split view.
- Omarchy shell plugin: bar button with a places menu, `manifest.json`, `install.sh`.
- Terminal panel (`Ctrl+\``), git badges, command palette, project actions from `.omafile.toml`, encrypted folders via gocryptfs, Quick Look, editable path with completion, undo / redo, Copy To / Move To, Duplicate, colour tags.

## 0.3.0 — 2026-11-07

### Added
- Modernised libadwaita UI with tabs, thumbnails, preview pane and SFTP from `~/.ssh/config`.

## 0.2.0 — 2026-11-07

### Changed
- Rewritten as a native GTK 4 / libadwaita application in Rust.

## 0.1.0 — 2026-11-07

### Added
- First release: a theme-aware terminal file manager for Omarchy.
