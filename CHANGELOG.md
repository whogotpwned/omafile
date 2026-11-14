# Changelog

All notable changes to omafile. The version here matches `Cargo.toml`, `PKGBUILD`, `manifest.json` and the README badge; `plugin/validate.sh` checks they agree.

## 0.32.0 — 2026-11-14

### Added
- **Update omafile from inside omafile.** *Menu › Update omafile…* (also in *Preferences › Updates* and the command palette) asks for the newest version, and *Update Now* fetches the source, builds and installs it in a window that shows every step and the full output. When it is done, *Restart omafile* starts the new version. Until now the command was only shown and run in a terminal.
- When this version is the newest already, *Update Anyway* fetches the latest state and installs it again: a fix can arrive without a new version number.
- An installation omafile does not know the checkout of gets one of its own: the source is cloned into `~/.local/share/omafile/src` and installed from there.
- When the update fails, the output says why, omafile keeps running unchanged, and *Run in a Terminal* repeats the same steps. Closing the window does not stop the update; the result is then announced in a notification.
- A package (AUR) is still updated in a terminal, because the package manager asks for the password.

## 0.31.1 — 2026-11-14

### Fixed
- **Songs and videos did not play on a stock Omarchy**: a click on the play badge did nothing. The built-in player is GTK's, which needs the GStreamer plugins (`gst-plugins-good`, `gst-libav`); Omarchy 4 ships `mpv` and not those, and the README wrongly said it had them. Now `mpv` takes over by itself when the built-in player cannot play a file: a song plays through omafile's mpv player (the radio's) and shows in *Now Playing*, the mini player and on the media keys; a video opens in mpv's own window. With the plugins installed, playback stays inside the preview as before.
- **Opening a file** (double click, `Enter`) starts its default application directly instead of going through the desktop portal, the way Nautilus does. The portal's detour could show a chooser on the first opens of a type, and on some setups nothing at all. *Open With…* still asks the desktop's chooser.

## 0.31.0 — 2026-11-14

### Changed
- **Skins are for omafile, not for the desktop.** The gallery (`Ctrl+Alt+A`, sidebar › *Skins*, formerly *Omarchy Themes*) used to turn Omarchy themes on for the whole system. Now a click gives **omafile alone** that look: the desktop, the terminal and every other program keep the theme Omarchy has on, and no Omarchy command is run. The first card, *Follow Omarchy*, is the default and the way back: omafile wears whatever theme is on and changes with it. The choice is kept as `skin` in the preferences.
- **A skin from a picture** is saved in `~/.config/omafile/skins` instead of Omarchy's theme folder, and *Save and Use* puts it on omafile only. `omafile --theme-from-picture` writes there too (`--into` names another folder). The folder still has the format of an Omarchy theme, so it can be copied over by hand.
- Every theme Omarchy ships or you installed can be worn as a skin without being turned on; nothing in those folders is changed, and only omafile's own skins can be moved to the trash from the gallery.

### Removed
- From the gallery: turning an Omarchy theme on, *Next Background*, *Add Backgrounds…* and dropping pictures on a card. They changed the desktop, which is not what skins are for. The explicit entries in a theme folder's context menu (*Apply Omarchy Theme*, *Set as Wallpaper*, export and install of theme archives) are still there.

## 0.30.0 — 2026-11-14

### Added
- **omafile tells you about a new version.** At most once a day it asks GitHub which version is the newest and, when there is one, shows a notification that stays until it is answered. *Update…* names the command for this installation (`git pull` and `./install.sh` for a checkout, the AUR helper for a package) and *Update Now* runs it in a terminal. omafile never updates itself. *Skip This Version* and *Later* are there too.
- What counts as the newest version follows the installation: the latest tagged release for a package, the `main` branch for a checkout. `install.sh` notes where the checkout is (`~/.local/share/omafile/source`) so the command can name it; run it once more to get that.
- *Check for Updates* in the palette and in *Help › About*, `omafile --check-update` on the command line (exit code 10 when a newer version exists).
- **Preferences › Updates** turns the daily question off. It is one request for a small public file and carries nothing about you or your files.

### Fixed
- The number of hidden files in the status line ("+8 hidden") is translated.
- The self-test lets a dialog finish sliding in before it closes it. It used to ask 90 ms after opening, which libadwaita 1.5 sometimes did not hear; asking again or forcing the dialog then made the library abort a few steps later. Nobody is that quick by hand; it was the test's own doing. `OMAFILE_SELFTEST_TRACE=1` names the dialogs it closes.

## 0.29.0 — 2026-11-13

### Added
- **Radio** (`Ctrl+Alt+O`, sidebar › *Radio*, `omafile --radio`, the bar button's menu): internet radio and podcasts in the file manager. Ten stations of electronic music from SomaFM and eight tech and hacking news podcasts (Security Now, Darknet Diaries, Risky Business, SANS Internet Stormcast, Hacker Public Radio, Changelog News, Logbuch:Netzpolitik, Chaosradio) are built in.
- **Your own stations**: *Add Station* takes the address of a stream, of a `.pls` / `.m3u` playlist or of a podcast's feed. omafile asks the address what it is, finds a name when none is given, and files it under a genre of your choice. Stations can be changed and removed; removed built-in ones come back with one click. The list is `~/.config/omafile/radio`.
- **It plays in the player that was already there.** The radio is a media player on the desktop (MPRIS), so *Now Playing*, the sidebar's mini player, the media keys and bar widgets show and control it like any other: the song a stream names, the episode of a podcast with its length and position, a cover per station. *Next* and *Previous* change the station; within a podcast they walk through the episodes.
- The radio goes on when the window is closed and stops with *Stop*. `mpv` does the playing (part of Omarchy; an optional dependency elsewhere).
- `scripts/check-radio.py` asks every built-in station for sound. It runs with every change and once a week, so a station that moved is noticed.
- Feeds: an episode's sound (the enclosure of a podcast feed) is read along with the article.

### Fixed
- The status line of a search, a tag view and a smart folder was half English in the other languages ("0 Treffer found"); the whole line is translated now.

## 0.28.2 — 2026-11-12

### Changed
- What is hidden in *Help › About* is a dedication now.

## 0.28.1 — 2026-11-12

### Added
- Something small is hidden in *Help › About*, for those who cannot leave an icon alone.

## 0.28.0 — 2026-11-12

Checked against Omarchy's own source (4.5.0 and the development branch), and tested from end to end by omafile itself.

### Fixed
- **The bar button did not load on Omarchy 4.5**: its menu used a component (`PopupMenuRow`) that only exists in Omarchy's development branch, and a component a shell does not know keeps the whole widget from loading. The rows are the widget's own now; it loads on both.
- **Themes made from a picture** are written in the palette Omarchy 4 names (`mode`, `accent`, `background`, `selection`, `muted`, `red` … `brown`, the bright colours), followed by the keys releases before 4 read (`cursor`, the selection pair, `color0` … `color15`): one theme, both generations. Until now only the older keys were written; Omarchy 4 accepted them but had to guess the rest. Theme folders get names Omarchy accepts (umlauts are written out). Icon sets come from the list in Omarchy's manual.
- **The theme that is on** is read from `~/.local/state/omarchy/current` first (Omarchy 4) and only then from the older place, so a leftover from before an upgrade no longer wins. Shipped themes are found in `/usr/share/omarchy`.
- **Captures** looks in `~/Pictures/Screenshots` and honours `OMASNAP_SCREENSHOT_DIR` (Omarchy 4), besides the older places. *Draw on it* opens Omasnap, Omarchy 4's editor; Satty is used where it is still the one installed.
- A theme card shows the theme's preview picture when its background is in a format that cannot be read (WebP without the loader).
- **Names with `&`, `<` or `%`**: a notification about such a file showed nothing, and the Properties dialog and several lists lost the name. Notifications no longer read their text as markup, and rows escape it.
- The *Change Icon* dialog scrolls in a small window instead of growing past it; its title is translated, and so is the *Undo* button on notifications.
- 0.26.0's notes called `color0` … `color15` Omarchy's current format; it is the older one. Both are read.

### Added
- `scripts/selftest.sh`: omafile runs every action of its window against itself, in a throwaway home folder, with five different selections, at two window sizes.
- `scripts/check-omarchy.py`: every assumption about Omarchy checked against a checkout of it, without running anything from it. CI runs both on every push and the Omarchy check once a week on its own.
- `omafile --theme-from-picture <picture> [--name <name>] [--dark | --light]` makes a theme without a window; `omafile --describe-theme <folder>` prints what omafile reads from one.

## 0.27.0 — 2026-11-12

### Added
- **A tour for new installations**: with the first window after omafile was installed, seven short stops show what it can do (keyboard, views, Omarchy integration, safety, devices and servers, where to find the rest), each with its shortcuts; the Omarchy stop shows a small desktop in the colours of the current theme. *Skip the Tour*, `Esc` or the last stop end it, and it never comes back by itself. Arrow keys, `Enter` and swiping walk through it.
- The tour is shown only on a fresh installation (no configuration and no state from an earlier version): upgrading does not trigger it. *Help › About › Take the Tour* and the command palette open it at any time.

## 0.26.0 — 2026-11-12

### Added
- **Theme atelier** (`Ctrl+Alt+A`, sidebar › Omarchy Themes): every Omarchy theme as a card with its background and colours; one click turns it on through `omarchy-theme-set`. Pictures dropped on a card become backgrounds of that theme. Right click: Turn On, Next Background, Add Backgrounds…, Open Folder, Move to Trash.
- **A theme from any picture** (*New from Picture…*, a picture's context menu, the command palette): the colours come out of the picture (background and text from its tone, accents to choose from its vivid colours, sixteen terminal colours that keep their meaning), every one readable on the background. A small desktop previews the result live, in dark or light. Saved as `~/.config/omarchy/themes/<name>` with a `colors.toml` for Omarchy's templates, the picture as background, a `preview.png` and a matching icon set.
- **Captures** (`Ctrl+Alt+G`, sidebar › Captures): Omarchy's screenshots and screen recordings by day. Drag a card into another program, copy it (picture and file), draw on it with Satty the way Omarchy starts it, save a recording as GIF, share, trash with Undo. A recording flips through its stills under the pointer; new captures appear by themselves.
- `omafile --captures` and `omafile --copy-capture`; the bar button's menu has *Screenshots and recordings* and *Copy the newest capture*.

### Fixed
- `omafile --import-photos` was refused as an unknown option. It and the new `--captures` are declared now, and are passed on to an omafile that is already running instead of opening a second window.
- Themes in Omarchy's current `colors.toml` format (`color0` … `color15`) had their red, green, yellow and other terminal colours ignored: omafile showed Tokyo Night's instead. They are read now.

## 0.25.0 — 2026-11-11

### Added
- **How backups are made** is set on the Backup page: the **time of day** for daily and weekly backups and the **day of the week**, **only on mains power** (automatic backups wait while the laptop runs on battery), **how long backups are kept** (a month, three, six, a year, or as long as there is room), **making room** when the drive runs full (the oldest go first, the newest three always stay), and a **notification** when a backup has finished.
- **Package lists**: `packages.txt` and `packages-aur.txt` beside each backup, in the form `pacman -S --needed - <` reads. With a fresh Omarchy install, the home folder from the backup and these lists, a computer is back.
- **Next to Omarchy, not across it**: the page shows that Omarchy's snapshots look after the system (and when the last one was made) and that this backup is for your own files on another drive. Snapshot folders are never copied, whatever the settings; nothing here calls snapper or changes Omarchy's snapshots; a destination that would end up inside the system's snapshots gets a warning.

### Changed
- The timer now asks every ten minutes whether a backup is due instead of firing once per interval. A backup that could not run (computer off, drive away, on battery) is made up for soon after, also when omafile is closed. A timer written by 0.24.0 is replaced on the next start.
- Hourly backups start at the top of the hour.

## 0.24.0 — 2026-11-11

### Added
- **Backup to another drive** (`Ctrl+Alt+K`, sidebar › Backup): complete, browsable copies of chosen folders on an external disk or stick. Unchanged files are hard links to the previous backup; old backups thin out (all of the last day, one a day for a month, one a week beyond). An interrupted run is continued.
- **Always in view**: the sidebar's Backup row says when the last backup ran, shows progress while one runs and warns when backups are overdue or failed. The page lists the recent backups with what each copied, the drive with its free space, and lets every backup be opened or deleted.
- **Automatic**: hourly, daily or weekly through a systemd user timer that runs `omafile --backup --scheduled`, also when omafile is closed; missed runs are made up for, and a drive that is away is waited for (it is recognised by its file system id). While omafile is open it also starts a due backup when the drive appears.
- **Settings**: which folders, exclusions by name or folder, and a switch that leaves out caches, the trash and folders marked with `CACHEDIR.TAG`. Other file systems mounted inside a folder (an unlocked vault, a network share) are not followed. Warnings for a destination on the same disk and for FAT / exFAT drives.
- *Earlier Versions* also reads the backups on the drive, so files are restored from there with the same dialog.
- `omafile --backup` for scripts and the timer.

## 0.23.0 — 2026-11-11

### Added
- **Earlier versions** (`Ctrl+Alt+H`, context menu): file history from Btrfs snapshots made by snapper. For a file: its distinct earlier states with *Compare*, *Restore a Copy* and *Restore* (in place, the current state goes to the trash). For a folder: every snapshot to look into or compare, and what was deleted from the folder with *Restore*. *Snapshot Now* takes one on demand. Snapshots are read straight from `<subvolume>/.snapshots`, so any snapper configuration works.
- **Turning versions on for the home folder**: Omarchy snapshots only the system. One dialog explains the step, shows the commands (snapper configuration, hourly timeline with automatic pruning, read access for the user) and runs them in a terminal with `sudo`.
- Help: *Earlier versions* in the feature list, snapper in the list of tools found on this computer.

## 0.22.0 — 2026-11-11

### Added
- **Show in folder from other programs**: omafile implements `org.freedesktop.FileManager1` (ShowItems, ShowFolders, ShowItemProperties). A browser's *Show in folder* opens the folder in a new tab with the file selected. A switch in Preferences turns it off.
- **Make Default** (Preferences › Default file manager): folders from other programs open in omafile, and a D-Bus service file in the user's data directory starts omafile for *Show in folder* when it is not running. *Undo* restores the previous default.
- **No sleep in the middle of a transfer**: while files are copied or moved, omafile holds a sleep and idle inhibitor with systemd-logind.

## 0.21.0 — 2026-11-11

### Added
- **Help** (`F1`): a dialog with three pages and one search field. *Features* lists what omafile can do, grouped by area, each with the way to reach it and, where it is safe, an *Open* button. *Shortcuts* is the full list of bindings (`Ctrl+?` opens it directly). *About* shows the version, what is new in it (from the built-in changelog), which optional tools were found on this computer, and where settings, state and caches are kept. The version is always visible in the corner of the dialog. The main menu has *Help and Features*; *About Files* opens the About page.

### Changed
- Shortcuts are drawn as keycaps.

## 0.20.0 — 2026-11-11

### Added
- **Vaults travel as one file**: *Export…* (sidebar menu, padlock page, context menu) writes a vault to `Name.gocryptfs.tar`, a tar archive of the encrypted folder. Nothing is decrypted, so the file is protected by the same master password. *Add Vault… › Import from File…* or *Import as Vault…* on such a file unpacks it into `~/Vaults`, lists it in the sidebar and shows its padlock; archives with paths that would leave the folder are refused.
- **Servers the WinSCP way**: connecting opens the server beside the local files (local left, server right); header arrows copy or move the selection to the other pane. Connections can be **saved in the sidebar** from the new connect dialog (name, password, edit, remove). Passwords are remembered by the **system keyring** through GVFS and never written to omafile's files; removing a connection deletes its password from the keyring. `alias/path` works for hosts from `~/.ssh/config`. Preferences › Servers switches the two-pane behaviour off.
- **Feeds** (`Ctrl+Alt+W`): RSS 2.0, RSS 1.0 and Atom feeds in the sidebar with unread counts and an *All Articles* view; a reader with the article list on the left and the article (lead picture, formatted text, links) on the right, keyboard driven. *Add Feed…* accepts a site's address and finds its feed. Own XML and HTML handling, no web engine; fetched with curl over http(s) only, cached for offline reading.

### Changed
- The copy / move buttons for the other pane are shown in the header whenever the view is split.

## 0.19.0 — 2026-11-11

### Added
- **Vaults in the sidebar**: any number of encrypted folders (gocryptfs), each with its own master password, under a *Vaults* section. *Add Vault…* creates one (name, place, password) or lists an existing one. A locked vault shows a page with a large animated padlock and the password field instead of the files: a wrong password shakes it, the right one swings the shackle open and the files take its place. Unlocked vaults carry a green open padlock and a lock button; context menu with Unlock / Lock, Change Password, Show Encrypted Data, Remove from Sidebar. The list lives in `~/.config/omafile/vaults` and holds no secrets.
- *New Encrypted Folder…* can put the vault anywhere (*Choose…*) and lists it in the sidebar; *Show in Sidebar under Vaults* in the context menu does the same for older ones. *Unlock Encrypted Folder…* now opens the padlock page.

### Changed
- **Icons and thumbnails are all the same size.** Every cell has a tile of fixed size (4:3 in the grid, square in list and columns): photos, videos, album art and folder previews fill it, pages, book covers and vector graphics are shown whole on a faint card, small originals are not blown up. In the list the names line up again; before, a landscape photo pushed its name to the right.
- Files cannot be dropped on a locked vault in the sidebar (its folder is an ordinary, unencrypted one while locked).
- An unlocked vault no longer appears under *Devices* and no longer raises the "is ready" toast meant for drives.

## 0.18.3 — 2026-11-11

### Fixed
- Animated GIFs larger than the thumbnail size had no thumbnail in list and grid (gdk-pixbuf's scaling loader gives up on them); the first frame is now taken from the animation and scaled.
- Two unit tests shared a temporary folder and could fail each other when run in parallel (seen in CI for 0.18.2).

### Changed
- README: a new picture at the top showing the grid with thumbnails, the preview, a phone in the sidebar, the mini player and the Now Playing view.

## 0.18.2 — 2026-11-11

### Changed
- Now Playing arrives and leaves in motion: the mini player slides up from the bottom of the sidebar while it fades in and glows once in the accent colour, the equalizer button slides into the header; both slide and fade away when the player quits. The small cover cross-fades between tracks and nods with the bass, a new title fades in, play / pause answers with a bounce, the progress line moves continuously, and the mini player has its own live equalizer. The parts of the full view fade in one after the other when it opens.
- The mini player's tooltip sits on the cover and title only; over the whole card it covered the buttons.

## 0.18.1 — 2026-11-11

### Changed
- Now Playing is much lighter: the cover, its halo and the spectrum bars are rendered once into layers and only composited per frame (drawing a frame took about 5.5 ms, now 0.8 ms); the animation clock stops when nothing moves (paused, no fade), runs at 60 frames a second in the full view even on faster screens and at 20 for the small header equalizer.

### Fixed
- Build with the current Rust toolchain: a new clippy lint rejected the audio capture loop, which failed CI for 0.18.0.

## 0.18.0 — 2026-11-11

### Added
- **Now Playing**: whatever plays on this computer (a browser tab on YouTube, Spotify, mpv, any MPRIS player) shows up in omafile. A live equalizer button in the header and a mini player at the bottom of the sidebar (cover, title, previous / play-pause / next, progress) appear while a player exists and vanish with it. `F8` slides in the full view: cover art with a halo that breathes with the bass, a blurred backdrop in the cover's colours, a 44-band spectrum with falling peaks and reflection, seek bar, shuffle, repeat, volume, a chooser when several players run and a button that brings the player to the front. Keys in the view: Space, ← →, ↑ ↓, N / P, Esc.
- The spectrum follows the real sound: omafile listens to the output monitor through `parec` or `pw-record` only while the view is open (own FFT, nothing stored); without them, or with the switch off, a generated animation runs instead. Preferences › Now Playing has switches for the feature and for the live spectrum.

## 0.17.1 — 2026-11-10

### Fixed
- *Share on Local Network* is served on the LocalSend port 53317 under a private path, the only port Omarchy's firewall lets in; on a random port the page was unreachable from a phone. A standalone port is still used when nearby devices are off, with the `ufw` command in the dialog.

### Changed
- Release tooling: GitHub Actions CI (build, test, clippy, catalogs, manifest, `makepkg`) and a tag-driven Release workflow with a prebuilt tarball; AUR packages `omafile` and `omafile-bin` under `packaging/aur/` with update and publish scripts; `scripts/bump-version.sh`, `scripts/make-dist.sh`, `scripts/check-i18n.py`; `docs/RELEASING.md` and a real-machine `docs/acceptance-checklist.md`.
- PKGBUILD: optional dependencies brought up to date (gvfs-afc, libimobiledevice, libheif, ripgrep, bat, jq, calibre, foliate, …), python dropped (the share page is served natively), plugin catalogs and changelog installed.
- Removed a stray `requirements.txt` (a Python test fixture that had slipped into the repository).

## 0.17.0 — 2026-11-10

### Added
- **Animated previews**: GIFs (and animated WebP when the pixbuf loader is installed) play in the preview pane, Quick Look and the gallery; a click on the picture pauses and resumes. Thumbnails in list and grid stay still.

## 0.16.0 — 2026-11-09

### Added
- **Removable drives and phones live in the sidebar**: a volume appears the moment it is plugged in, is mounted automatically (switch in Preferences), gets a toast with *Open*; unmounted ones show a + and mount on click; the row's eject button (or *Eject* in the context menu) ejects removable media; unplugging removes the entry, sends tabs inside it home and shows a toast.

## 0.15.0 — 2026-11-09

### Added
- **Nearby devices (LocalSend protocol)**: phones and computers running LocalSend appear under *iPhone & iPad* in the sidebar; drop files on one to send, receive with an Accept prompt into a chosen folder (`Ctrl+Alt+P` opens the devices dialog). Plain HTTP, discovery by UDP multicast, no external crates.
- **iPhone over USB**: devices mounted through gvfs-afc get their own sidebar section and *Import Photos* (camera roll → `Pictures/iPhone/YYYY-MM`, HEIC → JPEG, skips earlier imports, undoable); `omafile --import-photos`; bar-widget rows for a connected phone and nearby devices.
- **Share page with uploads**: *Share on Local Network* is now served by omafile itself with a phone-friendly page: download the selection, or drop / pick files to send them into the folder on show.
- Preferences › iPhone & Nearby: nearby on/off, HEIC conversion, import folder.

## 0.14.0 — 2026-11-09

### Added
- **eBooks**: covers of EPUB, CBZ / CBR and FB2 as thumbnails and in the preview; metadata (title, author, series, publisher, year, language, ISBN, chapters) in the preview pane and Properties; the page turner reads chapters (and comic pages) in Quick Look.
- Context menu › eBook: Open in Reader, Rename from Metadata (undoable), Send to eReader (Kindle, Kobo, Tolino, PocketBook…), Convert to EPUB / MOBI / AZW3 / PDF via Calibre.
- "eBooks" group in Group by Kind.

## 0.13.0 — 2026-11-09

### Changed
- Folders stream in while listing; skeleton rows for slow folders; a real crossfade between folders (the old view fades out over the new one).
- Thumbnails use the freedesktop cache (`~/.cache/thumbnails`, shared with Nautilus and Loupe); visible rows decode first, at most four at a time, recycled cells are skipped; the gallery pre-decodes its neighbours.
- Local subfolders are prefetched and recently listed folders (incl. remote) show instantly from a cache while the fresh listing arrives; a dropped SFTP share is remounted and retried automatically.
- Touchpad: two-finger swipe for back / forward, pinch to zoom the grid, kinetic scrolling.
- Per-folder view memory (`~/.config/omafile/views`, switch in Preferences): view, sort, grouping, icon size.
- Drag & drop: auto-scroll near list edges, spring-loaded tabs and breadcrumbs that accept drops, count badge on multi-item drags.
- Toasts of one kind merge; error toasts have Retry; the window title shows the transfer percentage.
- Hyprland snippets for the Shelf (float, pin, size, position) and opening on the monitor under the cursor.

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
