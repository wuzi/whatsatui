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


## Reactions, sent-text editing, and media replies

The local suite now passes **257 tests**. New coverage includes live and historical reaction normalization, sender-relative target ownership, replacement/removal ordering, pre-original delivery, PN/LID merges, account isolation, expiry/deletion and migration of a version-2 database. Outgoing mutations are committed before the transport runs, wait for a matching server acknowledgement, preserve the original on rejection/timeout, and recover interrupted work without automatic resend.

UI checks cover stable reaction targets during arrivals, counts and participant lists, reaction-row mouse targets at 40×16 and 120×35, edit save/cancel/failure, concurrent edits and old-account responses, unchanged normal drafts, remapped controls, and printable composer keys. Media replies have typed quote bodies and bounded previews. Quote jumps load cached pages by identity, ignore stale user navigation, and survive incoming messages during the lookup. Full live-service acceptance remains user-operated; these tests never send through a linked account.

One independent review found three Important issues: encrypted reaction failures could become unread chat rows, backend termination could leave an edit locked as pending, and a delayed clipboard read could enter an edit opened after switching panes. Each was reproduced with a failing regression and fixed. Encrypted reaction envelopes now stay out of the timeline, stopped actions become Unconfirmed while preserving proposed text, and opening an edit invalidates the normal composer's pending paste. No findings were deferred and no second review was performed. The full suite, formatting, and Clippy with warnings denied pass after these fixes.

The optimized demo passed with the complete example configuration: reaction participant list, add/change/remove, sending and editing text while preserving a separate draft, replying to an image, keyboard and mouse jumps to its cached original, resizing 120×34 → 40×13 → 120×32, Help/close, and terminal/mouse restoration on exit. This smoke run disabled inline graphics and avatars to inspect text and hit targets; existing synthetic graphics tests remain in the full suite. Its first attempt matched an unrelated sidebar preview before the active conversation's reply arrived; the harness now waits for the full reply in the conversation before navigating. The original trace and successful trace are retained. No linked account or real clipboard was accessed.

Remaining user-operated acceptance: receive and change reactions from a phone in direct/group chats, edit recently sent text and confirm it on another device, and follow a quoted image/sticker/document in Ghostty. Automated verification does not establish live WhatsApp acceptance.

## Received audio playback

The audio iteration passes **278 automated tests**, plus an explicit installed-mpv test normally excluded from the portable suite. Synthetic cases cover voice/non-voice metadata, view-once exclusion, Audio media-key decryption, old attachment JSON, verified cache reuse, snapshot lifetime, corruption, cancellation, expiry/deletion, missing or stalled players, malformed IPC, crashes, EOF, replacement, and process reaping. Reducer/rendering checks cover stale observations, rapid controls, account changes, playback across chats, captured menu targets, unchanged drafts, remapped keys, right-click actions, and clipped/covered/stale mouse targets.

The host's mpv 0.41.0 decoded an eight-second synthetic Opus fixture using `--ao=null`; pause/resume, 2× speed, observed position/duration, and EOF passed. This tests the real codec and IPC without playing through speakers. It does not verify the user's selected output device or live WhatsApp CDN. No linked account, real voice recording, or clipboard was accessed.

One independent review found three Important issues. Canceling a queued file copy could recreate a temporary file after its owner was dropped; a post-start IPC stall had no health deadline; progress observations cleared unrelated double-click history. All three were reproduced before fixing. The copy operation now owns its open temporary file through completion, paused and playing sessions receive bounded health queries, and playback-only observations preserve click history. An additional regression verifies normal EOF while a health query is pending. No findings were deferred and no second review was performed.

The optimized demo passed real mpv playback with null output, observed progress, keyboard pause/resume/speed/stop, mouse speed/stop, preserved drafts while switching chats, resizing 120×34 → 40×13 → 120×32, Help/close, child reaping on stop/quit, and terminal/mouse restoration. Inline graphics and avatars were disabled in this text/control smoke; the existing graphics tests remain in the suite.

Playback needs a complete audio reference from a fresh receive/history replay; old `[audio]` placeholders cannot be reconstructed locally. User-operated acceptance remains: receive a fresh voice note and audio file, listen through the normal output device, pause/resume and change speed, switch chats, then quit during playback. Audio sent as a generic document retains download/open actions; recording, sending audio, and seeking are outside this release.

## Composer shortcuts

The composer iteration passes **287 automated tests** (the native mpv test remains opt-in). New checks reproduce and verify Shift-Enter inserting at a Unicode caret without sending; `i` focusing Composer from Chats/Messages while remaining text inside editors; Ctrl-C clearing and saving text without losing its reply; cancellation of delayed clipboard input; clearing a draft before its initial load finishes; and isolation/locking of sent-message edits. Configuration tests cover remapping, legacy `i` reactions and Ctrl-C quit overrides, and rejected explicit collisions.

A Linux PTY sends modified Enter and Ctrl-C through the actual Crossterm parser, checks that the app stays open after clearing, and verifies keyboard-protocol restoration on normal exit and panic. Formatting and Clippy with warnings denied pass. All checks use synthetic/offline data and never access a linked account or real clipboard.

One independent review found duplicate keyboard-mode restoration during panic unwinding, changed interpretation of shifted custom shortcuts under CSI-u, and blocked local text controls while runtime jobs are saturated. All three were reproduced before fixing. Cleanup now claims restoration once; alternate-key reporting and ASCII fallback normalization preserve shifted bindings; clear/newline remain available during backpressure. Regressions cover legacy and enhanced shifted letters and symbols through the real parser. No findings were deferred and no second review was performed.

The optimized demo passed an offline PTY smoke for focusing from both panes, modified Enter without premature submission, multiline send, legacy/enhanced Ctrl-C, `i` as text, relocated reaction details, resizing 120×34 → 40×13 → 120×32, Help/close, enhanced Ctrl-Q, and keyboard/mouse/terminal restoration. Text-only rendering isolates these controls; existing graphics tests still run in the suite. Physical key routing in the user's Ghostty session remains user-operated acceptance.

## Avatar refresh stability

The refresh fix passes **294 automated tests** (one native mpv test remains opt-in), formatting and Clippy with warnings denied. Regressions reproduced avatars turning into initials during the renderer's periodic freshness check and after a failed refresh. Photos now remain visible while refreshing; unchanged thumbnail pixels reuse the terminal protocol and Kitty image ID. Changed and confirmed missing/private photos still replace or remove the previous image, and hiding a photo cancels refresh and releases its terminal image.

Six new renderer tests cover pending, unchanged (disk and network), changed, failed/retried, missing, hidden, and failed-persistence refreshes using synthetic photos. Both Kitty output and half-block pixels are checked. Tests advance private refresh deadlines without a production test API or real account access; the full suite retains existing sidebar, avatar-cache, graphics and terminal-cleanup checks.

One independent review found that failing to save a confirmed missing/private response could retain the old photo. A deterministic persistence-failure regression reproduced this. Confirmed removals now reach the renderer even if saving fails; obsolete files are removed when possible. If invalidation also fails, expired-photo disk fallback is disabled for that Cache lifetime so a later offline retry cannot revive the removed photo. A cache regression verifies this behavior and eventual positive recovery. No findings were deferred and no second review was performed.
