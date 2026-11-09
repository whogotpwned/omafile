import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui

// Omafile bar button, built like the first-party widgets: a BarIconButton in
// a BarWidget, colours and font from the bar, commands run through bar.run,
// and a PopupCard with PopupMenuRows for the right-click menu.
//
// Settings live inline on the shell.json layout entry:
//   omarchy bar set whogotpwned.omafile path ~/Projects   folder for a left click
//   omarchy bar set whogotpwned.omafile icon "󰉋"           glyph in the bar (Nerd Font)
BarWidget {
  id: root
  moduleName: "whogotpwned.omafile"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  property bool menuOpen: false

  // Strings follow the Omarchy language (~/.config/omarchy/language); the
  // catalogs live in plugin/i18n/<code>.json, English is the fallback.
  Commons.Translations { id: i18n; directory: Qt.resolvedUrl("i18n") }
  readonly property string folder: setting("path", "~")
  readonly property string windowClass: "io.github.whogotpwned.Omafile"

  // omafile writes ~/.local/state/omafile/status ("busy=0|1", "vaults=N") while
  // it runs; the bar shows a small marker for a running copy and for unlocked
  // encrypted folders.
  property bool busy: false
  property int vaults: 0
  readonly property string stateDir: (Quickshell.env("XDG_STATE_HOME") || (Quickshell.env("HOME") + "/.local/state")) + "/omafile"

  function readStatus(text) {
    var b = false, v = 0
    text.split("\n").forEach(function(line) {
      var kv = line.split("=")
      if (kv[0] === "busy") b = kv[1] === "1"
      if (kv[0] === "vaults") v = parseInt(kv[1]) || 0
    })
    busy = b
    vaults = v
  }

  FileView {
    id: statusFile
    path: root.stateDir + "/status"
    watchChanges: true
    printErrors: false
    onLoaded: root.readStatus(text())
    onFileChanged: reload()
    onLoadFailed: { root.busy = false; root.vaults = 0 }
  }

  // Required by PopupCard (owner.close) and by the shell's popout routing.
  function close() { menuOpen = false }
  function open() { menuOpen = true }
  readonly property bool opened: menuOpen

  function launch(target) {
    if (!root.bar) return
    root.bar.run("setsid uwsm-app -- omafile " + target)
  }

  // Left click: raise the window that is already open, otherwise start one.
  function focusOrLaunch() {
    if (!root.bar) return
    root.bar.run("omarchy-launch-or-focus " + windowClass + " 'uwsm-app -- omafile " + folder + "'")
  }

  function lockAll() {
    if (!root.bar) return
    root.bar.run("omafile --lock-all")
  }

  readonly property var places: [
    { label: i18n.t("Home"), target: "~" },
    { label: i18n.t("Downloads"), target: "~/Downloads" },
    { label: i18n.t("Documents"), target: "~/Documents" },
    { label: i18n.t("Pictures"), target: "~/Pictures" },
    { label: i18n.t("Projects"), target: "~/Projects" },
    { label: i18n.t("Trash"), target: "trash:///" },
    { label: i18n.t("Folder of the active terminal"), target: "\"$(omarchy-cmd-terminal-cwd)\"" },
    { label: i18n.t("New window"), target: root.folder }
  ]

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: setting("icon", "")
    active: root.menuOpen
    onPressed: function(mouseButton) {
      if (mouseButton === Qt.RightButton) {
        root.menuOpen = !root.menuOpen
      } else if (mouseButton === Qt.MiddleButton) {
        launch("\"$(omarchy-cmd-terminal-cwd)\"")
      } else {
        root.menuOpen = false
        focusOrLaunch()
      }
    }
  }

  PopupCard {
    id: menu
    anchorItem: root
    owner: root
    bar: root.bar
    open: root.menuOpen
    contentWidth: menu.fittedContentWidth(Style.space(260))
    contentHeight: menu.fittedContentHeight(column.implicitHeight)

    Column {
      id: column
      anchors.fill: parent
      spacing: Style.space(2)

      Text {
        text: i18n.t("Files")
        textFormat: Text.PlainText
        color: Commons.Color.popups.text
        font.family: Style.font.family
        font.pixelSize: Style.font.body
        font.bold: true
        bottomPadding: Style.space(4)
      }

      Repeater {
        model: root.places
        delegate: PopupMenuRow {
          required property var modelData
          text: modelData.label
          onTriggered: {
            root.menuOpen = false
            root.launch(modelData.target)
          }
        }
      }

      PopupMenuRow {
        visible: root.vaults > 0
        text: i18n.t("Lock all encrypted folders") + " (" + root.vaults + ")"
        onTriggered: {
          root.menuOpen = false
          root.lockAll()
        }
      }

      Text {
        visible: root.busy
        text: i18n.t("Copying…")
        textFormat: Text.PlainText
        color: Commons.Color.popups.text
        opacity: 0.7
        font.family: Style.font.family
        font.pixelSize: Style.font.caption
        topPadding: Style.space(4)
      }
    }
  }
}
