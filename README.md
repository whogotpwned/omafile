# omafile

A native GTK 4 / libadwaita file manager for [Omarchy](https://omarchy.org).
It looks like it shipped with the system: colours, accent and light/dark mode
come straight from the active Omarchy theme and follow along live when you
run `omarchy-theme-set`.

![omafile, Tokyo Night, grid view with thumbnails and the preview pane](docs/screenshot-grid.png)

<details>
<summary>More screenshots</summary>

List view

![list view](docs/screenshot-list.png)

Tabs and the preview pane showing a text file

![tabs and text preview](docs/screenshot-tabs.png)

Browsing a server over SFTP, after pasting a local file into it

![sftp](docs/screenshot-sftp.png)

Connect to Server

![connect dialog](docs/screenshot-connect.png)

Context menu

![context menu](docs/screenshot-menu.png)

The same build after switching Omarchy to a light theme at runtime

![light theme](docs/screenshot-light.png)

</details>

## What you get

- **Omarchy theming.** The 16-colour palette from
  `~/.config/omarchy/current/theme/alacritty.toml` is mapped onto every
  libadwaita named colour (window, sidebar, header, accent, selection,
  dialogs, toasts). Light or dark follows the theme's `light.mode` marker.
  A file monitor on the theme link re-skins the running app the moment the
  theme changes. No stylesheet to maintain, custom themes just work.
- **Modern libadwaita UI.** Flat header bar, pill-shaped breadcrumb path bar
  (click the empty part or press `Ctrl+L` to type), rounded content card,
  overlay split views for the places sidebar and the preview pane, tabs,
  toasts, adaptive dialogs, status line with selection size.
- **Tabs.** `Ctrl+T`, `Ctrl+W`, `Ctrl+Tab`; middle-click or `Ctrl+Enter`
  opens a folder in a new tab. Every tab has its own history and live
  directory monitor.
- **Thumbnails.** Images are decoded off the main thread and shown in both
  the grid and the list; existing freedesktop thumbnails are reused.
- **Preview pane** (`F3`): large image preview, text preview in monospace,
  folder item counts, type / size / modified line.
- **SFTP / SCP.** Hosts from `~/.ssh/config` are listed under *Remote* in the
  sidebar; one click mounts them through GVFS and opens a tab. *Connect to
  Server…* (`Ctrl+Shift+K`) takes `user@host[:port]/path` or any `sftp://`
  URI, and the location entry accepts the same. Password, passphrase and
  host-key prompts come up as native dialogs. Mounted servers appear under
  *Devices* with an unmount entry. Copy, paste, drag and drop, rename, delete
  and the preview all work on remote files exactly like on local ones, which
  is what `scp` does, minus the terminal.
- **Drag and drop** between tabs, onto folders and onto sidebar places, and
  with other GTK apps. Local-to-local drops move, everything else copies.
- **Bookmarks** (`Ctrl+D`) shared with the GTK file chooser, *Recent* and
  *Trash* places (restore and empty trash), removable drives.
- **Archives.** *Extract Here* and *Compress…* via `bsdtar` (libarchive,
  part of Arch base): zip, tar.gz, tar.xz, tar.zst, 7z.
- **Everything else** a file manager needs: cut/copy/paste shared with other
  apps, rename, new folder/file, trash and permanent delete with
  confirmation, properties, open with…, copy path, open terminal here via
  `uwsm app -- alacritty`, sort by name/size/type/modified, hidden files,
  rubber-band and multi-selection, live reload on directory changes.
- **No config file.** It follows the system.

## Install

```sh
git clone https://github.com/whogotpwned/omafile
cd omafile
./install.sh --all
```

That builds a release binary into `~/.local/bin`, installs the desktop entry
(so it appears in Walker and as an "Open With" target), makes omafile the
default application for folders, and replaces Nautilus on `SUPER+F` in
`~/.config/hypr/bindings.conf`. Pass `--default` or `--bind` for only one of
the last two, or nothing for just the binary.

Build dependencies on Omarchy: `sudo pacman -S --needed rust gtk4 libadwaita`.
Everything else is already there: `gvfs` for trash, drives and SFTP,
`libarchive` for archives, the Hyprland portal for "Open With".

Arch package:

```sh
makepkg -si
```

### Hyprland

`data/hyprland-bindings.conf` holds the `SUPER+F` binding plus optional window
rules for a centered floating window. The window class is
`io.github.whogotpwned.Omafile`.

## Keyboard shortcuts

| Keys | Action |
|---|---|
| `Alt+←` `Alt+→` | Back / forward |
| `Alt+↑` `Backspace` | Parent folder |
| `Alt+Home` | Home |
| `Ctrl+L` | Type a location (`~` works) |
| `Ctrl+F` | Filter the current folder |
| `F5` `Ctrl+R` | Reload |
| `Ctrl+1` `Ctrl+2` | List / grid view |
| `Ctrl+T` `Ctrl+W` | New / close tab |
| `Ctrl+Tab` `Ctrl+Shift+Tab` | Next / previous tab |
| `Ctrl+Enter` / middle click | Open folder in new tab |
| `F3` | Preview pane |
| `Ctrl+H` | Show hidden files |
| `F9` | Toggle sidebar |
| `Enter` | Open (folders navigate, files open with the default app) |
| `Ctrl+Shift+O` | Open with… |
| `F4` `Ctrl+Alt+T` | Open terminal here |
| `Ctrl+Shift+N` | New folder |
| `Ctrl+Alt+N` | New file |
| `F2` | Rename |
| `Ctrl+C` `Ctrl+X` `Ctrl+V` | Copy / cut / paste |
| `Ctrl+Shift+C` | Copy path |
| `Delete` | Move to trash |
| `Shift+Delete` | Delete permanently |
| `Ctrl+I` `Alt+Enter` | Properties |
| `Ctrl+D` | Bookmark this folder |
| `Ctrl+Shift+K` | Connect to server (SFTP) |
| `Ctrl+?` | Shortcut overview |
| `Ctrl+Q` | Quit |

Right-click opens a context menu on items and on the background. Rubber-band
selection, `Shift`/`Ctrl` multi-select and sorting by column header work as
in any GTK list.

## Command line

```
omafile [DIRECTORY | FILE]...
```

A file argument opens its folder. Without an argument the current working
directory is shown.

| Variable | Meaning |
|---|---|
| `TERMINAL` | terminal for "Open Terminal Here", default `alacritty` |
| `OMAFILE_THEME_DIR` | override `~/.config/omarchy/current/theme` |

## How the theming works

omafile does not ship a stylesheet. At startup it parses
`<theme>/alacritty.toml`, maps the palette onto libadwaita's named colours
(`window_bg_color`, `accent_bg_color`, `sidebar_bg_color`, …) with a few
derived shades, installs that as a `GtkCssProvider`, and sets the
`AdwStyleManager` colour scheme from `light.mode`. A `GFileMonitor` on the
`current` directory catches the symlink swap that `omarchy-theme-set`
performs. Custom themes work as long as they contain an `alacritty.toml`,
which every Omarchy theme does.

## Development

```sh
cargo run -- ~/Projects
cargo test
cargo clippy
```

| Module | Role |
|---|---|
| `theme` | palette parsing, CSS generation, live reload |
| `item` | `FileItem` GObject (file + info) for the list models |
| `ops` | async GIO operations: list, copy, move, trash, delete, rename, archives |
| `pane` | one tab: list/grid views, filter, selection, drag and drop, monitor |
| `window` | shell: sidebar, header, path bar, tabs, preview split, mounting |
| `preview` | preview pane |
| `thumbs` | thumbnail cache, decoding off the main thread |
| `remote` | ssh config hosts, server URIs, GTK bookmarks |
| `actions` | window actions, clipboard, dialogs, properties, shortcuts |

## License

MIT
