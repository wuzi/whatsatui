# Conversation UX Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans inline, with regression tests and one independent final review.

**Goal:** Ship approved steps 1–3: reliable sticker preparation, readable sender blocks with photos, and keyboard/mouse message selection.

**Architecture:** Keep the App reducer authoritative for selection and actions. Share timeline and popup geometry between rendering and hit testing. Add a separate bounded profile-photo provider/cache, connected to the existing terminal image preparation code.

**Tech Stack:** Rust 1.98, Ratatui 0.30, Crossterm 0.29, whatsapp-rust 0.7.0, image/WebP, Tokio.

**Spec:** `docs/superpowers/specs/2026-09-30-conversation-ux-design.md`

## Global Constraints

- One Cargo process at a time; `CARGO_TARGET_DIR=/home/wuzi/Projects/whatsapp-tui/target`, `-j 2`, `--test-threads=2`.
- Preserve current pinned dependencies and durable sending/draft/account behavior.
- No live account, real clipboard or WhatsApp sends during verification.
- Reactions and outgoing editing are outside this release.

## Review Focus

- Cached or in-flight photos after account changes must never appear under another identity.
- A popup or resized terminal must not leave clickable hidden rows or stray graphics.
- Long Unicode messages and media must remain readable and individually selectable after grouping.
- Incoming history, edits and identity merges must preserve valid selection and viewport anchors.
- An optimized sticker must remain in the picker through cancellation, refreshes and send errors without altering the composer.

## Task 1: Sticker preparation and quiet footer

**Files:** `src/media/outgoing.rs`, `src/app/update/stickers.rs`, `src/ui/mod.rs`, `src/ui/layout.rs`, `tests/sticker_sending.rs`, `tests/rendering.rs`.

**Interfaces:** Retain `import_sticker(source: &[u8], data_dir: &Path, preserve: bool) -> Result<LocalImage, String>`. Separate source WebP inspection from final-output size validation. `StickerImported` continues to carry immutable LocalImage snapshots.

- [x] Add `oversized_received_static_sticker_is_optimized_with_alpha`: source >102400 bytes, result <=102400 bytes, 512×512, transparent padding retained, immutable read succeeds. Keep existing animation preservation test.
- [x] Add a reducer regression: a transformed received sticker yields a local preview and no Prepare effect until another explicit activation; draft unchanged.
- [x] Run sticker tests and observe the new failures.
- [x] Implement static re-encoding only when needed; leave received animation unchanged. Retain the prepared picker choice for confirmation.
- [x] Remove footer hints, use one status row; update the existing Help/shortcut rendering assertion for the new requirement.
- [x] Run `cargo test -j 2 --test sticker_sending --test rendering -- --test-threads=2`; commit passing change.

## Task 2: Profile-photo service and rendering

**Files:** new `src/avatars.rs`, `src/ui/avatars.rs`, `src/whatsapp/avatars.rs`; modify `src/lib.rs`, `src/ui/images.rs`, `src/ui/mod.rs`, `src/runtime.rs`, backend handles and fixtures.

**Interfaces:** `avatars::Identity { account: AccountId, jid: String }`; async `Provider::fetch(&self, identity: &Identity) -> Result<Option<Vec<u8>>, String>`. A `Cache::load` interface owns validated, bounded disk thumbnails. `ui::Avatars` provides begin/draw/end/poll/stop/cleanup lifecycle like Images. Share image protocol preparation but use distinct Kitty IDs.

- [x] Write real cache tests with a fake network provider: account isolation, cache reuse, stale/missing removal, invalid bytes and oversize input, bounded disk contents. Observe failures.
- [x] Implement HTTPS profile fetch using the pinned contacts API; decode off the render thread with limits and a timeout. Save only sanitized thumbnails in private storage.
- [x] Implement bounded visible-avatar scheduling, initials fallback and cleanup, with no live lookups in demo.
- [x] Run focused avatar tests plus runtime/terminal tests; commit.

## Task 3: Sender blocks and distinct selection/scrolling

**Files:** `src/ui/timeline.rs` and focused supporting layout module as needed; `src/app/view_model.rs`, `src/app/update.rs`, `src/app/update/search.rs`, `src/config/bindings.rs`, theme; `tests/conversation_ux.rs` and existing long-message/media regressions.

**Interfaces:** A shared timeline layout owns wrapped rows, message identities and preview/avatar positions. Separate `timeline_anchor: Option<MessageKey>` from `selected_message`; retain wrapped-row offset metrics for scrolling. Rendering consumes the same layout later used for pointer hit testing.

- [x] Add sender-block tests for two group senders, own message distinction independent of selection, date boundaries, grouped messages retaining independent timestamps, narrow media clipping.
- [x] Add keyboard tests: j/k skips a long message as one item, J/K scroll preserves selection, incoming traffic preserves an older selection, End resumes newest.
- [x] Observe failures, then implement layout and reducer changes, integrating profile photos and retaining status/quote/media rendering.
- [x] Update regressions whose arrow-scroll contract was intentionally replaced; keep their start/middle/end visibility coverage using scroll controls.
- [x] Run focused conversation, rendering, navigation, rich text and preview tests; commit.

## Task 4: Mouse support, Help, and integration

**Files:** new `src/ui/interaction.rs`, `src/app/update/mouse.rs`; modify layout/renderers, `src/app/input.rs`, editor, runtime, config and terminal guard; `tests/conversation_ux.rs`, `tests/terminal_process.rs`, `docs/usage.md`.

**Interfaces:** UI returns an `InteractionMap` with account/chat/context identity and actual rendered hit regions. `Input::Rendered` installs it in App; terminal mouse events resolve against it and route through existing reducer actions. Screen's default methods retain compatibility for headless tests.

- [x] Write pointer tests for exact wrapped-message selection and right-click actions, scroll independent of selection, chat focus, Unicode composer position, popup list scrolling and double-click activation, stale/resized layouts, disabled mouse. Observe failures.
- [x] Add cursor positioning that snaps to grapheme boundaries; generate hit regions from renderer list offsets and shared text geometry.
- [x] Enable/restore mouse capture; add configuration and clickable Help/close controls. Keep hidden and background regions inert during popups.
- [x] Update Help and usage docs with selection vs scroll controls, mouse gestures, photo fallback and sticker preparation.
- [x] Run focused tests, then complete suite, fmt, Clippy and optimized demo smoke test. Commit and record verification.
- [x] Request one independent whole-branch review; address important findings with regression tests. Integrate locally and preserve validation evidence.
