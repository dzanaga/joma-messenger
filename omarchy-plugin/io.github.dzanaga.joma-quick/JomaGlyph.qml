import QtQuick
import QtQuick.Window

// The Joma logo for the bar: the blocky "j" in the bar's colour and its square dot in
// `dot` (the widget sets it from Joma's status). The shapes are the edges of
// assets/joma-logo-v2.png, as in scripts/icon.py.
Item {
  id: root

  property color color: "white"
  property color dot: "#b193f7"
  // Height.
  property real size: 16

  // The logo's box in source pixels, and its shapes: x0, y0, x1, y1.
  readonly property var box: [377, 297, 877, 1021]
  readonly property var shapes: [
    { r: [701, 297, 877, 460], isDot: true },
    { r: [701, 491, 877, 1021], isDot: false },
    { r: [377, 763, 556, 1021], isDot: false },
    { r: [556, 889, 701, 1021], isDot: false }
  ]
  readonly property real dpr: Screen.devicePixelRatio > 0 ? Screen.devicePixelRatio : 1
  readonly property real scale: size / (box[3] - box[1])

  // Edges on whole screen pixels, so the j's parts meet without seams.
  function snapX(v) { return Math.round((v - box[0]) * scale * dpr) / dpr }
  function snapY(v) { return Math.round((v - box[1]) * scale * dpr) / dpr }

  implicitWidth: snapX(box[2])
  implicitHeight: snapY(box[3])

  Repeater {
    model: root.shapes
    Rectangle {
      required property var modelData
      x: root.snapX(modelData.r[0])
      y: root.snapY(modelData.r[1])
      width: root.snapX(modelData.r[2]) - x
      // At bar sizes the gap under the dot rounds to nothing: keep one pixel.
      height: (modelData.isDot ? Math.min(root.snapY(modelData.r[3]), root.snapY(491) - 1 / root.dpr) : root.snapY(modelData.r[3])) - y
      antialiasing: false
      color: modelData.isDot ? root.dot : root.color
    }
  }
}
