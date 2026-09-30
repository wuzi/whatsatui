# Inline media and emoji implementation plan

**Goal:** Deliver Concord-style image/sticker viewing, image sending, and emoji picking within WhatsAppTUI.

**Architecture:** Reuse verified received-media caching for preview loads. Keep image decoding and protocol preparation outside the reducer and use a shared timeline row map for text and clipped graphics. Persist immutable local image references in drafts and outgoing records; use the native upload API before sending an image protobuf.

**Tech stack:** Rust, Ratatui 0.30, ratatui-image 11.1, image 0.25, emojis 0.9, whatsapp-rust 0.7.0.

**Spec:** `docs/superpowers/specs/2026-09-29-inline-media-design.md`

## Global constraints

- Inline implementation in the existing isolated worktree; one final review, local integration, no publication or real-account sends.
- Cargo target `/home/wuzi/Projects/whatsapp-tui/target`, jobs 2, test threads 2, no simultaneous Cargo commands.
- 16 MiB preview/input limit; 16 megapixel decode limit; 64 MiB decoder allocation limit; bounded preview cache and concurrency.
- Preserve existing schema compatibility, account isolation, send uncertainty, terminal cleanup, and pane controls.

## Review focus

1. Chat/account switches and deleted/expired messages while media is loading must not expose stale pixels.
2. Partial images at the viewport edge and overlays must not overwrite text or leave graphics behind.
3. Corrupt files, huge dimensions, changed source files, and missing local snapshots must fail visibly without sending text in place of an image.
4. Draft edits during attachment import, staging, or upload must survive; restart must not replay an uncertain send.
5. Emoji with multiple code points must insert at the caret and remain editable without corrupting text.

## Task 1: Verified previews and local image snapshots

Files: `src/media/{model,mod,worker,preview,outgoing}.rs`, `src/whatsapp/media.rs`, Cargo manifests, media tests/fixtures.

Interfaces: `AttachmentKind::Sticker`; `preview::load(message, store, downloader, cancel)` returns bounded decoded image; `outgoing::import(path, data_dir)` returns serializable `LocalImage`; `outgoing::read(image, data_dir)` verifies immutable stored bytes.

- [x] Write and run failing tests for sticker/view-once extraction, bounded image decoding, corrupt data, immutable snapshot import and validation.
- [x] Implement metadata, codecs, verified cached preview loading, and private content-addressed JPEG snapshots.
- [x] Run targeted tests and full suite; commit.

## Task 2: Inline timeline and composer graphics

Files: `src/ui/{timeline,images,mod}.rs`, `src/runtime.rs`, config, demo, render/runtime tests.

Interfaces: `ui::Images` owns bounded prepared protocols and pending/failed identities; `Screen` owns preview scheduling/results; shared timeline rows reserve and clip image slots consistently with viewport calculations. Preserve public stateless test rendering entrypoint.

- [x] Write and run failing tests for reserved rows, clipping, stale-result eviction, disabled previews, and renderer selection.
- [x] Implement asynchronous preview preparation, Ghostty Kitty/half-block selection, clipping, overlays, resize, demo sticker and config.
- [x] Run targeted tests and full suite; commit.

## Task 3: Durable image attachment and sending

Files: app models/reducer/composer overlays, storage worker, native/demo backend and encoder, usage/config examples, integration tests.

Interfaces: additive `Draft.attachment: Option<LocalImage>`; local image message body includes caption; import result carries request/account/chat/draft identity; outbound pipeline snapshots current draft, stages, uploads, then sends using original message key and quote.

- [x] Write and run failing tests for path dialog, cancel/remove, stale import, attachment-only send, restart/staging/newer draft preservation, and image protobuf encoding.
- [x] Implement dialog, durable snapshot use, composer preview, upload deadline/cancellation and explicit resend behavior.
- [x] Run targeted tests and full suite; commit.

## Task 4: Emoji picker and delivery verification

Files: `src/app/emoji.rs`, reducer/bindings, composer popup UI, demo/usage docs, UI and PTY tests.

Interfaces: searchable offline emoji results, modal editor + selected index, insert selected Unicode sequence at current composer caret and mark draft dirty.

- [x] Write and run failing tests for shortcode/name search, grapheme-safe insertion/cancel, literal typing, and no send on selection.
- [x] Implement picker, discoverable hints/help, complete docs, and exercise demo flows in a PTY.
- [x] Run fmt, Clippy all-targets with warnings denied, full tests, and release build serially. Review the complete diff, fix material findings with regression tests, integrate locally, preserve evidence.
