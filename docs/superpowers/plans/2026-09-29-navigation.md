# Conversation Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Find a conversation or cached message quickly without losing the user's writing or reading position.

**Architecture:** Extend the existing modal search and storage-effect flows. Pure search helpers own matching; a dedicated SQLite search module queries current message bodies, and a reducer child module owns finder transitions. Existing snapshot cursors perform history jumps.

**Tech Stack:** Rust 1.98.0, Ratatui 0.30, Diesel/SQLite, Tokio; existing locked dependencies.

**Spec:** `docs/superpowers/specs/2026-09-29-navigation-design.md`

## Global Constraints

- Preserve the cyan theme, Tab between panes, j/k in lists, and ordinary typing in the composer.
- Search inputs are single-line and bounded to 256 Unicode scalar values.
- Return the most recent 50 matches, ordered by timestamp and complete message identity.
- Use Rust 1.98.0, the pinned backend and current lockfile. Add no dependencies and do not open a real account or transmit messages during tests.
- Cap builds at two jobs and tests at two threads.
- Use `CARGO_TARGET_DIR=/home/wuzi/Projects/whatsapp-tui/target` for every Cargo command to reuse the existing build cache.

## Review Focus

- Background updates reorder or remove the highlighted chat; selection remains tied to identity (Task 1).
- Search contains Unicode or SQL wildcard characters; case comparison and literal matching remain correct (Tasks 1 and 2).
- Edit/delete/expiry or PN/LID merge happens during search; stale excerpts cannot restore old content (Tasks 2 and 3).
- A canceled/edited request completes after a new request; its response cannot replace results or navigate (Task 3).
- Older history is opened while a draft and unread messages exist; writing survives and unread is not cleared by search (Task 3).

---

### Task 1: Ranked chat switching and unread discovery

**Files:** Create `src/app/search.rs`, `src/ui/search.rs`, `tests/navigation.rs`; modify `src/app/{mod,update,view_model}.rs`, `src/ui/{mod,overlays}.rs`, `src/config/bindings.rs`.

**Interfaces:**
- Consumes: `ChatSummary`, `Editor`, existing `Overlay::Search` and `StoreCompletion::Chats`.
- Produces: `rank_chats(chats: &[ChatSummary], query: &str, unread_only: bool) -> Vec<ChatSummary>`; `normalize_query(text: &str) -> String`; `Overlay::Search` gains `unread_only: bool`; actions `Unread` and `ToggleUnread`.

- [x] **Step 1: Write behavior tests** in `tests/navigation.rs`: `switcher_ranks_exact_before_fuzzy` expects Alice before Alicia for `alice` and Alice Smith for `asm`; `unicode_and_phone_queries_match` covers uppercase accents and formatted phone digits; `unread_filter_preserves_query_and_draft` exercises u/Ctrl-U and cancel; `switcher_preserves_identity_during_reorder` asserts Enter still opens the highlighted ID after a Chats completion; `switcher_shows_local_drafts` asserts a dirty composer badge before a save finishes; `switcher_normalizes_paste` asserts newlines become spaces and length <=256. Add rendering assertions for narrow/wide views, result count, no matches, and custom ToggleUnread/Back keys.
- [x] **Step 2: Run tests**: `cargo test --locked -j 2 --test navigation -- --test-threads=2`. Expected: new behavior fails on the existing substring-only switcher.
- [x] **Step 3: Implement** the ranking helper (stable sort by aggregate token score; name, phone, identifier candidates), overlay filter/state, new configured shortcuts and rendering. Preserve selection by ChatId when Chats refreshes; normalize query insertions; overlay helpers display effective keys.
- [x] **Step 4: Verify**: `cargo test --locked -j 2 --test navigation --test interaction --test rendering --test configuration -- --test-threads=2`. Expected: all tests pass, existing bindings and typing remain valid.
- [x] **Step 5: Commit** as `feat: add fuzzy chat switching and unread discovery`.

### Task 2: Literal cached message search on SQLite

**Files:** Create `src/storage/search.rs`, `tests/message_search_store.rs`; modify `src/storage/{mod,worker}.rs`, `src/app/model.rs`.

**Interfaces:**
- Consumes: existing SQLite message JSON, canonical identity helpers, account/chat timeline index.
- Produces: `MessageSearchHit { key: MessageKey, created_at_ms: i64, preview: String, match_grapheme: usize }`; `MessageSearchPage { hits: Vec<MessageSearchHit>, has_more: bool }`; `Store::search_messages(account: AccountId, chat: ChatId, query: String, now_ms: i64) -> Result<MessageSearchPage, StoreError>`. The ephemeral match position lets rendering shorten preceding context by display width.

- [x] **Step 1: Write real-store tests**: `search_is_literal_unicode_and_scoped` asserts `CAFÉ`, `%_`, other accounts/chats, caption matching, and quote/draft exclusion; `search_tracks_edits_deletions_and_expiry` asserts changed bodies immediately stop matching and timestamps exclude unswept expiry; `search_finds_older_pages_with_bounded_results` inserts >100 messages, finds the oldest unique text and caps broad matches at 50 with has_more; `search_resolves_aliases` searches either identity after a merge; `search_excerpt_contains_distant_match` checks a hit beyond 160 graphemes is shown with a bounded Unicode excerpt.
- [x] **Step 2: Run tests**: `cargo test --locked -j 2 --test message_search_store -- --test-threads=2`. Expected: missing search API, then assertions fail with an empty stub before implementing the query.
- [x] **Step 3: Implement** the public model/facade and storage helper. Register the deterministic declared Unicode lowercase function on each opened connection. Use bound parameters, JSON extraction of body/caption only, expiry filtering, DESC timestamp/key and LIMIT 51. Lowercase matching is literal; reject oversized queries, return empty for whitespace-only input. Build excerpts around the match and truncate to at most 160 graphemes plus ellipses.
- [x] **Step 4: Verify**: `cargo test --locked -j 2 --test message_search_store --test store --test reconciliation -- --test-threads=2`. Expected: all pass without a schema migration.
- [x] **Step 5: Commit** as `feat: search cached conversation text and captions`.

### Task 3: Message finder, history jumps, and usable delivery

**Files:** Create `src/app/update/search.rs`, `tests/message_search_flow.rs`; modify `src/app/{search,update,view_model}.rs`, `src/runtime.rs`, `src/ui/{search,mod,overlays}.rs`, `src/config/bindings.rs`, `README.md`, `docs/{usage,backend-validation}.md`, `examples/config.toml`, `src/whatsapp/demo.rs` if richer demo fixtures are needed.

**Interfaces:**
- Consumes: Task 2 search model/facade; Task 1 query normalization and search rendering helpers; existing `PageDirection::AtOrBefore`, `LoadChat`, and `StoreCompletion::Chat`.
- Produces: boxed `Overlay::MessageSearch(MessageSearch)` with query editor, request binding, results/loading/error state; actions/context for `MessageSearch`; `Effect::SearchMessages { request, account, chat, query }` and matching `StoreCompletion::MessageSearch` with `result`.

- [x] **Step 1: Write reducer/runtime tests**: `finder_submits_only_on_enter` expects no I/O while typing and exactly one request while loading; `finder_ignores_stale_responses` covers edit/cancel/reopen/account switch; `finder_failure_can_retry` preserves text and creates a fresh request; `opening_old_hit_keeps_draft_and_unread` uses real Store/runtime execution to locate a message outside the initial 100, asserts selected full key, focus Messages, retained draft and no MarkRead effect, then End loads latest; `conversation_change_invalidates_results` exercises incoming changes and aliases; `finder_rendering_is_adaptive` covers loading/empty/error/50+ states, sanitization and remapped shortcuts at 40x12 and 120x40. Exercise query paste bounding and no-conversation handling.
- [x] **Step 2: Run tests**: `cargo test --locked -j 2 --test message_search_flow -- --test-threads=2`. Expected: absent finder behavior fails.
- [x] **Step 3: Implement** search transitions in the reducer child module. Ctrl-F opens it; Enter submits a new query or jumps to a fresh selected hit. Clear pending identity/results on edits and conversation invalidations. Ignore late results. Runtime calls the facade with the current epoch. Always jump through snapshot loading and keep at_bottom false during the transition. Render adaptive results, cursor, state, and actual configured shortcuts.
- [x] **Step 4: Document and verify** the new controls, literal/Unicode behavior, local-only scope, 50-result limit and roadmap. Add synthetic demo messages for discovery. Run `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -j 2 -- -D warnings`, `cargo test --locked --all-targets -j 2 -- --test-threads=2`, and `cargo build --locked --release -j 2`. Expected: all checks succeed. Run a synthetic PTY demo through switcher, unread filter, message search and exit; expected terminal state restored and no network factory.
- [x] **Step 5: Commit** as `feat: find messages and jump through cached history`.

## Completion

- [x] Review the complete branch once with a fresh reviewer, address material findings with failing regressions, and re-run relevant checks. The review reproduced hidden matching text after wide characters in narrow previews; a real-store/runtime/render regression now passes for CJK text and expanded tabs at 40x12.
- [ ] Provide the implementation, plan, verification evidence, runnable demo command, and the recommended next iteration. Preserve the established local integration workflow; do not push or publish.
