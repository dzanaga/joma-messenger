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

## Restyle (D-20, from 0.5.2.dev)

Why the 0.5.1 look fails: icons are Font Awesome glyphs typed as text, so the user's
Nerd Font sets their weight, width and baseline; the border is the background mixed 70 %
toward `muted`, bright grey on themes with a light `muted`; every button is boxed; no
spacing or type scale; no component layer.

Target: a numbered tab strip on top (chats … agents, then settings and your picture,
whose menu holds invisible, username and "to the tray"); the sidebar and the
conversation as tiles with 6 px gaps and 1 px borders, the focused tile in the accent;
4 px corners; messages as bubbles (yours right, on the accent's tint); the header
keeps call, video, share and a menu; one composer field with attach, emoji and record
inside; no top bar, no status line; compact rows (32 px). Tokens: spacing 4 / 8 / 12 /
16, text 11 / 12 / 13 / 15, one radius, border = background mixed 12 % toward
foreground. Icons: Joma's own SVGs in `assets/icons`, 24-unit grid, 1.5 stroke, square
ends, drawn at 16 px and tinted by the theme.

Built so far (0.5.2.dev0): `ui.rs` holds the icons (`Icon`, embedded with `Assets`,
painted in the text colour in effect and sized 1.25 × the text, so hover and "on"
colours reach them) and `button`; the border token.
`render/` has one file per view (`shell`, `sidebar`, `people`, `calls`, `stage_view`,
`chat`, `overlays`); `mod.rs` keeps the shared helpers and the root `render`.
