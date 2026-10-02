# WhatsApp TUI v0.1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the approved Linux WhatsApp terminal client with spotatui-inspired colors, comfortable pane navigation, and reliable personal/group text messaging.

**Architecture:** One Rust package separates application state, Ratatui rendering, SQLite persistence, configuration, and a WhatsApp adapter. A single UI loop consumes application events while backend and storage workers run independently. Persist inbound messages before acknowledgement and outgoing attempts before transmission; the live backend and deterministic demo exercise the same application workflow.

**Tech Stack:** Rust 2024, stable Rust 1.98.0 for development, Ratatui, Crossterm, Tokio, `whatsapp-rust = "=0.7.0"`, Diesel/SQLite, Serde/TOML, Clap, `unicode-segmentation`, `unicode-width`, `qrcode`, and `thiserror`. Use `tempfile` and Tokio's test clock for tests. Resolve compatible stable releases for other dependencies in Task 1 and commit `Cargo.lock`.

**Spec:** [Approved design](../specs/2026-09-29-whatsapp-tui-design.md). Approved for Native execution by the user on 2026-09-29.

## Global Constraints

The following requirements apply to every task; quoted sentences are copied from the spec.

- "Version 0.1 supports one account and one running application instance per data directory."
- "Use one Rust package with focused internal modules."
- "There are three focus targets: Chats, Messages, and Composer."
- "The UI never consumes raw protocol objects."
- "Draft persistence uses a 250 ms debounce, with a mandatory flush on chat switch and normal shutdown; the UI must report a failure to save."
- "Do not automatically submit or resubmit messages on reconnect or restart."
- "Demo mode cannot connect to WhatsApp or open the real account's data directory."
- "Only absolute XDG values override the fallback locations."
- "Do not log message bodies, QR contents, phone numbers, or session/key material."
- "The live exercise is user-operated against their own test conversations."
- Use the spec's exact palette, keyboard table, 80-column layout breakpoint, and minimum 40-column/12-row viewport. Preserve terminal-default foreground/background and provide ANSI color fallbacks.
- Media remains labeled inbound content; outgoing media, calls, group administration, automation, plugins, and a background service remain outside v0.1.

## Review Focus

These implied failure cases receive explicit tests in the tasks listed:

1. A draft changes while its earlier version is being committed for sending: the new text survives, and repeated Enter does not send the earlier version twice (Tasks 2 and 8).
2. A history burst fills event queues, or shutdown starts while a producer is waiting: committed messages remain available, and producers terminate without deadlock (Tasks 3 and 10).
3. A draft save completes out of order, or re-pairing changes the account: older writes cannot replace newer drafts or expose another account's content (Tasks 2 and 8).
4. A group reuses an ID from another sender, or a receipt/edit precedes the corresponding message: changes reach the correct identity and are retained until they can be applied (Tasks 2 and 9).
5. Combined emoji, combining marks, wide characters, pasted escape sequences, and an expired QR in a small terminal: editing stays valid, text stays inert, and clipped/expired pairing codes cannot be mistaken for usable ones (Tasks 5 and 6).

---

## Repository and dependency decisions

The repository currently contains only the approved design. At execution time use `superpowers:using-git-worktrees` to establish an isolated development branch; the current planning work does not create a product scaffold.

The pinned upstream source was inspected while planning:

- Its compiler floor is Rust 1.94. Set this application's `rust-version = "1.98"` and pin development/CI to 1.98.0, which is installed locally; do not claim support for an older application toolchain without testing it.
- Disable `whatsapp-rust` default features: its default `simd` feature uses nightly `portable_simd`. Enable only `sqlite-storage`, `tokio-transport`, `tokio-runtime`, `ureq-client`, `tokio-native`, and `signal`.
- Use Diesel 2.3 for the application database, matching the backend's existing SQL driver. The backend's bundled SQLite uses `libsqlite3-sys` 0.37. Keep one SQLite native-link dependency; verify with `cargo tree -i libsqlite3-sys`.
- `Client::generate_message_id`, `send_message_with_options`, `SendOptions::with_message_id`, `mark_as_read`, `BotBuilder::with_event_handler`, and `with_inbound_durability_hook` exist in the inspected release.
- The bot's default callback delivery can accumulate tasks; its ordered callback delivery drops events on overflow. Neither is a lossless bounded ingestion path. Use the durability hook for eligible inbound messages and a custom bounded event bridge for other subscribed events.
- Current online docs include APIs added after this tag. Compile against the pinned release and its downloaded source, not a guessed signature from `latest` documentation.

Use a small application-owned editor with grapheme-aware cursor movement. Its required operations are insertion, paste, newline, backspace/delete, arrows, Home, and End. This avoids tying the renderer to a second widget library's Ratatui version and keeps keyboard policy under application control.

## File map

| Files | Responsibility |
| --- | --- |
| `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.gitignore` | Reproducible native build and local artifact exclusions |
| `src/lib.rs`, `src/main.rs` | Library exports and CLI entry point |
| `src/app/{mod,model,editor,input,update,view_model}.rs` | Application records, editable text, input actions, state transitions, render-ready view |
| `src/whatsapp/{mod,encode,normalize,bridge,durability,demo}.rs` | Native client boundary, protobuf translation, bounded event delivery, durable receipt of messages, sample backend |
| `src/storage/{mod,worker,records,merge,paths}.rs` | Async store facade, blocking SQL owner, row conversion, reconciliation, secure directory lock |
| `migrations/00000000000001_initial/{up,down}.sql` | Account-partitioned application schema |
| `src/config/{mod,bindings,theme}.rs` | XDG resolution, TOML configuration, context-specific controls, palette |
| `src/ui/{mod,layout,chat_list,timeline,composer,overlays}.rs` | Ratatui widgets and view composition |
| `src/{runtime,terminal}.rs` | Service/effect execution, input/timers, terminal cleanup |
| `examples/backend_probe.rs` | Explicitly user-operated native integration exercise |
| `tests/{support/mod,store,configuration,interaction,rendering,runtime,live_flow,reconciliation,terminal_process}.rs` | Reusable synthetic fixtures and behavior checks |
| `docs/{backend-validation,usage}.md`, `examples/config.toml`, `README.md` | Validation evidence and user instructions |
| `.github/workflows/ci.yml` | Offline checks for the application on Linux |

Create files when their owning task needs them, rather than generating empty modules in advance. Add internal unit tests beside private adapter functions; integration tests use the public application types.

## Shared contracts

Task 1 defines these types in `app::model`; all subsequent interfaces use the same names.

Monotonic timer parameters use `tokio::time::Instant`, so debounce and QR-expiry tests can use Tokio's paused clock. Persisted timestamps use the epoch-millisecond fields below.

- `AccountId`, `ChatId`, `ParticipantId`, and `MessageId`: distinct string newtypes. `RequestId` is a monotonically increasing `u64` newtype local to an application run.
- `MessageKey { account: AccountId, chat: ChatId, sender: ParticipantId, id: MessageId, from_me: bool }`.
- `Quote { key: MessageKey, preview: String, availability: QuoteAvailability }`; availability is `Available`, `Missing`, `Unsupported`, `Deleted`, or `Expired`. Bound the preview to 160 graphemes.
- `Draft { text: String, reply: Option<Quote>, revision: u64 }`; revision increases for text and reply-target changes.
- `OutboundText { key: MessageKey, draft: Draft, created_at_ms: i64 }`.
- `SendState`: `Sending`, `Sent`, `Delivered`, `Read`, `Failed`, `Unconfirmed`.
- `ConnectionState`: `Connecting`, `PairingRequired`, `Connected`, `Reconnecting`, `Disconnected`; keep a separate sanitized display reason and separate history progress.
- `MessageBody`: `Text(String)`, `Unsupported { kind: String, caption: Option<String> }`, `Deleted`, or `Expired`.
- `MessageRecord { key, body, quote, created_at_ms, edited_at_ms, expires_at_ms, send_state }`, with optional values for quote, edit/expiry timestamps, and send state. Timestamps are UTC epoch milliseconds; format in local time only at the view boundary.
- `MessageChange`: `Upsert(MessageRecord)`, `Edit { key, text, edited_at_ms }`, `Delete { key }`, or `Expire { key }`.
- `MessageBatch { account: AccountId, source: MessageSource, changes: Vec<MessageChange> }`; source is `Live` or `History`. Offline messages received as new messages remain `Live`; an initial history replay is `History`.
- `Receipt { key: MessageKey, recipient: ParticipantId, state: ReceiptState, at_ms: i64 }`; receipt state is `Delivered` or `Read`.
- `ChatSummary`: account/chat identity, display name, optional phone number, group flag, latest preview/time, unread count, and draft flag. `ChatSnapshot` contains one summary, a message page, and its draft. `PageCursor` is the stable `(created_at_ms, MessageKey)` ordering cursor.
- `StoreChange { account: AccountId, chats: Vec<ChatId> }` is an invalidation notice; durable data is queried from the store. A coalesced notice must retain every affected chat.

`whatsapp::BackendHandle` has public fields `commands: tokio::sync::mpsc::Sender<BackendCommand>`, `events: tokio::sync::mpsc::Receiver<BackendEvent>`, and `control: BackendControl`. The runner can move them apart and continue draining events while awaiting `BackendControl::shutdown`. Commands are `PrepareText { request, chat, draft }`, `Transmit(OutboundText)`, and `MarkRead(Vec<MessageKey>)`; shutdown uses the separate control handle. Preparing a send allocates identity but never transmits. Events are `AccountKnown(AccountId)`, `ConnectionChanged`, `PairingQr { content, expires_at }`, `HistoryProgress`, `StoreChanged(StoreChange)`, `Prepared { request, message: OutboundText }`, `PreparationFailed { request, reason }`, `SendOutcome { key, state }`, and `Stopped`. Payloads omitted from the short names are respectively the connection state/reason and optional progress percentage.

Backend delivery into the application uses a bounded channel of 256 events; commands use 32. Chunk history writes into at most 100 message changes and local timeline reads into pages of 100. App-level requests carry the originating account/chat/request identity so stale completions can be ignored safely.

### Task 1: Compile the native dependency and define text-message translation

**Files:** Create build files, `src/lib.rs`, `src/app/{mod,model}.rs`, `src/whatsapp/{mod,encode}.rs`, and adapter unit tests. Create `tests/support/mod.rs` when shared fixtures first become necessary in Task 2.

**Interfaces:** Consumes the shared contracts above. Produces `encode::encode_text(&OutboundText) -> Result<EncodedText, BackendError>` and `encode::classify_send_error(&whatsapp_rust::SendError) -> SendState`. Private `EncodedText` contains the release's `Jid`, `wa::Message`, and `SendOptions`. Define `BackendError` with typed local causes and a sanitized user-facing category; do not derive an unrestricted sensitive-data dump for public diagnostics.

- [ ] **Step 1 — Add the minimal package and failing adapter tests.** Configure the dependency features above, Tokio's multithread runtime, and the lockfile. In `whatsapp::encode::tests`, construct synthetic direct/group messages; add `preserves_outbound_id_and_quote` and `only_definite_rejections_are_failed` with these assertions:

```rust
assert_eq!(encoded.options.message_id.as_deref(), Some("3EB0TEST0001"));
assert_eq!(encoded.message.text_content(), Some("  hello\nworld  "));
assert_eq!(quote_context.stanza_id.as_deref(), Some("original"));
assert_eq!(classify_send_error(&SendError::NotLoggedIn), SendState::Failed);
assert_eq!(classify_send_error(&SendError::InvalidRequest("bad".into())), SendState::Failed);
```

Also assert an opaque/internal error and a transport timeout are `Unconfirmed`, and a quote's participant is the original group sender, not the group ID. The fixture helpers live in this unit-test module and return the declared `OutboundText`/upstream error types.
- [ ] **Step 2 — Run the new tests before implementation.** `cargo test --lib whatsapp::encode` must fail on missing encoding/classification behavior. Dependency resolution or a missing compiler is setup failure, not a valid red test.
- [ ] **Step 3 — Implement the model and two functions.** Use the release's protobuf helpers for text/quoted context and the caller-supplied message-ID option. Match only proven pre-transmission failures as `Failed`; unknown variants remain `Unconfirmed`.
- [ ] **Step 4 — Verify the deliverable.** `cargo test --lib whatsapp::encode` and `cargo check --all-targets` exit 0; `cargo tree -i libsqlite3-sys` shows one native SQLite package. Confirm no nightly feature was enabled.
- [ ] **Step 5 — Commit.** `git commit -m "feat: define chat model and native text adapter"` after staging only this task's files and lockfile.

### Task 2: Build durable, account-scoped local storage

**Files:** Create `src/storage/{mod,worker,records,paths}.rs`, the initial migration, `tests/store.rs`, and `tests/support/mod.rs`; modify `src/lib.rs`.

**Interfaces:** Consumes Task 1 records. Produces `DataDirGuard::acquire(&Path) -> Result<DataDirGuard, StoreError>` and a cloneable `Store`. Its async methods are `open(PathBuf) -> Result<Store, StoreError>`, `apply_batch(MessageBatch) -> Result<StoreChange, StoreError>`, `save_draft(AccountId, ChatId, Draft) -> Result<(), StoreError>`, `stage_outgoing(OutboundText) -> Result<(), StoreError>`, `snapshot(AccountId, ChatId, Option<PageCursor>) -> Result<ChatSnapshot, StoreError>`, `list_chats(AccountId) -> Result<Vec<ChatSummary>, StoreError>`, `recover_sends(AccountId) -> Result<(), StoreError>`, and `flush() -> Result<(), StoreError>`. Also define `set_send_state(MessageKey, SendState)`, `record_receipt(Receipt)`, and `upsert_chats(AccountId, Vec<ChatSummary>)`, each async and returning `Result<StoreChange, StoreError>`, so Task 3 can persist native outcomes, receipts, and names.

- [ ] **Step 1 — Write `tests/store.rs` cases using temporary directories.** Define shared helpers `account(&str)`, `key(chat, sender, id)`, `draft(text, revision)`, and `outbound(key, draft)` in `tests/support/mod.rs`, with a fixed synthetic account/time. Test `reopen_keeps_history_and_draft`, `outgoing_commit_is_atomic`, `newer_draft_survives_older_send`, `stale_draft_write_is_ignored`, `message_keys_include_sender`, `accounts_are_isolated`, and `second_instance_is_rejected`. Pin the important outcomes:

```rust
assert_eq!(snapshot.draft.text, "new words"); // rev 2 saved before rev 1 send commits
assert_eq!(snapshot.messages.len(), 2);      // same stanza ID, different senders
assert!(other_account.messages.is_empty());
assert_eq!(reopened.messages[0].send_state, Some(SendState::Unconfirmed));
```

Use a transaction failure injected before commit to assert the old draft remains and no outgoing row exists. On Unix, verify directory mode `0o700`, database mode `0o600`, and that an occupied instance lock returns a typed error.
- [ ] **Step 2 — Run `cargo test --test store`.** The tests must fail against missing persistence/transaction behavior.
- [ ] **Step 3 — Implement the store facade and one blocking database worker.** Keep a bounded request queue; Diesel connections and migrations stay on the worker, outside the UI thread. Use composite identity keys, account filters, unique per-recipient receipts, and a table for not-yet-applicable edits/deletes/receipts. `stage_outgoing` inserts the attempt and clears the draft only if its revision still equals the submitted revision. Clearing advances the stored revision so a delayed save cannot resurrect submitted text; newer drafts remain. Add basic receipt/outcome persistence and chat-name upserts now; Task 9 completes reconciliation rules. Startup recovery changes only `Sending` to `Unconfirmed`; it emits no network work. Initialize database/WAL sidecars inside the private directory and hold the instance lock for the live session lifetime.
- [ ] **Step 4 — Run `cargo test --test store`.** All cases pass after close/reopen as well as in one connection.
- [ ] **Step 5 — Commit.** `git commit -m "feat: persist chats drafts and outgoing attempts"`.

### Task 3: Connect the real backend with bounded, durable ingestion

**Files:** Create `src/whatsapp/{normalize,bridge,durability}.rs`, `examples/backend_probe.rs`, `docs/backend-validation.md`; extend `src/whatsapp/mod.rs` and its internal tests.

**Interfaces:** Consumes Tasks 1–2. Produces `whatsapp::start(session_path: PathBuf, store: Store) -> Result<BackendHandle, BackendError>` and `BackendControl::shutdown(self) -> Result<(), BackendError>` as async functions. `normalize::message_batch(AccountId, MessageSource, &[InboundMessage]) -> MessageBatch` remains private to the adapter. The hook implements the pinned release's `InboundDurabilityHook`; the bridge implements its synchronous `EventHandler`.

- [ ] **Step 1 — Add adapter tests `durability_failure_returns_error`, `replayed_batch_is_idempotent`, `bridge_backpressures_without_dropping`, `shutdown_unblocks_full_bridge`, and `normalizes_group_quote_and_media_caption`.** Fill a capacity-one bridge with two synthetic events while the consumer is paused, then resume it and assert delivery order/count. A fake failing store must cause the hook to return an error. Replaying a committed batch must produce one stored row per full identity. Preserve caption, sender, quote key, and expiry metadata. Use a Tokio multithread test runtime for the synchronous event bridge.

```rust
assert!(failed_commit.is_err());
assert_eq!(delivered_ids, vec!["first", "second"]);
assert_eq!(stored_rows.len(), 1);
assert!(shutdown_result.is_ok());
```

- [ ] **Step 2 — Run `cargo test --lib whatsapp`.** Confirm failures identify the new mapping/durability/queue behavior.
- [ ] **Step 3 — Implement the native session and event bridge.** Build `Bot` with the SQLite device store, durability hook, and a custom raw handler. Use a bounded `std::sync::mpsc::sync_channel` for raw events; wrap a blocking producer send in `tokio::task::block_in_place` under the multithread runtime. A dedicated consumer forwards events into async normalization/storage; it must never synchronously call back into the client while holding a queue/storage lock. Subscribe only to relevant events. Continue draining the bridge and application events until client shutdown completes, then close producers and finish draining. An error/cancellation closes the receiving side to release blocked producers. The durability hook only normalizes, awaits the store transaction, and returns; it never sends a reply. Post-commit `Messages` events are idempotent replays but must still invalidate their chats so hook-persisted messages become visible. History and event-only recoveries pass through the same normalizer/store path. Coalescing invalidations must retain every affected chat.
- [ ] **Step 4 — Implement command execution and the probe.** Use the client's ID generator for `PrepareText`; use `encode_text` for `Transmit`; group read acknowledgements by chat and sender for `mark_as_read`. Emit the restored account identity before loading its cached UI state. Rely on the library lifecycle for reconnects; keep session revocation distinct from temporary disconnection. The probe takes an explicit test-data path and accepts interactive `send`, `reply`, and `quit` commands from the user; it never responds automatically to incoming messages or sends on startup. Create `docs/backend-validation.md` with the exact invocation, pinned dependency, and each live check initially marked **Not run**.
- [ ] **Step 5 — Verify and record evidence.** `cargo test --lib whatsapp` and `cargo check --example backend_probe` exit 0. The user can run `cargo run --example backend_probe -- --data-dir /tmp/whatsapp-tui-probe` to exercise the spec's five backend checks. Record observed results without message contents. If user linking is pending, continue with demo work and keep live checks explicitly unverified. Document the upstream hook's event-only recovery and disk-full limitations rather than promising exactly-once delivery.
- [ ] **Step 6 — Commit.** `git commit -m "feat: connect WhatsApp with durable event ingestion"`.

### Task 4: Define configurable theme, paths, and scoped controls

**Files:** Create `src/config/{mod,bindings,theme}.rs`, `examples/config.toml`, `tests/configuration.rs`; extend `src/lib.rs`.

**Interfaces:** Produces `Config::parse(&str) -> Result<Config, ConfigError>`, `Config::load(&Path) -> Result<Config, ConfigError>`, and `Paths::resolve(home: &Path, xdg: &XdgDirs) -> Paths`. `Config` holds `Theme` and `Bindings`. `Bindings::lookup(Context, KeyEvent) -> Option<ActionId>` and `Bindings::help(Context) -> Vec<(String, ActionId)>` share one effective map. Context is Chats, Messages, Composer, Search, Help, Resend, or Global. Define `ActionId` for every semantic action in the spec's keyboard table; editor character input is not an action binding.

- [ ] **Step 1 — Write `defaults_match_spec`, `relative_xdg_uses_home`, `duplicate_context_binding_is_rejected`, and `required_actions_remain_reachable`.** Assert the palette's exact RGB values, the complete default keyboard table, absolute XDG overrides, relative/empty fallback, and config errors naming the offending key. Test that a global binding cannot shadow another required action in a child context. Unknown action names and unknown configuration fields are errors. A missing config file uses defaults.

```rust
assert!(Config::parse(duplicate_binding_toml).is_err());
assert!(Config::parse(unknown_action_toml).is_err());
assert_eq!(resolved_config_path, home.join(".config/whatsapp-tui/config.toml"));
assert_eq!(focused_border, ratatui::style::Color::Rgb(0, 180, 180));
```

- [ ] **Step 2 — Run `cargo test --test configuration`; confirm the missing validation/default behavior fails.**
- [ ] **Step 3 — Implement config loading and theme conversion.** Resolve XDG variables from supplied values, so tests do not mutate process-global environment. Use `Theme::color(role: ThemeRole, truecolor: bool) -> ratatui::style::Color`; provide explicit Cyan/Gray/Yellow/Red ANSI alternatives and `Reset` for terminal defaults. The checked-in example contains the full effective binding map with short usage comments.
- [ ] **Step 4 — Run `cargo test --test configuration`; all defaults and rejection cases pass.**
- [ ] **Step 5 — Commit.** `git commit -m "feat: configure theme paths and pane bindings"`.

### Task 5: Implement editing, focus, search, and application transitions

**Files:** Create `src/app/{editor,input,update,view_model}.rs`, extend `src/app/mod.rs`, create `tests/interaction.rs`.

**Interfaces:** Consumes the model and config. Produces `Editor::new(String) -> Editor`, `Editor::apply(EditAction) -> bool` (whether text changed), `Editor::text() -> &str`, and `Editor::cursor() -> usize` (UTF-8 byte boundary). `EditAction` is `Insert(String)`, `Newline`, `Backspace`, `Delete`, `Left`, `Right`, `Up`, `Down`, `Home`, or `End`; paste is one Insert action. `Input` wraps terminal events, backend events, store completions, and ticks. `App::new(Config) -> App`, `App::update(Input, Instant) -> Vec<Effect>`, and `App::view() -> ViewModel` form the public application boundary. `Effect` represents loading a chat/list, saving a draft revision, preparing/staging/transmitting an outgoing message, persisting a send outcome, marking read, and shutdown. Every asynchronous effect/completion carries its request/account/chat identity. Define these payloads in `app::update`; do not expose driver types.

- [ ] **Step 1 — Write `composer_printable_keys_are_text`, `paste_never_submits`, `focus_cycle_matches_spec`, `search_cancel_restores_focus`, `reply_preserves_draft`, and `grapheme_editing_is_valid`.** Feed actual Crossterm key/paste events. Assert `"jk/r?"` remains that text in the composer, `"first\nsecond"` paste emits no transmit/prepare effect, Tab visits all three panes, Esc restores the prior overlay focus, and replying preserves existing text. Backspace removes a full combined emoji or combining-mark grapheme; movement remains on byte boundaries and vertical movement clamps to short lines. Ignore key-release events and prevent held Enter repeat events from submitting twice.

```rust
assert_eq!(editor.text(), "jk/r?");
assert_eq!(prepare_effect_count_after_paste, 0);
let mut emoji = Editor::new("hi👩‍💻".to_owned());
emoji.apply(EditAction::Backspace);
assert_eq!(emoji.text(), "hi");
assert!(emoji.text().is_char_boundary(emoji.cursor()));
```

- [ ] **Step 2 — Run `cargo test --test interaction`; confirm each new routing/editor behavior fails before implementing it.**
- [ ] **Step 3 — Implement the state machine and editor.** Route configured actions before allowed editor input, with printable navigation keys disabled in text fields. Map Alt-Enter to newline and ordinary Enter to submit. Search known chat/contact summaries case-insensitively, including a phone-number match. Restore per-chat drafts, preserve scroll anchors on incoming data, and implement reply-target removal and the resend confirmation. Use grapheme segmentation for editing and display-cell widths for the cursor. Keep the selected chat by ID, not by a row index that changes during updates.
- [ ] **Step 4 — Run `cargo test --test interaction`; all routing, overlay, and Unicode cases pass.**
- [ ] **Step 5 — Commit.** `git commit -m "feat: add pane navigation and text composition"`.

### Task 6: Render the chat interface and all visible states

**Files:** Create the `src/ui/` files in the file map and `tests/rendering.rs`; extend `src/lib.rs` and `app::view_model` as needed.

**Interfaces:** Consumes `ViewModel`, effective bindings, and theme. Produces `ui::render(frame: &mut ratatui::Frame, view: &ViewModel, config: &Config)` and `ui::layout::calculate(area: Rect, focus: Focus) -> LayoutRegions`. Focus is Chats, Messages, or Composer, defined by Task 5. ViewModel includes selection, message status, quote availability, composer/cursor, overlay, connection state, and history progress.

- [ ] **Step 1 — Write TestBackend cases for the ordinary three-pane view at 80×24 and 120×40, focused narrow views at 60×20, and resize notice at 39×24 and 80×11.** Assert `#00b4b4` focus, `#00c8c8` selection, terminal-default backgrounds, and the actual configured footer shortcuts. Add named cases `incoming_text_cannot_emit_terminal_controls`, `qr_is_never_clipped`, `expired_qr_is_not_displayed_as_valid`, and `wrapping_keeps_sender_and_status`. Include empty chats, no account, syncing, offline, send failure/unconfirmed, missing quote, media caption, and group receipt counts.

```rust
assert_eq!(actual_focus_color, ratatui::style::Color::Rgb(0, 180, 180));
assert!(!rendered_text.contains('\u{1b}'));
assert!(too_small_screen.contains("Resize"));
assert_eq!(visible_valid_qr_count_after_expiry, 0);
```

- [ ] **Step 2 — Run `cargo test --test rendering`; the new buffers/labels/layout assertions must fail.**
- [ ] **Step 3 — Implement layout and widgets.** Render only the loaded timeline page; preserve the app's scroll anchor by message identity. Convert control characters to visible/inert text at the view boundary, retaining intentional line breaks. Render QR modules from `qrcode` with a quiet zone; if the whole code cannot fit, display a resize instruction instead. Hide expired QR content until a replacement arrives. Use status words and a selection marker alongside color. Render local timestamps and Unicode widths consistently in both the timeline and composer.
- [ ] **Step 4 — Run `cargo test --test rendering`; the meaningful layout/state assertions pass without blanket snapshot replacement.**
- [ ] **Step 5 — Commit.** `git commit -m "feat: render the cyan chat interface"`.

### Task 7: Run an isolated demo with safe terminal lifecycle

**Files:** Create `src/{main,runtime,terminal}.rs`, `src/whatsapp/demo.rs`, and `tests/runtime.rs`; extend the app/backend integration points.

**Interfaces:** Produces `demo::start(store: Store) -> BackendHandle`, `runtime::run(app: App, store: Store, backend: BackendHandle) -> Result<(), AppError>` (async), and `TerminalGuard::enter() -> Result<TerminalGuard, AppError>`. The guard owns raw mode, alternate screen, bracketed paste, focus reporting, cursor restoration, and panic cleanup. Define `AppError` with sanitized display and source chaining for local diagnostics.

- [ ] **Step 1 — Write `demo_uses_only_temporary_storage`, `demo_has_no_network_factory`, `draft_tick_is_250_ms`, and `normal_exit_flushes_before_cleanup`.** Use injected terminal I/O and a fake clock; at 249 ms no save occurs, at 250 ms the current revision is saved. Point all real-account environment paths at a sentinel directory and assert it remains byte-for-byte unchanged after a demo session. Prove the live backend factory is never called in demo mode.

```rust
assert_eq!(live_backend_start_count, 0);
assert!(saved_revisions_at_249_ms.is_empty());
assert_eq!(saved_revisions_at_250_ms, vec![current_revision]);
assert_eq!(real_data_after, real_data_before);
```

- [ ] **Step 2 — Run `cargo test --test runtime`; verify the isolation/timing/lifecycle failures.**
- [ ] **Step 3 — Implement the runner and CLI.** Add `--demo`, `--config PATH`, `--data-dir PATH`, `--help`, and `--version`; demo mode rejects a live `--data-dir` and creates temporary data only. Use fixed synthetic contacts/chats and deterministic responses to user actions. The main loop selects input, service completions, backend events, and timers; slow effects run off the UI path. Draw on change, and restore the terminal before printing a fatal error. Graceful exit stops new submissions, saves current drafts, and starts `BackendControl::shutdown` while still draining events/store completions. After the backend producers stop, drain remaining work, perform the final store flush, and release the terminal and lock. Keep unwind enabled for panic cleanup.
- [ ] **Step 4 — Verify.** `cargo test --test runtime` passes; `cargo run -- --demo` presents the working interface in a terminal. Exercise navigation, search, multiline composition, replies, and resizing with synthetic conversations.
- [ ] **Step 5 — Commit.** `git commit -m "feat: run the interactive demo and restore terminal state"`.

### Task 8: Connect durable sending and revision-safe drafts

**Files:** Extend `src/{runtime.rs,app/update.rs,storage/mod.rs,storage/worker.rs,whatsapp/mod.rs}`; create `tests/live_flow.rs` using the fake backend.

**Interfaces:** Consumes `PrepareText → Prepared → Store::stage_outgoing → Transmit` and `Store::set_send_state` from earlier tasks. Store completion inputs distinguish draft saves, staged attempts, and failures by `RequestId`; backend outcomes always address the original MessageKey.

- [ ] **Step 1 — Add `send_waits_for_commit`, `commit_failure_keeps_draft`, `new_typing_survives_send_completion`, `double_enter_submits_once`, `restart_never_resends`, `late_receipt_resolves_original_attempt`, and `account_change_ignores_stale_completion`.** The fake backend records commands. Assert zero `Transmit` commands before storage succeeds or after a failed commit. Submit revision 1, type revision 2, then complete revision 1; assert revision 2 remains in both app state and the store. Disconnect mid-send and assert `Unconfirmed`; reconnect/reopen and assert the transmit count does not increase.

```rust
assert_eq!(transmit_count_before_commit, 0);
assert_eq!(transmit_count_after_failed_commit, 0);
assert_eq!(snapshot.draft.revision, 2);
assert_eq!(state_after_disconnect, SendState::Unconfirmed);
assert_eq!(transmit_count_after_restart, transmit_count_before_restart);
```

- [ ] **Step 2 — Run `cargo test --test live_flow`; confirm missing sequencing behavior fails.**
- [ ] **Step 3 — Implement the effect ordering.** A submission captures an immutable revision, allows continued editing, and deduplicates repeated submit input for that revision. Offline submission preserves text. Successful staging conditionally clears only the captured revision; every completion includes account and chat. Save each chat's latest draft on blur/shutdown and debounce ordinary edits by 250 ms. Display storage errors without discarding pending recoverable work. A resend is a new user-confirmed attempt with a new message ID; the old row remains addressable. Ensure in-flight sends can become Unconfirmed on shutdown without automatic retransmission on startup.
- [ ] **Step 4 — Run `cargo test --test live_flow` and `cargo test --test store`; all ordering and reopen cases pass.**
- [ ] **Step 5 — Commit.** `git commit -m "feat: persist sends before transmission and preserve draft revisions"`.

### Task 9: Complete history reconciliation, unread state, and retention

**Files:** Create `src/storage/merge.rs` and `tests/reconciliation.rs`; extend normalization, storage worker, runtime, and app updates.

**Interfaces:** Consumes Task 2's `record_receipt`, `upsert_chats`, and `apply_batch`. Adds async `Store::merge_alias(AccountId, ParticipantId, ParticipantId)`, `Store::mark_read(AccountId, ChatId, Vec<MessageKey>)`, and `Store::expire(AccountId, now_ms: i64)`, each returning `Result<StoreChange, StoreError>`. Add corresponding internal store-worker requests. Native normalization calls these services and emits invalidations; raw library identifiers stay behind the adapter.

- [ ] **Step 1 — Write `history_cannot_resurrect_deleted_text`, `expiry_removes_all_previews`, `receipt_before_message_is_retained`, `sender_collision_stays_distinct`, `alias_merge_is_idempotent`, `history_does_not_increment_unread`, `one_group_receipt_is_not_everyone_read`, and `reading_position_survives_new_messages`.** Apply each important sequence twice and in reversed history/live order. Assert one canonical chat, correct per-sender rows, unchanged unread count on history replay, monotonic receipt state, and removal of expired text from messages, chat previews, stored quote copies, and visible draft reply previews. A deleted/expired tombstone must survive a later older upsert. Test read actions only when Messages/Composer is at the bottom and foreground when that signal is available.

```rust
assert_eq!(canonical_chats.len(), 1);
assert_eq!(unread_after_history_replay, unread_before_history_replay);
assert!(!cached_text.contains("expired secret"));
assert_eq!(group_read_recipients.len(), 1);
assert_eq!(scroll_anchor_after_arrival, scroll_anchor_before_arrival);
```

- [ ] **Step 2 — Run `cargo test --test reconciliation`; verify failures in merge/unread/expiry behavior.**
- [ ] **Step 3 — Implement transactional merge rules and UI refresh.** Deletion/expiry tombstones outrank older content. Edit timestamps order text revisions; retain unapplied mutations and per-recipient receipts until a matching message appears. Use upstream-authoritative alias mappings to rewrite related chat/message/draft/quote keys transactionally without merging unrelated group senders. Apply initial unread baselines separately from new unique messages and persist read watermarks to avoid re-counting replays. Query cache pages with stable tie-breaking. Expire on startup and a one-second runtime tick; metadata missing from a replay never erases already-known expiry. Bound view-state memory to loaded pages. Refresh names from contact/group events and fall back to stable identifiers.
- [ ] **Step 4 — Run `cargo test --test reconciliation`, `cargo test --test live_flow`, and `cargo test --test rendering`; all retention, ordering, and view checks pass.**
- [ ] **Step 5 — Commit.** `git commit -m "feat: reconcile history receipts identities and unread state"`.

### Task 10: Enable normal startup and verify daily-use behavior

**Files:** Finish `src/main.rs`, runtime startup/error paths, `README.md`, `docs/usage.md`, `docs/backend-validation.md`, `.github/workflows/ci.yml`, and `tests/terminal_process.rs`.

**Interfaces:** Consumes all preceding task interfaces. Default startup loads config, acquires the data lock, starts the store and real backend, and runs the app. Demo remains an explicit isolated mode. Diagnostics expose categories and state, not unrestricted upstream error strings or personal fields.

- [ ] **Step 1 — Write process-level regression cases `demo_restores_tty_after_exit`, `controlled_panic_restores_tty`, and `saturated_shutdown_finishes`.** Run the binary under a Linux PTY using a test helper based on `nix` PTY support, compare termios before/after Ctrl-Q, and assert alternate-screen/cursor restoration. Use a test-only panic injection in the terminal-guard harness, not a public production CLI flag. Bound the shutdown test to five seconds and fail if a producer or worker remains blocked. Assert config failures occur before entering raw mode and formatting a sentinel-bearing backend error does not expose the sentinel.

```rust
assert_eq!(termios_after, termios_before);
assert!(shutdown_completed_within_five_seconds);
assert!(!error_output.contains("PRIVATE_SENTINEL"));
```

- [ ] **Step 2 — Run `cargo test --test terminal_process`; confirm the targeted process behaviors fail before their fixes.**
- [ ] **Step 3 — Finish startup and documentation.** Explain stable compiler/build requirements, linking, history availability, plaintext local cache, default controls, configuration, data paths, and recovery from revoked sessions. Check the backend validation record against what actually ran. CI builds without WhatsApp credentials and runs the commands below; live checks are never silently executed by CI. Correct issues exposed by the process tests before broad verification.
- [ ] **Step 4 — Run the release checks.** Each command must exit 0:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked --release
```

Then the user performs the spec's Linux acceptance workflow with the real account: pair/restore, direct and group messaging, quoted reply, two drafts, scroll during arrival, resize, restart, and reconnect. Record live results as Passed/Failed/Not run with a short reason. If linking is still unavailable, report the working demo and completed automated checks, while explicitly retaining the outstanding live acceptance items.
- [ ] **Step 5 — Commit and review.** `git commit -m "feat: complete live startup and document daily use"`. Use the chosen execution workflow's whole-branch review, address its findings, and rerun only checks affected by subsequent changes. Finish the branch according to the user's integration instructions; do not publish or merge by assumption.

## Coverage and handoff

| Spec requirement | Owning tasks |
| --- | --- |
| Backend feasibility, pairing, session restore, reconnect | 1, 3, 10 |
| Native stable build and pinned protocol dependency | 1 |
| Palette, configurable bindings, terminal defaults | 4, 6 |
| Pane navigation, composition, search, replies, placeholders | 3, 5, 6 |
| Durable drafts, interrupted sends, explicit resend | 2, 5, 8 |
| Local history, receipts, unread state, identity aliases, edits/deletes/expiry | 2, 3, 9 |
| Bounded work, isolated demo, cleanup, account isolation | 2, 3, 7, 8, 10 |
| Process tests, usage instructions, honest live validation | 3, 10 |

Recommended execution: **Native**, because these ten tasks share a small set of evolving adapter/store/application interfaces and are mostly sequential. One implementer can carry that context through the work, followed by an independent review of the completed branch. Subagent-driven execution remains available if the user prefers separate implementation and review gates for every task.

The user approved Native execution on 2026-09-29.

## Research references

- [Pinned backend source](https://github.com/oxidezap/whatsapp-rust/tree/v0.7.0), including [Cargo features](https://github.com/oxidezap/whatsapp-rust/blob/v0.7.0/Cargo.toml), [send API](https://github.com/oxidezap/whatsapp-rust/blob/v0.7.0/src/send/mod.rs), and [SQLite dependency](https://github.com/oxidezap/whatsapp-rust/blob/v0.7.0/storages/sqlite-storage/Cargo.toml).
- [Callback delivery and lifecycle](https://github.com/oxidezap/whatsapp-rust/blob/v0.7.0/src/bot.rs) and [inbound durability contract](https://github.com/oxidezap/whatsapp-rust/blob/v0.7.0/src/types/durability_hook.rs).
- [Published prelude/API reference](https://docs.rs/whatsapp-rust/latest/whatsapp_rust/prelude/index.html), checked against the downloaded tag rather than assumed identical to it.
- [Ratatui application patterns](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/).
