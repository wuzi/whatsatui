# Reading and acting on messages

## Intent

The user approved the next iteration: richer WhatsApp text formatting and a discoverable message action menu with clipboard/link support. Preserve the cyan palette, pane navigation, ordinary composer typing, drafts, cached search, and reading position. Media transfer follows in a later iteration. This is an architectural iteration with a small rendering module and a desktop integration boundary.

## Approach

Render WhatsApp-style formatting directly into Ratatui spans. A general Markdown renderer would interpret different delimiters and add unrelated behavior; leaving markup visible would miss the readability goal. Preserve original bodies for storage, searching, replies, transmission, and copying. Formatting is presentation only, including captions.

Support balanced `*bold*`, `_italic_`, `~strike~`, inline backticks, triple-backtick code, `> ` quotes, and `- `/`* ` bullets; numbered lists retain their numbers. Combine nested emphasis, leave unmatched delimiters and intraword underscores literal, and leave code content uninterpreted. Formatting delimiters must be complete graphemes. URL path characters remain literal; balanced emphasis around a URL uses the same delimiter ranges for rendering and link discovery. Code uses the accent color and dim styling in the terminal's existing monospace font. Sanitize controls before display. Wrap by grapheme display width with styles intact; use the same rows for rendering and scroll metrics.

The syntax is based on [WhatsApp's formatting reference](https://faq.whatsapp.com/539178204879377/?cms_platform=web&locale=en_US), consulted 2026-09-29. This is a documented subset, not a promise to duplicate all proprietary parsing edge cases.

## Actions

- Enter in Messages opens a menu for the selected full message identity. Show applicable Copy text, Open links, Reply, and Resend actions. Existing r/R semantics remain; resend still uses its confirmation. Escape dismisses without changing pane, selection, scrolling, or draft.
- `y` copies the original text or caption, including markup and line breaks. `o` opens a link picker; arrows/j/k select, Enter opens, y copies the selected URL, Escape returns to the action menu when appropriate. Even a single link is shown before opening. No automatic browser visits or previews.
- Link discovery uses `linkify` with explicit HTTP/HTTPS only, excludes credentials and controls, preserves order, deduplicates, caps at 32 links and 4096 bytes per URL. Show the actual URL, including a wrapped detail area for the selected link. The browser receives exactly that URL as one argument.
- Actions and links have distinct configurable contexts. The footer/help/menu display effective bindings. Printable shortcuts stay confined to lists and menus.
- Menus bind to a message snapshot and are dismissed when that message disappears or its body changes. Copy/open effects re-read the record immediately before acting; refuse changed, deleted, expired, or aliased records instead of using stale text. Completions bind to request/account so old results cannot overwrite a different account's notice. An explicitly activated desktop action may finish for its original account after switching accounts; switching dismisses unactivated menus. Only one desktop action can be pending.

## Desktop boundary

Use Linux `wl-copy` for Wayland, or `xclip`/`xsel` for X11; send content on stdin. Use `xdg-open` for browser requests. Run fixed executables directly, with no shell, null terminal streams, bounded input (1 MiB), and a three-second deadline that kills/reaps an unresponsive launcher. Missing tools, failure, and timeout produce actionable notices; success means the helper accepted the request. Clipboard ownership/browser lifetime belongs to the desktop helper. Do not change the real clipboard or open a browser during automated acceptance: use isolated executable fixtures at the OS boundary.

## Delivery

- Rust 1.98.0, existing pinned backend and database schema. One small dependency, `linkify`, owns URL tokenization; the lockfile records its exact version.
- Build with two jobs and test with two threads; reuse `/home/wuzi/Projects/whatsapp-tui/target`.
- Implement inline in an isolated worktree, with failing behavior tests, one independent final review, and the established local merge workflow. Do not push or publish.
- Cover formatting/Unicode wrapping, menu typing/remapping, stale identity/expiry, literal process arguments/stdin, bounded failures, and a synthetic PTY flow. Preserve live-account acceptance as unverified.
