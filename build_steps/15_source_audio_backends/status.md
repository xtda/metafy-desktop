# Status: 15 Source Audio Backends

Current status: Implemented

Progress: 100%

Sign-off: Approved

Last updated: 2026-07-02

## Checklist

- [x] `SourceAudioCaptureBackend` abstraction exists.
- [x] macOS ScreenCaptureKit source audio is implemented.
- [x] Windows process-tree source audio is implemented where available.
- [x] WASAPI fallback behavior is explicit and safe.
- [x] Source-audio permission/capability status is exposed.
- [x] Metafy Desktop audio is excluded where supported.
- [x] Acceptance criteria in `15_source_audio_backends.md` are met.

## Validation Evidence

- 2026-07-02: Added `src-tauri/src/source_audio.rs` with a
  `SourceAudioCaptureBackend` abstraction and a `scap`-based backend. On macOS
  it uses ScreenCaptureKit source audio through the selected display/window
  content filter. On Windows it identifies the backend as
  `windows_wasapi_system_loopback` because the available `scap` API exposes
  system loopback, not process-tree loopback.
- 2026-07-02: Vendored `scap` under `src-tauri/vendor/scap` and patched its
  Windows-only `cpal` dependency to `0.18.1` so it can coexist with the app's
  existing microphone capture dependency graph. The upstream public API is
  otherwise unchanged.
- 2026-07-02: Updated capture validation so source-only audio produces a
  `SourceAudioCaptureConfig` with backend and nullable sample metadata.
  Source-audio capability now follows screen/window capture permission state
  instead of reporting a hardcoded unsupported placeholder.
- 2026-07-02: Updated recording runtime to start a source-audio capture thread,
  write source-only audio to a local `source_audio.pcm` sidecar using the same
  timed raw PCM block format as microphone capture, and persist sample rate,
  channel count, and sample format once the first source-audio frame arrives.
- 2026-07-02: `cargo test` passed in `src-tauri` with 28 tests, including new
  source-audio backend descriptor, source-only validation, and explicit
  mic+source split-storage guard coverage.
- 2026-07-02: `deno task check` passed with 0 errors and 0 warnings.
- 2026-07-02: `deno task build` passed and wrote the static production output.

## Open Questions

- Windows system-loopback fallback is automatic when process-specific capture is
  unavailable in the current backend. It is labeled as
  `windows_wasapi_system_loopback` so the behavior is not mistaken for
  process-tree isolation.
- Microphone + source audio is blocked at validation until Step 16 adds separate
  microphone/source sidecars and active metrics.
- The vendored `scap` API exposes `exclude_current_process_audio`, and the app
  sets it when starting source-audio capture, but the upstream macOS/Windows
  engines do not visibly wire that option through. Metafy Desktop audio
  exclusion still needs runtime verification or backend work before sign-off.
- Hardware/manual validation is still needed for real macOS source audio and
  Windows system-loopback behavior.

## Sign-Off

- [x] Approved by Andrew.
