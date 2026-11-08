`omarchy` `omarchy-plugin` `hyprland` `wayland` `linux` `file-manager` `gtk4` `libadwaita` `rust` `sftp` `scp` `quickshell` `tags` `thumbnails` `pdf-preview` `git` `gocryptfs` `encryption` `command-palette` `vte` `terminal` `arch-linux`

# omafile

[![Omarchy plugin](https://img.shields.io/badge/Omarchy-shell%20plugin-7aa2f7)](#bar-button-omarchy-shell-plugin)
[![GTK 4 + libadwaita](https://img.shields.io/badge/GTK%204-libadwaita-9ece6a)](#how-it-follows-omarchy)
[![Rust](https://img.shields.io/badge/Rust-2021-e0af68)](Cargo.toml)
[![Arch package](https://img.shields.io/badge/Arch-PKGBUILD-ad8ee6)](PKGBUILD)
[![License: MIT](https://img.shields.io/badge/License-MIT-565f89)](LICENSE)

A native GTK 4 / libadwaita file manager for [Omarchy](https://omarchy.org),
plus an Omarchy shell plugin that puts it on the bar. It looks like it shipped
with the system: colours, accent, light/dark mode and the icon theme come from
the active Omarchy theme and follow along live when you switch themes.

![omafile, Tokyo Night, grid view with image and PDF thumbnails and the preview pane](docs/screenshot-pdf.png)

## Features

| | |
|---|---|
| ![list](docs/screenshot-list.png) | **List and grid views** with sortable columns, image and PDF thumbnails, rubber-band and multi-selection, hidden-file toggle, live reload when a folder changes. |
| ![tabs](docs/screenshot-tabs.png) | **Tabs and the preview pane** (`F3`). Images, PDFs (first page, page count), text files in monospace, folder item counts. |
| ![split](docs/screenshot-split.png) | **Split view** (`F6`): two sets of tabs side by side, each with its own history. Drag files across, or between a local and a remote tab. The active side carries the accent border. |
| ![path completion](docs/screenshot-complete.png) | **Editable path bar** (`Ctrl+L`): the breadcrumb turns into a text field with the cursor at the end, so you fix one segment instead of retyping (`Ctrl+A` selects it all to start over). Folders complete as you type: `Tab` accepts, `↑`/`↓` pick, `Enter` opens. `~`, relative paths, `sftp://` and `user@host` all work. |
| ![quick look](docs/screenshot-quicklook.png) | **Quick Look** (`Space`), as on macOS: a large preview of the selected file without opening it. PDFs show their first page and page count, images fill the window, text files show their content, folders their item count. `←`/`→` walk through the folder while it is open, `Space` or `Esc` closes. The preview pane (`F3`) is the persistent sidebar variant of the same thing; both can be toggled on and off at any time. |
| ![terminal](docs/screenshot-terminal.png) | **Terminal panel** (`` Ctrl+` `` or `F7`): your login shell in a VTE terminal docked under the file list, the way VS Code does it. It opens in the folder you are looking at and follows along as you navigate (a `cd` with a leading space, kept out of history); with fish, zsh or a bash prompt that reports its folder (OSC 7) a `cd` in the terminal moves the file view too. Colours are the same mapping Omarchy applies to Alacritty, so it looks like your other terminals; the font is the system monospace font. Drag the splitter to resize, `Ctrl+Shift+C`/`V` copy and paste, `exit` restarts the shell. One terminal per tab, so split view gives you two. |
| ![git](docs/screenshot-git.png) | **Git awareness**: inside a repository every row carries its status badge (`M` modified, `?` untracked, `A` added, `D` deleted, `!` conflict), folders inherit the strongest state of their contents, and the status line shows `repo on branch · 1 modified · 2 untracked`. *Git Diff in Terminal* on a file, *Git Status / Diff* on the folder. Read-only, `GIT_OPTIONAL_LOCKS=0`, refreshed with the folder. |
| ![palette](docs/screenshot-palette.png) | **Command palette** (`Ctrl+K`), the Walker / VS Code pattern: fuzzy search over every action with its shortcut, places, bookmarks, recent folders, SSH hosts and the project commands of the current folder. Type `> cargo test` to run anything in a terminal here. `↑`/`↓` or `Tab` pick, `Enter` runs. |
| ![project](docs/screenshot-project.png) | **Project actions**: omafile recognises what a folder is (`Cargo.toml`, `package.json`, `pyproject.toml`, `go.mod`, `PKGBUILD`, `compose.yml`, `Makefile`/`justfile`, an Omarchy plugin, a git checkout) and offers the matching commands in the context menu and the palette: `cargo test`, `npm run dev`, `makepkg -si`, `docker compose up`, `git log` … They run in Alacritty in that folder and wait for Enter, so output stays readable. *Open in Editor* (`Ctrl+E`, `Ctrl+Shift+E` for the folder) uses Omarchy's `omarchy-launch-editor`, so it lands in Neovim. Teams add their own commands with a `.omafile.toml` in the repository. |
| ![vault](docs/screenshot-vault.png) | **Encrypted folders** with gocryptfs: *New Encrypted Folder…* asks for a name and password and creates `Name.gocryptfs` (the ciphertext) next to `Name` (where files appear while unlocked). Unlock, lock and change the password from the context menu; the password only ever travels through a pipe. The master key for emergencies is shown once. Because gocryptfs encrypts file by file, the `.gocryptfs` folder is the thing to back up, sync or move to another machine: copy it anywhere, unlock it there with the same password. |
| ![undo](docs/screenshot-undo.png) | **Undo and redo** (`Ctrl+Z`, `Ctrl+Shift+Z`), borrowed from Nautilus and Finder: move, copy, paste, rename, batch rename, duplicate, new folder/file, trash and tag changes can all be taken back, every toast carries an Undo button, and trashed items are restored from the trash by their original path. |
| ![search](docs/screenshot-search.png) | **Recursive search** (`Ctrl+Shift+F`) through the current folder and everything below. `fd` streams matches while it walks, results appear in batches as they arrive, the previous search is killed the moment you type on, and the final list is ranked (name starts with the query, name contains it, path contains it). On a 5,300-file tree a short query returns in about 40 ms, a query with 200+ hits in under 300 ms including the file information for every hit. A GIO walk takes over where `fd` cannot go, such as SFTP servers. |
| ![batch rename](docs/screenshot-batch.png) | **Batch rename**: select several files and press `F2`. Pattern with `[name]`, `[ext]`, `[N]`, `[date]`, then find/replace, with a live preview and conflict detection. |
| ![sftp](docs/screenshot-sftp.png) | **SFTP / SCP**: hosts from `~/.ssh/config` in the sidebar, *Connect to Server…* for `user@host[:port]/path`, native credential prompts, mounted servers under *Devices*. Copy, paste, drag and drop, rename and preview work on remote files like on local ones. |
| ![icon chooser](docs/screenshot-icon.png) | **Custom icons** per folder or file (`Ctrl+Alt+I`): pick one from the icon theme or use your own image. Stored as GIO metadata, so Nautilus shows the same icon. |
| ![preferences](docs/screenshot-prefs.png) | **Preferences** (`Ctrl+,`): override the icon theme (by default omafile follows the Yaru variant Omarchy's theme asks for), thumbnails, default view, hidden files. |
| ![tags](docs/screenshot-tags.png) | **Tags**, Finder style: give folders and files one of seven colours (`Ctrl+Alt+1…7`, `Ctrl+Alt+0` removes). The dot shows in list and grid, every colour in use appears under *Tags* in the sidebar and lists all tagged items wherever they are. Tags are stored as GIO metadata on the file and in an index, and follow renames and moves done in omafile. |
| ![light](docs/screenshot-light.png) | **Theme following**: here the same build after `omarchy theme set "Catppuccin Latte"`, which turned the window light and the folders blue, without a restart. |

Also: *Copy To…* and *Move To…* with a folder picker and *Duplicate*
(`Ctrl+Shift+D`) from the context menu, bookmarks shared with the GTK file chooser (`Ctrl+D`), *Recent* and
*Trash* with restore and empty, drag and drop onto sidebar places, extract and
create archives via `bsdtar`, cut/copy/paste shared with other apps, properties,
open with…, copy path, open terminal here (`uwsm app -- alacritty`), context
menus and a shortcut for everything (`Ctrl+?`).

<details>
<summary>More screenshots</summary>

Context menu

![context menu](docs/screenshot-menu.png)

Connect to Server

![connect dialog](docs/screenshot-connect.png)

Custom folder icon applied

![custom icon](docs/screenshot-icon-applied.png)

Lock, unlock and change the password of an encrypted folder from its context menu

![vault menu](docs/screenshot-vault-menu.png)

A tag opened from the sidebar lists every tagged item with its location

![tag listing](docs/screenshot-tagview.png)

</details>

## Install

```sh
git clone https://github.com/whogotpwned/omafile
cd omafile
./install.sh --all
```

That builds a release binary into `~/.local/bin`, installs the desktop entry
(launcher and "Open With" target), makes omafile the default application for
folders, and rebinds the file manager shortcut: on current Omarchy (Lua
configuration) it appends to `~/.config/hypr/bindings.lua`

```lua
o.rebind("SUPER + SHIFT + F", "File manager", { launch = "omafile" })
o.rebind("SUPER + ALT + SHIFT + F", "File manager (cwd)", "setsid uwsm-app -- omafile \"$(omarchy-cmd-terminal-cwd)\"")
```

and on older releases it edits `~/.config/hypr/bindings.conf`. Pass `--default`
or `--bind` for only one of the two, or nothing for just the binary.

Build dependencies on Omarchy: `sudo pacman -S --needed rust gtk4 libadwaita vte4`.
Runtime extras are already part of a stock install: `gvfs` (trash, drives,
SFTP), `poppler` (PDF previews), `fd` (fast search), `libarchive` (archives),
`git` (status badges). Encrypted folders need `gocryptfs`
(`sudo pacman -S gocryptfs`); everything else works without it.

Arch package: `makepkg -si`, remove with `sudo pacman -Rns omafile`.

### Uninstall

```sh
./uninstall.sh            # binary, desktop entry, shortcuts, default-app setting
./uninstall.sh --purge    # also ~/.config/omafile (preferences, tags)
omarchy plugin remove whogotpwned.omafile   # the bar button, if you added it
```

`uninstall.sh` reverses `install.sh` step by step: it hands folders back to
Nautilus if omafile was the default, removes only the binding lines
`install.sh` appended, and leaves everything else in your config untouched.

### Bar button (Omarchy shell plugin)

The repository doubles as an Omarchy shell plugin: `manifest.json` at the root
and the widget in `plugin/BarWidget.qml`.

```sh
omarchy plugin add https://github.com/whogotpwned/omafile.git --enable
```

The widget lands in the `left` section next to the Omarchy menu. Put it where
you like, the same way as every other widget:

```sh
omarchy bar move whogotpwned.omafile --section right        # left | centerBegin | center | centerEnd | right | tray
omarchy bar move whogotpwned.omafile --section centerEnd --index 0
omarchy bar set  whogotpwned.omafile path ~/Projects        # folder opened on click
```

Drag and drop in the bar and *Appearance › Menu Bar* in the Omarchy menu work
too. `omarchy plugin add` asks for the section interactively as well.

What the button does:

| Click | Action |
|---|---|
| Left | Raises the omafile window that is already open (`omarchy-launch-or-focus`), otherwise opens the configured folder |
| Right | Places menu: Home, Downloads, Documents, Pictures, Projects, Trash, the folder of the active terminal, new window |
| Middle | Opens the folder of the active terminal (like Omarchy's own "File manager (cwd)" binding) |

The icon is a Nerd Font folder glyph drawn with the bar's font, exactly like
the first-party widgets, so it matches the bar in every theme. `omarchy bar
set whogotpwned.omafile icon "󰉋"` swaps it for any other glyph. The popup uses
the shell's own `PopupCard` and `PopupMenuRow` components and therefore takes
the theme's popup colours and corner radius.

Removal is `omarchy plugin remove whogotpwned.omafile`, which disables the
widget, drops it from the bar layout and deletes the checkout.

## Keyboard shortcuts

| Keys | Action |
|---|---|
| `Alt+←` `Alt+→` `Alt+↑` `Backspace` | Back, forward, parent |
| `Ctrl+L` | Edit the path in place; `Tab` completes folders (`~`, `/path`, `sftp://host/`, `user@host`) |
| `Ctrl+F` / `Ctrl+Shift+F` | Filter this folder / search subfolders |
| `Ctrl+T` `Ctrl+W` `Ctrl+Tab` | New, close, next tab |
| `Ctrl+Enter` / middle click | Open folder in new tab |
| `F3` `F6` `F9` | Toggle preview pane, split view, sidebar |
| `Ctrl+1` `Ctrl+2` `Ctrl+H` | List, grid, hidden files |
| `Enter` `Ctrl+Shift+O` `F4` | Open, open with…, terminal here |
| `Space` | Quick Look (toggle); `←` `→` walk the folder inside it |
| `Ctrl+Shift+N` `Ctrl+Alt+N` | New folder, new file |
| `F2` | Rename, batch rename with several files selected |
| `Ctrl+Alt+I` | Change icon |
| `Ctrl+Alt+1…7` / `Ctrl+Alt+0` | Tag red, orange, yellow, green, blue, purple, gray / remove tag |
| `Ctrl+C` `Ctrl+X` `Ctrl+V` `Ctrl+Shift+C` | Copy, cut, paste, copy path |
| `Ctrl+Z` `Ctrl+Shift+Z` | Undo, redo |
| `Ctrl+K` | Command palette |
| `` Ctrl+` `` `F7` | Terminal panel below the files |
| `Ctrl+E` `Ctrl+Shift+E` | Open selection / folder in the editor |
| `Ctrl+Shift+D` | Duplicate |
| `Delete` `Shift+Delete` | Trash, delete permanently |
| `Ctrl+D` `Ctrl+Shift+K` `Ctrl+I` | Bookmark, connect to server, properties |
| `Ctrl+,` `Ctrl+?` `Ctrl+Q` | Preferences, shortcuts, quit |

Every shortcut is exercised by an automated run (`OMAFILE_TRACE=1` logs each
action; a script presses every accelerator under Xvfb and checks that the
matching action fired). Shortcuts that also mean something in a text field
(`Ctrl+A/C/X/V`, `Backspace`, `Delete`) act on the text while a field has
focus and on files otherwise.

## How it follows Omarchy

- **Palette.** omafile reads `~/.local/state/omarchy/current/theme/colors.toml`
  (the semantic keys `background`, `foreground`, `accent`, `selection`,
  `muted`, `dark_background`, `lighter_background`, …) and maps them onto
  libadwaita's named colours plus its own widget styles. Older installs with
  `~/.config/omarchy/current/theme/alacritty.toml` are read as well.
- **Mode.** `mode = "light"` in `colors.toml` (or the legacy `light.mode`
  marker, or background luminance for custom themes) picks light or dark.
- **Icons.** The theme's `icons.theme` names the icon set (Omarchy ships Yaru
  variants); omafile applies it so the folders match even before GNOME
  settings catch up. Preferences can override it with any installed theme.
- **Live.** A `GFileMonitor` on the `current` directory catches the symlink
  swap `omarchy-theme-set` performs and re-skins open windows.
- **Launching.** Windows are opened through `uwsm-app`, the terminal through
  `uwsm app -- $TERMINAL`, files through the desktop portal.
- **Shell plugin.** The bar widget follows the first-party `omarchy.menu`
  widget: a `BarIconButton` inside a `BarWidget`, colours and font from the
  bar, commands run through `bar.run`. The manifest keeps the
  `whogotpwned.*` namespace (`omarchy.*` is reserved), declares
  `schemaVersion: 1`, the `bar-widget` kind with its `entryPoints.barWidget`,
  and a `defaultSection`. `omarchy plugin validate .` passes.

### Publishing as an Omarchy plugin

Omarchy plugins are distributed as git repositories; there is no package to
build. Anyone installs, updates and removes the bar button with the normal
plugin commands, interactively or with `--yes` for scripts:

```sh
omarchy plugin add https://github.com/whogotpwned/omafile.git --enable   # asks for the bar section
omarchy plugin list
omarchy plugin update whogotpwned.omafile
omarchy plugin disable whogotpwned.omafile
omarchy plugin enable whogotpwned.omafile --section right
omarchy plugin remove whogotpwned.omafile
```

`add` clones this repository into `~/.config/omarchy/plugins/whogotpwned.omafile`,
runs the same validation as the shell, refuses duplicate ids, never executes
anything from the repository and never asks for sudo. `remove` disables the
widget, drops it from the bar layout and deletes the checkout. The file
manager binary is separate (`install.sh`, PKGBUILD or a future AUR package),
so removing the plugin never touches it and vice versa.

Release checklist for the maintainer:

1. Bump `version` in `Cargo.toml`, `PKGBUILD` and `manifest.json` together.
2. Run `cargo test`, `cargo clippy` and `omarchy plugin validate .` (the same
   checks are mirrored in `plugin/validate.sh` for machines without Omarchy).
3. Commit, tag `vX.Y.Z`, push. `omarchy plugin update` fast-forwards users to
   the new commit on the default branch, so keep that branch releasable.
4. Submit the repository URL at [plugins.omarchy.org](https://plugins.omarchy.org),
   the marketplace the Omarchy manual points to. Suggested GitHub topics so it
   is found: `omarchy`, `omarchy-plugin`, `file-manager`, `hyprland`, `gtk4`.

What the shell enforces at load time and what this repository does:

| Rule | Here |
|---|---|
| Git repo with `manifest.json` at the root | yes |
| `schemaVersion` exactly `1`; `id`, `name`, `version`, `kinds`, `entryPoints` present | yes |
| `id` is `[A-Za-z0-9][A-Za-z0-9._-]*`, not in the reserved `omarchy.*` namespace, never changes | `whogotpwned.omafile` |
| Every kind has its entry point (`bar-widget` → `entryPoints.barWidget`), relative path, no `..`, file exists | `plugin/BarWidget.qml` |
| `barWidget.defaultSection` ∈ left, centerBegin, center, centerEnd, tray, right | `left` |
| No symlinks anywhere in the folder | none |
| Theme colours only via `qs.Commons`, UI via `qs.Ui` components | `BarIconButton`, `PopupCard`, `PopupMenuRow` |
| Plugins run unsandboxed inside `omarchy-shell`, no install hooks, no sudo | the widget only runs `omafile` and `omarchy-launch-or-focus` on click |

## Encrypted folders in detail

- **Format.** gocryptfs: AES-256-GCM per file, file names encrypted (EME),
  keys derived from the password with scrypt. The `.gocryptfs` directory
  holds only ciphertext plus `gocryptfs.conf` (the wrapped master key).
- **Migration and backup.** Copy or sync `Name.gocryptfs` with any tool
  (rsync, a USB stick, a cloud folder, `scp`). On the other machine, put it
  next to an empty `Name` folder and use *Unlock Encrypted Folder…*, or
  `gocryptfs Name.gocryptfs Name` on the command line. Changing the password
  rewraps the master key inside `gocryptfs.conf`, so a copy made before the
  change still opens with the old password; copy again afterwards. The master
  key shown once at creation opens the folder without any password, keep it
  in a password manager as the last resort.
- **Handling.** Unlocking mounts the folder for your user only (FUSE, no
  root). Lock it with the context menu, or it stays unlocked until logout.
  The password is piped to gocryptfs and never written to disk or passed on
  a command line. Deleting the ciphertext folder deletes the data.
- **Scope.** Local file systems only; a vault cannot live on an SFTP share.

## Project actions file

`.omafile.toml` in a project root adds commands to the *Project* submenu and
the command palette for everyone who opens the folder with omafile:

```toml
[[action]]
name = "Deploy to staging"
command = "./deploy.sh staging"

[[action]]
name = "Run migrations"
command = "docker compose exec app rails db:migrate"
```

Commands run through `bash -lc` in the project folder inside a terminal
window and wait for Enter before closing.

## Preferences file

`~/.config/omafile/omafile.conf`, written by the Preferences dialog:

```
icon-theme = Yaru-purple
thumbnails = true
view = grid
show-hidden = false
```

Tags live next to it in `~/.config/omafile/tags` (`uri<TAB>colour` per line).
Delete the directory to go back to following the theme for everything.

## Development

```sh
cargo run -- ~/Projects
cargo test
cargo clippy
```

| Module | Role |
|---|---|
| `theme` | colors.toml / alacritty.toml parsing, CSS, icon theme, live reload |
| `item` | `FileItem` GObject (file + info, custom icons) for the list models |
| `ops` | async GIO operations: list, copy, move, trash, delete, rename, archives |
| `pane` | one tab: views, filter, recursive search, selection, drag and drop |
| `window` | shell: sidebar, header, path bar, tabs, split view, preview, mounting |
| `preview` | preview pane |
| `thumbs` | thumbnail cache; images via gdk-pixbuf, PDFs via pdftoppm |
| `search` | recursive search with fd or a GIO walk |
| `remote` | ssh config hosts, server URIs, GTK bookmarks |
| `extras` | preferences, icon chooser, batch rename |
| `tags` | colour tags: metadata, index file, virtual listings |
| `git` | repository status for badges and the status line |
| `projects` | project detection, actions, `.omafile.toml`, editor and terminal launch |
| `palette` | command palette with fuzzy matching |
| `crypt` / `vaultui` | gocryptfs vaults and their dialogs |
| `termpanel` | embedded VTE terminal that follows the folder |
| `undo` | undo/redo journal |
| `completion` / `quicklook` | path completion, Quick Look |
| `prefs` | the preferences file |
| `actions` | window actions, clipboard, dialogs, properties, shortcuts |
| `plugin/` | the Omarchy shell bar widget |

## License

MIT
