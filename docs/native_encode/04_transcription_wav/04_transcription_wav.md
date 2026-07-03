# 04 Transcription WAV

## Objective

Remove FFmpeg from transcription by writing `transcript-audio.wav` directly from local recording sidecars.

## Scope

- `src-tauri/src/transcription.rs`
- Shared audio sidecar selection.
- Shared audio mixdown from Step 03.
- WAV writing for `whisper.cpp`.
- Transcription job retry behavior.

## Deliverables

- Direct 16 kHz mono signed 16-bit WAV writer.
- Transcription path that selects the same requested audio sidecars used for final encode.
- Transcription path that no longer shells out to FFmpeg.
- Updated transcription result diagnostics that do not expose `ffmpeg_path` or `ffmpeg_args` after compatibility cleanup.
- Tests for microphone-only, source-only, microphone + source, and no-audio transcription prep.

## Implementation Notes

The transcription path should:

1. Load recording and session metadata.
2. Select requested audio sidecars.
3. Run shared conversion/resampling/mixdown.
4. Write `transcript-audio.wav` as 16 kHz mono PCM.
5. Run local `whisper.cpp` against that WAV.

Video-only recordings should fail transcription gracefully with a clear no-audio message.

This step can keep `whisper.cpp` binary discovery unchanged. It should only remove FFmpeg from the audio extraction portion.

## Acceptance Criteria

- `extract_recording_audio` or its replacement does not invoke FFmpeg.
- `transcript-audio.wav` is generated directly from sidecars.
- Transcription retry still works.
- Raw Whisper JSON storage and segment parsing behavior remains unchanged.
- Tests prove FFmpeg is not needed for transcription prep.

## Out Of Scope

- Replacing `whisper.cpp`.
- Changing transcript storage schema unless compatibility requires it.
- Final MP4 encoding.
