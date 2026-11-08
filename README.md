# omafile

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
| ![search](docs/screenshot-search.png) | **Recursive search** (`Ctrl+Shift+F`) through the current folder and everything below, powered by `fd` when installed, with a GIO walk as fallback so it also works on servers. Results show their relative path. |
| ![batch rename](docs/screenshot-batch.png) | **Batch rename**: select several files and press `F2`. Pattern with `[name]`, `[ext]`, `[N]`, `[date]`, then find/replace, with a live preview and conflict detection. |
| ![sftp](docs/screenshot-sftp.png) | **SFTP / SCP**: hosts from `~/.ssh/config` in the sidebar, *Connect to Server…* for `user@host[:port]/path`, native credential prompts, mounted servers under *Devices*. Copy, paste, drag and drop, rename and preview work on remote files like on local ones. |
| ![icon chooser](docs/screenshot-icon.png) | **Custom icons** per folder or file (`Ctrl+Alt+I`): pick one from the icon theme or use your own image. Stored as GIO metadata, so Nautilus shows the same icon. |
| ![preferences](docs/screenshot-prefs.png) | **Preferences** (`Ctrl+,`): override the icon theme (by default omafile follows the Yaru variant Omarchy's theme asks for), thumbnails, default view, hidden files. |
| ![light](docs/screenshot-light.png) | **Theme following**: here the same build after `omarchy theme set "Catppuccin Latte"`, which turned the window light and the folders blue, without a restart. |

Also: bookmarks shared with the GTK file chooser (`Ctrl+D`), *Recent* and
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

Build dependencies on Omarchy: `sudo pacman -S --needed rust gtk4 libadwaita`.
Runtime extras are already part of a stock install: `gvfs` (trash, drives,
SFTP), `poppler` (PDF previews), `fd` (fast search), `libarchive` (archives).

Arch package: `makepkg -si`.

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
too. Left click opens the configured folder, right click the active terminal's
directory (like Omarchy's own "File manager (cwd)" binding), middle click `/`.

## Keyboard shortcuts

| Keys | Action |
|---|---|
| `Alt+←` `Alt+→` `Alt+↑` `Backspace` | Back, forward, parent |
| `Ctrl+L` | Type a location (`~`, `/path`, `sftp://host/`, `user@host`) |
| `Ctrl+F` / `Ctrl+Shift+F` | Filter this folder / search subfolders |
| `Ctrl+T` `Ctrl+W` `Ctrl+Tab` | New, close, next tab |
| `Ctrl+Enter` / middle click | Open folder in new tab |
| `F3` `F6` `F9` | Preview pane, split view, sidebar |
| `Ctrl+1` `Ctrl+2` `Ctrl+H` | List, grid, hidden files |
| `Enter` `Ctrl+Shift+O` `F4` | Open, open with…, terminal here |
| `Ctrl+Shift+N` `Ctrl+Alt+N` | New folder, new file |
| `F2` | Rename, batch rename with several files selected |
| `Ctrl+Alt+I` | Change icon |
| `Ctrl+C` `Ctrl+X` `Ctrl+V` `Ctrl+Shift+C` | Copy, cut, paste, copy path |
| `Delete` `Shift+Delete` | Trash, delete permanently |
| `Ctrl+D` `Ctrl+Shift+K` `Ctrl+I` | Bookmark, connect to server, properties |
| `Ctrl+,` `Ctrl+?` `Ctrl+Q` | Preferences, shortcuts, quit |

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

What the Omarchy manual and `bin/omarchy-plugin-validate` require, and what
this repository does about it:

| Rule | Here |
|---|---|
| Git repo with `manifest.json` at the root | yes, `omarchy plugin add <url>` clones this repo |
| `schemaVersion` exactly `1`; `id`, `name`, `version`, `kinds`, `entryPoints` present | yes |
| `id` is `[A-Za-z0-9][A-Za-z0-9._-]*`, not in the reserved `omarchy.*` namespace, never changes | `whogotpwned.omafile` |
| Every kind has its entry point (`bar-widget` → `entryPoints.barWidget`), relative path, no `..`, file exists | `plugin/BarWidget.qml` |
| `barWidget.defaultSection` ∈ left, centerBegin, center, centerEnd, tray, right | `left` |
| No symlinks anywhere in the folder | none |
| Theme colours only via `qs.Commons` (`Commons.Color.*`), animations via `Style.duration()` | the widget uses the bar's colours through `BarIconButton` |
| Plugins run unsandboxed inside `omarchy-shell`, no install hooks, no sudo | the widget only runs `uwsm-app -- omafile` on click |
| Listed on the marketplace at plugins.omarchy.org once public | to do after the first release |

The Omarchy desktop install still needs the binary (`install.sh` or the
PKGBUILD); the plugin itself never installs software.

## Preferences file

`~/.config/omafile/omafile.conf`, written by the Preferences dialog:

```
icon-theme = Yaru-purple
thumbnails = true
view = grid
show-hidden = false
```

Delete it to go back to following the theme for everything.

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
| `prefs` | the preferences file |
| `actions` | window actions, clipboard, dialogs, properties, shortcuts |
| `plugin/` | the Omarchy shell bar widget |

## License

MIT
