# Finding conversations and messages

## Intent and recommendation

The user wants to improve daily WhatsAppTUI use by learning from Concord and selected **fast navigation and finding conversations/messages** as the priority. Preserve the cyan theme, Tab between panes, j/k in lists, and ordinary typing in the composer.

Concord documents a fuzzy channel switcher, message search, and an unread inbox in [its feature overview](https://github.com/chojs23/concord#features) (reviewed 2026-09-29). Apply those interaction ideas to WhatsApp's existing chat model. Implement independently in this Rust application.

Three possible iterations: navigation/search; richer message formatting and actions; media previews. Implement navigation/search first because it improves every text conversation and works offline. Formatting, clipboard/link actions, notifications, and media remain subsequent iterations. Live pairing/session/messaging acceptance is still a separate user-operated check.

## Chat switcher

- Keep Ctrl-P from all three panes and `/` from Chats/Messages. Match names, phone numbers, and known identifiers with Unicode lowercase comparison and ranked subsequence matching. Exact, prefix, and contiguous matches rank above scattered characters; preserve existing recency order for ties and empty queries. Multiword queries match each token.
- `u` in Chats opens the switcher in unread-only mode. Ctrl-U within the switcher toggles All/Unread, retaining the query. Show unread counts, group/direct labels, and draft badges. Reflect unsaved local drafts as well as persisted ones.
- Arrow keys select, Enter opens the conversation/composer, Esc restores the previous pane and untouched draft. Keys remain configurable; no printable shortcuts intercept text in either search editor or the composer.
- Preserve highlighted conversation identity if a background list update changes the ordering; clamp safely when it disappears. Empty results have an explicit message. Show the current result count and effective keys. Search inputs are single-line and bounded to 256 Unicode scalar values.

## Search this conversation

- Ctrl-F from any main pane opens a separate message finder for the current conversation, retaining the previous pane and composer. With no conversation, display a short notice.
- Type a literal phrase, then Enter to search all locally cached text and media captions in that conversation. Search is explicitly submitted, avoiding a full history scan on each keystroke. Match using Unicode lowercase (no accent folding or linguistic stemming); `%`, `_`, quotes, and backslashes are literal.
- Return the most recent 50 matches, ordered by timestamp and complete message identity. Fetch one additional row to report truncation; tell the user to refine the query. No database migration or additional dependency is needed. Search executes on the existing storage worker and uses the account/chat timeline index to scope the scan.
- Results display date/time, sender, and a bounded excerpt around the first match. Shorten preceding context to fit the terminal's display width, keeping the start of the match visible even after wide characters or expanded tabs. Up/Down select a result; Enter loads its bounded timeline page and focuses Messages. Esc closes without moving the timeline. End in Messages still returns to the newest page.
- Editing a submitted query clears its results and invalidates the pending request. Repeated Enter while loading submits nothing. Errors keep the query available for retry. Loading and empty states are explicit; only one query for this overlay is outstanding at a time.
- Bind results to account, chat, request ID, and query. Late responses after edits, cancel, reopen, account changes, or identity merges cannot update a new search or navigate elsewhere. Changes to the searched conversation invalidate results immediately and require a fresh submission. Revalidate selected results through the existing snapshot loader; removed/expired messages must never be reconstructed from excerpts.
- Deleted/expired bodies, expired timestamps even before the expiry sweep, quote previews, and drafts are not searchable. Search cannot mutate unread counts. Opening an older hit must retain unread counts until the user actually returns to the visible newest messages.

## Implementation boundaries

- `app/search.rs`: pure chat ranking, input normalization, message finder state.
- `app/update/search.rs`: App search transitions; keep the growing general reducer focused on dispatch and persistence.
- `storage/search.rs`: scoped literal search and bounded excerpts using the existing database connection; register a deterministic Unicode lowercase SQL function via Diesel's declared-function API.
- Existing Effect/StoreCompletion/runtime plumbing carries requests and responses; existing cursor paging handles jumps.
- `ui/search.rs`: adaptive switcher and message finder rendering. Theme/config and existing shortcuts remain compatible, except newly assigned unused keys may conflict with custom bindings and report the existing clear configuration error.

## Verification and delivery

Use Rust 1.98.0, the pinned backend and current lockfile. Add no dependencies and do not open a real account or transmit messages during tests. Cap builds at two jobs and tests at two threads.

Prove fuzzy ordering, Unicode input, filters, local draft visibility and selection stability; use real SQLite to test scope, aliases, edits/deletions/expiry, literal queries, and bounded results beyond one timeline page. Exercise async stale responses, failed queries, draft preservation, exact history jumps and unread behavior. Render narrow/wide terminals, long queries, hostile terminal controls, no-result/loading states, and remapped keys. Finish with formatting, Clippy, all-target tests, release build, demo smoke, and one independent branch review.
