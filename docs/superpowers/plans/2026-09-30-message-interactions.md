# Message Interactions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Receive, inspect, send/change/remove reactions, edit own sent text, and reply to media with a cached-original jump.

**Architecture:** Reuse message menus, emoji search, timeline rows, and pagination. Store target-scoped reactions and durable outgoing mutations in a dedicated module, hydrate them with snapshots, and isolate transport calls behind a test seam. Keep normal drafts independent of the edit editor.

**Tech Stack:** Rust 1.98, Ratatui 0.30, Diesel/SQLite, pinned whatsapp-rust 0.7.0.

**Spec:** `docs/superpowers/specs/2026-09-30-message-interactions-design.md`

## Global Constraints

- No new dependencies; additive SQLite migration.
- One Cargo process at a time, shared root target directory, `-j 2`, `--test-threads=2`.
- Synthetic/offline tests only; no live-account sends or clipboard reads.
- Existing pane controls/theme/footer remain; normal draft survives reactions and editing.
- Inline execution, one final independent review, local merge and release build, retain evidence, no push.

## Review Focus

- Reaction arrives before the target or after removal: ordering and tombstones prevent resurrection (Task 1).
- PN/LID mapping merges both target and reactor: one canonical reaction and correct account scope (Task 1).
- Crash or timeout after transmission: no automatic replay or false confirmed content (Task 2).
- Incoming update while an editor/picker is open: stable target and original draft remain intact (Task 3).
- Mouse hit after clipping/resize or quote outside cached page: rendered rows and loaded stable identity agree (Tasks 3–4).

## Task 1: Receive and persist reactions

**Files:** `src/app/model.rs`, `src/storage/interactions.rs` (new), `src/storage/{mod,worker,merge}.rs`, `migrations/00000000000003_interactions/up.sql` (new), `src/whatsapp/{normalize,native}.rs`, `tests/reactions.rs` (new).

**Interfaces:** Produce `Reaction { key, reactor, emoji, at_ms, event_id }`, `MessageChange::Reaction(Reaction)`, `MessageInteractions { reactions, mutations }`, `ChatSnapshot.interactions`, and storage snapshot hydration for visible keys. `normalize::history_changes` supplements the existing single-message normalization with stored history reactions.

- [x] Write tests: live direct/group/self reaction targets, history summaries; replay, replacement, empty removal, late add, reaction before original, alias/account isolation, deletion/expiry. Assert one original chat message and unchanged unread/preview, literal emoji/counts and canonical identities.
- [x] Run the new test targets and observe the expected missing reaction behavior.
- [x] Implement normalization, schema version 3, reaction ordering/rekeying and snapshot hydration. Skip malformed reaction envelopes; validate target scope.
- [x] Run `cargo test -j 2 --test reactions --lib -- --test-threads=2`; expect all pass.
- [x] Commit the completed receive/storage slice.

## Task 2: Durable outgoing reactions and edits

**Files:** `src/message_actions.rs`, `src/storage/interactions.rs`, `src/whatsapp/interactions.rs` (new), `src/whatsapp/{mod,native,demo}.rs`, `tests/mutations.rs` (new).

**Interfaces:** Produce `MutationKind::{Reaction { emoji }, Edit { text }}`, `MutationAttempt { id, target, kind, created_at_ms, state }`, store stage/finish APIs with target-version validation, `BackendCommand::Mutate { request, message, kind }`, and `BackendEvent::MutationOutcome { request, account, result }`. Transport trait sends a validated attempt; test fake records the committed journal before returning an outcome.

- [x] Write tests for durable-before-send, success, rejection, timeout, restart recovery, simultaneous target operation refusal, stale body/version, ownership, 15-minute boundary and expiry. Assert original send state/quote and normal draft unchanged; never replay interrupted work.
- [x] Run and observe missing behavior; implement the shared orchestration plus native/default-method transport and demo transport.
- [x] Run `cargo test -j 2 --test mutations --lib -- --test-threads=2`; expect all pass.
- [x] Commit the durable sending slice.

## Task 3: Reaction and edit UI

**Files:** `src/app/update/interactions.rs` (new), `src/app/{update,view_model}.rs`, `src/app/update/{actions,mouse}.rs`, `src/config/bindings.rs`, `src/runtime.rs`, `src/ui/{actions,emoji,composer,message_body,timeline,interaction,overlays}.rs`, `src/ui/reactions.rs` (new), `tests/message_interactions.rs` (new).

**Interfaces:** Consume Tasks 1–2 state and commands. Produce captured reaction-picker target, reaction-details popup, separate `EditingMessage { message, editor, request, error }` state and mutation effect. Render reaction totals/You and durable operation status inside message rows, keeping hit regions aligned with wrapped rows.

- [x] Write tests for target stability through arrivals, reaction change/remove via picker/menu, group participant details, narrow rendering and click targets, draft preservation during edit/save/cancel/failure, stale edits and old-account completions, printable composer keys and remapped controls.
- [x] Run and observe missing behavior; implement reducer, rendering, mouse hit maps and command routing.
- [x] Run `cargo test -j 2 --test message_interactions --test message_actions_flow --test interaction --test emoji_picker -- --test-threads=2`; expect all pass.
- [x] Commit the usable UI slice.

## Task 4: Media replies, original jumps and release verification

**Files:** `src/app/model.rs`, `src/message_actions.rs`, `src/app/update/interactions.rs`, `src/whatsapp/{normalize,encode}.rs`, `src/runtime.rs`, `src/ui/{message_body,timeline,interaction}.rs`, `tests/media_replies.rs` (new), docs and demo fixtures.

**Interfaces:** Produce serde-defaulted quote media kind and `message_actions::quote(message, now_ms) -> Option<Quote>`, original lookup effect/completion, and stable quote hit targets. Reuse existing page cursor rather than inventing another navigation model.

- [x] Write tests for captionless image/sticker/document quotes, typed wire quote and group sender, stale/missing/expired originals, cross-account/chat refusal, cached-page jump, clipped quote hits, and existing text replies.
- [x] Run and observe missing behavior; implement and update usage/validation docs and demo fixtures.
- [x] Run focused media-reply tests, then full `cargo test -j 2 -- --test-threads=2`, `cargo fmt --check`, and `cargo clippy -j 2 --all-targets -- -D warnings`; expect all pass.
- [x] Commit, obtain one independent whole-branch review, resolve actionable findings with regression evidence.
- [x] Build `cargo build --release -j 2`; run demo PTY actions/edit/reaction/quote/resize/quit smoke with frame synchronization; retain logs.
- [ ] Fast-forward local main after source verification, remove owned worktree/branch and report controls, checks, and service-testing limitations.
