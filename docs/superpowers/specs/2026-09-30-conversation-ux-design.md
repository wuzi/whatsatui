# Conversation UX

The user approved steps 1–3 of the proposed conversation UX release and chose Concord-style sender blocks for both direct and group conversations. The goal is to recognize people and one's own messages, read without a shortcut bar, and select individual messages with either keyboard or mouse. Reactions, outgoing edits, and expanded media replies remain a following release.

## Sticker preparation and footer

The existing resend path rejects static WebP over 100 KiB before compression. Validate the source independently of the prepared output: valid static sources up to the existing 500 KiB intake limit should go through the existing 512×512 transparent WebP encoder when necessary. Preserve accepted original stickers byte-for-byte, including animation. Never flatten received animation. When optimization changes a received sticker, retain the prepared sticker in the picker and show its preview before the user confirms sending. Preserve the composer and all existing cancellation and durable-send behavior.

Remove the footer's shortcut row; retain one quiet row for notices, errors and connection explanations. Help remains accessible using configured keys and a clickable Help label in the header. Help and menus must remain usable in a 40×12 terminal.

## Sender blocks and profile photos

Use a small avatar, bold sender name and time at the beginning of each block. Group consecutive messages from the same sender within five minutes, without crossing local dates; every message retains independent selection and timing. Show date separators. Own messages have a permanent accent and You label independent of the stronger selected-message marker. Keep delivery failures, edited labels, quotes, images and stickers visible. Group messages use the participant identity, while the conversation header uses the group's photo. Missing, unavailable or private photos fall back to sanitized initials.

Fetch photos only for visible identities. Cache small decoded thumbnails on disk and a bounded set of terminal protocols in memory. Scope identity by account; cap download size, decoded dimensions, request time and concurrent work. A missing/private response removes stale photos. Periodically refresh cached photos and back off after failures. Cancel irrelevant work and clean terminal images on overlays, resize, identity changes and exit. Reuse the existing Kitty/Ghostty and half-block renderer; do not query stdin for graphics capabilities.

## Navigation and pointer interaction

Arrow keys and j/k select whole messages. J/K and Page Up/Down scroll the timeline independently; End returns to newest messages. Preserve selection and reading position during incoming traffic. Build rendering and hit areas from shared layout information. Clicking a message selects that exact message without shifting the viewport; right-click opens its existing actions. Clicking a chat selects it, clicking the composer focuses it and positions the cursor at a grapheme boundary. Clicking menu rows selects them; double-click activates like Enter. Wheel events scroll the pane or popup under the pointer. Popups capture pointer input and clicks never reach obscured content. A header Help control and popup close control make mouse usage discoverable. Mouse capture is enabled by default, configurable, and restored on quit and panic.

## Constraints and verification

- Rust 1.98, current pinned WhatsApp backend and graphics dependencies; avoid dependency upgrades.
- One Cargo process at a time, at most two build jobs and two test threads, shared target directory.
- Use demo data and fake external providers for automated tests; do not access the live account or clipboard or send messages.
- Keep existing drafts, account boundaries, expiration and send durability intact.
- Verify large static stickers, animation, group identity, selection vs scrolling, wrapped media, resizing, grapheme cursor positioning, popup hit areas and terminal restoration.
- Finish with the complete test suite, formatting, Clippy, optimized demo smoke test, and one independent branch review.
