# Status: 07 Windows Native Encoder

Current status: Accepted

Progress: 70%

Sign-off: Accepted

Last updated: 2026-07-02

## Checklist

- [x] Windows backend compiles behind platform cfg.
- [x] Media Foundation Sink Writer path exists.
- [x] BGRA/RGB32 or converted video frames encode to H.264.
- [x] Shared mixed audio encodes to AAC when present.
- [x] Metadata is populated without FFprobe.
- [x] Thumbnail path works without FFmpeg.
- [ ] Packaged Windows validation passes without FFmpeg/FFprobe.
- [ ] Acceptance criteria in `07_windows_native_encoder.md` are met.

## Validation Evidence

- 2026-07-02: Added `media::backends::windows::WindowsRecordingEncoder` behind `target_os = "windows"` using the `windows` crate and Media Foundation Sink Writer.
- 2026-07-02: `encoding.rs` now selects the Media Foundation backend on Windows, keeps AVFoundation on macOS, and leaves the temporary FFmpeg backend for remaining non-native targets.
- 2026-07-02: Windows backend initializes COM and Media Foundation per encode, writes H.264 MP4 video through RGB32/BGRA input when accepted, and falls back to BGRA-to-NV12 conversion if RGB32 input setup fails.
- 2026-07-02: Windows backend converts the shared prepared `f32` 48 kHz stereo mixdown to PCM16 samples for Media Foundation AAC encode at the existing `160k` target.
- 2026-07-02: Metadata remains populated through backend-neutral `derive_media_info`; `ffprobePath`, `ffprobeJson`, and backend command diagnostics stay empty for the native Windows path.
- 2026-07-02: Thumbnail generation uses the accepted shared raw-BGRA JPEG path from Step 05, so Windows encode completion does not invoke FFmpeg for thumbnails.
- 2026-07-02: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` passed.
- 2026-07-02: `cargo check --manifest-path src-tauri/Cargo.toml` passed. Existing warnings remain from vendored `scap` and unused resize-normalization debug fields.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml encoding::tests` passed: 10 tests, covering storage/session encode behavior across video-only, microphone-only, source-only, and microphone + source modes on the current macOS host path, with Windows-native assertions cfg-gated for Windows.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml` passed: 72 tests.
- 2026-07-02: `deno task check` passed: `svelte-check found 0 errors and 0 warnings`.
- 2026-07-02: `cargo check --manifest-path /tmp/metafy-windows-check/Cargo.toml --target x86_64-pc-windows-msvc --tests` passed in a temporary isolated crate that includes `src-tauri/src/media/backends/windows.rs`; this type-checks the Windows Media Foundation module and its Windows synthetic tests without pulling in Tauri or `ring`.
- 2026-07-02: Full `cargo check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc` could not reach repo code on this macOS machine because `ring` failed to compile for the Windows MSVC target without the Windows C headers/toolchain (`assert.h` missing).
- 2026-07-02: Andrew approved Step 07 implementation status. Packaged Windows runtime validation remains a follow-up item.

## Open Questions

- Runtime packaged-app validation on Windows remains required to prove actual H.264/AAC encode, playback in Windows media player, and operation without `ffmpeg.exe` or `ffprobe.exe`.
- The backend attempts RGB32/BGRA first and falls back to NV12 conversion if Media Foundation rejects the RGB32 input type; real Windows hardware/runtime validation should record which path is used.

## Sign-Off

- [x] Approved by Andrew on 2026-07-02.
