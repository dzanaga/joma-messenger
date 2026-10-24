import QtQuick
import Quickshell.Io

// Polls `joma quick --json` (the running Joma answers over its socket) and sends
// quick messages with `joma send-quick`. Joma itself holds the Jami account.
Item {
  id: root
  visible: false

  readonly property int refreshIntervalMs: 15 * 1000
  readonly property string joma: "joma"

  property bool running: false
  property bool registered: false
  property string account: ""
  property int unread: 0
  property var targets: []
  property var messages: []
  property string lastResult: ""

  function refresh() {
    if (!showProcess.running) showProcess.running = true
  }

  function sendQuick(conversation, index) {
    sendProcess.command = [root.joma, "send-quick", conversation, String(index + 1)]
    sendProcess.running = true
  }

  function sendText(conversation, text) {
    sendProcess.command = [root.joma, "send", conversation, text]
    sendProcess.running = true
  }

  function openJoma() {
    openProcess.running = true
  }

  function openPopup() {
    popupProcess.running = true
  }

  Timer {
    interval: root.refreshIntervalMs
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  Process {
    id: showProcess
    running: false
    command: [root.joma, "quick", "--json"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        try {
          var data = JSON.parse(text)
          root.running = data.running !== false
          root.registered = !!data.registered
          root.account = data.account || ""
          root.unread = data.unread || 0
          root.targets = data.targets || []
          root.messages = data.messages || []
        } catch (e) {
          root.running = false
          root.targets = []
        }
      }
    }
  }

  Process {
    id: sendProcess
    running: false
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.lastResult = text.trim()
    }
    onExited: function(code) {
      if (code === 0) root.lastResult = "Sent"
      root.refresh()
    }
  }

  Process {
    id: openProcess
    running: false
    command: ["sh", "-c", "setsid joma >/dev/null 2>&1 &"]
  }

  Process {
    id: popupProcess
    running: false
    command: [root.joma, "quick"]
  }
}
