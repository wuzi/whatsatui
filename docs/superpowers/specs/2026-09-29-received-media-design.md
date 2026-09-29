# Received images and documents

## Intent

The user chose receiving images/files, downloading them, and opening them in a viewer as the next useful iteration. Preserve the cyan theme, pane navigation, ordinary composition, drafts, cached search, and existing message actions. Success means a received image or document has readable metadata, can be downloaded explicitly, and can be opened in the default desktop viewer after download without holding up text messaging.

## Approach

Use the existing action menu for Download attachment and Open downloaded file, with configurable shortcuts confined to Messages and its menu. Keep `o` for web links and `y` for caption copying. Show kind, available filename, and declared size in the timeline, followed by the formatted caption. Notices report downloading, success, unavailable references, limits, and viewer failure. Download never opens a viewer automatically; Open never starts a download. Repeated downloads reuse a verified local copy. One desktop/media action runs at a time; navigation and composition remain usable.

External viewers are the first delivery: they work with the current desktop boundary and common document formats. Inline terminal graphics would add terminal-protocol and image-decoding work; outgoing attachments would add durable upload/send state. Both remain subsequent iterations. Received images and documents are supported here; video/audio/sticker messages keep their placeholders. View-once content is not cached or made downloadable.

## Data and compatibility

Add `MessageBody::Media(Attachment)` with kind, filename, MIME type, caption, declared byte length, direct path, media key, and encrypted/plaintext hashes. Capture it from live and history messages before acknowledging through the existing durable ingestion path. Require complete encrypted references with 32-byte keys/hashes, positive length, and a bounded relative CDN path. Malformed or incomplete media stays a readable placeholder. Preserve caption copying, styling, chat previews, and literal cached search.

The existing JSON message column can store the additive variant without a SQL migration. Old records still deserialize. Earlier versions discarded download metadata, so existing placeholders cannot gain downloads until fresh WhatsApp history/message data arrives. Never claim that old media can be recovered from captions.

## Download and local storage

Use the pinned `whatsapp-rust = 0.7.0` streaming downloader with persisted parameters and its default WhatsApp CDN hosts. Its session-independent downloader permits downloading references without putting work on the text-send command queue; expired CDN references may still be unavailable. No backend upgrade. Inspect the pinned source as authoritative for exact APIs; upstream reference: [whatsapp-rust](https://github.com/oxidezap/whatsapp-rust).

A separate worker process streams and decrypts into a private temporary file. The parent controls its lifetime: a 60-second deadline and explicit shutdown cancellation kill and reap it. The worker receives bounded JSON on stdin, with null terminal streams; media keys and URLs never appear in command arguments or user-facing errors. Streaming bounds memory; reject declared and actual plaintext sizes above 50 MiB, and cap encrypted response bytes at plaintext limit plus 26 bytes. Verify exact byte length and SHA-256 before publication and before opening a cached file.

Store managed downloads below `<data-dir>/media`, with directories mode 0700 and files mode 0600. Use generated names derived from full message identity and attachment fingerprint, never received filesystem paths. Filename metadata remains display-only. Use controlled extensions for recognized image, PDF, text, and office MIME types; unknown formats download as `.bin` and cannot be opened automatically by this version. Reject symlinks and nonregular managed files. Publish only complete files, using a small manifest for cleanup and reuse; remove partial files on failure/cancellation. Limit managed storage to 512 MiB and 128 attachments; a full store reports its location for manual cleanup instead of silently evicting downloads.

Re-read the full message identity before downloading, after transfer, and before opening. Refuse changed, deleted, expired, or aliased records. Purge managed copies and manifests for such records during startup/expiry maintenance. Do not delete exports outside the app's managed media directory. An activated download may finish for its original account; request/account-scoped completion notices cannot overwrite another account's notice. No automatic opening follows completion or account changes.

## Verification and delivery

Implement inline in the established isolated-worktree/local-merge workflow. All Cargo commands use `CARGO_TARGET_DIR=/home/wuzi/Projects/whatsapp-tui/target`, builds `-j 2`, and tests `--test-threads=2`. Add only a direct SHA-256 dependency already present transitively. Preserve the current Rust 1.98.0 toolchain and configured remote; do not push or publish.

Use synthetic messages, temporary SQLite/files, fake downloaders/viewers, and a demo attachment. Cover old-record compatibility; live/history metadata; view-once/incomplete references; caption search/rendering; exact bytes/hash/size; path traversal/symlinks; failed, stalled, canceled, and obsolete transfers; cached reuse/corruption; expiry/deletion cleanup; account completion isolation; configurable keys and draft preservation; and a Linux PTY download/open/quit flow with an inert viewer. Run formatting, Clippy with warnings denied, all targets, and an optimized build. Perform one independent branch review and fix reproduced findings. No automated acceptance uses a real account, WhatsApp CDN, or desktop viewer. Live compatibility remains unverified until user-operated testing.
