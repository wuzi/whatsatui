# Native backend validation

Pinned dependency: `whatsapp-rust = 0.7.0`. Stable Rust 1.98.0, default backend features disabled (no nightly SIMD).

Automated adapter checks cover text/quote encoding, caller-assigned IDs, conservative send error classification, durability failure propagation, replay deduplication, bounded bridge backpressure, cancellation, group sender identity, captions, and supplied expiry metadata. These checks use synthetic data and local SQLite only.

On 2026-09-29, local automated checks also exercised initial unread baselines/read watermarks, receipts and edits before content, authoritative PN/LID merging, stored and visible quote expiry, stale draft writes, concurrent edits during sends, and bounded backward/forward paging during arrivals. Linux PTY tests passed for demo Ctrl-Q cleanup, controlled panic cleanup, invalid config before raw mode, and shutdown with saturated queues. The manual demo exercise covered multiline Unicode paste and resizing 80×24 → 39×11 → 120×32. These establish local behavior, not compatibility with a live account.

The implementation release gate passed: formatting, Clippy with warnings denied, automated tests across all targets (including the PTY child harness), and the optimized build. After a workstation restart, compilation was capped at two jobs and the complete gate rerun. Exit now waits for draft persistence; a failed write cancels quit. A read captured from an empty timeline cannot mark a later arrival read.

The independent branch review identified five issues, each reproduced before its fix: premature Sent status after a socket write, alias draft overwrite, buffered typing stranded by a failed initial load, shutdown behind blocked command jobs, and inaccessible long message content. The v0.1 regression suite had **86 passing test cases**. Positive/negative/reordered acknowledgements and a 30-second missing-ack deadline, both edited alias composers, initial-load retry, a full production-size command queue during draft flushing, and full-message scrolling/resizing have automated coverage. No second review pass or live-account execution is claimed.

The navigation iteration adds coverage for fuzzy and unread switching, stable selection during pending read acknowledgements, literal Unicode searches over older cached pages, alias reconciliation, current edits/deletions/expiry, bounded excerpts/results, stale asynchronous responses, and preservation of drafts and unread state when opening an older hit. Its synthetic Linux PTY smoke exercises the switcher, message finder, unread filter, and terminal restoration. One independent review found that wide characters could hide matching text in a narrow preview; a failing regression reproduced this before the fix, and matching text now stays visible after CJK text and tabs at 40x12. The navigation gate passed with **111 test cases**, formatting, Clippy with warnings denied, and an optimized build, using two build jobs and two test threads. This still does not establish live-account compatibility.

The message reading/actions iteration passed **138 test cases**, formatting, Clippy with warnings denied, and an optimized build. New checks exercise styled Unicode rendering and scrolling, original-text copying, link filtering and literal arguments, stale/expired/aliased message rejection, action-menu identity during new arrivals, one pending desktop request, account-scoped completions, helper failures/timeouts, and remapped controls. A Linux PTY test uses isolated fake `wl-copy`/`xdg-open` executables to verify the full menu → copy → link picker → open → quit flow and terminal restoration. It does not modify the real clipboard or launch a browser. Desktop integration still depends on the user's available clipboard tools and configured browser; live-account compatibility remains unverified.

Its single independent review had two axes. Standards found no structural violations and one correctness issue: links with fragments immediately after a hostname were truncated. Spec found two issues: formatting could change link destinations, and delimiter parsing could consume emoji or combining characters. Each issue was reproduced before its fix. Shared delimiter rules now preserve URL paths and complete graphemes, including quote/bullet prefixes; link discovery preserves original fragment bytes. All regressions passed, and no material finding was deferred. Already-activated desktop requests retain their original account; old completion notices remain isolated after switching accounts.

The received-media iteration passed **160 test cases**, formatting, Clippy with warnings denied, and an optimized build (two jobs/two test threads). Synthetic cases cover image/document reference persistence, old JSON compatibility, caption search/rendering, view-once rejection, encrypted streaming/decryption, exact size/hash checks, corrupt/symlinked files, private storage and capacity limits, reuse and cleanup, stale identities, worker timeout/cancellation/reaping, remapped controls, and account-scoped completions. A stalled-download runtime test verifies continued typing and draft persistence before shutdown. A Linux PTY test downloads the offline demo PNG, verifies that downloading does not open it, explicitly opens it through an isolated fake viewer, and checks terminal restoration. No real account, WhatsApp CDN, clipboard, or viewer was used by these checks.

Its single independent review found two issues, both reproduced before fixing: incoming media-caption edit envelopes left the original attachment unchanged, and manually removing a downloaded file failed to release its manifest's capacity slot. Caption edits now preserve references, including edits received before the original and older-history replay, and invalidate a transfer using the old caption. Cache maintenance and new downloads reconcile missing files before counting slots. No material finding was deferred and no second review was performed. The optimized demo also passed the complete example configuration, download/open via a fake viewer, resizing 80×24 → 40×12 → 120×32, and Ctrl-Q terminal restoration.

Review scope decisions: live CDN and actual desktop integration remain user-operated acceptance (compatibility may still differ from fixtures); same-user hostile process substitution is outside the private-permissions guarantee (another process with the same ownership can access or replace local files); sending media, inline graphics, and unsupported media kinds remain deferred (those workflows are unavailable in this increment).

Run the interactive probe yourself in a dedicated test directory:

```sh
cargo run --example backend_probe -- --data-dir /tmp/whatsapp-tui-probe
```

Scan the displayed QR from WhatsApp → Linked devices. Type `chats` for cached identities. `send CHAT_JID TEXT` and `reply CHAT_JID MESSAGE_ID SENDER_JID TEXT` transmit only in response to the command you type; use your own test conversations. `quit` flushes local state. The final application offers the same workflow through panes.

| Live exercise | Result | Reason |
| --- | --- | --- |
| Pair, clean shutdown, restore session without scan | Not run | Requires user-operated account linking |
| Direct and existing group text, both directions | Not run | Requires user-operated test conversations |
| Quoted reply, stable IDs, receipt events | Not run | Requires linked account |
| Names, initial history, live/history deduplication | Not run | Requires linked account |
| Network interruption, reconnect, uncertain send reconciliation | Not run | Requires linked account and user-operated network interruption |
| Drafts in two real chats, arrival while scrolled, resize, restart | Not run | Local fixtures passed; the live-account workflow remains user-operated |
| Receive image/document, download/open, reuse after restart, interrupted or expired-reference transfer | Not run | Synthetic encrypted fixtures and offline demo only; requires a linked account for CDN acceptance |

No automated tests or CI jobs send messages to contacts. Live release acceptance remains outstanding until this table is updated with actual observed results.

The upstream durability hook runs before acknowledgement for eligible inbound messages. Event-only placeholder recoveries follow the event ingestion path; newsletters are outside scope. Disk-full failures can also prevent upstream replay buffers from being written. Local transactions and idempotent replay improve recovery; they do not establish exactly-once end-to-end delivery. Never infer server acceptance solely from a locally persisted outgoing attempt.


## Inline media and emoji verification

The inline-media iteration passes 183 synthetic tests and Clippy with warnings denied. Tests cover sticker/view-once normalization, corrupt/oversized decoding, verified cache reuse and stale/deleted bodies, clipping/resize, Ghostty protocol selection, private immutable JPEG snapshots, persisted attachment-only drafts and restart uncertainty, newer draft preservation, image-only alias drafts, caption/quote/message-ID encoding, upload failure/account change/missing snapshots, cancellable HTTP, and grapheme-safe emoji insertion. A Linux PTY exercises Ctrl-O attachment, Ctrl-E shortcode search, image-plus-emoji sending, group receipt display, received stickers, Kitty upload/delete sequences, and terminal restoration. This verifies protocol output, not actual Ghostty pixels. No real WhatsApp account or CDN was used.

Native image uploads use asynchronous buffered HTTP, with a 60-second overall deadline and bounded native command concurrency; image decode/preparation is serialized to bound peak memory. Received previews use the existing cancellable worker and private verified cache. Prepared outgoing snapshots have a separate bounded folder and are retained for draft recovery/resending.

Its single independent final review found three Important issues and one Minor: conflicting image drafts could lose one attachment, a stale identity merge could restore a removed attachment, the new HTTP adapter restricted streamed history to 64 MiB, and an import could remain busy after an identity changed. All four were fixed. Regression checks cover distinct image/caption/reply recovery, canonical composers with unsaved edits, removal/replacement during reconciliation, swapping and sending recovered drafts, quote deletion, restart, import cancellation, and streaming beyond 64 MiB with fixed memory buffers. History retains the upstream streaming limit; preview and buffered response limits remain separate. No finding was deferred and no second review was performed.

The optimized demo also passed with the complete example configuration: attach an image with spaces in its path, insert an emoji, send and observe a demo group receipt, open a received sticker, resize 80×24 → 40×12 → 120×32, and quit with Kitty image cleanup and terminal mode restoration.

Pending live acceptance: send PNG/JPEG/WebP input to a test contact/group, confirm captions and quotes on a phone, receive static/animated stickers, inspect actual Ghostty clipping/resize/overlays, interrupt an upload, and restart with an image draft. Animated previews intentionally show a still frame; standard emoji use the terminal font.


## Clipboard images and sticker sending

The clipboard/sticker iteration passes 201 synthetic tests. Ctrl-V is covered in Linux pseudo-terminals with isolated fake `wl-paste` helpers for image bytes, a copied file URI containing spaces, and plain text. No test reads the real clipboard. Reducer checks cover caption edits during import, one pending paste, removal, account changes, cancelled sticker sends, keeping the existing composer during sticker staging, and refreshing a picker after its source conversation changes. Native transport checks verify actual WebP bytes and a StickerMessage with the correct MIME type, animation flag, dimensions, and stable message ID.

Generated static stickers use transparent 512 × 512 padding and bounded WebP encoding; valid received animations retain their bytes. Snapshot verification, account-scoped/deduplicated recent stickers, draft persistence, and restart uncertainty are tested. A new normalizer regression reproduces phone-origin images becoming placeholders when a complete media reference uses `/o1/v/`; these paths are now accepted alongside `/v/` while traversal and fragment rejection remain in place. Old placeholders have no recoverable media key/path in the app record and require a history replay or resend.

Remaining user-operated acceptance: receive a newly sent phone image, paste a real desktop image with a caption, send/reuse a static and animated sticker to a test conversation, and inspect actual Ghostty pixels. Automated checks use synthetic data and never send to a WhatsApp account. The picker uses cached conversation stickers; syncing phone favorites and packs remains outside this iteration.


Its single independent review found two Important issues and one Minor, all reproduced and fixed: pre-staging send failures could discard a pasted sticker, a new copy of a selected sticker could move the selection to different content, and a successful refresh could leave a stale load error. The picker now retains its selection until durable staging succeeds, keeps it available after preparation/storage failures, and follows the selected content hash across refreshes. Esc can cancel preparation before storage starts; an in-flight storage transaction finishes before the picker closes. Tests cover both failure paths, retry without modifying the composer, content-preserving reorder, error recovery, and cancellation before staging. No finding was deferred and no second review was performed.


The final optimized binary passed with the complete example configuration and an isolated fake clipboard helper: image paste with caption, reuse of a received animated sticker, creation and sending of a pasted sticker, retained composer text, and picker resize 80×24 → 40×12 → 120×32. Kitty protocol output/cleanup and terminal-mode restoration passed. Formatting and Clippy with warnings denied also pass. These checks validate terminal protocol output, not actual Ghostty pixels or live WhatsApp delivery.


## Conversation layout, profile photos, and mouse

The conversation UX iteration passes **223 synthetic tests**, formatting, Clippy with warnings denied, and an optimized build with two build jobs and two test threads. Checks cover oversized static sticker optimization and explicit prepared-preview confirmation, preserving received animation, bounded account-scoped avatar caching and privacy/missing-photo invalidation, sender grouping, own-message styling, independent selection and scrolling, stable popup offsets, exact message hit targets, grapheme-safe composer clicks, resize invalidation, and mouse capture restoration.

The optimized demo passed with the complete example configuration: Ghostty graphics protocol output, mouse Help/close, Unicode send and automatic reply, message actions, keyboard navigation, resizing 80×24 → 40×12 → 120×32, a received sticker, and terminal/Kitty cleanup. Automated avatar tests use fake providers and decoded synthetic images; the demo uses initials. No real profile photos, clipboard contents, account, or live sends were accessed.

User-operated acceptance: check direct/group participant photos and private-photo fallback, own-message contrast, mouse selection in Ghostty, scrolling during incoming messages, and resending an oversized static sticker after reviewing the prepared preview. Reactions and outgoing message edits remain the next feature iteration.

Its single independent review found hidden-item mouse targets, missing visual CRLF line breaks, and sender names obscuring timestamps in narrow terminals. Each was reproduced with a failing regression and fixed; unused list rows are inert, CRLF retains correct rows and byte-based caret positions, and long Unicode names shorten to reserve time and edit metadata. All 223 tests pass after the fixes. No finding was deferred and no second review was performed.


## Sidebar profile photos

The user confirmed conversation profile photos looked good in their Ghostty session. The sidebar now reuses those account-scoped photos and initials, including group photos, without changing its two-line rows. Long names shorten to preserve unread counts and draft indicators.

The sidebar iteration passes **225 synthetic tests**, formatting and Clippy with warnings denied. New checks cover actual decoded sidebar pixels, group identity, fetching only complete visible rows across scrolling, avatar click targets, hidden/disabled-photo cleanup, and badge visibility with long names. One independent review found no actionable issues. Automated checks use synthetic photos and demo data; no live profile fetches or messages were performed by the agent.

The optimized demo also passed clicking a sidebar avatar to switch conversations, opening message actions, resizing 80×24 → 40×13 → 120×32, mouse Help/close, and terminal restoration. The smoke harness waits for each resized frame before sending its next action.
