# 16 Split Audio Session Storage

## Objective

Update recording sessions, temporary files, and metadata so microphone audio and source audio are stored and tracked separately.

## PRD Coverage

- Separate microphone and source audio tracks
- Recoverable temporary media
- SQLite recording session metadata
- Local-only raw media storage
- Failure recovery

## Deliverables

- Recording session schema fields for:
  - `video_source_id`
  - `video_source_kind`
  - `audio_mode`
  - `microphone_audio_path`
  - `source_audio_path`
  - microphone audio byte count, sample rate, channels, and sample format
  - source audio byte count, sample rate, channels, and sample format
- Backward-compatible reads for existing `audio_path` as microphone audio.
- Temporary media layout with `microphone.pcm` and `source_audio.pcm`.
- Active recording snapshots with separate microphone and source-audio metrics.
- Session sidecar metadata that describes both audio streams.
- Recovery logic that preserves both audio sidecars after failures.

## Implementation Notes

- Avoid overloading `include_microphone` to mean any audio.
- Existing completed recordings should remain readable.
- Keep raw media paths out of optional AI payloads.
- Prefer additive migrations with compatibility shims over destructive schema rewrites.

## Acceptance Criteria

- Recordings can be created with no audio, microphone-only, source-only, or microphone + source audio.
- Microphone and source audio write to separate local files.
- Active recording state reports each audio stream independently.
- Failed recordings preserve both audio files when present.
- Existing recordings with old `audio_path` metadata still load.

## Out Of Scope

- Native source-audio backend internals.
- FFmpeg mixing.
- Transcript model changes beyond consuming final MP4 output.
- Post-recording track editing.
