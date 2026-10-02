# whatsatui

A personal WhatsApp terminal client in Rust and Ratatui, with pane-based keyboard navigation.

For local validation and the pre-push hook, see [development checks](docs/conventions.md#development-checks).

<img width="1767" height="1123" alt="demo2" src="https://github.com/user-attachments/assets/503bd6d4-bd16-423b-bfb4-fd3718ad7de8" />

## What v0.1 includes

- Direct and existing group text chats; fuzzy switching by name or known phone/identifier, with an unread filter.
- Search cached message text and media captions in a conversation, then jump to a match in history.
- Unicode composition, multiline paste, quoted replies, and persistent per-chat drafts.
- Styled message text and captions: emphasis, code, quotes, and lists.
- Readable business announcements with text, images, button labels, and web links.
- A message action menu, original-text copying, and an explicit web-link picker.
- Inline images, animated stickers, and silently looping GIFs, with Ghostty/Kitty graphics and a half-block fallback (GIFs require FFmpeg).
- Clipboard image paste with captions, persistent drafts, a recent-sticker picker, and searchable Unicode emoji.
- Verified downloads for received images, stickers, audio, videos, and documents, with a separate viewer action.
- Video playback in an mpv window with sound, seeking, and fullscreen controls.
- In-TUI voice-message/audio playback with pause/resume, progress, and 1× / 1.5× / 2× speed (requires mpv).
- Sender blocks with cached profile photos, date separators, and distinct own-message styling.
- Mouse selection, message actions, popup controls, and composer cursor placement.
- Local history, unread counts, delivery states, and known group receipt counts.
- Silent desktop notifications for new messages, with focus-aware suppression and optional private previews.
- Durable outgoing attempts before transmission, with explicit confirmation for resending uncertain attempts.
- Phone-number/LID reconciliation, edits, deletion/expiry placeholders, and bounded message pages.
- Configurable colors and scoped bindings; layouts for ordinary and narrow terminals.

## Main controls

| Key | Action |
| --- | --- |
| Tab / Shift-Tab | Move between Chats, Messages, and Composer |
| j/k or arrows | Navigate lists |
| Enter in Chats | Open the composer |
| i in Chats / Messages | Focus the composer |
| Enter in Composer | Send |
| Shift-Enter / Alt-Enter | Insert a newline |
| Ctrl-C in Composer | Clear the current text |
| J/K in Messages | Scroll rows without changing the selected message |
| Right-click a message | Open its actions |
| Ctrl-V in Composer | Paste a clipboard image, copied image file, or text |
| Ctrl-S in Composer | Choose a recent sticker, or paste an image to create one |
| Ctrl-O in Composer | Attach an image by path |
| Alt-A in Composer | Remove the attached image |
| Ctrl-E in Composer | Search and insert an emoji |
| Esc | Return to the previous pane or close an overlay |
| / in lists, Ctrl-P | Fuzzy chat/contact switcher |
| u in Chats, Ctrl-U in switcher | Open unread chats / toggle All and Unread |
| Ctrl-F | Find messages in the current conversation |
| Enter in message finder | Search, then open the selected match |
| Enter in Messages | Show the selected message's actions |
| y in Messages / link picker | Copy original text / selected URL |
| o in Messages | Choose a link to open in your browser |
| d / v in Messages | Download attachment / open its downloaded file |
| p / Space in Messages | Play or pause the selected audio |
| s / x in Messages | Cycle playback speed / stop audio |
| r in Messages | Quote the selected message |
| R in Messages | Confirm a resend of a failed/unconfirmed attempt |
| Alt-R in Composer | Remove the quote, keep your text |
| PageUp / PageDown, End | Browse messages, return to newest |
| ? in lists, F1 | Show help |
| Ctrl-Q | Save drafts and quit |

## Local configuration and data

Defaults:

- Configuration: `~/.config/whatsapp-tui/config.toml`
- History/drafts: `~/.local/share/whatsapp-tui/chat.sqlite3`
- Linked-device credentials: `~/.local/share/whatsapp-tui/session.sqlite3`
- Downloaded attachments: `~/.local/share/whatsapp-tui/media/`
- Prepared image snapshots: `~/.local/share/whatsapp-tui/outgoing/`
