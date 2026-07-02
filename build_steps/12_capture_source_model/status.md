# Status: 12 Capture Source Model

Current status: Implemented

Progress: 100%

Sign-off: Approved

Last updated: 2026-07-02

## Checklist

- [x] `CaptureVideoSource` model exists.
- [x] Source id parsing supports display, application, and window prefixes.
- [x] Capture status returns source-oriented payloads.
- [x] Capture validation handles stale or unsupported sources.
- [x] Capture preferences persist selected source and audio mode.
- [x] Existing `screen_source_id` data remains readable.
- [x] Acceptance criteria in `12_capture_source_model.md` are met.

## Validation Evidence

- 2026-07-02: Implemented `CaptureVideoSource` and `CaptureVideoSourceKind`
  in `src-tauri/src/capture.rs` with `display:`, `application:`, and
  `window:` source id parsing. `CaptureStatus` now returns `videoSources`,
  `sourceAudio`, and source-oriented validated config while retaining
  `displays`/`screenSource` compatibility fields.
- 2026-07-02: `capture_status` enumerates display sources and backend window
  targets on macOS/Windows when screen permission is granted. Application
  sources are modeled at the command boundary, but native app enumeration is
  intentionally deferred to Step 14.
- 2026-07-02: Capture validation now distinguishes missing capture permission,
  no video sources, invalid/stale selected source ids, non-display video
  sources that are not capturable until Step 14, and source-audio modes that
  are unsupported until Step 15.
- 2026-07-02: Updated `capture_preferences` to schema version 8 with
  `video_source_id` and `audio_mode`; reads coalesce legacy `screen_source_id`
  into `videoSourceId`, and recording session payloads expose `videoSourceId`
  while retaining `screenSourceId`.
- 2026-07-02: Updated `src/routes/+page.svelte` to consume `videoSourceId`,
  save explicit `audioMode: "microphone"`, and gate recording start on the
  validated source-oriented capture config without implementing the Step 13
  source picker redesign.
- 2026-07-02: Svelte MCP autofixer found no issues for the updated capture
  binding/type slice.
- 2026-07-02: `cargo test` passed in `src-tauri` with 19 tests, including
  source id parsing, stale source validation, unsupported source audio
  validation, video-source preference compatibility, and audio-mode persistence.
- 2026-07-02: `deno task check` passed with 0 errors and 0 warnings.
- 2026-07-02: `deno task build` passed and wrote the static production output.

## Open Questions

- Application ids are accepted as opaque `application:<native_id>` values at the
  command boundary. The concrete macOS/Windows native id choice remains deferred
  to Step 14, where application enumeration/capture backends are implemented.
- Source audio modes are persisted and validated, but source audio capture is
  intentionally unavailable until Step 15.

## Sign-Off

- [x] Approved by Andrew.
