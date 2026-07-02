# 15 Source Audio Backends

## Objective

Capture selected application/window source audio on macOS and Windows independently from microphone audio.

## PRD Coverage

- Application/window audio capture
- Separate microphone and source audio
- macOS source audio support
- Windows source audio support
- Local-only raw audio handling

## Deliverables

- `SourceAudioCaptureBackend` abstraction.
- macOS ScreenCaptureKit source-audio backend using a content filter that matches the selected app/window.
- Windows source-audio backend using process-tree application loopback where available.
- Windows WASAPI system-loopback fallback only when process-specific capture is unavailable or explicitly selected.
- Source-audio permission/capability status in capture validation.
- Source-audio sample format metadata for encoding.
- Default exclusion of Metafy Desktop process audio where supported.

## Implementation Notes

- Do not treat source audio as a microphone device.
- Windows may only support process-tree audio for a selected window; document that selected-window audio can include sibling windows from the same process.
- Keep source-audio callbacks lightweight and write through the same local sidecar pattern as microphone audio.
- If macOS video and audio come from the same ScreenCaptureKit stream, isolate that behind the backend boundary.

## Acceptance Criteria

- macOS can capture source audio for a selected app/window.
- Windows can capture source audio for a selected app/window process tree where supported.
- Source-audio unsupported states are reported before recording starts.
- Source audio is captured without enabling microphone capture.
- Microphone capture can be enabled or disabled independently.
- Raw source audio remains on-device.

## Out Of Scope

- Final MP4 mixing.
- Post-recording audio controls.
- Linux source audio support.
- Cloud upload or remote processing.
