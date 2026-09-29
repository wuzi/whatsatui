# WhatsApp TUI v0.1 design

Date: 2026-09-29

Working project name: `whatsapp-tui`

Status: Approved by the user on 2026-09-29.

## Purpose and agreed direction

Build a personal WhatsApp terminal application that is pleasant enough to use every day. The user enjoys spotatui and wants its visual feel in a Rust application, with more comfortable keyboard interaction than their current whatscli setup.

The user chose polished text messaging for personal and group chats, with media support added later. They chose pane-based interaction: Tab moves between panes, j/k or arrows navigate lists, and the composer accepts normal typing. They approved Rust with Ratatui, an initial `whatsapp-rust` backend, persistent local history and drafts, quoted replies, unread counts, delivery status, and a Linux-first release.

The observed spotatui configuration uses the Default (Cyan) theme. Reproduce its palette and focus cues in a layout designed for conversations. Do not depend on spotatui being installed.

Success means the user can pair once, find a conversation, read and reply, switch chats without losing a draft, and restart or recover from a network interruption without losing stored history or silently duplicating a send. Actual backend compatibility is an implementation milestone, not a result established by this document.

## Scope

Version 0.1 supports one account and one running application instance per data directory. It provides QR pairing, session restoration, direct and existing group chats, text composition, quoted replies, chat search, a contact/chat switcher, a local message cache, per-chat drafts, unread indicators, and message status. Selecting a known, synced contact can start a direct conversation even if it has no cached messages.

Incoming images, audio, video, files, and other unsupported content appear as labeled message entries. Preserve a supplied text caption, sender, and timestamp. They must not vanish from the timeline or make a conversation look empty. Outgoing reactions, message editing/deletion, media handling, calls, group administration, multiple accounts, shell-driven message sending, desktop notifications, plugins, and a background daemon are outside this release.

Handle received text edits, deletion notices, and supplied expiry metadata so cached conversations do not knowingly display obsolete text. This is message rendering and retention behavior, not an outgoing editing feature.

`whatsapp-tui` is a local working name. Publishing, release distribution, and final branding are separate work.

## User experience

### Layout and theme

The normal layout has a chat list on the left and a conversation on the right, with the composer beneath the conversation. A compact footer shows connection state and the configured actions for the current focus. Chat rows show name, recent message preview, time, unread count, and a draft indicator. Messages show sender, timestamp, body, optional quoted context, and applicable status.

Use the following semantic defaults, taken from the user's spotatui palette:

| Role | Default |
| --- | --- |
| Background and ordinary text | Terminal defaults |
| Focused border | `#00b4b4` |
| Accent and selected text | `#00c8c8` |
| Inactive borders and secondary labels | `#808080` |
| Hints | `#c8c800` |
| Error text | `#ff6464` |
| Error border | `#c80000` |

Show selection with a marker or emphasis as well as color. Use text labels for status, and require no Nerd Font. Theme values and bindings are configurable in TOML. RGB colors fall back to the corresponding ANSI colors on terminals without truecolor support.

At widths below 80 columns, show either the chat list or the conversation/composer area according to focus; the same navigation actions remain available. Below 40 columns or 12 rows, show a resize notice and retain all application state. The ordinary layout must work at 80 by 24. Resizing must preserve drafts and selection.

### Focus and keyboard contract

There are three focus targets: Chats, Messages, and Composer. There is no separate Vim insert mode. Overlays such as search and help temporarily own input and restore the previous focus when dismissed.

| Input | Context | Action |
| --- | --- | --- |
| Tab / Shift-Tab | Main screen | Cycle Chats, Messages, Composer in either direction |
| j/k or Up/Down | Chats or Messages | Move selection |
| Enter | Chats | Open the selected chat and focus Composer |
| Enter | Composer | Send a nonempty draft if connected |
| Alt-Enter | Composer | Insert a newline |
| Esc | Composer | Focus Messages and preserve the draft |
| Esc | Messages | Focus Chats |
| Esc | Overlay | Close it without applying an unconfirmed selection |
| / | Chats or Messages | Open the searchable chat/contact switcher |
| Ctrl-P | Any main-screen pane | Open the same switcher |
| Enter | Switcher | Open the selected result and focus Composer |
| r | Messages | Quote the selected supported message and focus Composer |
| R | Messages, own Failed or Unconfirmed message | Open a resend confirmation; Enter creates a new attempt, Esc cancels |
| Alt-R | Composer | Remove the reply target, retaining the draft text |
| PageUp / PageDown | Messages | Scroll through cached messages |
| End | Messages | Return to the newest message |
| ? | Chats or Messages | Show help |
| F1 | Any main-screen pane | Show help |
| Ctrl-Q | Anywhere | Save pending local state and exit normally |

When a text field has focus, printable characters such as j, k, /, r, and ? are text. Text cursor movement and editing remain local to that field. Only documented modifier shortcuts and focus actions are intercepted there. A bracketed paste is inserted as one text operation; embedded newlines never trigger sending.

Bindings map keys to semantic actions and are scoped by context. Conflicts within a context and invalid configuration produce an actionable startup error naming the offending setting. Help and the footer are generated from the effective bindings. Essential focus, help, and exit actions must remain reachable after configuration is loaded.

### Reading and composing

Open a conversation at its newest messages. If the user scrolls upward, incoming messages keep their reading position and show a new-message indicator. End returns to the bottom. Switching chats saves and restores each draft and its quoted-message target. Draft persistence uses a 250 ms debounce, with a mandatory flush on chat switch and normal shutdown; the UI must report a failure to save. A crash can lose edits still inside that debounce window. Ignore a whitespace-only submission, and preserve intentional whitespace in other messages.

Searching filters known chats and contacts by case-insensitive name or available phone number. Search covers these cached entries, not message bodies or an unrestricted remote directory. Prefer contact names, then group subjects or supplied display names, with an identifier fallback.

Quoted replies retain the original message identifier, sender, and a bounded text preview. If the original message is missing, deleted, unsupported, or expired, show that state explicitly. Selecting a reply never discards an existing draft.

## Message and connection behavior

### Pairing and lifecycle

On first launch, show the current QR code and instructions for linking a device. Refresh expired codes in place. Successful pairing advances to the chat screen and synchronization. Subsequent starts restore the saved session. Connection state is always visible: connecting, pairing required, connected, reconnecting, or disconnected with a reason. History synchronization has a separate progress indicator. Once the backend reports the session ready, sending is available even while history continues to sync.

Temporary network failures use the library's reconnect lifecycle; do not layer a second competing reconnect loop over it. A revoked session returns to pairing only after the backend has classified it as unusable. A network timeout alone must not erase session state or cached conversations.

### Sending and receipts

The send workflow records an outgoing message durably before transmitting it. Allocate or obtain a stable protocol message identifier before the network operation and retain it for reconciliation. Persist the outgoing record and removal of the submitted draft in one application-store transaction. If persistence fails, retain the draft and do not transmit.

| State | Meaning and display rule |
| --- | --- |
| Sending | A recorded send attempt is in progress |
| Sent | The backend confirms server acceptance |
| Delivered | A recipient delivery receipt has arrived |
| Read | A recipient read receipt has arrived |
| Failed | The backend can establish that the attempt was not accepted |
| Unconfirmed | Transmission may have occurred, but its outcome is unknown |

A timeout, disconnect, or restart during a send is Unconfirmed unless backend evidence establishes another outcome. A persisted Sending record found on startup becomes Unconfirmed while reconciliation runs. Later receipts or matching message events can resolve it. Receipt processing must not downgrade an already established state.

Do not automatically submit or resubmit messages on reconnect or restart. Pressing Enter while disconnected keeps the draft intact and displays that it was not sent. An explicit resend action creates a new attempt; for an Unconfirmed message it must state that the original may already have arrived and require confirmation. Preserve the original record and its identity so a late receipt still has a destination.

For group messages, show the receipts actually known, such as a delivered/read count. Never infer that every participant has read a message from one participant's receipt. Respect unavailable receipt information instead of manufacturing a status.

### Receiving, history, and unread state

Normalize history and live messages through the same storage path. Upsert by stable message identity so a live event, history batch, outgoing echo, or replay cannot create duplicate timeline rows. History replay does not create a fresh unread increment, overwrite newer message content with an older version, or resurrect a deleted or expired body.

Use synced unread information as the initial baseline, then account for newly received, unique live messages. Mark the currently visible conversation read when Messages or Composer has focus at the bottom of the timeline. If terminal focus reporting is supported, also require foreground focus. Otherwise, pane focus is the available signal. Merely selecting a chat in the list does not mark it read. Send read acknowledgements through the backend according to the account's applicable privacy behavior.

History consists of what WhatsApp supplies to this linked device plus messages subsequently observed by this application. Show syncing and cache-empty states accurately; an empty local store does not prove that a remote conversation is empty. Scrollback queries page through the local cache. A complete import of all history on the phone is not a release requirement.

Apply supported incoming text edits and deletion events to the original message. A deletion retains a tombstone with its identity. When expiry metadata is supplied, remove expired message bodies, previews, and quoted copies on startup and during operation. Unsupported payloads have stable placeholder entries and never crash ingestion.

## Architecture

Use one Rust package with focused internal modules. Additional crates, backend plugins, or services are unnecessary for this release.

| Boundary | Responsibility | Dependency rule |
| --- | --- | --- |
| `app` and its domain model | Focus, selection, drafts, application actions, message states, and coordination | Knows application types rather than WhatsApp protobuf types |
| `ui` | Ratatui rendering and conversion of terminal input into actions | Reads application state; performs no network or database I/O while rendering |
| `whatsapp` | Pairing, lifecycle, protocol mapping, contacts, messages, receipts, and the library's device store | Owns all `whatsapp-rust` and protocol-specific types |
| `storage` | Application message cache, outgoing records, drafts, migrations, and paginated queries | Presents application records; hides SQL and database connections |
| `config` | XDG paths, TOML parsing, theme roles, and scoped bindings | Validates settings before starting the interactive client |

Ratatui with Crossterm handles the terminal. Tokio runs the connection and asynchronous work. A single application loop owns UI state; network and storage work return typed events. Blocking database work stays outside the rendering/input path. Restore the terminal on ordinary exits, errors, and panics.

The backend accepts a small command surface: connect, send text with optional quoted context and a stable identifier, mark read, and shut down. Its events describe pairing, connection state, contact/chat updates, message changes, receipt changes, and send outcomes. The UI never consumes raw protocol objects. Channel capacity is bounded, and durable message events are never silently dropped when capacity is reached. Process history in batches so typing stays responsive.

The sending flow is: terminal input becomes an action; the application validates it and requests durable storage; successful storage permits a backend send; subsequent backend events update the stored message and the rendered view. Receiving follows the reverse direction: backend event, normalized application record, durable upsert, then UI update. Recoverable storage failures keep affected events pending and visibly stop progress rather than acknowledging successful local persistence.

A deterministic sample backend powers `--demo` and automated tests. Demo mode cannot connect to WhatsApp or open the real account's data directory. It exercises the same application actions and view as the live backend.

## Storage and identity

Use SQLite for application history and drafts, separately from the protocol library's session/key database. Application schema ownership must not depend on undocumented library tables.

Default Linux locations:

- Configuration: `$XDG_CONFIG_HOME/whatsapp-tui/config.toml`, falling back to `~/.config/whatsapp-tui/config.toml`.
- Application database: `$XDG_DATA_HOME/whatsapp-tui/chat.sqlite3`, falling back to `~/.local/share/whatsapp-tui/chat.sqlite3`.
- Library session database: `session.sqlite3` in that same application data directory.
- Diagnostic logs, when enabled: the application's directory under `$XDG_STATE_HOME`, falling back to `~/.local/state`.

Only absolute XDG values override the fallback locations. Create account-data directories with owner-only access and database files with owner read/write access. The local SQLite cache is not encrypted at rest. Do not log message bodies, QR contents, phone numbers, or session/key material. Render received control characters as inert text rather than raw terminal commands.

The application store contains chats, known contacts, messages, receipt information, outgoing attempts, drafts, and schema version metadata. Message identity includes the account, chat, protocol message ID, and applicable sender/direction information. Let the library resolve WhatsApp's phone-number and linked identifiers; normalize mapped aliases without creating duplicate chats or merging distinct group participants. If a mapping is unavailable, retain the stable protocol identifier until a mapping arrives.

Keep a bounded timeline page in memory and fetch older records on demand. Use schema migrations and transactions for dependent changes. Acquire a per-data-directory instance lock before opening a live session, and explain when another instance owns it. Re-pairing must not silently delete cached conversations or drafts. Stored records are partitioned by account identity, and only the currently paired account's records are displayed. This does not add a multi-account switching interface.

## Backend feasibility milestone

The preferred backend is `whatsapp-rust`; its 0.7 release and current documentation describe the necessary capability families. Pin the dependency used for implementation and commit the resulting lockfile. Public capability claims are evidence for evaluating it, not evidence that our application already works.

Before investing in the full interface, build and exercise a minimal adapter that demonstrates:

1. QR pairing, session persistence, a clean shutdown, and restoration without another scan.
2. Receiving and sending text in a direct chat and an existing test group.
3. Quoted replies, stable message identifiers, and receipt events.
4. Contact/group names, initial history delivery, and the live/history identity needed for deduplication.
5. Reconnection after a network interruption and reconciliation of an interrupted send without an application-level automatic resend.

The live exercise is user-operated against their own test conversations. Automated tests use fixtures and a fake transport; they do not send messages to personal contacts. Record precisely which live checks were run and which need the user's linked account. If the live exercise is waiting on the user, interface and storage work can continue against the sample backend, but live integration remains unverified and release acceptance remains incomplete.

If a required behavior cannot be supported, report the specific limitation and revise this design before substituting a Go/whatsmeow helper. That alternative adds packaging and process-lifecycle work and is not part of the initial implementation.

## Delivery sequence

1. Validate the backend boundary and record the results above.
2. Build the themed interface against deterministic sample conversations, including focus, search, composition, and resizing.
3. Add the application store and connect the live event flow, including drafts, replies, unread state, and send reconciliation.
4. Complete automated checks and the user-operated daily-use acceptance exercise, then document running and configuring the application.

This is the intended order, not a task-level implementation plan. That plan follows review of this specification.

## Verification and acceptance criteria

Tests concentrate on observable failures that would disrupt ordinary conversations:

- Input routing: printable shortcut characters remain text inside the composer and switcher; navigation acts only in the intended pane; bracketed multiline paste never sends.
- Persistence: switching chats and restarting retain saved drafts and quoted targets; a failed database write prevents sending and leaves recoverable text.
- Reconciliation: live/history duplicates yield one message; outgoing echoes reconcile with the pending row; late or repeated receipts cannot regress status; interrupted attempts remain Unconfirmed and are not resent automatically.
- Identity and retention: aliased contact identifiers do not split a known chat; received edits/deletions update the right message; known expiries remove bodies and quoted copies.
- Presentation: Ratatui test-backend checks cover the three panes, pairing, search, help, empty/syncing/error states, narrow terminals, Unicode/emoji, safe control-character rendering, and long wrapped text.
- Lifecycle: demo mode remains isolated; normal exit flushes pending state and restores the terminal; a second live instance cannot own the same data directory.

Release acceptance is a manual workflow on Linux: pair, locate a direct chat and an existing group, send and receive text, reply to a selected message, switch between two saved drafts, scroll while a new message arrives, resize the terminal, restart, and reconnect after an interruption. Status must match available server/recipient evidence. Finish with formatting, lint, and appropriate automated test checks passing. A demo-only pass is not a claim that live WhatsApp integration has been validated.

## Sources consulted

- [spotatui](https://github.com/LargeModGames/spotatui) for the Rust/Ratatui reference application. Exact palette values above were read from the user's installed theme configuration.
- [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust) and its [0.7.0 release](https://github.com/oxidezap/whatsapp-rust/releases/tag/v0.7.0) for the preferred backend and dependency boundary.
- [WhatsApp Rust introduction](https://whatsapp-rust.jlucaso.com/introduction), [authentication](https://whatsapp-rust.jlucaso.com/concepts/authentication), [storage](https://whatsapp-rust.jlucaso.com/concepts/storage), and [receiving messages](https://whatsapp-rust.jlucaso.com/guides/receiving-messages) for the advertised integration capabilities.
- [whatsmeow](https://github.com/tulir/whatsmeow) for the fallback protocol library.
- [whatscli](https://github.com/normen/whatscli) for existing interaction behavior, configuration, and linked-device history limitations.
- [Ratatui application patterns](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/) for separating state updates from rendering.
