import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Hyprland
import qs.Commons
import qs.Ui

// The bar glyph shows the unread count. Its panel lists contacts (pinned and unread on
// top, then whoever is online); choosing one opens a quick chat: the latest messages, a
// reply box, and buttons to call, share the desktop or send a file.
BarWidget {
  id: root
  moduleName: "io.github.dzanaga.joma-quick"

  readonly property var svc: bar && bar.shell ? bar.shell.serviceFor(root.moduleName) : null
  readonly property bool live: !!(svc && svc.running)
  readonly property int unread: svc ? svc.unread : 0
  readonly property color fg: Color.popups.text
  readonly property color dim: Qt.darker(fg, 1.55)
  readonly property color accent: Color.accent
  // The j's dot shows Joma's status, in the colours of the widget's settings in
  // shell.json: unread messages, offline (or not running), invisible, else online.
  function setting(key, fallback) { return settings && settings[key] ? settings[key] : fallback }
  readonly property color dotColor: !live ? setting("offlineColor", "#f7768e")
    : unread > 0 ? setting("unreadColor", "#39ff88")
    : !svc.registered ? setting("offlineColor", "#f7768e")
    : svc.invisible ? setting("invisibleColor", "#7a7f8a")
    : setting("onlineColor", "#b193f7")
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  property bool popupOpen: false
  property int selected: 0
  readonly property bool opened: popupOpen
  readonly property bool chatting: !!(svc && svc.chatConversation !== "")
  readonly property var chat: svc ? svc.chat : null
  readonly property string chatId: svc ? svc.chatConversation : ""

  function open() {
    popupOpen = true
    selected = 0
    if (svc) svc.refresh()
  }
  function close() {
    popupOpen = false
    if (svc && chatting) svc.closeChat()
  }
  function toggle() { if (popupOpen) close(); else open() }
  function closeForPopoutSwitch() { close() }

  function openChat(i) {
    var t = svc && svc.targets[i]
    if (!t) return
    selected = i
    svc.openChat(t.conversation)
    Qt.callLater(function() { reply.forceActiveFocus() })
  }
  function back() {
    svc.closeChat()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }
  function clock(ts) {
    var d = new Date(ts * 1000)
    var now = new Date()
    var hm = Qt.formatTime(d, "hh:mm")
    return d.toDateString() === now.toDateString() ? hm : Qt.formatDate(d, "dd MMM") + " " + hm
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  // `omarchy-shell io.github.dzanaga.joma-quick toggle` (Super+Alt+M) reaches the
  // service; the widget on the focused monitor's bar answers it.
  readonly property bool onFocusedScreen: {
    var w = root.QsWindow.window
    var m = Hyprland.focusedMonitor
    return !!(w && w.screen && m && m.name === w.screen.name)
  }
  Connections {
    target: root.svc
    function onToggleRequested() { if (root.onFocusedScreen) root.toggle() }
    function onOpenRequested() { if (root.onFocusedScreen) root.open() }
    function onCloseRequested() { root.close() }
  }

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    // The glyph is drawn below; the label is unused.
    text: " "
    labelVisible: false
    fixedWidth: glyph.implicitWidth + scaledHorizontalMargin * 2
    dimmed: !root.live
    tooltipText: !root.live ? "Joma is not running" : (root.unread > 0 ? root.unread + " unread in Joma" : "Joma")
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.LeftButton) root.toggle()
      else if (buttonCode === Qt.RightButton && root.svc) root.svc.openJoma()
    }

    JomaGlyph {
      id: glyph
      anchors.centerIn: parent
      size: button.fontSize * 1.2
      color: button.foreground
      dot: root.dotColor
    }
  }

  KeyboardPanel {
    id: popup
    anchorItem: button
    bar: root.bar
    owner: root
    open: root.popupOpen
    focusTarget: root.chatting ? reply : keyCatcher
    contentWidth: fittedContentWidth(Style.space(400))
    contentHeight: fittedContentHeight(root.chatting ? chatColumn.implicitHeight : listColumn.implicitHeight, Style.space(620))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      blocked: root.chatting
      onCloseRequested: root.close()
      onMoveRequested: function(dx, dy) {
        var n = root.svc ? root.svc.targets.length : 0
        if (n > 0) root.selected = Math.max(0, Math.min(n - 1, root.selected + dy))
      }
      onActivateRequested: root.openChat(root.selected)
      onReturnRequested: root.openChat(root.selected)

      // ---- The contact list ----------------------------------------------------
      Flickable {
        visible: !root.chatting
        anchors.fill: parent
        contentWidth: width
        contentHeight: listColumn.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        interactive: contentHeight > height

        Column {
          id: listColumn
          width: parent.width
          spacing: Style.space(8)

          PanelHero {
            width: parent.width
            title: "Joma"
            meta: !root.live
              ? "not running"
              : (root.svc.account + " · " + (root.svc.registered ? (root.svc.invisible ? "invisible" : "online") : "offline")
                 + (root.unread > 0 ? " · " + root.unread + " unread" : ""))
            foreground: root.fg
            fontFamily: root.fontFamily
          }

          Button {
            visible: !root.live
            text: "Open Joma"
            iconText: "󰭹"
            foreground: root.fg
            fontFamily: root.fontFamily
            onClicked: { root.svc.openJoma(); root.close() }
          }

          Text {
            visible: root.live && root.svc.targets.length === 0
            width: parent.width
            text: "Nobody is online, and nothing is pinned or unread."
            wrapMode: Text.WordWrap
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.bodySmall
          }

          Repeater {
            model: root.live ? root.svc.targets : []
            Rectangle {
              id: row
              required property var modelData
              required property int index
              width: listColumn.width
              height: rowColumn.implicitHeight + Style.space(10)
              radius: Style.cornerRadius
              color: index === root.selected || rowMouse.containsMouse
                ? Qt.rgba(root.fg.r, root.fg.g, root.fg.b, 0.10) : "transparent"

              Text {
                id: presence
                x: Style.space(6)
                anchors.verticalCenter: parent.verticalCenter
                text: "󰝥"
                color: row.modelData.online ? root.accent : Qt.rgba(root.fg.r, root.fg.g, root.fg.b, 0.25)
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
              }

              Column {
                id: rowColumn
                anchors.left: presence.right
                anchors.leftMargin: Style.space(8)
                anchors.right: badge.left
                anchors.rightMargin: Style.space(6)
                anchors.verticalCenter: parent.verticalCenter
                spacing: Style.space(1)
                Text {
                  width: parent.width
                  text: (row.modelData.favorite ? "󰓎 " : "") + row.modelData.title
                  color: row.modelData.online || row.modelData.unread > 0 || row.modelData.favorite ? root.fg : root.dim
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.body
                  font.bold: row.modelData.unread > 0
                  elide: Text.ElideRight
                }
                Text {
                  visible: row.modelData.last !== ""
                  width: parent.width
                  text: row.modelData.last
                  color: root.dim
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.caption
                  elide: Text.ElideRight
                }
              }

              Rectangle {
                id: badge
                visible: row.modelData.unread > 0
                anchors.right: parent.right
                anchors.rightMargin: Style.space(8)
                anchors.verticalCenter: parent.verticalCenter
                width: Math.max(height, badgeText.implicitWidth + Style.space(8))
                height: badgeText.implicitHeight + Style.space(2)
                radius: height / 2
                color: root.accent
                Text {
                  id: badgeText
                  anchors.centerIn: parent
                  text: row.modelData.unread
                  color: Color.popups.background
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.caption
                  font.bold: true
                }
              }

              MouseArea {
                id: rowMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.openChat(row.index)
              }
            }
          }

          PanelSeparator { visible: root.live; width: parent.width; foreground: root.fg }

          Button {
            visible: root.live
            text: "Open Joma"
            iconText: "󰭹"
            foreground: root.fg
            fontFamily: root.fontFamily
            onClicked: { root.svc.openJoma(); root.close() }
          }
        }
      }

      // ---- The quick chat ------------------------------------------------------
      Column {
        id: chatColumn
        visible: root.chatting
        width: parent.width
        spacing: Style.space(8)

        Row {
          width: parent.width
          spacing: Style.space(4)

          PanelActionButton {
            iconText: "󰁍"
            tooltipText: "Back (Esc)"
            foreground: root.fg
            onClicked: root.back()
          }

          Column {
            width: parent.width - Style.space(4) * 6 - Style.space(26) * 6
            anchors.verticalCenter: parent.verticalCenter
            Text {
              width: parent.width
              text: root.chat ? root.chat.title : "…"
              color: root.fg
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              font.bold: true
              elide: Text.ElideRight
            }
            Text {
              width: parent.width
              text: !root.chat ? "" : (root.chat.composing ? "typing…"
                : (root.chat.in_call ? "in a call" : (root.chat.online ? "online" : "offline")))
              color: root.chat && root.chat.online ? root.accent : root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
            }
          }

          PanelActionButton {
            iconText: "󰏲"
            tooltipText: "Call"
            foreground: root.fg
            onClicked: root.svc.call(root.chatId, false)
          }
          PanelActionButton {
            iconText: "󰕧"
            tooltipText: "Video call"
            foreground: root.fg
            onClicked: root.svc.call(root.chatId, true)
          }
          PanelActionButton {
            iconText: "󰍹"
            tooltipText: "Share your screen (no call; Jami users get a call with the microphone off)"
            foreground: root.fg
            onClicked: root.svc.shareScreen(root.chatId)
          }
          PanelActionButton {
            iconText: "󰏢"
            tooltipText: "Send a file"
            foreground: root.fg
            onClicked: root.svc.sendFile(root.chatId)
          }
          PanelActionButton {
            iconText: "󰏌"
            tooltipText: "Open in Joma"
            foreground: root.fg
            onClicked: { root.svc.openIn(root.chatId); root.close() }
          }
        }

        PanelSeparator { width: parent.width; foreground: root.fg }

        ListView {
          id: lines
          width: parent.width
          height: Style.space(300)
          clip: true
          spacing: Style.space(6)
          model: root.chat ? root.chat.messages : []
          boundsBehavior: Flickable.StopAtBounds
          onCountChanged: Qt.callLater(function() { lines.positionViewAtEnd() })
          delegate: Item {
            id: line
            required property var modelData
            width: lines.width
            height: modelData.event ? eventText.implicitHeight : bubble.height

            Text {
              id: eventText
              visible: line.modelData.event
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              text: line.modelData.text + " · " + root.clock(line.modelData.ts)
              color: root.dim
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
            }

            Rectangle {
              id: bubble
              visible: !line.modelData.event
              anchors.right: line.modelData.mine ? parent.right : undefined
              anchors.left: line.modelData.mine ? undefined : parent.left
              width: Math.min(line.width * 0.82, Math.max(body.implicitWidth, stamp.implicitWidth) + Style.space(16))
              height: body.implicitHeight + stamp.implicitHeight + Style.space(10)
              radius: Style.cornerRadius
              color: line.modelData.mine
                ? Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.18)
                : Qt.rgba(root.fg.r, root.fg.g, root.fg.b, 0.08)
              Text {
                id: body
                x: Style.space(8)
                y: Style.space(4)
                width: Math.min(implicitWidth, line.width * 0.82 - Style.space(16))
                text: line.modelData.text
                wrapMode: Text.Wrap
                color: root.fg
                font.family: root.fontFamily
                font.pixelSize: Style.font.bodySmall
              }
              Text {
                id: stamp
                x: Style.space(8)
                anchors.top: body.bottom
                text: (line.modelData.mine ? "" : line.modelData.author + " · ") + root.clock(line.modelData.ts)
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
              }
            }
          }
        }

        TextField {
          id: reply
          width: parent.width
          placeholderText: "Reply, then Enter"
          foreground: root.fg
          onAccepted: {
            if (text.trim() !== "") {
              root.svc.sendText(root.chatId, text)
              text = ""
            }
          }
          Keys.onPressed: function(event) {
            if (event.key === Qt.Key_Escape) {
              root.back()
              event.accepted = true
            }
          }
        }

        Text {
          visible: root.svc && root.svc.lastResult !== ""
          width: parent.width
          text: root.svc ? root.svc.lastResult : ""
          wrapMode: Text.WordWrap
          color: Color.urgent
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
        }
      }
    }
  }
}
