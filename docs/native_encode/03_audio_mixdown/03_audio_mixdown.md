# 03 Audio Mixdown

## Objective

Move PCM conversion, resampling, channel normalization, and microphone/source mixing out of FFmpeg and into shared app-owned logic.

## Scope

- Microphone PCM sidecars.
- Source-audio PCM sidecars.
- Missing, empty, silent, and unsupported audio warning behavior.
- Final encode audio preparation.
- Shared mix policy for macOS, Windows, and Linux.

## Deliverables

- Audio sample conversion into an internal `f32` representation.
- Resampling to 48 kHz for final AAC encode.
- Channel normalization to stereo for final encode.
- Deterministic one-source passthrough behavior.
- Deterministic two-source equal-gain mix behavior.
- Timeline alignment based on sidecar timing headers.
- Tests covering the audio modes and edge cases currently covered by FFmpeg command tests.

## Implementation Notes

Supported input sample formats should match current capture metadata:

- `i8`, `u8`
- `i16`, `u16`
- `i24`, `u24`
- `i32`, `u32`
- `f32`
- `f64`

Required mix behavior:

- One valid source passes through after conversion, resampling, and channel normalization.
- Two valid sources mix with deterministic equal gain.
- Mixed duration uses the longest source.
- Leading gaps become silence.
- Shorter sources are padded with silence.
- Missing or invalid optional sources warn and omit only the affected source.

Use a small Rust DSP/resampling dependency if it materially reduces risk. Keep the app's mix policy explicit and covered by tests.

## Acceptance Criteria

- FFmpeg `aresample`, `aformat`, `amix`, and `volume` behavior is replaced by app-owned logic.
- Prepared final-encode audio is 48 kHz stereo.
- All four audio modes produce predictable prepared audio or clear no-audio output.
- Missing, silent, unsupported, or incomplete audio sources continue to warn without unnecessarily dropping other valid sources.
- Audio tests do not require FFmpeg.

## Out Of Scope

- Writing the final MP4.
- Writing the 16 kHz Whisper WAV.
- Platform-specific AAC encoding.
- Post-recording mixer UI.
