import QtQuick
import qs.Ui

// Omafile bar button. Follows the omarchy.menu widget: a BarIconButton inside a
// BarWidget, colours and font taken from the bar, commands run through the bar.
//
// Settings (inline on the shell.json layout entry, editable with
// `omarchy bar set whogotpwned.omafile path ~/Projects`):
//   path  folder opened on left click, "~" by default
//   icon  glyph shown in the bar, a Nerd Font folder by default
BarWidget {
  id: root
  moduleName: "whogotpwned.omafile"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  readonly property string folder: setting("path", "~")

  function launch(target) {
    if (!root.bar) return
    // uwsm-app keeps the window in the user session like every Omarchy launcher.
    root.bar.run("setsid uwsm-app -- omafile " + target)
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: setting("icon", "")
    onPressed: function(mouseButton) {
      if (mouseButton === Qt.RightButton) {
        // Same behaviour as Omarchy's "File manager (cwd)" binding.
        launch("\"$(omarchy-cmd-terminal-cwd)\"")
      } else if (mouseButton === Qt.MiddleButton) {
        launch("/")
      } else {
        launch(root.folder)
      }
    }
  }
}
