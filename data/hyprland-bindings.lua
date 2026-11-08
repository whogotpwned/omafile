-- omafile bindings for Omarchy (Lua configuration, Omarchy 4 "quattro" and later).
-- Add to ~/.config/hypr/bindings.lua, then reload with SUPER+ESC.

-- Replace Nautilus as the file manager.
o.rebind("SUPER + SHIFT + F", "File manager", { launch = "omafile" })
o.rebind("SUPER + ALT + SHIFT + F", "File manager (cwd)", "setsid uwsm-app -- omafile \"$(omarchy-cmd-terminal-cwd)\"")

-- Optional: give omafile the same glassy transparency as Nautilus.
-- o.transparent_window("io\\.github\\.whogotpwned\\.Omafile")
