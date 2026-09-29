# Received Media Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan inline, task by task. Steps use checkbox syntax for tracking.

**Goal:** Receive images/documents, explicitly download verified files, and open them in a desktop viewer while keeping chats responsive.

**Architecture:** Preserve attachment references in normalized message bodies. A media service owns validation, private managed storage, and a bounded worker process; the reducer invokes it through asynchronous effects using existing request/account completion isolation. Reuse the action menu and desktop launcher.

**Tech Stack:** Rust 1.98.0, Ratatui, Tokio, Diesel/SQLite, pinned whatsapp-rust 0.7.0, SHA-256.

**Spec:** `docs/superpowers/specs/2026-09-29-received-media-design.md`

## Global constraints

- Receive images/documents; leave other media and view-once content as placeholders. Preserve captions, navigation, drafts, search, and existing shortcuts.
- Keep original message text and complete identities. Additive JSON variant; no SQL migration or backend upgrade. Only direct SHA-256 dependency already present in Cargo.lock.
- Limit plaintext to 50 MiB, encrypted responses to 50 MiB + 26 bytes, worker lifetime to 60 seconds, and managed storage to 512 MiB / 128 attachments.
- Private files/directories (0600/0700); generated filenames; no shells, automatic downloads, or automatic viewer launches.
- Every Cargo command uses `CARGO_TARGET_DIR=/home/wuzi/Projects/whatsapp-tui/target`; builds `-j 2`, tests `--test-threads=2`.
- Synthetic acceptance only. Inline implementation, one independent final review, local merge, no publication.

## Review focus

- View-once wrappers and incomplete/hostile reference fields must never create a downloadable attachment (Task 1).
- Replayed history and preexisting JSON must retain captions/search behavior without inventing download metadata (Task 1).
- Filename traversal, symlinks, corrupt caches, size mismatches, and partial files must not become viewer inputs (Task 2).
- Edits, deletion, expiry, aliases, and account switches during transfer must not publish or open stale content (Tasks 2/3).
- A stalled worker, full storage, or repeated activation must preserve navigation, drafts, and bounded shutdown (Tasks 2/3).

### Task 1: Persist received attachment references

**Files:** Create `src/media/model.rs`, `src/media/mod.rs`, `src/whatsapp/media.rs`; modify `src/lib.rs`, `src/app/model.rs`, `src/whatsapp/normalize.rs`, `src/storage/{worker,search}.rs`, `src/message_actions.rs`, `src/ui/timeline.rs`; extend normalization/store/search/render tests.

**Interfaces:** `Attachment` contains `kind: AttachmentKind`, `filename/mime/caption: Option<String>`, `size: u64`, `direct_path: String`, and `[u8; 32]` key/plain/encrypted hashes. `MessageBody::Media(Box<Attachment>)`. `whatsapp::media::attachment(&wa::Message) -> Option<Attachment>` rejects view-once/incomplete references. `Attachment::validate() -> Result<(), String>` owns bounded metadata/reference checks; `Attachment::extension() -> Option<&'static str>` owns supported viewer suffixes.

- [x] Write failing tests for image/document live/history capture, nested view-once rejection, incomplete keys/hashes/paths, legacy JSON, and original caption copying/search/style.
- [x] Run normalization and targeted store/search/render targets; confirm meaningful failures before implementation.
- [x] Implement the additive body model, normalization, and all exhaustive presentation/search matches. Keep incomplete references as existing placeholders and redact sensitive reference fields from Debug.
- [x] Run focused checks; expect all existing and new cases to pass.
- [x] Commit `feat: preserve received attachment metadata`.

### Task 2: Verified downloads and managed files

**Files:** Create `src/media/{cache,download,worker}.rs`, `tests/media_files.rs`; modify `src/media/mod.rs`, `src/storage/mod.rs`, `src/desktop.rs`, `src/main.rs`, `Cargo.toml`, `Cargo.lock`.

**Interfaces:** `MediaAction::{Download, Open}`; injectable async `Downloader::download(&self, attachment: &Attachment, destination: &Path, cancel: watch::Receiver<bool>) -> Result<(), String>`; `media::execute(message: MessageRecord, action: MediaAction, store: Store, downloader: &dyn Downloader, desktop: &impl Desktop, cancel: watch::Receiver<bool>) -> Result<String, String>`. `media::prune(store: Store) -> Result<(), String>` removes obsolete managed copies. `Store::data_dir() -> &Path` identifies the private root. `Desktop::open_file(&Path)` launches one literal absolute path. Native downloader uses the current binary's internal worker mode; the worker consumes bounded stdin and the pinned streaming API.

- [x] Write failing tests for exact bytes and SHA/length checks, repeated reuse, unknown MIME download/open behavior, hostile filenames/symlinks, 50 MiB limits, partial/corrupt files, 512 MiB/128-entry accounting, and stale/expired/aliased records before and after transfer. Use local SQLite and an inert downloader/viewer.
- [x] Run `cargo test --locked -j 2 --test media_files -- --test-threads=2`; observe missing API then behavior failures.
- [x] Implement temporary-file lifetime, validated cache naming/manifests, resource limits, locking, cleanup, source revalidation, verified reuse, and explicit viewer opening. Native worker receives secrets through stdin, uses a limited writer/response, and is killed/reaped on deadline or cancellation.
- [x] Test process stdin/argv, timeout/cancellation, failure cleanup, and internal worker rejection of malformed input without network traffic; run media/storage/desktop targets.
- [x] Commit `feat: download and open verified received files`.

### Task 3: Message controls, runtime, and acceptance

**Files:** Modify `src/app/{input,update}.rs`, `src/app/update/actions.rs`, `src/config/bindings.rs`, `src/ui/actions.rs`, `src/runtime.rs`, `src/whatsapp/{mod,native,demo}.rs`, `examples/config.toml`, README/usage/validation docs; create `tests/media_flow.rs` and extend `tests/terminal_process.rs` plus backend fixtures.

**Interfaces:** `ActionId::{DownloadMedia, OpenMedia}` with Messages/menu defaults `d` and `v`. `Effect::MediaAction { request, message, action }` completes through existing `Input::DesktopAction { request, account, result }`. `BackendHandle` carries an `Arc<dyn Downloader>` so the demo/test backend supplies an inert implementation. Runtime execution passes a shutdown watch channel to downloads, invokes cache maintenance, and preserves the existing command/storage scheduling.

- [x] Write failing reducer/render tests for captionless attachment menus, `d` download without opening, `v` explicit open, remapping, repeated activation, full identity/stale account handling, and ordinary composer typing/draft preservation. Add a synthetic PTY flow that downloads demo bytes, opens through a fake viewer, and restores the terminal on quit.
- [x] Run the focused flow target and confirm missing behavior fails.
- [x] Wire applicable menu rows, notices, runtime downloader injection/cancellation/maintenance, demo fixtures, configurable controls, and user documentation. Explain old-cache limitations and managed storage behavior.
- [x] Run fmt check, Clippy all-targets with `-D warnings`, all-target tests, and the optimized build under the resource limits. Inspect terminal cleanup using only fake external helpers.
- [x] Commit `feat: add received media actions`.

## Completion

- [x] Independent final review; reproduce/fix material findings and record decisions.
- [ ] Verify and merge locally, preserve evidence, remove the feature worktree/branch, and deliver the binary and completed plan.
