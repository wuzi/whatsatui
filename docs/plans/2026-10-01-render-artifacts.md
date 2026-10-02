# Emoji Redraw Correction Plan

> **For agentic workers:** Use superpowers:executing-plans for inline implementation.

**Goal:** Keep timestamps intact and remove the stray digits produced by conversation redraws.

**Architecture:** Ratatui 0.30's VS16 trailing-cell updates can make Crossterm print past a wide emoji, leaving text outside its tracked position. Mark wide emoji cells with their existing display width before buffer diffing so the glyph is emitted as a unit. Preserve image protocol metadata and ordinary incremental rendering.

**Tech Stack:** Existing Rust, Ratatui/Crossterm and Unicode crates; no dependency changes.

**Spec:** The user's annotated screenshot and bounded design in this session: correct timestamps and clean blank rows as messages arrive, scroll, and disappear; retain emoji, avatars and inline media.

## Global Constraints

- Inline implementation in an isolated worktree; one final independent review, local merge/release, evidence preservation, no push.
- One Cargo process, shared root target, `-j 2` and `--test-threads=2`.
- Synthetic/offline checks; no real messages, profile photos, or live account access.

## Review Focus

- Emoji sender headers moving over older text must keep timestamps in their intended columns.
- Clearing or scrolling content must erase every old digit.
- Plain text, other wide glyphs and styled emoji must keep their layout.
- Overlays, narrow layouts and sidebar emoji must receive the same protection.
- Kitty placeholders must retain their width, one-time uploads and cleanup behavior.

## Task 1: Reproduce and correct terminal redraws

**Files:** `src/ui/mod.rs`, `tests/terminal_rendering.rs`, this plan and `docs/backend-validation.md`.

**Interfaces:** Existing `ui::render_interactive` finalizes the frame before Ratatui diffs it. Private `preserve_emoji_widths(&mut Buffer)` preserves wide VS16 cells using `CellDiffOption::ForcedWidth` without modifying image cells. Tests replay actual `CrosstermBackend::draw` output, asserting text at its intended terminal coordinates across successive frames.

- [x] Establish a clean baseline with rendering, conversation UX and inline-media tests.
- [x] Run `cargo test --offline --locked --target-dir /home/wuzi/Projects/whatsapp-tui/target -j 2 --test terminal_rendering -- --test-threads=2`; regressions must fail on shifted timestamp/leftover digits.
- [x] Implement `preserve_emoji_widths` in `src/ui/mod.rs`, call after all frame rendering, and verify the regressions pass. Extend terminal-output checks for the Review Focus inputs.
- [x] Run fmt, full tests and Clippy. Commit and request the single final review; fix Important findings with failing regressions before rerunning checks.
- [x] Build release, document verification and its limits, merge locally, preserve evidence and remove the owned worktree/branch.

Verified source: b4a8d9f. All 340 automated tests pass (three native helpers remain opt-in), along with formatting and Clippy. The independent review found no issues. Release SHA256: `22d67a5a128191b06c01fbbde629be93e794c425e78dea7918f8a76f5af0afc7`. Verification evidence is preserved under `.superpowers/sdd/2026-10-01-render-artifacts/`. Local integration only; nothing pushed.
