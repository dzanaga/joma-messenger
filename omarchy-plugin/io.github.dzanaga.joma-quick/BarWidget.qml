import QtQuick
import QtQuick.Controls
import qs.Commons
import qs.Ui

// A chat glyph with the unread count. The popup lists favourite and recent contacts;
// choose one, then a quick message to send it.
BarWidget {
  id: root
  moduleName: "io.github.dzanaga.joma-quick"

  readonly property var svc: bar && bar.shell ? bar.shell.serviceFor(root.moduleName) : null
  readonly property int unread: svc ? svc.unread : 0
  readonly property color fg: Color.popups.text
  readonly property color dim: Qt.darker(fg, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  property bool popupOpen: false
  property int selected: 0
  readonly property bool opened: popupOpen

  function open() { popupOpen = true; selected = 0; if (svc) svc.refresh() }
  function close() { popupOpen = false }
  function toggle() { if (popupOpen) close(); else open() }
  function closeForPopoutSwitch() { close() }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: "󰭹" + (root.unread > 0 ? " " + root.unread : "")
    dimmed: !(root.svc && root.svc.running) || root.unread === 0
    tooltipText: !(root.svc && root.svc.running)
      ? "Joma is not running"
      : (root.unread > 0 ? root.unread + " unread in Joma" : "Joma: quick messages")
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.LeftButton) root.toggle()
      else if (buttonCode === Qt.RightButton && root.svc) root.svc.openJoma()
    }
  }

  KeyboardPanel {
    id: popup
    anchorItem: button
    bar: root.bar
    owner: root
    open: root.popupOpen
    focusTarget: keyCatcher
    contentWidth: fittedContentWidth(Style.space(380))
    contentHeight: fittedContentHeight(contentColumn.implicitHeight, Style.space(560))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()

      Flickable {
        anchors.fill: parent
        contentWidth: width
        contentHeight: contentColumn.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        interactive: contentHeight > height

        Column {
          id: contentColumn
          width: parent.width
          spacing: Style.space(10)

          PanelHero {
            width: parent.width
            title: "Joma"
            meta: !(root.svc && root.svc.running)
              ? "not running"
              : (root.svc.account + " · " + (root.svc.registered ? "online" : "offline")
                 + (root.unread > 0 ? " · " + root.unread + " unread" : ""))
            foreground: root.fg
            fontFamily: root.fontFamily
          }

          Button {
            visible: !(root.svc && root.svc.running)
            text: "Open Joma"
            iconText: "󰭹"
            foreground: root.fg
            fontFamily: root.fontFamily
            onClicked: { root.svc.openJoma(); root.close() }
          }

          PanelSectionHeader {
            visible: root.svc && root.svc.running
            width: parent.width
            text: "SEND TO"
            foreground: root.fg
            fontFamily: root.fontFamily
          }

          Repeater {
            model: root.svc && root.svc.running ? root.svc.targets : []
            Rectangle {
              required property var modelData
              required property int index
              width: contentColumn.width
              height: targetText.implicitHeight + Style.space(8)
              color: index === root.selected ? Qt.rgba(root.fg.r, root.fg.g, root.fg.b, 0.12) : "transparent"
              Text {
                id: targetText
                anchors.verticalCenter: parent.verticalCenter
                x: Style.space(6)
                width: parent.width - Style.space(12)
                text: (modelData.favorite ? "★ " : "  ") + modelData.title
                  + (modelData.unread > 0 ? "  (" + modelData.unread + ")" : "")
                color: root.fg
                font.family: root.fontFamily
                font.pixelSize: Style.font.bodySmall
                elide: Text.ElideRight
              }
              MouseArea {
                anchors.fill: parent
                onClicked: root.selected = index
              }
            }
          }

          PanelSeparator { visible: root.svc && root.svc.running; width: parent.width; foreground: root.fg }

          Flow {
            visible: root.svc && root.svc.running && root.svc.targets.length > 0
            width: parent.width
            spacing: Style.space(6)
            Repeater {
              model: root.svc ? root.svc.messages.slice(0, 9) : []
              Button {
                required property var modelData
                required property int index
                text: modelData
                iconText: String(index + 1)
                foreground: root.fg
                fontFamily: root.fontFamily
                onClicked: {
                  var t = root.svc.targets[root.selected]
                  if (t) root.svc.sendQuick(t.conversation, index)
                }
              }
            }
          }

          Row {
            visible: root.svc && root.svc.running
            spacing: Style.space(8)
            Button {
              text: "Open Joma"
              iconText: "󰭹"
              foreground: root.fg
              fontFamily: root.fontFamily
              onClicked: { root.svc.openJoma(); root.close() }
            }
          }

          Text {
            visible: root.svc && root.svc.lastResult !== ""
            width: parent.width
            text: root.svc ? root.svc.lastResult : ""
            wrapMode: Text.WordWrap
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
          }
        }
      }
    }
  }
}
