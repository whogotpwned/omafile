-- omafile bindings for Omarchy (Lua configuration, Omarchy 4 "quattro" and later).
-- Add to ~/.config/hypr/bindings.lua, then reload with SUPER+ESC.

-- Replace Nautilus as the file manager.
o.rebind("SUPER + SHIFT + F", "File manager", { launch = "omafile" })
o.rebind("SUPER + ALT + SHIFT + F", "File manager (cwd)", "setsid uwsm-app -- omafile \"$(omarchy-cmd-terminal-cwd)\"")

-- Optional: give omafile the same glassy transparency as Nautilus.
-- o.transparent_window("io\\.github\\.whogotpwned\\.Omafile")

-- The Shelf (Ctrl+Shift+S in omafile) is a small window titled "Shelf": keep it floating and pinned.
-- Add to ~/.config/hypr/windows.conf (or hyprland.conf):
--   windowrule = float, class:^(io\.github\.whogotpwned\.Omafile)$, title:^(Shelf)$
--   windowrule = pin, class:^(io\.github\.whogotpwned\.Omafile)$, title:^(Shelf)$
--   windowrule = size 280 380, class:^(io\.github\.whogotpwned\.Omafile)$, title:^(Shelf)$
--   windowrule = move 100%-300 60, class:^(io\.github\.whogotpwned\.Omafile)$, title:^(Shelf)$
