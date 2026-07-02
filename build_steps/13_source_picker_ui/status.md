# Status: 13 Source Picker & Audio Mode UI

Current status: Implemented

Progress: 100%

Sign-off: Approved

Last updated: 2026-07-02

## Checklist

- [x] Source selector supports grouped display/application/window options.
- [x] Audio mode control exists.
- [x] Unsupported source-audio states are visible and safe.
- [x] Active recording shows separate mic/source audio status.
- [x] Stale source validation is shown in the UI.
- [x] Acceptance criteria in `13_source_picker_ui.md` are met within the UI scope.

## Validation Evidence

- 2026-07-02: Updated `src/routes/+page.svelte` so the capture selector is
  labeled `Source` and renders `videoSources` in grouped Displays,
  Applications, and Windows optgroups. The control falls back to legacy
  `displays` data and keeps a stale selected source visible as a disabled
  missing-source option so users can choose another source without restarting.
- 2026-07-02: Added a compact audio-mode control for no audio, microphone,
  source audio, and microphone + source audio. The save payload now persists
  the selected `audioMode` and derives `includeMicrophone` from that mode.
- 2026-07-02: Unsupported microphone/source-audio modes are disabled from the
  capability response. Current source-audio modes are visible but disabled with
  the backend-provided explanation until Step 15 implements source audio.
- 2026-07-02: Active recording metrics now show separate Mic audio and Source
  audio indicators instead of a single combined audio byte count.
- 2026-07-02: Added an in-console `Refresh sources` action beside permission
  request so stale app/window selections can be recovered after the source list
  changes.
- 2026-07-02: `deno task check` passed with 0 errors and 0 warnings.
- 2026-07-02: `deno task build` passed and wrote the static production output.
- 2026-07-02: `cargo test` passed in `src-tauri` with 19 tests.

## Open Questions

- Unsupported source-audio modes are shown as disabled choices with the
  platform/backend explanation from `captureStatus.sourceAudio`.
- Native application enumeration and application/window video capture remain
  deferred to Step 14; the grouped UI will render application sources when the
  backend returns them.
- Source-audio capture remains deferred to Step 15; Step 13 only exposes the
  mode choice and disabled capability feedback.

## Sign-Off

- [x] Approved by Andrew.
