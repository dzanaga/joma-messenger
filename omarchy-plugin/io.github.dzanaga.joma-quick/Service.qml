import QtQuick
import Quickshell.Io

// Talks to the running Joma through its CLI (which uses Joma's socket): the contact
// list (`joma quick --json`), a quick chat (`joma chat`), sending, calls, desktop share
// and files. Joma itself holds the Jami account.
Item {
  id: root
  visible: false

  readonly property int refreshIntervalMs: 15 * 1000
  readonly property int chatRefreshMs: 2 * 1000
  readonly property string joma: "joma"

  property bool running: false
  property bool registered: false
  property bool invisible: false
  property string account: ""
  property int unread: 0
  property var targets: []
  property var messages: []
  property string lastResult: ""

  // Asked over IPC; the bar widget on the focused monitor acts on it.
  signal toggleRequested()
  signal openRequested()
  signal closeRequested()

  IpcHandler {
    target: "io.github.dzanaga.joma-quick"
    function toggle(): void { root.toggleRequested() }
    function open(): void { root.openRequested() }
    function close(): void { root.closeRequested() }
  }

  // The open quick chat, if any.
  property string chatConversation: ""
  property var chat: null

  function refresh() {
    if (!showProcess.running) showProcess.running = true
  }

  function openChat(conversation) {
    chatConversation = conversation
    chat = null
    lastResult = ""
    refreshChat()
  }

  function closeChat() {
    chatConversation = ""
    chat = null
    refresh()
  }

  function refreshChat() {
    if (chatConversation === "" || chatProcess.running) return
    chatProcess.command = [root.joma, "chat", chatConversation, "30"]
    chatProcess.running = true
  }

  function run(args) {
    actionProcess.command = [root.joma].concat(args)
    actionProcess.running = true
  }

  function sendText(conversation, text) { run(["send", conversation, text]) }
  function sendQuick(conversation, index) { run(["send-quick", conversation, String(index + 1)]) }
  function call(conversation, video) { run(video ? ["call", conversation, "--video"] : ["call", conversation]) }
  function shareScreen(conversation) { run(["share", conversation]) }
  function sendFile(conversation) { run(["send-file", conversation]) }
  function openIn(conversation) { run(["open", conversation]) }

  function openJoma() {
    openProcess.running = true
  }

  Timer {
    interval: root.refreshIntervalMs
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  Timer {
    interval: root.chatRefreshMs
    running: root.chatConversation !== ""
    repeat: true
    onTriggered: root.refreshChat()
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
          root.invisible = !!data.invisible
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
    id: chatProcess
    running: false
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: {
        try {
          var data = JSON.parse(text)
          if (data.conversation === root.chatConversation) root.chat = data
        } catch (e) {
          // Joma quit, or the conversation went away: keep what is shown.
        }
      }
    }
  }

  Process {
    id: actionProcess
    running: false
    stderr: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.lastResult = text.trim()
    }
    onExited: function(code) {
      if (code === 0) root.lastResult = ""
      root.refreshChat()
      root.refresh()
    }
  }

  Process {
    id: openProcess
    running: false
    command: ["sh", "-c", "setsid joma >/dev/null 2>&1 &"]
  }
}
