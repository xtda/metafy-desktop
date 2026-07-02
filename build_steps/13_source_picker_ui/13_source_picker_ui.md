# 13 Source Picker & Audio Mode UI

## Objective

Update the capture configuration UI so users can choose a display, application, or window source and explicitly choose microphone audio, source audio, both, or no audio.

## PRD Coverage

- Capture source selection
- Audio mode selection
- Permission and validation feedback
- Local-only recording workflow

## Deliverables

- Source selector renamed from `Display` to `Source`.
- Grouped source options for displays, applications, and windows.
- Source labels that show app/window context where available.
- Audio mode control with:
  - No audio
  - Microphone
  - Source audio
  - Microphone + source audio
- Platform-specific disabled states for unsupported source audio.
- Separate live recording indicators for microphone audio and source audio.
- Validation messaging for disappeared app/window sources.

## Implementation Notes

- Keep the UI compact and work-focused; this is a recording console, not a setup wizard.
- Do not show unsupported audio modes as selectable.
- Keep source changes disabled during active recording.
- Do not ask users to avoid resizing captured windows.

## Acceptance Criteria

- A user can select a display, application, or window source when available.
- A user can select microphone-only, source-only, combined, or silent capture.
- Unsupported source-audio modes are clearly unavailable.
- Active recording UI shows separate microphone and source-audio status.
- Stale selected sources are recoverable without app restart.

## Out Of Scope

- Native capture backend implementation.
- Post-recording audio mixer controls.
- Video preview.
- Recording library redesign.
