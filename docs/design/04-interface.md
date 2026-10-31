# Interface

Omarchy's look: flat and square, thin borders, the terminal font (Alacritty's family),
the current theme's palette (`colors.toml`, followed live), the accent for focus and
selection, a status line that teaches the keys. Keyboard first; every action has a key.

Layout: a top bar (account, presence, unread), a rail of tabs on the far left (chats,
calls, contacts, devices, accounts, agents; our picture at the bottom; 0.4.3), the
sidebar showing the tab, the
conversation on the right with the call panel above the messages. The call panel grows
to fill the window (F11) when it shows video or a shared desktop. Messages read like a
chat log: time, author (one colour per person), text; system lines are quiet.

Controls (0.4.2): one height for every button; a glyph sits centred in a cell of its
own (Nerd Font glyphs differ in width), icon-only buttons are square; header actions are
grouped with thin separators. People show as avatars: their picture, else their
initial on their colour, with the presence dot in the corner (list rows 32 px, header
34, messages 18 at the start of a run). Pictures in a conversation are sized from the
file's header, so the list never jumps while they load.

Pictures: `avatar.rs` reads `PHOTO` from the vCards the daemon keeps (folded lines
too) and caches it under `~/.cache/joma/avatars` (the name changes with the picture:
GPUI caches by path). Our own is set with `updateProfile(…, path, "PNG", "", 0)`
(flag 2 removes it): a chosen picture is cropped square to 256 px; a drawing is 16×16
cells exported at 8 px per cell, kept as `avatar-<account>.json` to edit again.

Sidebar order lives in `quick.toml`: `favorites` (starred chats, in order), `starred`
and `contact_order` (contacts). A drop runs `joma_quick::drop_on`: on a starred row the
item joins the stars there, elsewhere it leaves them; contacts also keep a free order,
chats below the stars follow the latest message. Jami can hold two one-to-one
conversations with one peer (unblocking starts a new one while the old one keeps
working): `Model::conversation_with` picks the one with the latest real message and the
other is hidden.
