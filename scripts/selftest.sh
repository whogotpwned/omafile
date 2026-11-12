#!/usr/bin/env bash
# omafile tests itself: every command, in a throwaway home folder.
#
#   scripts/selftest.sh [path to the omafile binary]     (default: target/release/omafile)
#
# Needs xvfb-run (or a display) and dbus-run-session. Exit code 0 when omafile got through
# every command, quit by itself, and GTK had nothing critical to say on the way. Otherwise
# the output names the command it was at.
set -uo pipefail
bin="$(realpath "${1:-$(dirname "$0")/../target/release/omafile}")"
[ -x "$bin" ] || { echo "selftest: no binary at $bin (cargo build --release)" >&2; exit 2; }
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
home="$work/home"
mkdir -p "$home/Start/Folder" "$home/.config" "$home/.local/state" "$home/.cache" "$work/run"
chmod 700 "$work/run"
printf 'Some notes.\nA second line.\n' > "$home/Start/notes.txt"
printf 'inside\n' > "$home/Start/Folder/inside.txt"
# A name with everything markup trips over: wherever omafile shows a file name, it must
# show it as it is.
printf 'x\n' > "$home/Start/R&D <draft> 100%.txt"
# The smallest PNG there is: one grey pixel.
printf '\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x00\x00\x00\x00:~\x9bU\x00\x00\x00\nIDATx\x9cc\xa8\x07\x00\x00\x81\x00\x80\x9d\xa6\xb5\x8c\x00\x00\x00\x00IEND\xaeB`\x82' > "$home/Start/picture.png"
# Not a first start: the tour has its own test, and here it would sit over everything.
mkdir -p "$home/.config/omafile" && printf 'tour = seen\n' > "$home/.config/omafile/omafile.conf"

# Its own home, its own session bus, no Wayland, nothing of the caller's desktop.
run=(env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u OMARCHY_PATH -u OMAFILE_TRACE
  HOME="$home" LANG=C.UTF-8
  XDG_CONFIG_HOME="$home/.config" XDG_STATE_HOME="$home/.local/state" XDG_CACHE_HOME="$home/.cache" XDG_DATA_HOME="$home/.local/share" XDG_RUNTIME_DIR="$work/run"
  GDK_BACKEND=x11 GSK_RENDERER=cairo GTK_A11Y=none NO_AT_BRIDGE=1 OMAFILE_SELFTEST=1 OMAFILE_SELFTEST_SIZE="${OMAFILE_SELFTEST_SIZE:-}" OMAFILE_SELFTEST_ONLY="${OMAFILE_SELFTEST_ONLY:-}" OMAFILE_SELFTEST_REPEAT="${OMAFILE_SELFTEST_REPEAT:-1}" OMAFILE_SELFTEST_ROUND="${OMAFILE_SELFTEST_ROUND:-}" OMAFILE_LANG="${OMAFILE_LANG:-en}"
  timeout "${SELFTEST_TIMEOUT:-420}" dbus-run-session -- ${SELFTEST_WRAP:-} "$bin" "$home/Start")
if [ -n "${DISPLAY:-}" ] && [ -z "${SELFTEST_XVFB:-}" ]; then
  "${run[@]}" > "$work/log" 2>&1
else
  xvfb-run -a -s "-screen 0 1280x800x24" "${run[@]}" > "$work/log" 2>&1
fi
code=$?
[ -z "${SELFTEST_LOG:-}" ] || cp "$work/log" "$SELFTEST_LOG"
steps=$(grep -c '^selftest: ' "$work/log" || true)
if [ "$code" -ne 0 ] || ! grep -q '^selftest: ok' "$work/log"; then
  echo "selftest: FAILED with exit code $code after $steps steps. The last lines:" >&2
  grep -v '^$' "$work/log" | tail -25 >&2
  exit 1
fi

# Criticals, panics, text that broke as markup and dialogs too big for the window, each
# with the command that was running. One kind is GTK's own and
# comes from the test, not from omafile: the test closes the system's file chooser while
# the pointer of the virtual screen rests on it, and GTK's tooltip timer then asks a window
# that is gone where the pointer is. The stack of that message holds no omafile code.
known="assertion .GDK_IS_SURFACE \\(surface\\). failed"
awk -v known="$known" '
  /^selftest: / { step = $0; next }
  /-CRITICAL \*\*|panicked at|error parsing markup|exceeds AdwDialog (height|width)/ { if ($0 !~ known) { print "  during \"" step "\": " $0; bad++ } else { ignored++ } }
  END { if (ignored) print "selftest: " ignored " message(s) of GTK about a file chooser the test closed, not counted" > "/dev/stderr"; exit bad ? 1 : 0 }
' "$work/log" > "$work/criticals"
if [ -s "$work/criticals" ]; then
  echo "selftest: FAILED, omafile ran through but GTK reported:" >&2
  cat "$work/criticals" >&2
  exit 1
fi
grep '^selftest: ok' "$work/log"
