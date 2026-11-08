import QtQuick
import Quickshell
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
  readonly property string folder: setting("path", "~")
  readonly property string windowClass: "io.github.whogotpwned.Omafile"

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

  readonly property var places: [
    { label: "Home", target: "~" },
    { label: "Downloads", target: "~/Downloads" },
    { label: "Documents", target: "~/Documents" },
    { label: "Pictures", target: "~/Pictures" },
    { label: "Projects", target: "~/Projects" },
    { label: "Trash", target: "trash:///" },
    { label: "Folder of the active terminal", target: "\"$(omarchy-cmd-terminal-cwd)\"" },
    { label: "New window", target: root.folder }
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
        text: "Files"
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
    }
  }
}
