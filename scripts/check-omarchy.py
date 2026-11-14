#!/usr/bin/env python3
"""Check omafile against an Omarchy source tree.

    scripts/check-omarchy.py <omarchy checkout> [--bin target/release/omafile]

omafile leans on Omarchy in many small ways: commands it runs, folders it reads,
the format of a theme, the templates that turn a theme's colours into config
files, the shell's plugin interface. Each of those is an assumption that an
Omarchy release can break. This script states every one of them and tests it
against a checkout of Omarchy, so a change upstream shows up here, with the
name of what broke, instead of on somebody's desktop.

Nothing from the checkout is executed: its files are only read. What is run is
omafile itself (to describe themes and to write one).
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
problems = []
passed = 0


def ok(text):
    global passed
    passed += 1
    if os.environ.get("VERBOSE"):
        print(f"  ok  {text}")


def bad(text):
    problems.append(text)
    print(f"  FAIL {text}")


def check(condition, text):
    ok(text) if condition else bad(text)


def read(path):
    try:
        with open(path, encoding="utf-8", errors="replace") as handle:
            return handle.read()
    except OSError:
        return None


def section(title):
    print(f"\n{title}")


def parse_colors(text):
    """`key = "value"` lines, the way Omarchy's own parser reads them."""
    out = {}
    for line in text.splitlines():
        if "=" not in line or line.lstrip().startswith("#"):
            continue
        key, value = line.split("=", 1)
        key = key.strip().strip("\"'")
        value = value.strip()
        match = re.search(r"[\"']([^\"']*)[\"']", value)
        out[key] = match.group(1) if match else value.split("#")[0].strip()
    return out


def main():
    args = sys.argv[1:]
    binary = os.path.join(ROOT, "target/release/omafile")
    if "--bin" in args:
        at = args.index("--bin")
        binary = os.path.abspath(args[at + 1])
        del args[at:at + 2]
    if len(args) != 1 or not os.path.isdir(os.path.join(args[0], "bin")):
        print(__doc__)
        return 2
    omarchy = os.path.abspath(args[0])
    version = (read(os.path.join(omarchy, "version")) or "?").strip()
    print(f"omafile against Omarchy {version} in {omarchy}")

    # ----- 1. commands ---------------------------------------------------------------------
    section("Commands omafile runs exist")
    # Names that look like commands but are not: a project template's id, a file name in a test.
    not_commands = {"omarchy-plugin", "omarchy-theme"}
    sources = [os.path.join(ROOT, "src", f) for f in os.listdir(os.path.join(ROOT, "src"))]
    sources += [os.path.join(ROOT, "plugin/BarWidget.qml"), os.path.join(ROOT, "install.sh")]
    used = {}
    for path in sources:
        for name in re.findall(r"omarchy-[a-z0-9]+(?:-[a-z0-9]+)*", read(path) or ""):
            used.setdefault(name, os.path.relpath(path, ROOT))
    # Commands of releases before 4 that omafile still knows how to use when they are there.
    older = {"omarchy-theme-bg-set", "omarchy-theme-bg-next"}
    for name in sorted(set(used) - not_commands):
        exists = os.path.isfile(os.path.join(omarchy, "bin", name))
        check(exists or name in older, f"{name} (used in {used[name]})")

    # ----- 2. places and formats -------------------------------------------------------------
    section("Folders, names and formats are where omafile expects them")
    contracts = [
        ("bin/omarchy-theme-set", r'CURRENT_THEME_PATH="\$HOME/\.local/state/omarchy/current/theme"', "the current theme is ~/.local/state/omarchy/current/theme"),
        ("bin/omarchy-theme-set", r'current/theme\.name"', "its name is kept in current/theme.name"),
        ("bin/omarchy-theme-set", r'USER_THEMES_PATH="\$HOME/\.config/omarchy/themes"', "user themes are in ~/.config/omarchy/themes"),
        ("bin/omarchy-theme-set", r'OMARCHY_THEMES_PATH="\$OMARCHY_PATH/themes"', "shipped themes are in $OMARCHY_PATH/themes"),
        ("bin/omarchy-theme-set", r"tr '\[:upper:\]' '\[:lower:\]' \| tr ' ' '-'", "a theme is found by its name in lower case with dashes"),
        ("bin/omarchy-theme-set", r"omarchy-theme-set-templates", "theme-set renders the templates from colors.toml"),
        ("bin/omarchy-theme-bg-next", r'\$HOME/\.config/omarchy/backgrounds/\$THEME_NAME/', "a user's own backgrounds are in ~/.config/omarchy/backgrounds/<theme>"),
        ("bin/omarchy-theme-bg-set", r"realpath|readlink|ln ", "omarchy-theme-bg-set takes a picture's path"),
        ("bin/omarchy-theme-color", r"\[red\]=color1", "the older color0..color15 names are still understood"),
        ("bin/omarchy-theme-color", r"light\.mode", "the light.mode marker is still understood"),
        ("bin/omarchy-theme-color", r"THEME_COLORS\[mode\]", "a theme's mode is the `mode` key"),
        ("bin/omarchy-capture-screenshot", r"OMASNAP_SCREENSHOT_DIR", "screenshots honour OMASNAP_SCREENSHOT_DIR (and OMARCHY_SCREENSHOT_DIR)"),
        ("manual/12-screenshots-recording.md", r"~/Pictures/Screenshots", "screenshots are saved in ~/Pictures/Screenshots"),
        ("manual/12-screenshots-recording.md", r"`screenshot-\d{4}-\d\d-\d\d_\d\d-\d\d-\d\d", "screenshots are named screenshot-<date>_<time>…"),
        ("config/imv/config", r'omasnap "\$imv_current_file"', "Omasnap opens a picture for drawing when given its path"),
        ("bin/omarchy-capture-screenrecording", r"OMARCHY_SCREENRECORD_DIR:-\$\{XDG_VIDEOS_DIR:-\$HOME/Videos\}", "recordings go to OMARCHY_SCREENRECORD_DIR, else the Videos folder"),
        ("bin/omarchy-capture-screenrecording", r"screenrecording-\$\(date \+'%Y-%m-%d_%H-%M-%S'\)\.mp4", "recordings are named screenrecording-<date>_<time>.mp4"),
        ("default/hypr/bindings/utilities.lua", r'"PRINT", *"[^"]*", *"omarchy-capture-screenshot"', "Print takes a screenshot"),
        ("default/hypr/bindings/utilities.lua", r'"ALT \+ PRINT"', "Alt + Print records the screen"),
        ("bin/omarchy-i18n", r"omarchy/language", "the language is in ~/.config/omarchy/language"),
        ("default/bash/env-bootstrap", r"OMARCHY_PATH=/usr/share/omarchy", "Omarchy itself is in /usr/share/omarchy"),
        ("manual/47-system-snapshots.md", r"not your `/home`", "system snapshots leave /home alone (which is why omafile backs it up)"),
        ("bin/omarchy-launch-or-focus", r".", "omarchy-launch-or-focus exists for the bar button"),
    ]
    for path, pattern, meaning in contracts:
        text = read(os.path.join(omarchy, path))
        if text is None:
            bad(f"{meaning}: {path} is gone")
        else:
            check(re.search(pattern, text) is not None, f"{meaning} ({path})")

    # ----- 3. every theme Omarchy ships reads the same in omafile ----------------------------------
    section("omafile reads every shipped theme as Omarchy defines it")
    themes_dir = os.path.join(omarchy, "themes")
    names = sorted(d for d in os.listdir(themes_dir) if os.path.isfile(os.path.join(themes_dir, d, "colors.toml")))
    check(len(names) >= 10, f"{len(names)} themes found")
    compared = ["accent", "background", "foreground", "red", "green", "yellow", "blue", "magenta", "cyan"]
    for name in names:
        folder = os.path.join(themes_dir, name)
        theirs = parse_colors(read(os.path.join(folder, "colors.toml")))
        run = subprocess.run([binary, "--describe-theme", folder], capture_output=True, text=True)
        ours = dict(line.split("\t", 1) for line in run.stdout.splitlines() if "\t" in line)
        wrong = [f"{k}: Omarchy {theirs[k]}, omafile {ours.get(k)}" for k in compared if k in theirs and theirs[k].lower() != (ours.get(k) or "").lower()]
        mode = theirs.get("mode") or ("light" if os.path.exists(os.path.join(folder, "light.mode")) else None)
        if mode and mode != ours.get("mode"):
            wrong.append(f"mode: Omarchy {mode}, omafile {ours.get('mode')}")
        check(run.returncode == 0 and not wrong, f"{name}" + (": " + "; ".join(wrong) if wrong else ""))

    # ----- 4. a theme omafile writes has everything Omarchy's templates ask for ---------------------
    section("A theme made by omafile gives the templates every colour they ask for")
    with tempfile.TemporaryDirectory() as scratch:
        made = {}
        for mode, picture in (("dark", "docs/screenshot-skins.png"), ("light", "docs/screenshot-light.png")):
            run = subprocess.run([binary, "--theme-from-picture", os.path.join(ROOT, picture), "--name", f"Compat {mode}", f"--{mode}", "--into", scratch], capture_output=True, text=True)
            folder = run.stdout.strip()
            check(run.returncode == 0 and os.path.isfile(os.path.join(folder, "colors.toml")), f"omafile writes a {mode} theme ({run.stderr.strip() or 'ok'})")
            if run.returncode == 0:
                made[mode] = folder
        stock = parse_colors(read(os.path.join(themes_dir, "tokyo-night", "colors.toml")) or "")
        # What Omarchy's resolver adds by itself to whatever a theme defines (omarchy-theme-color).
        derived = {"cursor", "purple", "bright_purple", "theme_type", "mode", "bg", "fg", "dark_bg", "darker_bg", "lighter_bg", "dark_fg", "light_fg", "bright_fg"} | {f"color{i}" for i in range(16)}
        templates_dir = os.path.join(omarchy, "default/themed")
        templates = sorted(f for f in os.listdir(templates_dir) if f.endswith(".tpl"))
        check(len(templates) >= 10, f"{len(templates)} templates found")
        for mode, folder in made.items():
            name = os.path.basename(folder)
            check(re.fullmatch(r"[a-z0-9_][a-z0-9._+-]*", name) is not None, f"{mode}: the folder name {name} is one Omarchy accepts")
            text = read(os.path.join(folder, "colors.toml"))
            colors = parse_colors(text)
            check(colors.get("mode") == mode, f"{mode}: mode = \"{mode}\"")
            check(mode != "light" or os.path.exists(os.path.join(folder, "light.mode")), f"{mode}: light.mode marker for releases before 4")
            missing = sorted(set(stock) - set(colors))
            check(not missing, f"{mode}: every key of a shipped theme is written" + (f" (missing {', '.join(missing)})" if missing else ""))
            # The characters Omarchy's parser lets through (it drops a key otherwise).
            odd = [k for k, v in colors.items() if not re.fullmatch(r"[A-Za-z0-9_-]+", k) or not re.fullmatch(r"[A-Za-z0-9#(),._+/% -]*", v)]
            check(not odd, f"{mode}: keys and values use only characters Omarchy accepts" + (f" ({odd})" if odd else ""))
            hexes = [k for k, v in colors.items() if k != "mode" and not re.fullmatch(r"#[0-9a-f]{6}", v)]
            check(not hexes, f"{mode}: every colour is #rrggbb" + (f" ({hexes})" if hexes else ""))
            known = set(colors) | derived
            for template in templates:
                body = read(os.path.join(templates_dir, template)) or ""
                unknown = set()
                for token in re.findall(r"\{\{([^{}]*)\}\}", body):
                    words = token.split()
                    if not words:
                        continue
                    if words[0] in ("mix", "mix_strip", "mix_rgb"):
                        refs = words[1:3]
                    elif words[0] in ("hypr_gradient", "gradient_start", "shell_gradient"):
                        # The first is the key; a second word is a fallback and may be anything.
                        refs = [] if len(words) > 2 else words[1:2]
                    else:
                        refs = [re.sub(r"_(strip|rgb)$", "", words[0])]
                    unknown |= {r for r in refs if r not in known}
                check(not unknown, f"{mode}: {template}" + (f" asks for {', '.join(sorted(unknown))}" if unknown else ""))
            check(any(f.lower().endswith((".png", ".jpg", ".jpeg", ".webp")) for f in os.listdir(os.path.join(folder, "backgrounds"))), f"{mode}: the picture is the theme's background")
            check(os.path.isfile(os.path.join(folder, "preview.png")), f"{mode}: preview.png for Omarchy's theme picker")
            check(not os.path.exists(os.path.join(folder, ".git")), f"{mode}: no .git, so Omarchy treats the theme as the user's own")
            described = subprocess.run([binary, "--describe-theme", folder], capture_output=True, text=True).stdout
            check(f"accent\t{colors.get('accent')}" in described, f"{mode}: omafile reads its own theme back")

    # ----- 5. the bar button -----------------------------------------------------------------------
    section("The bar button fits the shell's plugin interface")
    manifest = json.loads(read(os.path.join(ROOT, "manifest.json")))
    validator = read(os.path.join(omarchy, "bin/omarchy-plugin-validate")) or ""
    check(".schemaVersion == 1" in validator and manifest.get("schemaVersion") == 1, "schemaVersion 1 is what the shell understands")
    required = re.search(r"for field in ([a-zA-Z ]+); do", validator)
    for field in (required.group(1).split() if required else ["id", "name", "version", "kinds", "entryPoints"]):
        check(field in manifest, f"manifest has '{field}'")
    check("bar-widget:barWidget" in validator and "bar-widget" in manifest.get("kinds", []) and "barWidget" in manifest.get("entryPoints", {}), "kind bar-widget with entryPoints.barWidget")
    sections = re.search(r'\["left"[^\]]*\]', validator)
    allowed = json.loads(sections.group(0)) if sections else []
    check(manifest.get("barWidget", {}).get("defaultSection") in allowed, f"defaultSection is one of {allowed}")
    check(os.path.isfile(os.path.join(ROOT, manifest["entryPoints"]["barWidget"])), "the entry point file exists")
    check(not manifest["id"].startswith("omarchy.") and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", manifest["id"]) is not None, "the id is valid and not in the reserved namespace")
    widget = read(os.path.join(ROOT, "plugin/BarWidget.qml")) or ""
    # Every component the widget instantiates must be one the shell exports, one QtQuick or
    # Quickshell brings, or one the widget defines itself: a type the shell does not know
    # keeps the whole widget from loading.
    exported = set()
    for module in ("shell/Ui/qmldir", "shell/Commons/qmldir"):
        exported |= set(re.findall(r"^(?:singleton\s+)?([A-Z]\w+)\s+[\d.]+\s+\S+\.qml", read(os.path.join(omarchy, module)) or "", re.M))
    own = set(re.findall(r"^\s*component\s+([A-Z]\w+)\s*:", widget_text := (read(os.path.join(ROOT, "plugin/BarWidget.qml")) or ""), re.M))
    basics = {"Item", "Rectangle", "Text", "Column", "Row", "Repeater", "MouseArea", "Timer", "Image", "Loader", "Component", "QtObject", "FileView", "Process", "Connections"}
    for used_type in sorted(set(re.findall(r"^\s*(?:\w+\.)?([A-Z]\w+)\s*\{", widget_text, re.M)) | set(re.findall(r":\s*(?:\w+\.)?([A-Z]\w+)\s*\{", widget_text))):
        check(used_type in exported or used_type in own or used_type in basics, f"the widget's {used_type} is a component this shell has")
    api = [
        ("shell/Commons/qmldir", r"\bStyle\b", "Commons.Style"),
        ("shell/Commons/qmldir", r"\bColor\b", "Commons.Color"),
        ("shell/Ui/BarWidget.qml", r"\bmoduleName\b", "BarWidget.moduleName"),
        ("shell/Ui/BarWidget.qml", r"function setting\(", "BarWidget.setting()"),
        ("shell/Ui/BarWidget.qml", r"property (var|QtObject|[A-Za-z.]+) bar\b", "BarWidget.bar"),
        ("shell/Ui/BarIconButton.qml", r"(?m)^WidgetButton \{", "BarIconButton is a WidgetButton"),
        ("shell/Ui/WidgetButton.qml", r"signal pressed\(int button\)", "WidgetButton.pressed(button)"),
        ("shell/Ui/WidgetButton.qml", r"property bool active\b", "WidgetButton.active"),
        ("shell/Ui/WidgetButton.qml", r"property string text\b", "WidgetButton.text"),
        ("shell/Ui/WidgetButton.qml", r"property var bar\b", "WidgetButton.bar"),
        ("shell/Ui/PopupCard.qml", r"\banchorItem\b", "PopupCard.anchorItem"),
        ("shell/Ui/PopupCard.qml", r"\bowner\b", "PopupCard.owner"),
        ("shell/Ui/PopupCard.qml", r"function fittedContentWidth\(", "PopupCard.fittedContentWidth()"),
        ("shell/Ui/PopupCard.qml", r"function fittedContentHeight\(", "PopupCard.fittedContentHeight()"),
        ("shell/Ui/PopupCard.qml", r"property bool open\b", "PopupCard.open"),
        ("shell/Commons/Style.qml", r"readonly property int popupRowHeight\b", "Style.spacing.popupRowHeight"),
        ("shell/Commons/Style.qml", r"readonly property int controlPaddingX\b", "Style.spacing.controlPaddingX"),
        ("shell/Commons/Style.qml", r"readonly property color hoverFill\b", "Style.hoverFill"),
        ("shell/Commons/Style.qml", r"property int cornerRadius\b", "Style.cornerRadius"),
        ("shell/Commons/Translations.qml", r"property (string|url|var) directory\b", "Translations.directory"),
        ("shell/Commons/Translations.qml", r"function t\(", "Translations.t()"),
        ("shell/Commons/Style.qml", r"function space\(", "Style.space()"),
        ("shell/Commons/Color.qml", r"\bpopups\b", "Color.popups"),
    ]
    for path, pattern, what in api:
        text = read(os.path.join(omarchy, path))
        check(text is not None and re.search(pattern, text) is not None, f"{what} ({path})")
    run_api = "\n".join(read(os.path.join(omarchy, p)) or "" for p in ("shell/Ui/PluginBarApi.qml", "shell/plugins/bar/Bar.qml"))
    check(re.search(r"function run\(", run_api) is not None, "bar.run() to start a command")
    for word in ("uwsm-app", "omarchy-launch-or-focus", "omarchy-cmd-terminal-cwd"):
        check(word in widget, f"the widget starts programs through {word}")
    check(os.path.isfile(os.path.join(omarchy, "bin/omarchy-cmd-terminal-cwd")), "omarchy-cmd-terminal-cwd exists")

    # Qt's own linter reads the widget against this shell's components, when it is installed.
    # Quickshell itself is not on a build machine, so what it brings stays unresolved, and
    # with it the shell's PopupCard, which is one of Quickshell's windows.
    linter = next((p for p in ("qmllint", "qmllint6", "/usr/lib/qt6/bin/qmllint", "/usr/lib/qt6/bin/qmllint6") if shutil.which(p) or os.path.isfile(p)), None)
    if linter:
        with tempfile.TemporaryDirectory() as imports:
            os.symlink(os.path.join(omarchy, "shell"), os.path.join(imports, "qs"))
            run = subprocess.run([linter, "-I", imports, os.path.join(ROOT, "plugin/BarWidget.qml")], capture_output=True, text=True)
        # Of what it says, two things count here: a component it cannot find, and a syntax
        # error. Quickshell's own types cannot be found on a build machine, and what is
        # written inside them cannot be judged either.
        absent = ("Quickshell", "FileView", "PopupWindow", "PopupCard", "jsroot")
        complaints = [line.strip() for line in (run.stdout + run.stderr).splitlines() if (line.startswith("Error:") or re.match(r"\S+\.qml:\d+:\d+: ", line) or "is not resolved" in line or "was not found" in line) and not any(word in line for word in absent)]
        check(not complaints, "qmllint finds every component and no syntax error in the widget" + (": " + " | ".join(complaints[:4]) if complaints else ""))
    else:
        print("  (qmllint is not installed: the widget's QML was not linted)")

    print(f"\n{passed} checks passed, {len(problems)} failed (Omarchy {version})")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
