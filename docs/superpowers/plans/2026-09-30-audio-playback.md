# Received Audio Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans for inline implementation. Follow checkbox steps in order.

**Goal:** Play received voice messages and audio files with pause/resume, progress and speed inside the TUI.

**Architecture:** Extend verified attachments, prepare a private audio snapshot, and run a single cancellable mpv actor. The reducer submits desired playback state and consumes generation-scoped observations; timeline and header share those observations.

**Tech Stack:** Existing Rust/Ratatui/Tokio/SQLite and JSON; external mpv, no new Rust dependencies.

**Spec:** `docs/superpowers/specs/2026-09-30-audio-playback-design.md`

## Global Constraints

- One Cargo process; shared root target; `-j 2`, `--test-threads=2`.
- Synthetic/offline verification, null audio output for real-player checks; no linked account/clipboard.
- Existing verified 50 MiB cache, no view-once caching, no auto-play or invented played receipts.
- Preserve drafts and pane controls. Player: one at a time, 1x/1.5x/2x, stable account/message/generation.
- Inline implementation, one final review, local merge/release, preserve evidence, no push.

## Review Focus

- Rapid play/stop/replacement during download or startup must not play stale audio (Tasks 1–2).
- Cache corruption, deletion and expiry during off-screen playback must stop/reject the file (Tasks 1–2).
- Missing mpv, a stalled IPC peer or a crashed child must release resources and show a useful error (Task 2).
- Incoming messages or account changes must not retarget controls or accept old observations (Task 3).
- Tiny/clipped/covered playback rows and stale header hits must not trigger hidden or different controls (Task 3).

## Task 1: Audio attachments and verified preparation

**Files:** `src/media/{model,mod,worker,audio}.rs`, `src/whatsapp/{media,normalize,encode}.rs`, relevant fixtures and `tests/audio_media.rs`.

**Interfaces:** `AttachmentKind::Audio`; serde-defaulted `Attachment.audio: Option<AudioMetadata { seconds: Option<u32>, voice: bool }>`; `media::audio::prepare(&MessageRecord, &Store, &dyn Downloader, watch::Receiver<bool>) -> Result<NamedTempFile,String>`. Snapshot remains private and unlinked on drop.

- [x] Write tests for live/history voice/audio metadata, view-once exclusion, correct Audio decryption type, old attachment JSON, verified cache reuse/snapshot lifetime, corruption, cancellation and expiry during preparation. Run focused tests and observe missing behavior.
- [x] Implement the metadata/normalization, common cache acquisition, audio snapshot and typed quotes. Update existing attachment literals/exhaustive matches.
- [x] Run `cargo test -j 2 --lib --test audio_media --test media_files --test media_model --test media_replies -- --test-threads=2`; expect all pass, commit.

## Task 2: Cancellable controlled player

**Files:** new `src/audio/{mod,mpv,worker}.rs`, `src/lib.rs`, `src/config/mod.rs`, `tests/audio_player.rs`.

**Interfaces:** `Speed::{Normal,OneHalf,Double}`; `Request { id: RequestId, message: MessageRecord, paused: bool, speed: Speed }`; `Phase::{Loading,Playing,Paused,Finished,Failed}`; `Playback { request: Request, phase, position_ms:u64, duration_ms:Option<u64>, error:Option<String> }`; `Player::start(store, Arc<dyn Downloader>, executable:PathBuf)`, `Player::set(Option<Request>)`, watch event receiver and lifecycle shutdown. MPV child uses private socket/snapshot, bounded JSON, observed properties and explicit command replies.

- [ ] Write player boundary tests using an isolated fake IPC executable: startup/missing player, pause/resume and speed, progress/EOF, malformed/closed/stalled IPC, replacement/stop, deletion/expiry and drop cleanup. Observe expected missing API/behavior failures.
- [ ] Implement owned child/IPC transport and actor; configure `[audio].player` default `mpv`. Do not block the terminal loop on playback or commands; coalesce desired state and reject stale generations.
- [ ] Run `cargo test -j 2 --test audio_player --test audio_media --test configuration -- --test-threads=2`; expect all pass, commit.

## Task 3: Message controls, rendering and release

**Files:** new `src/app/update/audio.rs`, `src/ui/audio.rs`; existing reducer/view/input/runtime/bindings/menu/mouse/timeline; demo/audio fixture; `tests/audio_flow.rs`, docs/config examples.

**Interfaces:** `Effect::Audio(Option<audio::Request>)`, `Input::Playback(audio::Playback)`, `ViewModel.playback: Option<audio::Playback>`. Runtime owns Player and routes effects outside ordinary store-job slots. Rendered audio/control targets include message or active request identity.

- [ ] Write reducer/rendering tests for play/pause/replacement, captured menu targets, speed, stop, stale events/account change, expiry/deletion, playback across chat changes, unchanged drafts/composer keys, configured bindings, actual clipped mouse hits at 40x12 and 120x34. Observe RED then implement controls and runtime routing.
- [ ] Add an offline synthetic voice note and update usage/validation/example configuration. Run focused audio tests, then full `cargo test -j 2 -- --test-threads=2`, fmt and Clippy; expect all pass.
- [ ] Commit; request one whole-branch review and fix actionable findings with regressions. Build release, run demo PTY + player smoke (null output), preserve logs, merge main, verify tested source, remove owned worktree/branch.
