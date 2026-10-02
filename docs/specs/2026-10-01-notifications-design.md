# Silent desktop notifications

The user misses new messages while using whatsapp-tui as their main client and chose desktop popups without sound. This is a small architectural addition to the existing durable inbound, reducer and asynchronous desktop-effect boundaries.

## Behavior

- Enable silent Linux desktop popups by default, using the installed `notify-send`. Normal urgency respects desktop notification policy. Send the standard `suppress-sound` hint and never play audio ourselves.
- Notify for genuinely new incoming live messages committed while this app session is running. Own sends, history sync, replayed records, edits, reactions, receipts, deleted/expired content and startup backlog do not alert.
- Suppress a conversation only when the terminal is known to be foreground, Messages/Composer is focused, its bottom is visible, and no overlay/loading blocks it. Unknown foreground state is conservative: allow alerts. Keyboard, paste and mouse input establish foreground; focus loss clears it.
- Recheck suppression while messages wait. Opening the conversation before dispatch cancels its queued alert. Revalidate stored content/read state before constructing the popup.
- Coalesce arrivals into a single popup every two seconds under continuous traffic. One conversation shows its name, a group sender when relevant, and a short latest-message preview. Multiple conversations show a compact summary. Cap pending keys at 128; larger bursts degrade to a generic new-message alert, not unbounded memory or helper processes.
- `[notifications] enabled = true, previews = true`; disabling previews hides names and content. Demo mode never emits desktop notifications. A failed helper gives one actionable in-app notice, retries later with a 60-second cooldown, and never interrupts chat or drafts.
- This version requires the TUI to stay running. No click-to-open-chat integration, avatars in popups, notification sound, or WhatsApp mute synchronization. Desktop Do Not Disturb and presentation are controlled by the notification daemon.

## Implementation choices

Use an ephemeral after-commit incoming-message signal from the existing durability hook. Have storage report only first live unread insertions from the same transaction; the later protocol event's repeated write therefore produces no second signal. Avoid guessing from changing unread totals or maintaining a second durable notification database. The hook's bounded event channel applies backpressure, but notification-helper failures never affect durable ingestion.

The reducer owns eligibility and a bounded burst queue; a regular asynchronous effect revalidates the candidates and calls a bounded `notify-send` process. Escape body markup, sanitize control characters, limit strings, pass literal arguments with `--`, and suppress helper output. No new Cargo dependencies.

Protocol reference: [Freedesktop notification hints](https://specifications.freedesktop.org/notification/latest/hints.html) and [body markup](https://specifications.freedesktop.org/notification/latest/markup.html). Hints are daemon-dependent.

## Validation and delivery

Synthetic stores, injected/fake helpers and offline reducer/runtime checks cover persistence replay, history, failed commits, privacy, focus, bursts and process failure. Do not access the live WhatsApp store or send test popups to the real desktop. Keep one Cargo process, `-j 2`, test threads 2. Implement inline, request one independent final review, preserve evidence, merge locally and rebuild the release; do not push.

## Review refinements

Overflow keeps a bounded account/timestamp range and rechecks unread storage independently, so canceling the retained keys cannot silence an omitted chat. This conservative generic fallback may include already-notified unread messages sharing that range. Pending effects observe current account, focus, pairing and privacy through a watch channel; context changes restart preparation or cancel an already submitted helper without resubmission. Storage resolves active-chat aliases before filtering.
