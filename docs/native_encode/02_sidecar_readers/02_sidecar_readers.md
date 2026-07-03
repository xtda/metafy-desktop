# 02 Sidecar Readers

## Objective

Extract the existing raw video and raw audio parsing into reusable streaming readers shared by encoding, transcription, tests, and future platform backends.

## Scope

- `METAFY_RAW_VIDEO_V1` parsing.
- `METAFY_RAW_AUDIO_V1` parsing.
- Existing BGRA frame validation.
- Existing PCM block validation.
- Timestamp metadata needed for cross-backend alignment.

## Deliverables

- Streaming raw video sidecar reader.
- Streaming raw audio sidecar reader.
- Shared data shapes for video frames and audio blocks.
- Focused tests for valid, empty, truncated, malformed, and unsupported sidecars.
- Existing encode path migrated to use the shared readers.

## Implementation Notes

The video reader should validate:

- Magic header.
- BGRA format code.
- Positive dimensions.
- Stable normalized frame dimensions.
- Byte count equals `width * height * 4`.
- Frame timing headers are available to the caller.

The audio reader should validate:

- Magic header.
- Block timing headers.
- Nonnegative byte counts.
- Sample metadata provided by the recording session.
- File existence and empty file behavior without panicking.

The readers should stream frames/blocks. Do not load a full recording into memory.

## Acceptance Criteria

- Encoding no longer has private ad hoc sidecar parsing.
- Tests cover raw sidecar parsing independent of FFmpeg.
- Video-only, microphone-only, source-only, and microphone + source sidecars can be enumerated through shared readers.
- Error messages identify the bad sidecar and preserve retryability.
- No backend-specific code is introduced in the readers.

## Out Of Scope

- Audio format conversion.
- Audio resampling or mixing.
- Native MP4 encoding.
- Thumbnail generation.
