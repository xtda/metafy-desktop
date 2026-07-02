# 18 Multi-Input Encoding & Validation

## Objective

Encode recordings that may contain zero, one, or two audio sources, then validate source capture behavior across macOS and Windows.

## PRD Coverage

- FFmpeg MP4 encoding
- Microphone and source-audio mixing
- Local playback
- Cross-platform validation
- Local-only raw media handling

## Deliverables

- FFmpeg command builder support for:
  - video-only recordings
  - microphone-only recordings
  - source-only recordings
  - microphone + source recordings
- Deterministic audio mix for microphone + source audio.
- Warnings for missing, unsupported, or silent requested audio sources.
- Recording metadata update after multi-input encode.
- Playback verification for mixed-audio MP4 output.
- Manual validation checklist for macOS and Windows source video/audio capture.
- Regression checks that optional AI payloads still exclude raw media paths.

## Implementation Notes

- Use FFmpeg for audio mixing instead of mixing PCM in Rust.
- Preserve source sidecars until final encode succeeds.
- Keep transcription pointed at the final MP4 or an explicit transcription mixdown.
- If separate MP4 audio tracks are added later, keep the MVP playback path simple with a default mixed track.

## Acceptance Criteria

- Video-only recordings encode and play.
- Microphone-only recordings encode and play.
- Source-only recordings encode and play.
- Microphone + source recordings encode and play with both sources audible.
- Encoding failure preserves temporary media for retry.
- macOS manual validation covers app/window video, source audio, mic + source audio, and resize.
- Windows manual validation covers app/window video, source audio, mic + source audio, process-loopback limitations, and resize.

## Out Of Scope

- Post-recording audio mixer UI.
- Separate editable audio tracks.
- Cloud upload.
- Linux source-audio validation.
