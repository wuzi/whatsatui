# Desktop Notifications Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for inline implementation. Follow checkbox steps in order.

**Goal:** Alert the user to new WhatsApp messages with quiet, focus-aware desktop popups.

**Architecture:** Report first live incoming inserts after storage commits, forward them through the native durability hook, coalesce eligible records in the app reducer, and use a bounded asynchronous desktop effect for Linux delivery.

**Tech Stack:** Rust, Tokio, existing SQLite worker, external notify-send; no new Rust dependencies.

**Spec:** `docs/superpowers/specs/2026-10-01-notifications-design.md`

## Global Constraints

- One Cargo process; shared root target; `-j 2`, `--test-threads=2`.
- Synthetic/offline verification only; no live WhatsApp data or desktop popups.
- Silent default notifications; configuration can disable them or hide all identifying preview content.
- Inline implementation, one independent final review, local merge/release, evidence preservation, no push.

## Review Focus

- Durability hook plus protocol replay must emit once; rollback/history/own messages must emit nothing (Task 1).
- Focus, pending conversation changes, unknown foreground and account changes must not lose or misroute alerts (Task 2).
- Deleted/expired/read messages and aliases must be revalidated at dispatch (Task 2).
- Markup, option-like text, large Unicode input and hidden previews must remain bounded and private (Task 2).
- Missing, failed or hanging helpers, shutdown and large bursts must not block messaging or grow without limit (Task 2).

## Task 1: Committed incoming-message signal

**Files:** `src/storage/{mod,worker}.rs`, `src/whatsapp/{mod,durability,native}.rs` and storage/durability unit tests.

**Interfaces:** `Store::apply_batch_with_incoming(MessageBatch) -> Result<(StoreChange, Vec<MessageRecord>), StoreError>` (crate-visible), preserving existing `apply_batch`. `BackendEvent::IncomingMessages(Vec<MessageRecord>)` carries only first committed live unread inserts. `DurableInbox` receives the bounded backend event sender. Existing UI backend matching temporarily accepts the new event without behavior until Task 2.

- [x] Add RED tests for first insertion vs replay/history/own/edit/reaction/delete, transactional rollback, alias canonicalization, and the real durability helper followed by protocol replay.
- [x] Implement the transactional result and hook forwarding. The normal inbound event path forwards any first-insertion result too; only one path can produce one.
- [x] Run `cargo test -j 2 --lib whatsapp::durability -- --test-threads=2`; expect pass and commit.

## Task 2: Notification policy and desktop delivery

**Files:** new `src/notifications.rs`, `src/storage/notifications.rs`, `src/app/update/notifications.rs`, notification flow tests; `src/{lib,desktop,runtime}.rs`, config, app input/update.

**Interfaces:** Task 1's event feeds `notifications::Inbox`, which emits bounded `notifications::Request { keys, overflow, previews }` through `Effect::Notify`. `notifications::prepare(Request, &Store, now_ms) -> Result<Option<Popup>, String>` revalidates storage, then `notifications::deliver(&Popup, &impl Notifier)` calls a replaceable notifier. `NativeNotifier` uses a bounded process. `Input::NotificationResult(Result<(), String>)` drives a single notice and retry cooldown. `NotificationConfig { enabled, previews }` defaults true/true.

- [x] Write and run RED tests for foreground/current chat suppression, background/other chats/scrollback/unknown focus, pending cancellation, startup/history/own exclusions, account switching, queue cap and burst timing. Cover disabled/demo and privacy configuration.
- [x] Implement config, the burst policy, storage revalidation and app/runtime effect wiring with shutdown cancellation. Add notification process/content tests covering private previews, group/media names, markup/options/control characters, timeout/failure/missing helper and bounded retries.
- [x] Run `cargo test -j 2 --lib --test notifications --test configuration --test runtime -- --test-threads=2`; expect pass and commit.

## Task 3: Validation and release

**Files:** README, usage and backend-validation docs, this plan and synthetic verification evidence.

**Interfaces:** Tasks 1–2 preserve existing send/read/desktop effect behavior; no external account or desktop dependencies in ordinary tests.

- [x] Document behavior, config, notify-send dependency and version-one limitations. Run fmt, full `cargo test -j 2 -- --test-threads=2`, and Clippy; expect pass.
- [x] Commit and request one fresh whole-branch review. Resolve important findings with RED→GREEN regressions; verify affected code and full suite after fixes.
- [ ] Build the release, preserve evidence, merge into local main, verify source/release correspondence and remove the owned worktree/branch. No push.
