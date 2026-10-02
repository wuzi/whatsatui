# Received audio playback

The user uses WhatsAppTUI as their main client and explicitly chose in-TUI playback with pause/resume, progress and speed. Success means selecting a received voice message or audio file and hearing it without leaving the terminal. Keep drafts, existing navigation, mouse support and the quiet idle footer intact.

## Design

Normalize complete non-view-once AudioMessage payloads into an Audio attachment with optional duration and voice-note metadata. Use the native Audio media key type for decryption, the existing verified 50 MiB cache and download/open actions, and typed audio quotes. Old serialized attachments default missing metadata. Previously stored Unsupported audio has no media reference; only a subsequent history replay/resend can upgrade it.

Play on demand, using `p` or Space on a selected audio message, its message menu, or a click on its playback row. The first play downloads and verifies the file. A private temporary snapshot pins the verified bytes for the lifetime of playback without locking the whole media cache. Never auto-play incoming messages or another item on completion.

Use one mpv child controlled over documented JSON IPC: no window, terminal ownership, user config/scripts, or arbitrary network/playlist references. The TUI owns download cancellation, the child and snapshot. Runtime observations determine pause, duration and position. Bound startup/command deadlines and IPC line size; clear errors explain a missing player, invalid audio or unavailable output. Executable path is configurable under `[audio]`, default `mpv`. Installation on this Fedora host requires the user's sudo password; implementation proceeds independently.

One audio plays at a time. Pause/resume preserves position, speed cycles 1x / 1.5x / 2x with pitch correction, and Stop releases the process and snapshot. Selecting a different audio cancels any old preparation and stops the previous child before starting it. Playback can continue while changing chats; a compact clickable player temporarily occupies the top app-status line, preserving the footer for notices. `s` changes speed and `x` stops in Messages; header controls remain accessible elsewhere. Printable composer keys stay text. Idle/finished/error restores the usual header. Message playback rows reflect loading, playing, paused, finished or failed state. All hit regions are clipped and carry stable identity. No shortcut dump in the footer.

A request generation and full account/message key scope each command/event. Old completions cannot restart or relabel a newer player. Account changes, quit and runtime errors cancel playback. Deletion, expiry or changed attachment stops playback after storage revalidation; this also works when the original chat is off-screen. Ordinary connection loss does not stop a verified local file. Playback does not invent played receipts.

## Implementation and validation

Separate audio preparation, mpv IPC and the playback actor from reducer/UI integration. Keep the established effect model; the runtime routes audio desired state to one actor via a watch channel so rapid changes coalesce without unbounded work. Observe player events through a bounded watch channel. Use existing media transfer bounds and cancellation. No new Rust dependencies or schema migration.

Three slices: audio normalization/cache; controlled player; reducer/rendering/demo/docs. Tests use synthetic Opus/audio and fake downloads/IPC, never the linked account or real clipboard. Verify actual mpv via a null audio output when available. Test cancellation, stale account/generation, missing player, malformed input, child exit, pause/speed/progress, cache verification, expiry/deletion, narrow/wide mouse controls, draft preservation and quit cleanup. One Cargo process, two build jobs/test threads, one final independent review, release demo smoke, local merge, retain evidence, no push. Recording/sending audio, transcription, playlists and seeking are outside this release.

Reference: [mpv JSON IPC and command interface](https://mpv.io/manual/stable/#json-ipc).
