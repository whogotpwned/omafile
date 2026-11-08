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

Clicks: left opens the configured folder (`~` by default), right opens the
active terminal's directory, middle opens a window at `/`.

Validate after changes:

```sh
omarchy plugin validate .
```
