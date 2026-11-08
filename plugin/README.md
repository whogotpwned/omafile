# Omafile shell plugin

A bar widget for the Omarchy shell that opens Omafile. The plugin lives in
this repository so `omarchy plugin add` can install it straight from git; the
file manager itself is installed separately (see the main README).

```sh
omarchy plugin add https://github.com/whogotpwned/omafile.git --enable
```

Placement follows the normal Omarchy rules. The manifest asks for the `left`
section (next to the Omarchy menu). Move it like any other widget:

```sh
omarchy bar move whogotpwned.omafile --section right
omarchy bar move whogotpwned.omafile --section centerEnd --index 0
omarchy bar set  whogotpwned.omafile path ~/Projects
```

or drag it in the bar, or use the menu under *Appearance › Menu Bar*. Valid
sections are `left`, `centerBegin`, `center`, `centerEnd`, `right` and `tray`.

Clicks: left raises the open omafile window or opens the configured folder
(`~` by default), right shows a places menu (Home, Downloads, Documents,
Pictures, Projects, Trash, folder of the active terminal, new window), middle
opens the active terminal's folder. The icon is a Nerd Font glyph in the bar's
font; change it with `omarchy bar set whogotpwned.omafile icon "󰉋"`.

Remove with `omarchy plugin remove whogotpwned.omafile`.

Validate after changes:

```sh
omarchy plugin validate .
```

## Status marker

omafile writes `~/.local/state/omafile/status` while it runs:

```
busy=1      a copy or move is running
vaults=2    unlocked gocryptfs folders
```

The widget watches that file (`FileView` with `watchChanges`) and shows `⇅` next to the icon while busy and a lock glyph while vaults are open. The popup then offers *Lock all encrypted folders*, which runs `omafile --lock-all`.
