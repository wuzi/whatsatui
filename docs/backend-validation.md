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

The final optimized binary passed a real PTY run with four synthetic avatars seeded only into the demo's temporary cache. Across 67 idle seconds, including the 60-second refresh deadline, it emitted no avatar retransmissions or deletions. Opening Help then deleted the four images, and quitting restored keyboard, mouse and terminal state. No real profile photos, account, clipboard, or messages were accessed. This verifies the terminal command stream; physical Ghostty appearance remains user-operated acceptance.

## Animated stickers and received video

The media-motion iteration passes **310 automated tests**, formatting and Clippy with warnings denied. Two opt-in tests also pass against installed mpv 0.41.0 using null outputs: Opus audio and a synthetic H.264/AAC MP4, including progress/duration, pause/resume, speed and EOF. No display window or speaker output is used in those codec tests.

New regressions cover composed WebP frames and timing, bounded frame sampling, oversized-duration still fallback, cancellation, all-frame Kitty cleanup, off-screen redraw suspension, and local/sent animated previews. Video checks cover live/history normalization, view-once and invalid-reference exclusions, Video media-key decryption, caption edits/quotes, caption rendering and narrow mouse/key controls, GUI-only mpv output arguments, normal window closure, repeated desired-state commands, and native pause/speed reconciliation. Existing avatar, static-image, playback lifecycle and terminal restoration tests remain green.

Old video placeholders cannot recover missing media keys locally; fresh delivery or history replay is required. Automated verification does not establish physical Ghostty animation appearance, normal video-window/audio output on this desktop, or live WhatsApp CDN behavior.

The single final independent review identified three issues; all were reproduced and fixed with regression tests. WebP preflight now verifies embedded bitstream dimensions (including ALPH + VP8) before animation decoding and validates the still fallback’s first frame. Control revisions prevent native pause observations from being reused by rapid TUI toggles. Observed or queued normal completion ends a control batch without further writes to a closed mpv socket. No review finding was deferred.

The final optimized binary passed a synthetic Ghostty-protocol PTY check: two initial animation frames uploaded once and reused across loops; hiding, restoring and resizing used eight frame IDs, all explicitly cleaned up. Keyboard video launch, pause/speed/resume/stop, native mpv child reaping, private snapshot deletion and terminal restoration passed with null video/audio outputs. Source ea97c3f was fast-forwarded to local main; release SHA256 is `0170bec80906a0f0c0427c5cd3def73dded63f304669e313c4756a28e7186c77`. Verification artifacts were preserved and byte-checked before the owned worktree/branch were removed; nothing was pushed.


## Silent desktop notifications

The notification iteration passes **336 automated tests**, formatting and Clippy with warnings denied. Storage checks prove first live insertion versus history/replay/own traffic, transaction rollback/retry, canonical sender identity, edits/reactions/deletions and read watermarks. Reducer checks cover focus, scrollback, pending cancellation, account changes, old messages, bursts, bounded queues, privacy, failure cooldown and shutdown. Runtime checks cover content revalidation and cancellation before/during desktop delivery. The backend probe continues to compile without producing popups.

Native helper tests use isolated scripts for literal arguments, bounded Unicode, escaped body markup, missing helpers, failures and timeouts. An additional opt-in test runs the installed notify-send against a synthetic service on a **private D-Bus session**. It verifies the application name, body escaping, normal urgency, im.received category and suppress-sound hint through the actual wire call. No real desktop notification, clipboard, WhatsApp account or message send was involved. Two existing native mpv tests remain opt-in and were not repeated for this notification-only change.

The first full regression run caught queued keyboard input overriding explicit FocusLost and marking messages read; read_requires_visible_bottom_and_foreground reproduced it and passes after fixing precedence. Keyboard/mouse input now establishes initial focus only; known focus loss remains authoritative until FocusGained. A compile check also caught the backend probe's exhaustive event match, which now explicitly ignores the new popup-candidate event.

Actual popup appearance and desktop Do Not Disturb behavior remain user-operated acceptance. Notifications require a running TUI and desktop session; clicking a popup does not focus a chat and WhatsApp mute settings are not synchronized.

The single independent review identified two Important findings, both fixed with regression tests: overflow lost evidence of later chats after earlier candidates were read/deleted, and queued notification effects could retain stale account/focus/privacy. Overflow now carries a bounded account/timestamp window, checked against unread storage separately. A live context channel invalidates preparation and cancels stale delivery; canonical chat IDs are checked inside storage, and an already-submitted popup is never resubmitted after context changes. The full suite passes 336 tests after these fixes. No review findings were deferred and no second review was dispatched.

Overflow deliberately favors a generic alert over silence: unread messages already notified within the same timestamp window may be included in this fallback. The normal non-overflow path remains keyed to first committed live inserts. Actual desktop appearance and daemon enforcement of silence/Do Not Disturb remain unverified by the synthetic bus test; the documented first-version exclusions are unchanged.

Source c501c8a was fast-forwarded into local main and the optimized release rebuilt; SHA256 is `c2ff692fca931e3753ce3139c24cb5aa98151622f3c134dbc95ebd5d5eccaab9`. The merged code matches the tested source, and the binary reports whatsapp-tui 0.1.0. Thirteen verification artifacts were preserved and hash-checked before cleanup of the owned worktree/branch. Nothing was pushed.

## Timestamp and stray-digit redraws

The rendering correction passes **340 automated tests**; three native mpv/desktop-helper tests remain opt-in. Two regressions reproduced shifted timestamps and stray digits after scrolling sender headers containing a heart emoji. Both pass after treating wide VS16 emoji as complete glyphs during buffer diffing. This is a display correction; stored timestamps and messages are unchanged.

The new tests replay actual Crossterm output across successive synthetic conversation frames, checking cursor placement rather than only the desired Ratatui buffer. Coverage includes incoming-message growth, scrollback, clearing, narrow layouts, sidebar and message emoji, styled text, other wide glyphs, Help overlays, and replacement with ASCII text. A graphics regression confirms that finalizing emoji does not alter Kitty image uploads or placeholders and retains one-time transmission. Existing avatar refresh, animation, image cleanup and terminal-restoration tests also pass.

The test replay handles cursor positioning, text, styling sequences and virtual Kitty transmissions; it is not a full terminal emulator or a physical Ghostty visual check. No live account, private messages or profile photos were accessed. Restart the TUI to load the correction and clear artifacts left by the old renderer.

Formatting and Clippy with warnings denied pass. The single independent review verified the cause against the pinned dependency code and found no issues; no findings were deferred.

The optimized release was built from reviewed source b4a8d9f. Its SHA256 is `22d67a5a128191b06c01fbbde629be93e794c425e78dea7918f8a76f5af0afc7`. The fix and release record are integrated into local main; verification evidence is preserved under `.superpowers/sdd/2026-10-01-render-artifacts/`. Nothing was pushed.

## Compact message timestamps

The timestamp-grouping iteration passes **347 automated tests** (three native helper checks remain opt-in). Consecutive messages from the same sender in one minute share a displayed time. Empty timestamp-only rows are removed; delivery states, edit markers and group receipt counts remain visible. New minutes, sender changes, date boundaries and out-of-order timestamps retain their own times.

Regression checks cover adjacent body rows, avatar-enabled/disabled layouts, per-message keyboard selection and mouse actions, and sender/time context while scrolling a compact group. The viewport preserves the latest body and reports only fully displayed messages. Synthetic Crossterm-output replay also checks compact emoji conversations for timestamp fragments. Existing quote, media, avatar and terminal-restoration tests pass. No live account or private messages were used.

The single independent review identified one Important issue: scrolling from compact bodies onto a header could leave a blank bottom row. A failing regression reproduced it before the fix. Reserved context space now stays above the content at header boundaries, preserving the newest body's position and mouse target. The regression covers minute, edit-metadata and sender transitions. No findings were deferred and no second review was performed.

Formatting, Clippy with warnings denied, and whitespace checks pass after the review fix. Physical Ghostty appearance remains user-operated acceptance; automated checks use synthetic buffers and emitted terminal commands.

The optimized release was built from corrected source 2117153 and reports whatsapp-tui 0.1.0. Its SHA256 is `b9a61eac3134adbe33b4db2e1a821edd211f7c8a63b37432c4fc33a049e84cf1`. Verification evidence is preserved under `.superpowers/sdd/2026-10-01-compact-timestamps/` during local integration; nothing is pushed.

## Ten-minute sender blocks

The grouping follow-up passes **347 automated tests** (three native helper checks remain opt-in). Both sender blocks and timestamp suppression now use a gap of less than ten minutes between consecutive messages. Exact ten-minute gaps, sender changes, local midnight and out-of-order messages start new headers. Continuous runs can span more than ten minutes, with context restored while scrolling.

Existing timestamp regressions now cover gaps across minutes, the previous five-minute limit, just under/exactly ten minutes, metadata, and longer scrolled runs. Three cases failed against the old behavior before the implementation changed. The full suite retains keyboard/mouse, media, avatar and emitted-terminal-output coverage. Verification used only synthetic/offline data.

Formatting and Clippy with warnings denied pass. The single independent review found no issues. Physical Ghostty appearance remains user-operated acceptance. The optimized release was built from reviewed source 24f993b and reports whatsapp-tui 0.1.0; SHA256 is `d6ad24b7d74143b5b47559e7879ea44ede7154780e04abad0192131a8fc8674b`. Evidence is preserved under `.superpowers/sdd/2026-10-01-relaxed-timestamps/` during local integration; nothing is pushed.

## Inline send-status marks

The inline status iteration passes **353 automated tests** (three native helper checks remain opt-in). Outgoing messages display a small status mark after their text or media caption/label. Sent and delivered use muted checks; read uses the accent color. Sending, failed and unconfirmed remain distinct. Full rows wrap the mark without clipping text or covering quotes/previews. Group recipient counts and edit markers remain explicit, and Help includes a colored legend. Backend send/receipt semantics are unchanged.

New regressions cover all six states, colors, incoming-message exclusion, Unicode and formatting, narrow wrapping, local/received media with empty/nonempty captions, preview offsets and quote/mouse targets. Existing selection tests now exercise outgoing compact blocks too. Actual Crossterm-output replay checks status changes alongside emoji during scrolling. Initial inline-state and Help cases failed against the old renderer before passing with the implementation. Verification used only synthetic/offline data.

Formatting and Clippy with warnings denied pass. The single independent review found no issues. Physical Ghostty glyph appearance remains user-operated acceptance. The optimized release was built from reviewed source 0557f1f and reports whatsapp-tui 0.1.0; SHA256 is `dd86df566168548fe3d88e844764dfba4ca5981106bd61712afc4099cc13222a`. Evidence is preserved under `.superpowers/sdd/2026-10-01-inline-send-status/` during local integration; nothing is pushed.

## Download helper after an application update

The worker-launch correction passes **356 automated tests** (three native helper checks remain opt-in). The user confirmed that the TUI remained open during a release rebuild. Linux's resolved current-executable path then referred to a deleted file, preventing the separate media helper from starting. The Linux downloader now executes the unresolved `/proc/self/exe` link, preserving the running executable across replacement or removal. Other targets retain their existing lookup. Spawn failures include the OS error and a restart suggestion.

A synthetic Rust probe reproduced the missing-path error. The regression then reproduced it through NativeDownloader in a running copy of the real libtest executable; unchanged, replaced and removed pathnames pass with the fix. The reexecuted test binary rejects the internal worker argument, proving process launch without a network request or account access. Separate checks cover missing/non-executable workers, private-parameter omission, stdin transport, cancellation, timeout and reaping. Existing synthetic media verification/decryption/cache tests pass. No live WhatsApp data was accessed.

Formatting and Clippy with warnings denied pass. The single independent review found no issues. The optimized release was built from reviewed source ce3ac293 and reports whatsapp-tui 0.1.0; SHA256 is `a55aa398da9595534f844b0db6f2b78d61df7a309e1900a61fffa1ea8c01d82c`. Evidence is preserved under `.superpowers/sdd/2026-10-01-download-worker-executable/` during local integration; nothing is pushed. Restart the TUI to load the correction. Live WhatsApp downloads remain user-operated acceptance.

## Muted conversation notifications

Mute synchronization and filtering pass **365 automated tests** (three native helper checks remain opt-in). The original notification version omitted mute events and storage entirely. The native adapter now consumes MuteUpdate and history mute metadata, stores settings per account and canonical chat, and requests a regular_high snapshot on the first connection each launch to backfill previously ignored settings. Permanent and temporary mutes suppress both ordinary and generic overflow popups; unmuted/expired chats can notify again. Muted content, names and counts are excluded from mixed popups.

The nine new tests cover native event subscription/handling, millisecond expiry and indefinite values, direct/group chats, privacy modes, mixed bursts, overflow, persistence across reopening, metadata refresh, stale updates and alias/account separation. A runtime regression proves committed mute changes cancel an in-progress helper without resubmission. Each new behavior was reproduced failing before its correction. The tests use synthetic stores and injected desktop helpers; no live account, private message, clipboard or real popup was accessed.

Existing JSON chat records default to unknown mute state without a schema migration. History seeds do not replace timestamped app-state updates. Committed mute changes invalidate notification preparation; desktop popups already shown cannot be retracted. Existing settings on the first upgraded launch depend on the server's settings sync arriving. The upstream snapshot API owns retries and does not expose a completion result; real sync timing remains user-operated acceptance. Mute controls remain in WhatsApp.

Formatting and Clippy with warnings denied pass. The single independent review found no issues. The optimized release was built from reviewed source bfd8cd3 and reports whatsapp-tui 0.1.0; SHA256 is `501df8326ed3c6394ce436187aa73ac3c0c109eb918807e8453dd341d0939753`. Evidence is preserved under `.superpowers/sdd/2026-10-01-muted-notifications/` during local integration; nothing is pushed.

## Ctrl-Q shutdown with retained notification senders

The shutdown correction passes **368 automated tests** (three native helper checks remain opt-in). The user reported a frozen WhatsApp screen after Ctrl-Q. Avatars retain the native client, whose durability hook retains a notification sender. Runtime waited for natural event-channel closure after the backend had already stopped, while the avatar service remained alive until runtime returned. This circular lifetime dependency was introduced when notification delivery added the retained sender.

Runtime now closes the receiving side after the backend shutdown task resolves, then drains buffered events and their storage effects before final recovery, flush and terminal restoration. It continues accepting final updates while the backend is stopping, including after an advisory Stopped event. Backend errors still return after cleanup. No force-kill or production timeout was introduced.

Three new regressions failed before the correction and pass afterward. Synthetic runtime checks hold the sender alive through successful and failed shutdown, save a dirty draft, and preserve eight final send outcomes over a capacity-two event channel. A PTY child runs the real screen/input loop with an avatar provider retaining the sender, sends legacy and Kitty-protocol Ctrl-Q, and verifies restored terminal settings, alternate screen, cursor and input modes. Existing saturated-shutdown, media cancellation and draft-save-failure checks pass. No live account, private message, clipboard or real notification was accessed; physical Ghostty/live-session acceptance remains user-operated.

Formatting and Clippy with warnings denied pass. The single independent review found no issues. The optimized release was built from reviewed source 9dec385 and reports whatsapp-tui 0.1.0; SHA256 is `7b92f9ee05a3fadf754d534faa06997c22f2127f0e29f0734f04a0b943ddf951`. Evidence is preserved under `.superpowers/sdd/2026-10-01-quit-shutdown/` during local integration; nothing is pushed. An already-frozen session needs to be closed once and relaunched to load the fix.

## GNOME notification grouping

The grouping correction passes **367 automated tests** (three native checks remain opt-in). The native notification transport is now covered by one content test and an expanded private-bus integration check, replacing the two obsolete helper-process tests. GNOME 50.5's installed source confirmed that unregistered applications are grouped by sender PID and application name; a new notify-send process for each popup gave every alert its own source. The private-bus regression reproduced two different sender PIDs before the change.

The runtime now owns one lazy, shared D-Bus connection using pinned zbus 5.19.0 with the Tokio backend. Notification effects reuse it, and the three-second deadline covers both connection setup and delivery. Cancellation never resubmits a possibly displayed notification; a closed connection reconnects on the next new alert. Existing locked dependency versions, silence, normal urgency, markup escaping, content limits, mute/focus/privacy filtering and batching are preserved. No desktop file or additional helper installation is needed for grouping during the running instance.

The private service observed eight synthetic calls covering distinct conversations, one live connection, service failure, timeout, cancellation and recovery after disconnect, with no duplicate retries. It also checked the wire payload, application name, icon, category, urgency and suppress-sound hint. Run `python3 tests/notification_bus.py` with D-Bus tools and PyGObject installed to repeat it; an optional `--target-dir` selects a shared Cargo target. This harness uses its own session bus and never sends real desktop popups. No WhatsApp data or live account was accessed.

Physical GNOME appearance remains user-operated acceptance. Restart the TUI and dismiss the old separate notifications after upgrading. Grouping is per running instance; notifications retained from earlier launches may remain separate. Clicking notifications still does not navigate to a conversation.

Formatting, whitespace checks and Clippy with warnings denied pass. The final portable private-bus harness also passes, including its existing-service ownership guard. The single independent review found no issues. The optimized release was built from reviewed source 26bc6cb and reports whatsapp-tui 0.1.0; SHA256 is `f53ac833427dbfb5f59eaaa61b0bfbc19a3f08801431e8a1b2be0258588630db`. Evidence is preserved under `.superpowers/sdd/2026-10-01-notification-grouping/` during local integration; nothing is pushed.

## Public screenshot demo

The expanded `--demo` opens a fictional English group conversation and populates 22 chats. Nine generated portraits, a café photo, and a trail photo are bundled with documented prompts under `assets/demo/`. The avatar provider serves only the synthetic account and known fixture identities. Existing temporary storage, default demo settings, disabled desktop notifications, and separation from the native WhatsApp backend are preserved. No private contacts, messages, account data, clipboard, or reference screenshots were used to populate the scene.

The new avatar regression failed before implementation and now exercises all eleven distinct assets through the real decoder/cache, including wrong-account and unknown-identity misses. Existing PTY media checks now locate the photo through message search and verify its exact saved JPEG bytes; other terminal controls and synthetic media examples remain covered. **368 automated tests pass**, with three existing opt-in native checks ignored. Formatting, whitespace checks and Clippy with warnings denied pass. The full suite needed approved localhost socket access for its synthetic HTTP tests.

The optimized 160×48 PTY demo shows the café photo, reply, reactions, populated sidebar, and seventeen avatar uploads, then restores the terminal on Ctrl-Q. This verifies layout and graphics protocol output; physical Ghostty pixels remain user-operated. The single independent final review found no issues and inspected all eleven images, including their lack of private metadata. Source `7d04ed6` produced whatsapp-tui 0.1.0 with SHA256 `030ebfbe6c5915dfbc7a809f8ef9c57a5b0ea52f7e617bd983a93d88faae3d2b`. Evidence is preserved under `.superpowers/sdd/2026-10-01-screenshot-demo/`; nothing is pushed.
