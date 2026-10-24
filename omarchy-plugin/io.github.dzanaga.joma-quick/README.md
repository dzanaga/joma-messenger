# Joma Quick (Omarchy shell plugin)

A bar widget for [Joma](../../README.md): the unread count of your Jami conversations,
and a popup to send a quick message to a favourite or recent contact. Left click opens
the popup, right click opens Joma.

It talks to the running Joma through the `joma` command (`joma quick --json`,
`joma send-quick`), so `joma` must be on your `PATH`. Messages and favourites live in
`~/.config/joma/quick.toml`; star a conversation in Joma (Ctrl+Shift+F) to pin it here.

Install: link this folder into `~/.config/omarchy/plugins/`, then add the widget to the
bar in `~/.config/omarchy/shell.json`.
