# Status: 14 Application & Window Video Capture

Current status: Implemented

Progress: 100%

Sign-off: Approved

Last updated: 2026-07-02

## Checklist

- [x] Generic video target resolver exists.
- [x] macOS window video capture path is implemented through `open-gpui-scap`
  ScreenCaptureKit window targets.
- [x] macOS application video capture behavior is explicitly constrained.
- [x] Windows window video capture path is implemented through `open-gpui-scap`
  Windows Graphics Capture HWND targets.
- [x] Windows application video capture behavior is explicitly constrained.
- [x] Stale source capture failures are recoverable.
- [x] Existing display capture still passes automated validation.
- [x] Acceptance criteria in `14_application_window_video_capture.md` are met.

## Validation Evidence

- 2026-07-02: Updated `src-tauri/src/recorder.rs` with a generic video
  target resolver for `display:` and `window:` source ids. The resolver maps
  validated source ids to native `scap::Target` display/window targets, rejects
  unsupported `application:` targets with a recoverable error, and reports stale
  selected targets as no longer available before capture starts.
- 2026-07-02: Updated `src-tauri/src/capture.rs` so selected window video
  sources pass capture validation. Application capture remains explicitly
  unavailable with the current backend and directs users to select a concrete
  window instead.
- 2026-07-02: Updated recording session persistence to schema version 9 with
  source metadata fields for source kind, title, app name, process id, and
  window id. Recording sidecar metadata now includes selected source metadata
  and initial output dimensions alongside frame rate and media paths.
- 2026-07-02: `cargo test` passed in `src-tauri` with 24 tests, including
  window-source validation, resolver parsing, stale target matching, storage
  migration, and synthetic display encoding coverage.
- 2026-07-02: `deno task check` passed with 0 errors and 0 warnings.
- 2026-07-02: `deno task build` passed and wrote the static production output.
- 2026-07-02: Andrew accepted Step 14.

## Open Questions

- The current `open-gpui-scap` backend exposes display and window targets but
  does not expose application targets or owning application metadata. For Step
  14, application capture is explicitly constrained to selecting a concrete
  window from the application.
- A future application-capture backend must decide whether an application source
  means all visible windows for an application or a single main/foreground
  window, then persist that behavior in session metadata.

## Sign-Off

- [x] Approved by Andrew.
