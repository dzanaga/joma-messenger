# Interface

Omarchy's look: flat and square, thin borders, the terminal font (Alacritty's family),
the current theme's palette (`colors.toml`, followed live), the accent for focus and
selection, a status line that teaches the keys. Keyboard first; every action has a key.

Layout: a top bar (account, presence, unread), the sidebar on the left (tabs: chats,
contacts), the
conversation on the right with the call panel above the messages. The call panel grows
to fill the window (F11) when it shows video or a shared desktop. Messages read like a
chat log: time, author (one colour per person), text; system lines are quiet.

Sidebar order lives in `quick.toml`: `favorites` (starred chats, in order), `starred`
and `contact_order` (contacts). A drop runs `joma_quick::drop_on`: on a starred row the
item joins the stars there, elsewhere it leaves them; contacts also keep a free order,
chats below the stars follow the latest message. Jami can hold two one-to-one
conversations with one peer (unblocking starts a new one while the old one keeps
working): `Model::conversation_with` picks the one with the latest real message and the
other is hidden.
