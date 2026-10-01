# Usage and recovery

## Starting and linking

Run `whatsapp-tui --demo` to explore offline, or `whatsapp-tui` to start the native WhatsApp adapter. Pair through your phone's Linked devices screen. QR codes expire and refresh; a terminal too small to display a complete code asks you to resize. The regular chat layout works at 80×24; truecolor terminals show the cyan palette, with ANSI fallback elsewhere.

The app uses one account per active session. Histories and drafts are keyed by account, including when you link a different account into an existing data directory. Only conversations and contacts that WhatsApp supplies are available. Search a known contact and open its composer to start a direct chat.

## Reading and composing

Tab changes panes. Select a conversation in Chats, then Enter to type, or press `i` from Chats or Messages to focus the composer directly. Enter sends a nonempty draft; Shift-Enter or Alt-Enter adds a line at the caret. Paste, including multiline Unicode text, stays in the composer until you submit it. Drafts save after 250 ms without editing and flush on pane/chat changes and normal quit. A crash can lose edits within that debounce window. If a draft write fails during quit, the app cancels quit, keeps the draft visible, and asks you to fix storage before trying again.

Ctrl-C in Composer clears the current text and resets the caret, keeping any attached image and reply target. It also cancels a pending clipboard paste. When editing a sent message, it clears the proposed edit while preserving your normal draft; an edit already being saved remains locked until its result arrives. Ctrl-Q saves drafts and quits.

Shift-Enter uses the terminal's [keyboard disambiguation protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/#disambiguate-escape-codes), enabled while the app runs and restored on exit. Ghostty supports this protocol. Alt-Enter remains available for terminals that cannot distinguish Shift-Enter. Remap `focus_composer` in `[bindings.chats]` / `[bindings.messages]`, and `clear_text` / `newline` in `[bindings.composer]`. Explicit custom keys take precedence over these new defaults. If an older configuration sets `reactions = ["i"]`, change it to `["I"]` to make `i` available for composing; Help always shows your effective bindings.

In Messages, use arrows or j/k to select a message; r quotes it without discarding existing composer text. Alt-R removes the quote. Missing, deleted, expired, and unsupported quoted content has an explicit placeholder. Only a bounded preview is retained. If identity reconciliation combines two distinct saved drafts, both texts are kept in one composer, separated by a blank line; the canonical draft's reply target takes precedence. Concurrent local edits are also retained, with a notice to review the combined draft. This conservative merge can leave an older passage alongside its edited version; remove any duplication before sending.

Arrows and j/k select whole messages, including long messages. J/K scroll wrapped rows without changing the message selected for actions; PageUp/PageDown scroll a screen at a time. Scrolling past the cached page requests more history (at most 100 records per page). Sender and status remain visible while reading long messages. New arrivals preserve an older selection or scrolled reading position and show an indicator; End returns to the newest page. WhatsApp may not provide your complete historical archive.

Messages/Composer at the bottom mark the conversation read; selecting a chat while staying in Chats does not. Terminal foreground focus is also required when focus reporting is available. Read acknowledgements use the backend's account behavior. Synced unread information initializes the local baseline; it is not an ongoing mirror of every other device's unread counter.

## Sender blocks, photos, and mouse

The sidebar shows a small profile photo beside each chat’s name and latest-message preview, with initials when no photo is available. Group rows use the group photo. Long names shorten to keep unread counts and draft indicators visible; clicking the photo selects the same chat as clicking its text.

Direct and group chats use sender blocks: profile photo, bold name and time. Consecutive messages from the same person within five minutes share a block; dates start new blocks. Every message keeps its timestamp and its own selection. Your messages show **You** and a green identity marker (`theme.own`); the cyan marker identifies the selected message. Group messages use participant photos, and the conversation header uses the group photo.

Visible profile photos load in the background and fall back to initials if missing or private. Photos are scoped by account, resized to 96×96 thumbnails, and cached under `<data-dir>/avatars/` with private permissions. The cache retains at most 128 thumbnails; the renderer holds at most 32 visible photo protocols separately from message previews. Downloads are limited to 1 MiB and decoding to 1024×1024. Photos refresh after an hour; unavailable photos are checked again after five minutes. Existing photos stay visible during refresh, and unchanged photos remain in place. A failed refresh keeps the previous photo until a later attempt; a successful missing/private response removes it. Set `[media] avatars = false` to hide photos and stop these lookups. The demo uses initials without network lookups.

Click a chat to select it, a message to select that exact message, or the composer to place its cursor. Right-click a message to open its action menu. Double-click an item to activate it like Enter; a single menu click changes selection. The wheel scrolls the pane or popup under the pointer. Popups capture clicks, and their **[×]** control closes them. Click **Help** in the header or press F1 for shortcuts; Help scrolls with arrows, PageUp/PageDown, or the wheel. The footer contains only notices and connection explanations.

Mouse handling is enabled by default. Set `[ui] mouse = false` to leave pointer handling to the terminal. Keyboard copy (`y`) remains available while mouse handling is enabled. Normal exit and panic cleanup disable mouse capture.

## Finding conversations and messages

Ctrl-P opens the chat switcher from any pane; `/` also opens it from Chats or Messages. Type parts of a name, phone number, or known identifier: `asm` can find Alice Smith, and `smith ali` also matches. Exact and contiguous matches rank before scattered characters. Empty queries retain the recent-chat order. Phone queries can omit spaces, parentheses, and dashes.

Press `u` in Chats for unread conversations, or Ctrl-U inside the switcher to toggle All/Unread without clearing your query. Each result shows its direct/group type, unread count, and any draft. Arrows select, Enter opens the composer, and Esc returns to the pane you came from. Background updates keep the highlighted conversation selected when its position changes.

Ctrl-F opens a message finder for the current conversation. Type a phrase and press Enter to search, then use arrows and Enter to open a match in Messages. End returns to the latest page. Search is local and works offline, including messages outside the currently loaded page. Only history downloaded to this client can appear.

Message queries are literal: `%`, `_`, quotes, and backslashes have no special meaning. Unicode lowercase comparison makes `CAFÉ` match `café`; `cafe` does not match `café`. There is no accent normalization or stemming. Text and media captions are included; deleted/expired bodies, quoted previews, and unsent drafts are excluded. The newest 50 results are shown; refine your phrase when the count says `50+`.

Search runs only when submitted, with one scan at a time for the open finder. Editing clears old results. If the conversation changes while the finder is open, submit again for fresh results. Esc closes the finder and keeps your timeline and draft; opening an older result also preserves the draft and does not mark newer messages read. Both search inputs use a single line of at most 256 Unicode characters. Their controls can be changed in `[bindings.search]` and `[bindings.message_search]`.

In the demo, use Ctrl-P and `alc` to open Alice, then Ctrl-F and `cyan` to find a message. Return to Chats and press `u` to try the unread filter.

## Formatting and message actions

Messages and media captions display balanced `*bold*`, `_italic_`, and `~strikethrough~` text. Single backticks mark inline code; triple backticks mark code that can span lines. Code uses dim cyan text in your terminal's existing monospace font, and its contents stay literal. Lines beginning with `> ` become quotes; `- ` or `* ` become bullets. Numbered lists keep their original numbers. Nested emphasis works; unmatched markers and underscores within words remain visible. This follows a subset of [WhatsApp's formatting syntax](https://faq.whatsapp.com/539178204879377/?cms_platform=web&locale=en_US). The composer, storage, search, and outgoing messages keep the original text.

In Messages, press Enter for the selected message's action menu. Use arrows or j/k and Enter, or the displayed shortcut. Copy text is available for text messages and media captions; Reply is available for text, images, stickers, audio, videos, and documents; Resend appears for your failed or unconfirmed text/image attempts and retains its confirmation step. Escape closes the menu while preserving your draft and reading position. The menu stays attached to its original message when new messages arrive; changed or expired content is rechecked before an action runs.

Press `y` in Messages to copy the original text, including formatting markers and line breaks. Press `o` to choose among that message's HTTP/HTTPS links. Even one link is shown before opening: Enter requests the default browser, `y` copies the selected URL, and Escape returns to the menu or Messages. Long URLs have a wrapped preview; copy the full URL to inspect text beyond the preview. Discovery shows at most 32 unique links, each at most 4096 bytes, and excludes other schemes and URLs containing credentials. Links are never opened or fetched merely by rendering a message.

Clipboard support uses `wl-copy` from wl-clipboard on Wayland, or `xclip`/`xsel` on X11. Browser requests use `xdg-open` from xdg-utils. Run in a desktop session with those tools on PATH. Missing or failing helpers show a notice; the TUI stays usable. A browser-success notice means the desktop helper accepted the request. Clipboard text is limited to 1 MiB, and an unresponsive launcher times out after three seconds. Terminal clipboard forwarding over SSH is not included in this version.

Bindings live in `[bindings.messages]`, `[bindings.message_actions]`, and `[bindings.message_links]`; the complete example includes the defaults. The menu and link picker require an Open and Back binding. In the demo, open Alice, Shift-Tab from the composer to Messages, then Enter to explore the actions. Its newest message includes formatting and an example.org link. Copying and opening from an interactive demo use your real desktop helpers when you select those actions.

## Reactions, edits, and replies

Select a message and press `a`, or choose **React / change reaction** in its action menu. Search the usual emoji picker and press Enter. Choosing your current emoji removes it; choosing another replaces it. Counts appear beneath messages, with **You** beside your reaction. Click the reaction row or press `I` (Shift-i) for the participant list. In that popup, `a` changes your reaction and `x` removes it. Group participants use their known contact names.

Press `e` on your successfully sent text to edit it within 15 minutes of the original send. The composer says **Editing**: Enter saves, Shift-Enter or Alt-Enter inserts a newline, and Escape cancels. Your normal draft, including its image and reply, remains intact. Changing chats cancels an unsaved edit. If the original changes on another device or the edit window closes, the proposed text remains visible, but the app refuses to overwrite the newer message. Media captions cannot be edited here yet.

Sending status is separate from the original message's delivery/read state. A reaction or edit is marked sent only after a matching server acknowledgement. Failed or unconfirmed operations appear beneath the message. A timeout, disconnect, or interrupted app session may leave an unconfirmed action; check WhatsApp before deliberately trying again. The app never automatically resends these actions after restart.

`r` replies to text or media without discarding your current draft. Media quotes retain a label and bounded caption/filename preview; they do not carry a downloaded thumbnail. Click the quoted lines, or select the reply and press `q`, to open the original from local history. Missing, deleted, expired, and cross-conversation originals cannot be opened. End returns to the newest messages.

All new message controls can be remapped in `[bindings.messages]` and `[bindings.message_actions]`; the reaction list uses `[bindings.reactions]`. These letters remain normal text in the composer. Help shows configured controls. In `--demo`, Alice's latest message includes sample reaction counts; send a new text to try editing within its real 15-minute window. Demo actions stay offline.

## Images, stickers, and files

Images and stickers appear below their message label, with captions underneath. Visible previews load automatically, one at a time, from verified media; they never launch a viewer. Ghostty locally uses Kitty graphics. Other terminals, tmux, and SSH use colored half blocks. No terminal input query or tmux configuration change is needed. PNG/JPEG/WebP decode within limits of 16 MiB, 16 megapixels, and 64 MiB of decoder allocations. Animated WebP plays while visible, including in the sticker picker and sent-message previews. Playback is sampled at up to 20 fps, with at most 96 thumbnails per preview; unusually large, long, or damaged animations fall back to the first frame. Hidden animations stop and release their terminal graphics. Unsupported/damaged media shows a readable notice and keeps its download action.

Set `[media] inline = false` to use labels and explicit downloads only. `protocol = "auto"` is the default; `"kitty"` or `"halfblocks"` selects a renderer for local terminals. The renderer keeps at most eight visible previews and clears their graphics when you change conversations or open overlays.

In the composer, **Ctrl-V** reads a clipboard image or a copied image file and attaches it directly, keeping your typed text as its caption. Ordinary clipboard text inserts at the caret. Copy one image at a time; PNG, JPEG, and WebP are supported. This uses `wl-paste` (wl-clipboard) on Wayland or `xclip` on X11. In Ghostty use the app's **Ctrl-V**; the terminal's Ctrl-Shift-V/menu Paste generally forwards text only and cannot send bitmap data to the app. Clipboard reads happen only when requested, with bounded size/time; they never submit a message.

**Ctrl-O** remains available as an image path dialog. Paste a path (spaces and `~/` work), then Enter to attach it. The app makes a private immutable JPEG snapshot from PNG, JPEG, or WebP, flattens transparency onto white, and shows a thumbnail beside the caption. Your original file is untouched. Type a caption if desired; Enter sends one image, including with an empty caption. **Alt-A** removes the attachment. Esc cancels the dialog. No upload starts until you send.

Image drafts survive switching chats and restarting. Edits made while an earlier revision is staged remain in the draft. Uploads have a 60-second deadline; a failed upload never sends the caption as a text message. Failed/uncertain images remain in the conversation and use the existing explicit `R` resend confirmation. Immutable snapshots live in `<data-dir>/outgoing/` (0600 files, 0700 directory), capped at 128 files/512 MiB. They remain for draft recovery, previews, and resending; remove snapshots you no longer need when full. Removing a referenced snapshot makes its preview/resend unavailable until you attach the original again.

If WhatsApp joins two identities of a contact that both have drafts, image drafts retain their own captions and replies. The composer shows a saved-draft count. Open Ctrl-O with an empty path, choose a saved draft with the arrows, and press Enter to restore it. Your current draft moves into that list if it has content. Restoring does not send; sending the active draft keeps the others for later, including after restart.

**Ctrl-S** opens the sticker picker. It shows up to 60 distinct recent stickers from this account's cached conversations, with a preview of the selection. Use arrows or j/k to choose, then Enter to send. Oversized received static stickers up to 500 KiB are automatically optimized to the outgoing 100 KiB budget with transparency preserved. When optimization changes the image, the prepared preview stays in the picker; press Enter again to send it. Valid received animated WebP stickers retain their animation when sent; the terminal preview plays while visible. Phone sticker packs/favorites are not synchronized into this list.

To create a sticker, copy an image, open **Ctrl-S**, then press **Ctrl-V**. The app makes a static 512 × 512 WebP with transparent padding, within the outgoing 100 KiB budget. Review its preview, then Enter to send; Esc cancels. Sending a sticker preserves the composer text, image attachment, and reply. Stickers carry no caption. Once staged, sticker attempts are durable and use the same explicit resend flow as images; an unsent pasted picker selection is temporary and is discarded when the picker closes.

**Ctrl-E** opens emoji search above the composer. Search a name or shortcode such as `rocket` or `:thumbsup:`, use arrows to choose, and Enter to insert at the caret. Esc keeps your original draft. Picking never sends. Emoji are standard Unicode text, displayed by your terminal's font. Discord's custom emoji have no direct WhatsApp equivalent; paste custom pictures as images or create stickers with Ctrl-S.

Select an image, sticker, audio, video, or document in Messages and press `d` to download it, or choose **Download attachment** from Enter's action menu. The notice gives the saved path. Press `v` separately to request your desktop's default viewer with `xdg-open`. Opening never starts a download, and downloading never opens a viewer. Captions remain searchable and copyable with `y`. These actions also work on attachments without captions, and `d`/`v` remain ordinary text in the composer.

Downloads require a usable reference from a newly received message or history sync. Some media cached by an older version contains only a placeholder and cannot be retroactively downloaded unless WhatsApp supplies that message again. This includes images previously rejected because their CDN path begins with `/o1/v/`; new receipts and history replays now retain those references. Resending an affected image from the phone creates a fresh reference. View-once attachments are excluded. Inline video rendering, document/video sending, and sticker-pack creation are not included yet; received videos play in mpv as described below.

Files are saved under `<data-dir>/media/`, with generated filenames and private directory/file permissions (0700/0600). The received filename is display-only. The client checks the declared size and content hash before publishing a download and again before opening an existing copy. Repeating `d` reuses a verified copy. Supported viewer types are JPEG, PNG, GIF, WebP, Ogg/Opus, MP3, M4A, AAC, WAV, FLAC, PDF, plain text, CSV, Word/Excel/PowerPoint, and ODT/ODS. Other MIME types download as `.bin` and cannot be opened from the app.

One attachment/desktop action runs at a time; navigation and typing remain available. Files are limited to 50 MiB and a transfer to 60 seconds. Quitting cancels the transfer. Failed, interrupted, expired-reference, and full-storage downloads show a notice and can be retried with `d`; no partial file is offered for opening. The managed folder allows up to 128 attachments and 512 MiB. When full, quit the app and remove unneeded files from that folder (or clear the folder) before retrying.

Managed copies for deleted, expired, changed, or reconciled-away messages are removed at startup and during periodic maintenance while the app is running; an active transfer may delay cleanup. Files you copy elsewhere are outside this cleanup. Downloads are plaintext and private permissions are not encryption or forensic erasure. The app rechecks message availability before downloading and opening. Switching accounts prevents an old completion notice appearing in the new account; a download already requested may finish for its original account.

Try inline stickers offline with Ctrl-P → `leo` → Enter. Try clipboard images, stickers, or emoji from any demo composer using Ctrl-V/Ctrl-S/Ctrl-E. Clipboard paste in the interactive demo reads your desktop clipboard only when you request it. For download/viewer controls: Ctrl-P → `weekend` → Enter, Shift-Tab to Messages, then `d` and `v`. The demo downloads a small synthetic cyan PNG without contacting WhatsApp; `v` uses your real desktop viewer when explicitly selected. Demo files disappear on exit. Remap `download_media` and `open_media` in both `[bindings.messages]` and `[bindings.message_actions]`.

## Audio and videos

Install **mpv** for received voice messages and audio files: `sudo dnf install mpv` on Fedora or `sudo apt install mpv` on Debian/Ubuntu. Audio plays in the background without a window. Received videos open in an mpv window with sound and normal video quality. Set `[audio] player = "/path/to/mpv"` if it is not on your PATH.

Click an audio or video message's `[Play]` row, select it in Messages and press `p` or Space, or choose **play/pause media** from its action menu. The first play downloads and verifies the file; a private temporary copy stays available during playback even if the media cache is pruned. Voice messages show their duration, then playback progress. `p` / Space toggles pause; `s` cycles 1×, 1.5×, and 2× with pitch correction; `x` stops. These letters remain text in Composer. The corresponding configurable actions are `play_audio`, `audio_speed`, and `audio_stop` in `[bindings.messages]`, plus `play_audio` in `[bindings.message_actions]`.

While playing or paused, the header shows clickable pause/resume, progress, speed, and stop controls. Playback continues when you change chats. Starting another audio or video stops the previous one; new arrivals never start playback automatically. Quit, account changes, or deletion/expiry of the source stop playback. A missing player, damaged file, or unavailable output shows an error; select the message and play again to retry. Listening does not send a special played receipt.

Only newly received or replayed audio/videos with a complete media reference can play. Existing `[audio]` or `[video]` placeholders from older versions lack that reference; ask for a resend or wait for a history replay. View-once audio and videos are excluded. Audio sent as a generic document keeps its download/open actions. Recording and sending voice messages/videos are not included. Seeking is available in the mpv video window; the TUI has no seek control.

In the video window, use Space to pause/resume, Left/Right to seek, `f` for fullscreen and `q` to close. TUI pause/resume and speed controls follow changes made in the window. Closing the window ends playback normally; the message offers Replay. The existing `[audio].player` setting is used for both audio and video.

To try playback offline, Ctrl-P → `maya` → Enter, Escape to Messages, then `p`. This demo contains an eight-second synthetic tone and uses your normal audio output when played. Leo’s conversation contains an animated sticker and a silent synthetic video; select the video and press `p` to open its window.

## Connection and send state

An outgoing attempt receives a stable ID and is committed locally before transmission. Text typed while an earlier revision is being sent remains in the composer. The states shown are:

| State | Meaning |
| --- | --- |
| Sending | Locally committed; backend completion is pending |
| Sent | Server acceptance reported |
| Delivered / Read | Recipient receipt reported |
| Failed | A definite rejection or failure before acceptance |
| Unconfirmed | Acceptance could not be determined |

Group receipts show known recipient counts, not a claim that everyone has read the message. A disconnected send keeps the draft. Restart and reconnect never automatically resend an attempt. R on a Failed or Unconfirmed message opens a confirmation; resending creates a new ID, so an unconfirmed original may also arrive. Late receipts still reconcile against the original ID.

A successful socket write stays Sending until positive server acknowledgement or receipt evidence arrives. Attempts without that evidence become Unconfirmed after 30 seconds. A negative acknowledgement remains Failed even if local transport completion arrives later.

## Configuration

Run `whatsapp-tui --help` for command-line options. Start from [the complete example](../examples/config.toml), remove entries you want to keep at default, and edit the rest. Theme values accept `default` or `#RRGGBB`. Binding values are lists, for example:

```toml
[bindings.chats]
next = ["down", "j"]
previous = ["up", "k"]

[theme]
focus = "#00b4b4"
```

Bindings are scoped to each pane or overlay. Duplicate keys, missing essential actions, and printable composer/search shortcuts are rejected at startup before raw terminal mode. An invalid file reports a configuration error. Demo uses built-in defaults unless you explicitly pass `--config`; it rejects `--data-dir`.

## Storage and session recovery

Use an absolute `--data-dir` to keep separate installations independent. The instance lock protects each directory. Close the other instance if you see the ownership error. Back up the whole data directory only with the app stopped, including any SQLite sidecars. App history and protocol credentials are separate databases, both private plaintext files.

For disk or permissions errors, free space or restore access to your private directory, then retry saving. If you need to terminate an app whose draft cannot be saved, copy the text elsewhere first. After a crash, a previously Sending attempt becomes Unconfirmed and remains available for explicit review.

For a revoked linked session, first quit and check Linked devices on your phone. To re-pair while keeping local chat history, **with every instance stopped**, move `session.sqlite3` and any `session.sqlite3-wal` / `session.sqlite3-shm` sidecars into a private backup directory. Leave `chat.sqlite3` and its sidecars in place, then restart and scan a new QR. Never move an active database. Alternatively, use a fresh `--data-dir`; the old cache remains in the original directory.

Ctrl-Q requests a graceful shutdown; external SIGINT and SIGTERM signals do too. Ctrl-C inside Composer clears its text. Terminal state is restored on normal exit, input errors, and a Rust panic. SIGKILL and machine failure cannot run cleanup; use your shell's `reset` command if the terminal was left in a bad state.

## Live acceptance

Use your own test conversations. Run through pairing and restoration, direct/group text in both directions, a quoted reply, drafts in two chats, scrolling during new arrivals, narrow-terminal resizing, restart, and a network interruption. For media, receive a fresh image and document, download and explicitly open each, restart and reuse a saved copy, and try an expired reference or interrupted download. Record actual observations in [backend-validation.md](backend-validation.md). These checks require a user-linked account and are not performed by the automated suite.
