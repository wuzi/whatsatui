# Usage and recovery

## Starting and linking

Run `whatsapp-tui --demo` to explore offline, or `whatsapp-tui` to start the native WhatsApp adapter. Pair through your phone's Linked devices screen. QR codes expire and refresh; a terminal too small to display a complete code asks you to resize. The regular chat layout works at 80×24; truecolor terminals show the cyan palette, with ANSI fallback elsewhere.

The app uses one account per active session. Histories and drafts are keyed by account, including when you link a different account into an existing data directory. Only conversations and contacts that WhatsApp supplies are available. Search a known contact and open its composer to start a direct chat.

## Reading and composing

Tab changes panes. Select a conversation in Chats, then Enter to type. Enter sends a nonempty draft; Alt-Enter adds a line. Paste, including multiline Unicode text, stays in the composer until you submit it. Drafts save after 250 ms without editing and flush on pane/chat changes and normal quit. A crash can lose edits within that debounce window. If a draft write fails during quit, the app cancels quit, keeps the draft visible, and asks you to fix storage before trying again.

In Messages, use arrows or j/k to select a message; r quotes it without discarding existing composer text. Alt-R removes the quote. Missing, deleted, expired, and unsupported quoted content has an explicit placeholder. Only a bounded preview is retained. If identity reconciliation combines two distinct saved drafts, both texts are kept in one composer, separated by a blank line; the canonical draft's reply target takes precedence. Concurrent local edits are also retained, with a notice to review the combined draft. This conservative merge can leave an older passage alongside its edited version; remove any duplication before sending.

Scroll upward to hold your reading position. Within a long message, arrows/j/k scroll wrapped lines and PageUp/PageDown scroll a screen at a time; its sender and status stay visible. New arrivals show an indicator; End returns to the newest page. Beyond a message, PageUp/PageDown traverse the cached history in pages of at most 100 records. WhatsApp may not provide your complete historical archive.

Messages/Composer at the bottom mark the conversation read; selecting a chat while staying in Chats does not. Terminal foreground focus is also required when focus reporting is available. Read acknowledgements use the backend's account behavior. Synced unread information initializes the local baseline; it is not an ongoing mirror of every other device's unread counter.

## Finding conversations and messages

Ctrl-P opens the chat switcher from any pane; `/` also opens it from Chats or Messages. Type parts of a name, phone number, or known identifier: `asm` can find Alice Smith, and `smith ali` also matches. Exact and contiguous matches rank before scattered characters. Empty queries retain the recent-chat order. Phone queries can omit spaces, parentheses, and dashes.

Press `u` in Chats for unread conversations, or Ctrl-U inside the switcher to toggle All/Unread without clearing your query. Each result shows its direct/group type, unread count, and any draft. Arrows select, Enter opens the composer, and Esc returns to the pane you came from. Background updates keep the highlighted conversation selected when its position changes.

Ctrl-F opens a message finder for the current conversation. Type a phrase and press Enter to search, then use arrows and Enter to open a match in Messages. End returns to the latest page. Search is local and works offline, including messages outside the currently loaded page. Only history downloaded to this client can appear.

Message queries are literal: `%`, `_`, quotes, and backslashes have no special meaning. Unicode lowercase comparison makes `CAFÉ` match `café`; `cafe` does not match `café`. There is no accent normalization or stemming. Text and media captions are included; deleted/expired bodies, quoted previews, and unsent drafts are excluded. The newest 50 results are shown; refine your phrase when the count says `50+`.

Search runs only when submitted, with one scan at a time for the open finder. Editing clears old results. If the conversation changes while the finder is open, submit again for fresh results. Esc closes the finder and keeps your timeline and draft; opening an older result also preserves the draft and does not mark newer messages read. Both search inputs use a single line of at most 256 Unicode characters. Their controls can be changed in `[bindings.search]` and `[bindings.message_search]`.

In the demo, use Ctrl-P and `alc` to open Alice, then Ctrl-F and `cyan` to find a message. Return to Chats and press `u` to try the unread filter.

## Connection and send state

An outgoing attempt receives a stable ID and is committed locally before transmission. Text typed while an earlier revision is being sent remains in the composer. The states shown are:

| State | Meaning |
| --- | --- |
| Sending | Locally committed; backend completion is pending |
| Sent | Server acceptance reported |
| Delivered / Read | Recipient receipt reported |
| Failed | A definite rejection or failure before acceptance |
| Unconfirmed | Acceptance could not be determined |

Group receipts show known recipient counts, not a claim that everyone has read the message. A disconnected send keeps the draft. Restart and reconnect never automatically resend an attempt. R on a Failed or Unconfirmed message opens a confirmation; resending creates a new ID, so an unconfirmed original may also arrive. Late receipts still reconcile against the original ID.

A successful socket write stays Sending until positive server acknowledgement or receipt evidence arrives. Attempts without that evidence become Unconfirmed after 30 seconds. A negative acknowledgement remains Failed even if local transport completion arrives later.

## Configuration

Run `whatsapp-tui --help` for command-line options. Start from [the complete example](../examples/config.toml), remove entries you want to keep at default, and edit the rest. Theme values accept `default` or `#RRGGBB`. Binding values are lists, for example:

```toml
[bindings.chats]
next = ["down", "j"]
previous = ["up", "k"]

[theme]
focus = "#00b4b4"
```

Bindings are scoped to each pane or overlay. Duplicate keys, missing essential actions, and printable composer/search shortcuts are rejected at startup before raw terminal mode. An invalid file reports a configuration error. Demo uses built-in defaults unless you explicitly pass `--config`; it rejects `--data-dir`.

## Storage and session recovery

Use an absolute `--data-dir` to keep separate installations independent. The instance lock protects each directory. Close the other instance if you see the ownership error. Back up the whole data directory only with the app stopped, including any SQLite sidecars. App history and protocol credentials are separate databases, both private plaintext files.

For disk or permissions errors, free space or restore access to your private directory, then retry saving. If you need to terminate an app whose draft cannot be saved, copy the text elsewhere first. After a crash, a previously Sending attempt becomes Unconfirmed and remains available for explicit review.

For a revoked linked session, first quit and check Linked devices on your phone. To re-pair while keeping local chat history, **with every instance stopped**, move `session.sqlite3` and any `session.sqlite3-wal` / `session.sqlite3-shm` sidecars into a private backup directory. Leave `chat.sqlite3` and its sidecars in place, then restart and scan a new QR. Never move an active database. Alternatively, use a fresh `--data-dir`; the old cache remains in the original directory.

Ctrl-Q, Ctrl-C, and SIGTERM request a graceful shutdown. Terminal state is restored on normal exit, input errors, and a Rust panic. SIGKILL and machine failure cannot run cleanup; use your shell's `reset` command if the terminal was left in a bad state.

## Live acceptance

Use your own test conversations. Run through pairing and restoration, direct/group text in both directions, a quoted reply, drafts in two chats, scrolling during new arrivals, narrow-terminal resizing, restart, and a network interruption. Record actual observations in [backend-validation.md](backend-validation.md). These checks require a user-linked account and are not performed by the automated suite.
