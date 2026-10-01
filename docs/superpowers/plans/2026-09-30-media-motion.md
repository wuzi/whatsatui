# Media Motion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for inline implementation. Follow checkbox steps in order.

**Goal:** Animate visible stickers and play received videos in an mpv window.

**Architecture:** Extend the verified preview loader with bounded animation frames and a clocked protocol cache. Extend existing attachment and mpv playback paths for videos without changing persisted audio settings or keybinding IDs.

**Tech Stack:** Rust, image/WebP, Ratatui, Kitty graphics, Tokio, external mpv; no new Rust dependencies.

**Spec:** `docs/superpowers/specs/2026-09-30-media-motion-design.md`

## Global Constraints

- One Cargo process; shared root target; `-j 2`, `--test-threads=2`.
- Synthetic/offline verification and null outputs for native player tests; no live account or desktop clipboard.
- No new Rust dependencies. Retain static/avatar rendering and existing media validation.
- Inline implementation, one final independent review, local merge/release, preserve evidence, no push.

## Review Focus

- Malformed, huge or very fast animations must stay bounded and leave a usable still (Task 1).
- Hidden/resized/deleted animation frames must release all Kitty resources without refreshing avatars (Task 1).
- Video view-once wrappers and invalid download metadata must never become playable cached media (Task 2).
- Window pause/close and rapid TUI controls must converge without stale playback or orphaned children (Task 2).
- Video captions, quotes, tiny layouts and mouse targets must remain useful and correctly scoped (Task 2).

## Task 1: Bounded animated previews

**Files:** new `src/media/animation.rs`, `src/media/{mod,preview}.rs`, `src/ui/images.rs`; preview/rendering tests and synthetic WebP fixture.

**Interfaces:** `animation::Preview { frames: Vec<Frame>, loops: Option<u32> }`, `Frame { image: DynamicImage, duration_ms: u64 }`; animation-aware `preview::load`/`load_local` return Preview, while `preview::decode` stays static for validation. `Images::poll` advances the visible frame and signals redraw; static `images::prepare` used by avatars stays unchanged.

- [x] Write regressions asserting two distinct rendered frames, delay/loop timing, bounded sampling/fallback, cancellation and complete Kitty cleanup; run focused tests, expect missing behavior.
- [x] Implement bounded WebP decode, preview loading, clocked prepared-frame cache and per-frame unique Kitty IDs. Limit animated resize to fit the thumbnail; static paths keep current scale.
- [x] Run `cargo test -j 2 --lib --test inline_media_tests --test inline_rendering --test sticker_sending -- --test-threads=2`; expect pass and commit.

## Task 2: Received video playback

**Files:** `src/media/{model,worker,audio}.rs`, `src/whatsapp/{media,normalize,encode,media_edit_tests}.rs`, `src/audio/{mod,mpv,worker}.rs`, `src/app/update/audio.rs`, `src/message_actions.rs`, `src/ui/{audio,message_body}.rs`, bindings/help; audio/media tests and fake player.

**Interfaces:** `AttachmentKind::Video`; existing player Request/Playback accepts Audio or Video and reports observed playback controls. Existing PlayAudio action/config key remains compatible and presents playback wording.

- [ ] Write failing live/history/download/quote tests for video, view-once exclusions, mpv window arguments, normal close, native pause/speed and rapid controls. Assert video caption rendering and mouse/play actions.
- [ ] Implement video metadata normalization and decryption, verified snapshot playback with GUI output, observed control reconciliation and video rendering/actions. Preserve audio mode and safe player lifecycle.
- [ ] Run `cargo test -j 2 --lib --test audio_flow --test audio_player --test audio_media --test media_files --test media_model --test media_replies -- --test-threads=2`; expect pass and commit.

## Task 3: Validation and release

**Files:** README/validation docs, synthetic native playback fixture and smoke evidence.

**Interfaces:** Tasks 1–2 preserve existing runtime polling, image cleanup and Player ownership.

- [ ] Update usage and limitations; run complete `cargo test -j 2 -- --test-threads=2`, fmt and Clippy, expect pass. Run native mpv with null outputs on synthetic video and optimized synthetic PTY animation checks.
- [ ] Commit, request one whole-branch review, fix important findings with RED→GREEN regressions, and verify changed code.
- [ ] Build release, preserve evidence, merge to local main, verify source/release correspondence and clean up owned worktree/branch. No push.
