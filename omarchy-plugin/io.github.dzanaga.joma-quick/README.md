# Joma Quick (Omarchy shell plugin)

A bar widget for [Joma](../../README.md): the Joma hexagon in the theme's accent colour
(`JomaGlyph.qml`) with the unread count, and a panel with your contacts and a quick chat
for each (messages, reply, quick messages, call, share, files). Left click or
Super+Alt+M opens the panel, right click opens Joma.

It talks to the running Joma through the `joma` command (`joma quick --json`,
`joma send-quick`), so `joma` must be on your `PATH`. Messages and favourites live in
`~/.config/joma/quick.toml`; star a conversation in Joma (Ctrl+Shift+F) to pin it here.

Install: link this folder into `~/.config/omarchy/plugins/`, then add the widget to the
bar in `~/.config/omarchy/shell.json`.
