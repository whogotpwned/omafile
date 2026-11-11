# Acceptance checklist

Everything in omafile was developed and exercised under Xvfb in a container.
This list covers what could **not** be verified there: real Hyprland / Wayland,
real hardware, the Omarchy shell. Work through it once on an Omarchy machine
before tagging a release; tick the boxes, note what misbehaves.

Run with tracing while testing, it prints every action and any panic:

```sh
OMAFILE_TRACE=1 omafile 2>&1 | tee /tmp/omafile.log
```

Report a failing item with the item number, what you did, what happened, and
the tail of that log.

## 0. Install

- [ ] 0.1 `./install.sh --all` builds and finishes without errors; the list of optional tools looks right.
- [ ] 0.2 omafile is in the launcher (Super+Space) with its icon and a translated name.
- [ ] 0.3 `Super+Shift+F` opens omafile; `Super+Alt+Shift+F` opens it in the terminal's folder.
- [ ] 0.4 `xdg-open ~/Downloads` opens omafile (default app for folders).
- [ ] 0.5 `makepkg -si` from `packaging/aur/omafile` (after a tag exists) builds and installs.

## 1. Look and feel on Hyprland

- [ ] 1.1 Colours, accent and icon theme match the current Omarchy theme.
- [ ] 1.2 `omarchy theme set <other>` re-skins the open window within a second; a light theme switches to light mode.
- [ ] 1.3 Window corners, borders and gaps look right when tiled, floating and fullscreen.
- [ ] 1.4 Panel animations are smooth (terminal panel `Ctrl+\``, split `F6`, folder crossfade); with "reduce motion" they stop.
- [ ] 1.5 Text is sharp on a HiDPI / fractional-scale monitor; thumbnails are not blurry.
- [ ] 1.6 `omafile --toggle` (bind it) shows and hides a drop-down window on the special workspace.

## 2. Keyboard and pointer

- [ ] 2.1 `F1` lists the bindings; spot-check ten of them, including `Ctrl+K`, `F3`, `F6`, `Ctrl+L`, `Space`.
- [ ] 2.2 No omafile shortcut collides with an Omarchy / Hyprland binding you use (note collisions).
- [ ] 2.3 Vim keys (Preferences › Keys): `h j k l`, `gg`, `G`, `/`, `:` work in the list and never in text fields or the terminal panel.
- [ ] 2.4 Touchpad: two-finger swipe left / right goes back / forward; pinch zooms the grid; scrolling is kinetic.
- [ ] 2.5 Right-click menus open at the pointer and close on Escape.

## 3. Drag and drop (not testable under Xvfb)

- [ ] 3.1 Drag files between two tabs, between the two sides of the split, and onto a sidebar place.
- [ ] 3.2 Drag from omafile into another app (browser upload, terminal, editor) and from another app into omafile.
- [ ] 3.3 Hover a folder, a tab and a breadcrumb while dragging: they spring open after a moment.
- [ ] 3.4 Dragging several items shows the count badge; the list auto-scrolls near its edges.
- [ ] 3.5 Shelf (`Ctrl+Shift+S`): drop files onto it from two folders, drag them all out into a third. With the windowrules from `data/hyprland-bindings.conf` it floats and stays on top.

## 4. Devices

- [ ] 4.1 Plug in a USB stick: it appears under Devices, mounts on its own, a toast offers *Open*.
- [ ] 4.2 Click the eject button: toast says it can be unplugged; the entry disappears when you pull it.
- [ ] 4.3 Pull a stick **without** ejecting while a tab is inside: the tab goes home, no crash.
- [ ] 4.4 With auto-mount switched off (Preferences): the stick shows a + and mounts on click.
- [ ] 4.5 SD card / external disk / Android phone (MTP) behave the same.

## 5. iPhone and iPad

Needs `sudo pacman -S gvfs-afc libimobiledevice`, optionally `libheif`.

- [ ] 5.1 USB: plug in, unlock, tap *Trust*: the phone appears under *iPhone & iPad*; DCIM is browsable.
- [ ] 5.2 *Import Photos*: files land in `~/Pictures/iPhone/YYYY-MM`, HEIC became JPEG, a second run imports nothing new; undo removes them.
- [ ] 5.3 `omafile --import-photos` and the bar-menu row do the same.
- [ ] 5.4 LocalSend app on the phone, Encryption off: the phone appears in the sidebar within ~10 s, and this computer appears in the app.
- [ ] 5.5 Drop three files on the phone in the sidebar: they arrive in the Files app.
- [ ] 5.6 Send photos from the phone: the Accept dialog appears, files land in `~/Downloads`, *Show* reveals them.
- [ ] 5.7 With Encryption **on** in the app: omafile shows "encryption on (cannot send)" instead of failing silently.
- [ ] 5.8 *Share on Local Network*: scan the QR code with the camera; the page opens in Safari, a download works, *Choose files* uploads photos into the folder on show.
- [ ] 5.9 The firewall (ufw is on in Omarchy) lets port 53317 TCP/UDP and the share port through, or note the rule you needed.

## 6. Bar widget (Omarchy shell)

- [ ] 6.1 `omarchy plugin add https://github.com/whogotpwned/omafile.git --enable` adds the button; `omarchy plugin validate .` passes.
- [ ] 6.2 Left click opens / focuses omafile; right click shows the places menu in the system language.
- [ ] 6.3 A running copy shows the busy marker; an unlocked vault shows *Lock all encrypted folders*; a connected phone shows *Import photos from iPhone*.

## 7. Files and previews with real data

- [ ] 7.1 Open `~` and a folder with 10,000+ files: rows appear at once, scrolling stays smooth.
- [ ] 7.2 A photo folder in grid view: thumbnails fill in from the visible rows first; a second visit is instant (disk cache).
- [ ] 7.3 Preview / Quick Look: a long PDF (page turning), an MP3 and an H.264 video (needs `gst-libav`), a GIF (animated), an EPUB (cover, chapters).
- [ ] 7.4 Copy a multi-GB file to a USB stick: progress banner, title percentage, cancel works and leaves no partial file.
- [ ] 7.5 Trash, restore, undo (`Ctrl+Z`) and redo across a copy, a move, a rename and a tag.
- [ ] 7.6 Terminal panel follows the folder, uses your shell and theme, activates a Python venv.
- [ ] 7.7 Git: badges in a repository, stage by clicking a badge, commit dialog, branch switch.
- [ ] 7.8 Encrypted folder: create, lock, unlock, change password (`gocryptfs` installed).

## 8. Remote

- [ ] 8.1 A host from `~/.ssh/config` connects from the sidebar; browsing, copy in both directions.
- [ ] 8.2 Suspend the laptop or drop Wi-Fi with an SFTP tab open: on return the tab reconnects or offers *Retry*.

## 9. Language

- [ ] 9.1 With Omarchy set to German (Setup › Language): menus, dialogs, toasts and the bar widget are German; nothing obvious is left in English.
- [ ] 9.2 `OMAFILE_LANG=fr omafile` switches a single run.

## 10. Optional tools

- [ ] 10.1 Calibre installed: context menu › eBook › Convert to produces a file next to the original.
- [ ] 10.2 Folder rules on `~/Downloads`: a new PDF moves on its own a few seconds after the download finishes (not while the browser still writes it).
- [ ] 10.3 Disk usage map, Find Duplicates on a real project tree: finish in reasonable time, trash from within works.

## 11. Now Playing

- [ ] 11.1 Play a YouTube video in the browser: the equalizer button appears in the header, the mini player in the sidebar shows title and cover; pause / next from omafile reach the browser.
- [ ] 11.2 `F8`: the full view slides in; the spectrum moves with the actual sound (bass on the left); with the music paused it settles.
- [ ] 11.3 Spotify or mpv next to the browser: the chooser at the top switches between them; seeking, volume, shuffle and repeat work where the player supports them.
- [ ] 11.4 Quit the player: mini player and header button disappear, the view says *Nothing is playing*.
- [ ] 11.5 `pavucontrol` › Recording shows omafile only while the Now Playing view is open.

## 12. Vaults

- [ ] 12.1 Sidebar › Add Vault… › Create New…: after name and password the master key is shown once, the vault appears under Vaults and opens.
- [ ] 12.2 Lock it with the button in its row, click it again: the padlock page appears; a wrong password shakes the padlock, the right one opens it and shows the files.
- [ ] 12.3 A vault on a USB stick: unplugged, its page says *Not found*; plugged in again it unlocks as usual.
- [ ] 12.4 Reboot with a vault unlocked: it is locked afterwards (nothing is mounted automatically, no password is stored).
- [ ] 12.5 Grid and list in a mixed folder (photos, PDFs, eBooks, archives): every icon and thumbnail has the same size.
