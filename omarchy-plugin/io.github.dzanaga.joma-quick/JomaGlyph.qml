import QtQuick
import QtQuick.Window

// The Joma pixel-art logo for the bar: the outline of "jm" in the bar's colour, and the
// inside of the j's dot filled with `fill` when `filled` (unread messages). The grid
// comes from assets/joma-messenger-pixel.png (scripts/icon.py prints it).
Item {
  id: root

  property color outline: "white"
  property color fill: "#39ff88"
  property bool filled: false
  // Height. Cells snap to screen pixels, so at sizes that are not a whole number of
  // pixels per logo pixel some lines come out a pixel thicker.
  property real size: 16
  // The width as if the logo were this tall: a whole number of screen pixels per logo
  // pixel, so the columns stay even.
  property real widthSize: size

  readonly property var rows: [
    "..###......",
    "..#.#......",
    "..###......",
    "...........",
    "..#########",
    "..#.#.....#",
    "..#.#.#.#.#",
    "###.#.#.#.#",
    "#.#.#.###.#",
    "#.#.#.#.#.#",
    "#...#.#.#.#",
    "#######.###"
  ]
  readonly property int cols: rows[0].length
  readonly property real dpr: Screen.devicePixelRatio > 0 ? Screen.devicePixelRatio : 1
  readonly property real cell: size / rows.length
  readonly property real cellWidth: Math.max(1, Math.round(widthSize * dpr / rows.length)) / dpr

  // The rows of the j's dot: only its inside is filled.
  readonly property int dotRows: 3

  // Every cell: 1 outline, 2 inside (enclosed by the outline), 0 outside.
  readonly property var cells: {
    var h = rows.length, w = cols
    var kind = []
    for (var y = 0; y < h; y++) {
      kind.push([])
      for (var x = 0; x < w; x++) kind[y].push(rows[y][x] === "#" ? 1 : 2)
    }
    // Flood the outside from the border; what stays 2 is enclosed.
    var stack = []
    for (var i = 0; i < w; i++) { stack.push([i, 0]); stack.push([i, h - 1]) }
    for (var j = 0; j < h; j++) { stack.push([0, j]); stack.push([w - 1, j]) }
    while (stack.length > 0) {
      var p = stack.pop()
      var px = p[0], py = p[1]
      if (px < 0 || py < 0 || px >= w || py >= h || kind[py][px] !== 2) continue
      kind[py][px] = 0
      stack.push([px + 1, py]); stack.push([px - 1, py]); stack.push([px, py + 1]); stack.push([px, py - 1])
    }
    var out = []
    for (var yy = 0; yy < h; yy++)
      for (var xx = 0; xx < w; xx++)
        if (kind[yy][xx] === 1 || (kind[yy][xx] === 2 && yy < dotRows)) out.push({ x: xx, y: yy, inside: kind[yy][xx] === 2 })
    return out
  }

  implicitWidth: cols * cellWidth
  implicitHeight: rows.length * cell

  Repeater {
    model: root.cells
    Rectangle {
      required property var modelData
      // Edges on whole screen pixels, so neighbouring cells meet without seams.
      x: Math.round(modelData.x * root.cellWidth * root.dpr) / root.dpr
      y: Math.round(modelData.y * root.cell * root.dpr) / root.dpr
      width: Math.round((modelData.x + 1) * root.cellWidth * root.dpr) / root.dpr - x
      height: Math.round((modelData.y + 1) * root.cell * root.dpr) / root.dpr - y
      antialiasing: false
      visible: !modelData.inside || root.filled
      color: modelData.inside ? root.fill : root.outline
    }
  }
}
