# Message Reading and Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make formatted conversations readable and let users copy, reply, resend, or open a selected message's links without leaving the keyboard.

**Architecture:** Add a presentation-only formatter, pure action/link helpers plus an asynchronous desktop adapter, and two small overlays wired through existing reducer effects. Re-read the full message key before external actions; retain existing reply/resend workflows.

**Tech Stack:** Rust 1.98.0, Ratatui, Tokio, Diesel/SQLite, linkify.

**Spec:** `docs/superpowers/specs/2026-09-29-message-actions-design.md`

## Global Constraints

- Keep cyan colors, pane controls, ordinary typing, drafts, cached search, and reading position.
- Keep source text intact. No database migration or backend update. Add only linkify for URL tokenization.
- HTTP/HTTPS only; at most 32 unique links, each at most 4096 bytes. Clipboard limit 1 MiB; helper deadline three seconds.
- Use fixed desktop executables directly, no shell; automated tests never touch the real clipboard, browser, or WhatsApp account.
- Every Cargo command uses `CARGO_TARGET_DIR=/home/wuzi/Projects/whatsapp-tui/target`; builds `-j 2`, tests `--test-threads=2`.
- Implement inline; one independent whole-branch review; merge locally using the established workflow, without publishing.

## Review Focus

- Malformed/nested markup and Unicode must preserve readable content and avoid runaway parsing (Task 1).
- Formatting must keep wrapping and long-message scroll metrics consistent, including captions and control characters (Task 1).
- URL punctuation, credentials, unsupported schemes, and shell metacharacters must not change the chosen destination or execute commands (Task 2).
- Edits, deletion, expiry, aliases, or account changes between selection and execution must not act on stale content (Tasks 2/3).
- Missing or stalled helpers and repeated activation must leave navigation and shutdown responsive (Tasks 2/3).

### Task 1: Styled message rendering

**Files:** Create `src/ui/rich_text.rs`, `tests/rich_text.rs`; modify `src/ui/{mod,timeline}.rs`.

**Interfaces:** Consume raw text and existing theme styles; produce `rich_text::lines(source: &str, width: usize, accent: Style, muted: Style) -> Vec<Line<'static>>` used by timeline body/caption rendering and its existing viewport calculation.

- [ ] **Step 1:** Write buffer-level tests: balanced nested styles remove delimiters and apply bold/italic/strike; code preserves literal markers; unmatched/intraword markers remain; quote/bullet/numbered content is readable; CJK/emoji wrapping preserves text and styles; captions format while source stays unchanged; terminal controls remain sanitized; long formatted messages remain scrollable.
- [ ] **Step 2:** Run `cargo test --locked -j 2 --test rich_text -- --test-threads=2`; expect new formatting assertions to fail on plain rendering.
- [ ] **Step 3:** Implement a bounded, nonrecursive delimiter pass with literal fallback and style-preserving grapheme wrapping. Integrate both text and captions without touching storage or composer text.
- [ ] **Step 4:** Run rich_text, rendering, and interaction targets; expect all pass.
- [ ] **Step 5:** Commit `feat: render WhatsApp message formatting`.

### Task 2: Validated desktop actions

**Files:** Create `src/message_actions.rs`, `src/desktop.rs`, `tests/desktop_actions.rs`; modify `src/lib.rs`, `src/storage/mod.rs`, `Cargo.toml`, `Cargo.lock`.

**Interfaces:** `message_actions::text(message: &MessageRecord, now_ms: i64) -> Option<&str>`; `web_links(text: &str) -> Vec<String>`; `DesktopAction::{CopyText, OpenLink(String), CopyLink(String)}`; `Store::get_message(key: MessageKey) -> Result<Option<MessageRecord>, StoreError>`; asynchronous `desktop::execute(message, action, store, integration) -> Result<String, String>`. Injectable `Desktop` trait exposes copy/open; native implementation owns fixed commands and deadlines.

- [ ] **Step 1:** Write tests for ordered/deduplicated web links, punctuation/Unicode, blocked schemes/credentials/control input, original copy bytes, edited/deleted/expired/aliased message rejection using real SQLite, and helper stdin/argv/error/timeout behavior using temporary executable fixtures. External effects must stay behind the test boundary.
- [ ] **Step 2:** Run desktop_actions; expect unavailable API then behavior failures with inert stubs.
- [ ] **Step 3:** Implement helpers, current-record validation, narrow storage getter, and bounded native command adapter. Validate selected URL membership again immediately before opening/copying it.
- [ ] **Step 4:** Run desktop_actions and store/reconciliation targets; expect all pass with no real desktop changes.
- [ ] **Step 5:** Commit `feat: add validated clipboard and browser actions`.

### Task 3: Message menu and link picker

**Files:** Create `src/app/update/actions.rs`, `src/ui/actions.rs`, `tests/message_actions_flow.rs`; modify app model/input/overlay/reducer, runtime, bindings, UI dispatch, demo, README, usage, validation record, example config, and PTY tests.

**Interfaces:** Boxed message snapshot overlays with selected entry and optional menu return; `Effect::DesktopAction { request, message, action }`; `Input::DesktopAction { request, account, result }`; one pending desktop request in App. Reuse Task 2 helpers and native adapter; keep existing Reply/Resend actions.

- [ ] **Step 1:** Write reducer/render tests for Enter menu, y copy, o picker without opening, navigation/cancel preserving draft/focus, applicable actions, configured hints at 40x12/120x40, one in-flight action, stale message invalidation, stale account completion, and composer typing. Add PTY navigation through menu/link picker and copy/open using fake desktop executables.
- [ ] **Step 2:** Run message_actions_flow; expect missing action behavior to fail.
- [ ] **Step 3:** Implement menu/picker, configured shortcuts/help, request/result flow, stale validation, and runtime wiring. Add demo formatting and example.org links. Document syntax, clipboard helpers, limits, and controls.
- [ ] **Step 4:** Run fmt check, Clippy all-targets with `-D warnings`, all-target tests, and optimized build under the resource limits. Expect all green; inspect synthetic PTY restoration.
- [ ] **Step 5:** Commit `feat: add message actions and link picker`.

## Completion

- [ ] Independent review; reproduce and fix material findings; record any deferred issues.
- [ ] Verify and merge locally, preserve evidence, deliver runnable binary and completed plan.
