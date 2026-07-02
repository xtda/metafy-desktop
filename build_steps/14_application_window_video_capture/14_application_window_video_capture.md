# 14 Application & Window Video Capture

## Objective

Capture video from selected applications and windows on macOS and Windows using the source-oriented capture config.

## PRD Coverage

- Window capture
- Application capture
- macOS desktop capture support
- Windows desktop capture support
- Local temporary video capture

## Deliverables

- Generic video target resolver for display, application, and window source ids.
- macOS source-video path for selected application/window targets.
- Windows source-video path for selected application/window targets.
- Clear start-recording failures when a selected app/window disappears.
- Capture metadata that records source kind, app name, process id, window id, frame rate, and initial output dimensions.
- Tests or fixtures for source id resolution and stale source handling.

## Implementation Notes

- Prefer one macOS backend that can eventually own both video and source audio.
- On Windows, window capture can use HWND-backed Windows Graphics Capture through the existing dependency or a dedicated backend.
- Application capture may map to one or more windows depending on platform capability; record the exact behavior in metadata.
- Keep video frames local and continue writing the raw frame stream.

## Acceptance Criteria

- macOS can record video from a selected window.
- macOS can record video from a selected application where platform filtering supports it.
- Windows can record video from a selected window.
- Windows can record video from a selected application or application-derived target.
- Closing the selected source fails or stops capture with a recoverable state.
- Existing display capture still works.

## Out Of Scope

- Source audio capture.
- Resize normalization.
- Multi-input audio encoding.
- Linux application/window capture.
