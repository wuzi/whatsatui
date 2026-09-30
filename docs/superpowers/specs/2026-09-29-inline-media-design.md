# Images, stickers, and emoji in the terminal

The user wants Concord's visual conversation experience in WhatsAppTUI on local Ghostty: see received images and stickers in the timeline, attach and send images without leaving the terminal, and search/insert emoji above the composer. Keep the existing cyan theme and pane keyboard model.

## Interaction

- Visible image/sticker messages reserve a bounded preview area below their label, above their caption. Load verified media in the background and reuse the existing private cache. Loading/errors remain readable; ordinary text, scrolling, overlays, and quit stay responsive.
- Ghostty uses Kitty graphics with Unicode placeholders; other terminals use half blocks. Choose without a blocking stdin probe. A configuration switch can disable inline previews or force the renderer. Clip previews to the visible timeline and clear them when switching chats, resizing, or showing overlays.
- Static PNG/JPEG/WebP and the first frame of animated WebP are supported. Unsupported sticker encodings retain a useful label and download action. View-once content remains excluded. Bound decoding to 16 megapixels and 64 MiB of decoder allocations; preview downloads to 16 MiB; one active preview download, bounded in-memory thumbnails.
- `Ctrl-O` in the composer opens an image-path dialog. Enter validates and copies an immutable snapshot into private app storage; Esc cancels. Support paths with spaces and `~/`. Show the attached filename and thumbnail, allow `Alt-A` to remove it, and treat composer text as its caption. One image per message. PNG/JPEG/WebP inputs, at most 16 MiB and 16 megapixels, are normalized to JPEG for predictable WhatsApp delivery.
- Attachment drafts survive restart and chat switches. Staging is atomic with draft clearing and preserves newer edits. No upload occurs before Send. Failed or uncertain sends require explicit resend; never replay automatically after restart. Preserve existing quotes, stable message IDs, and acknowledgment semantics.
- Contact identity merges preserve conflicting image drafts as separate recoverable drafts with their captions and replies. Ctrl-O lists these when the path is empty; Enter restores without sending and retains the previous active draft. Later edits, including attachment removal, take precedence over an older merge snapshot. Sending clears only the active draft.
- `Ctrl-E` opens a searchable emoji list above the composer; arrows select, Enter inserts at the caret, Esc cancels without changing text. Search names and shortcodes, render Unicode emoji in rows. Picking does not send the message. Plain pasted/typed emoji continue to work. Discord custom emoji are represented by ordinary images/stickers on WhatsApp, not invented server emoji.

## Boundaries and verification

Use Ratatui 0.30, ratatui-image 11.1, image 0.25 with only required codecs, and an offline emoji catalog. Keep decoded pixels/protocols outside cloned application state. Recheck message/account/body identity after background work, and discard stale results. Local attachment references are generated content IDs, never arbitrary source paths in outgoing persisted messages. Uploads run with a bounded deadline and shutdown cancellation.

Add synthetic demo media and tests for sticker normalization, corrupt/oversized images, clipping/resize, stale results, persistence, caption-only/attachment-only sends, outgoing protobuf fields, emoji insertion, and terminal restoration. Do not use the user's WhatsApp account or send real messages during verification. Run Cargo serially with `-j 2`, shared target directory, and at most two test threads.

This iteration does not add sticker-pack creation, GIF playback, image albums, clipboard-image integration, avatars, or a Discord-compatible custom emoji service.

Primary implementation references: [ratatui-image](https://github.com/ratatui/ratatui-image), [WhatsApp stickers](https://github.com/WhatsApp/stickers), [emoji catalog](https://docs.rs/emojis/0.9.0/emojis/), and the pinned whatsapp-rust 0.7.0 source.
