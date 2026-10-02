# whatsapp-tui

A personal WhatsApp terminal client in Rust and Ratatui, with a cyan interface inspired by spotatui and pane-based keyboard navigation.

**Status: v0.1 candidate.** The offline demo and automated checks run locally. Real WhatsApp pairing, session restoration, and messaging still require user-operated acceptance; see [the validation record](docs/backend-validation.md). The native adapter uses the unofficial `whatsapp-rust` library, pinned to 0.7.0. This project is not affiliated with WhatsApp.

## Try it

Linux, Rust **1.98.0** (selected by `rust-toolchain.toml`), and a C compiler are required. On Debian/Ubuntu install `build-essential` and `pkg-config`. The backend dependency bundles SQLite. Rustup installs the pinned compiler when you run Cargo. No Go sidecar or nightly compiler is needed.

```sh
cargo run --locked -- --demo
```

The demo uses synthetic conversations and a temporary database. It never connects to WhatsApp or reads your live session. Its messages and drafts disappear on exit.

For your account:

```sh
cargo run --locked --release
```

Scan the QR using WhatsApp on your phone: **Settings → Linked devices → Link a device**. Keep the terminal large enough for the entire code. After linking, use the same data directory to restore the session.

```sh
cargo install --locked --path .
whatsapp-tui
```

## What v0.1 includes

- Direct and existing group text chats; fuzzy switching by name or known phone/identifier, with an unread filter.
- Search cached message text and media captions in a conversation, then jump to a match in history.
- Unicode composition, multiline paste, quoted replies, and persistent per-chat drafts.
- Styled message text and captions: emphasis, code, quotes, and lists.
- A message action menu, original-text copying, and an explicit web-link picker.
- Inline images and animated stickers, with Ghostty/Kitty graphics and a half-block fallback.
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

Images and stickers load previews within the conversation; animated WebP plays while visible. Documents retain download/open actions. Sending documents/videos, sticker-pack management, calls, group management, statuses, and newsletters remain outside this iteration. History is limited to what WhatsApp syncs and what this client has cached.

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

Printable keys remain ordinary text in the composer. Bracketed paste never submits a message. Shortcuts live in Help (F1 or the header button) and use your configured bindings. The footer is reserved for status and notices.

Desktop notifications are enabled by default while the app runs. They connect directly to your desktop's notification service, stay silent, and combine rapid arrivals. A shared connection lets GNOME group alerts from the running TUI under **whatsapp-tui**. The conversation you are actively reading does not alert. Set `[notifications] previews = false` to hide names and message text, or `enabled = false` to turn them off. See [notifications](docs/usage.md#notifications) for details.

For voice messages and audio files, install **mpv** (`sudo dnf install mpv` on Fedora, `sudo apt install mpv` on Debian/Ubuntu). Click the audio row or select it and press `p`; the first play downloads and verifies it. Playback controls also appear in the header and remain available while changing chats. See [listening to audio](docs/usage.md#listening-to-audio), including the limitation for old `[audio]` placeholders.

The message finder searches downloaded history, including older cached pages, while offline. Type a literal phrase and press Enter; use arrows and Enter to jump to a match. It shows the newest 50 matches and asks you to refine broader searches. Esc returns to your previous pane with your draft intact. See [finding conversations and messages](docs/usage.md#finding-conversations-and-messages) for details.

## Local configuration and data

Defaults:

- Configuration: `~/.config/whatsapp-tui/config.toml`
- History/drafts: `~/.local/share/whatsapp-tui/chat.sqlite3`
- Linked-device credentials: `~/.local/share/whatsapp-tui/session.sqlite3`
- Downloaded attachments: `~/.local/share/whatsapp-tui/media/`
- Prepared image snapshots: `~/.local/share/whatsapp-tui/outgoing/`

Absolute `XDG_CONFIG_HOME` and `XDG_DATA_HOME` override those bases. `--config PATH` and `--data-dir PATH` select explicit locations. Copy [examples/config.toml](examples/config.toml) to customize the palette and keys.

The cache and credentials are **plaintext local files** in a private directory; database files are mode 0600 and the directory is 0700. One instance may own a data directory at a time. Do not share these files or include them in bug reports. Known disappearing-message deadlines remove bodies and cached quote previews from application records; this is not forensic erasure from SQLite pages, WAL files, backups, or the upstream session store.

See [usage and recovery](docs/usage.md), [design](docs/superpowers/specs/2026-09-29-whatsapp-tui-design.md), and [backend validation](docs/backend-validation.md).

## Next iterations

The [navigation plan](docs/superpowers/plans/2026-09-29-navigation.md) applies ideas from [Concord's fuzzy switcher, search, and unread inbox](https://github.com/chojs23/concord#features) to this app's pane controls. The [message reading and actions plan](docs/superpowers/plans/2026-09-29-message-actions.md) adds formatting, clipboard support, and a link picker. The [received-media plan](docs/superpowers/plans/2026-09-29-received-media.md) adds downloads and external viewers. The [inline media and emoji plan](docs/superpowers/plans/2026-09-29-inline-media.md) adds previews, image sending, and emoji search. Real-account media acceptance and visual checks in your Ghostty session remain the next validation step.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked --release
```

Tests use synthetic fixtures, SQLite, and Linux pseudo-terminals. CI requires no account and never sends WhatsApp messages.
