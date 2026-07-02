# 12 Capture Source Model

## Objective

Replace the current display-only capture model with a source-oriented model that can represent displays, applications, and windows on macOS and Windows.

## PRD Coverage

- Display, application, and window source selection
- Local capture preferences
- Platform capability detection
- Backward-compatible capture configuration

## Deliverables

- `CaptureVideoSource` model with source kind, id, title, app name, process id, and window id fields.
- Source id parsing for:
  - `display:<native_id>`
  - `application:<native_id>`
  - `window:<native_id>`
- Capture status response that returns source-oriented lists instead of display-only lists.
- Validation that distinguishes missing permission, missing source, unsupported source audio, and disappeared source.
- Local preference storage for selected video source and audio mode.
- Backward-compatible read path for existing `screen_source_id` preferences and recording session rows.

## Implementation Notes

- Keep source ids prefixed so old display ids cannot collide with application or window ids.
- Avoid committing to one native backend at the public command boundary.
- Keep platform-specific native handles out of serialized frontend payloads.
- Treat Linux as display-only unless a later platform pass expands it.

## Acceptance Criteria

- Capture status can return display, application, and window source entries where supported.
- Existing display preferences still load.
- Invalid or stale source ids produce clear validation errors.
- Recording commands receive a validated source-oriented capture config.
- No recording, encoding, transcription, or AI payload sends raw media off-device.

## Out Of Scope

- Actual application/window video capture.
- Source audio capture.
- UI redesign beyond payload compatibility.
- Final MP4 encoding changes.
