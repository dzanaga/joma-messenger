import QtQuick

// The Joma icon for the bar: the hexagon and the J of assets/icon/joma.svg as neon tubes,
// in the theme's accent colour, without the dark tile.
Canvas {
  id: root

  property color neon: "#2bea7a"
  // A pale core, as in the app icon, tinted towards the neon colour.
  readonly property color core: Qt.tint(neon, Qt.rgba(1, 1, 1, 0.7))

  implicitWidth: 18
  implicitHeight: 18
  // Room for the glow around the tubes.
  readonly property real pad: 2

  onNeonChanged: requestPaint()
  onWidthChanged: requestPaint()
  onHeightChanged: requestPaint()

  function shape(ctx) {
    // The SVG's geometry, inside 512x512, trimmed to the hexagon's bounds (84..440).
    ctx.beginPath()
    var hex = [[256, 84], [410.2, 173], [410.2, 351], [256, 440], [101.8, 351], [101.8, 173]]
    ctx.moveTo(hex[0][0], hex[0][1])
    for (var i = 1; i < hex.length; i++) ctx.lineTo(hex[i][0], hex[i][1])
    ctx.closePath()
    ctx.moveTo(200, 176)
    ctx.lineTo(312, 176)
    ctx.moveTo(292, 176)
    ctx.lineTo(292, 304)
    ctx.bezierCurveTo(292, 352, 262, 372, 232, 372)
    ctx.bezierCurveTo(204, 372, 184, 358, 176, 330)
  }

  onPaint: {
    var ctx = getContext("2d")
    ctx.reset()
    var side = Math.min(width, height) - pad * 2
    if (side <= 0) return
    var s = side / 356
    ctx.translate((width - side) / 2, (height - side) / 2)
    ctx.scale(s, s)
    ctx.translate(-256 + 178, -84)
    ctx.lineCap = "round"
    ctx.lineJoin = "round"
    // Glow, then the tube, then its bright core; widths in SVG units, kept visible when small.
    ctx.shadowColor = root.neon
    ctx.shadowBlur = 3
    ctx.strokeStyle = root.neon
    ctx.lineWidth = Math.max(22, 1.2 / s)
    shape(ctx)
    ctx.stroke()
    ctx.shadowBlur = 0
    ctx.strokeStyle = root.core
    ctx.lineWidth = Math.max(7, 0.45 / s)
    shape(ctx)
    ctx.stroke()
  }
}
