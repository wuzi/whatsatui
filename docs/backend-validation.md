# Native backend validation

Pinned dependency: `whatsapp-rust = 0.7.0`. Stable Rust 1.98.0, default backend features disabled (no nightly SIMD).

Automated adapter checks cover text/quote encoding, caller-assigned IDs, conservative send error classification, durability failure propagation, replay deduplication, bounded bridge backpressure, cancellation, group sender identity, captions, and supplied expiry metadata. These checks use synthetic data and local SQLite only.

On 2026-09-29, local automated checks also exercised initial unread baselines/read watermarks, receipts and edits before content, authoritative PN/LID merging, stored and visible quote expiry, stale draft writes, concurrent edits during sends, and bounded backward/forward paging during arrivals. Linux PTY tests passed for demo Ctrl-Q cleanup, controlled panic cleanup, invalid config before raw mode, and shutdown with saturated queues. The manual demo exercise covered multiline Unicode paste and resizing 80×24 → 39×11 → 120×32. These establish local behavior, not compatibility with a live account.

The implementation release gate passed: formatting, Clippy with warnings denied, automated tests across all targets (including the PTY child harness), and the optimized build. After a workstation restart, compilation was capped at two jobs and the complete gate rerun. Exit now waits for draft persistence; a failed write cancels quit. A read captured from an empty timeline cannot mark a later arrival read.

The independent branch review identified five issues, each reproduced before its fix: premature Sent status after a socket write, alias draft overwrite, buffered typing stranded by a failed initial load, shutdown behind blocked command jobs, and inaccessible long message content. The v0.1 regression suite had **86 passing test cases**. Positive/negative/reordered acknowledgements and a 30-second missing-ack deadline, both edited alias composers, initial-load retry, a full production-size command queue during draft flushing, and full-message scrolling/resizing have automated coverage. No second review pass or live-account execution is claimed.

The navigation iteration adds coverage for fuzzy and unread switching, stable selection during pending read acknowledgements, literal Unicode searches over older cached pages, alias reconciliation, current edits/deletions/expiry, bounded excerpts/results, stale asynchronous responses, and preservation of drafts and unread state when opening an older hit. Its synthetic Linux PTY smoke exercises the switcher, message finder, unread filter, and terminal restoration. One independent review found that wide characters could hide matching text in a narrow preview; a failing regression reproduced this before the fix, and matching text now stays visible after CJK text and tabs at 40x12. The navigation gate passed with **111 test cases**, formatting, Clippy with warnings denied, and an optimized build, using two build jobs and two test threads. This still does not establish live-account compatibility.

The message reading/actions iteration passed **134 test cases**, formatting, Clippy with warnings denied, and an optimized build. New checks exercise styled Unicode rendering and scrolling, original-text copying, link filtering and literal arguments, stale/expired/aliased message rejection, action-menu identity during new arrivals, one pending desktop request, account-scoped completions, helper failures/timeouts, and remapped controls. A Linux PTY test uses isolated fake `wl-copy`/`xdg-open` executables to verify the full menu → copy → link picker → open → quit flow and terminal restoration. It does not modify the real clipboard or launch a browser. Desktop integration still depends on the user's available clipboard tools and configured browser; live-account compatibility remains unverified.

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

No automated tests or CI jobs send messages to contacts. Live release acceptance remains outstanding until this table is updated with actual observed results.

The upstream durability hook runs before acknowledgement for eligible inbound messages. Event-only placeholder recoveries follow the event ingestion path; newsletters are outside scope. Disk-full failures can also prevent upstream replay buffers from being written. Local transactions and idempotent replay improve recovery; they do not establish exactly-once end-to-end delivery. Never infer server acceptance solely from a locally persisted outgoing attempt.
