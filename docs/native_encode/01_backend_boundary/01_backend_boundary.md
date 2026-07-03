# 01 Backend Boundary

## Objective

Introduce a platform-neutral encoder backend boundary so FFmpeg becomes a temporary implementation detail rather than the shape of the encoding system.

## Scope

- `src-tauri/src/encoding.rs`
- Current `EncodingResult`
- Job completion and retry paths in `src-tauri/src/jobs.rs`
- Backend-neutral diagnostics for encode jobs
- Module scaffold for future media processing code

## Deliverables

- New media-processing module scaffold under `src-tauri/src/media/`.
- Backend-neutral `EncodeInput`, `EncodeOutput`, and diagnostic structs.
- `RecordingEncoder` trait or equivalent narrow interface.
- Current FFmpeg encode path wrapped as a temporary backend.
- Existing public Tauri commands continue to work.
- Existing job retry and failure-preservation behavior remains intact.

## Implementation Notes

Suggested module shape:

```text
src-tauri/src/media/
  mod.rs
  encode.rs
  sidecar.rs
  audio.rs
  wav.rs
  thumbnail.rs
  metadata.rs
  backends/
    mod.rs
    ffmpeg.rs
    macos.rs
    windows.rs
    linux_gstreamer.rs
```

Keep platform-specific media API types out of the public backend trait. The job system should only know that an encode either succeeded with output paths and metadata, or failed with a retryable error string.

The FFmpeg backend can continue returning command diagnostics during the migration, but new structs should not require every backend to expose command-line arguments.

## Acceptance Criteria

- Existing encode command behavior is unchanged.
- Existing FFmpeg synthetic encode tests still pass.
- Encoding failures still preserve raw sidecars and temp files needed for retry.
- The new backend contract does not mention FFmpeg, FFprobe, AVFoundation, Media Foundation, or GStreamer types.
- Platform backends can be added without changing Tauri command signatures.

## Out Of Scope

- Removing FFmpeg.
- Replacing audio mixing.
- Implementing macOS, Windows, or Linux backends.
- Changing final MP4 output format.
