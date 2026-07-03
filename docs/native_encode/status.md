# Native Encoding Status

Last updated: 2026-07-02

Current phase: Step 10 Remove FFmpeg review

Overall progress: 85%

Overall sign-off: Pending

## Step Tracker

| Step | Status | Progress | Sign-Off | Step Brief | Detail Status |
| --- | --- | ---: | --- | --- | --- |
| 01 Backend Boundary | Accepted | 100% | Accepted | [Brief](01_backend_boundary/01_backend_boundary.md) | [Status](01_backend_boundary/status.md) |
| 02 Sidecar Readers | Not Started | 0% | Pending | [Brief](02_sidecar_readers/02_sidecar_readers.md) | [Status](02_sidecar_readers/status.md) |
| 03 Audio Mixdown | Accepted | 100% | Accepted | [Brief](03_audio_mixdown/03_audio_mixdown.md) | [Status](03_audio_mixdown/status.md) |
| 04 Transcription WAV | Accepted | 100% | Accepted | [Brief](04_transcription_wav/04_transcription_wav.md) | [Status](04_transcription_wav/status.md) |
| 05 Metadata & Thumbnail | Accepted | 100% | Accepted | [Brief](05_metadata_thumbnail/05_metadata_thumbnail.md) | [Status](05_metadata_thumbnail/status.md) |
| 06 macOS Native Encoder | Accepted | 100% | Accepted | [Brief](06_macos_native_encoder/06_macos_native_encoder.md) | [Status](06_macos_native_encoder/status.md) |
| 07 Windows Native Encoder | Accepted | 70% | Accepted | [Brief](07_windows_native_encoder/07_windows_native_encoder.md) | [Status](07_windows_native_encoder/status.md) |
| 08 Linux GStreamer Encoder | In Progress | 65% | Pending | [Brief](08_linux_gstreamer_encoder/08_linux_gstreamer_encoder.md) | [Status](08_linux_gstreamer_encoder/status.md) |
| 09 Backend Selection & Readiness | Accepted | 100% | Accepted | [Brief](09_backend_selection_readiness/09_backend_selection_readiness.md) | [Status](09_backend_selection_readiness/status.md) |
| 10 Remove FFmpeg | Ready for Review | 100% | Pending | [Brief](10_remove_ffmpeg/10_remove_ffmpeg.md) | [Status](10_remove_ffmpeg/status.md) |
| 11 Cross-Platform Validation | Not Started | 0% | Pending | [Brief](11_cross_platform_validation/11_cross_platform_validation.md) | [Status](11_cross_platform_validation/status.md) |

## Milestone Gates

- [x] Step 01 approved: backend boundary is stable.
- [ ] Step 02 approved: sidecar readers are reusable and covered.
- [x] Step 03 approved: shared audio mixdown replaces FFmpeg audio policy.
- [x] Step 04 approved: transcription no longer invokes FFmpeg.
- [x] Step 05 approved: metadata and thumbnail generation no longer need FFprobe or FFmpeg.
- [x] Step 06 approved: macOS native encode path works.
- [x] Step 07 approved: Windows native encoder implementation is accepted; packaged runtime validation remains a follow-up.
- [ ] Step 08 approved: Linux GStreamer encode path works.
- [x] Step 09 approved: backend readiness and diagnostics are product-ready.
- [ ] Step 10 approved: FFmpeg and FFprobe are removed from product code and packaging.
- [ ] Step 11 approved: packaged platform validation is complete.

## Current Focus

Review Step 10 Remove FFmpeg. Step 08 Linux-hosted compile and synthetic encode validation remains pending separately, and Step 11 packaged cross-platform validation remains the next validation gate.

## Recent Updates

- 2026-07-02: Created native encoding step tracker from the initial FFmpeg removal plan.
- 2026-07-02: Completed Step 01 implementation: added backend-neutral media encode contracts, wrapped the current FFmpeg path as a temporary backend, and validated with `cargo check`, `cargo test encoding::tests`, and `cargo test`.
- 2026-07-02: Andrew accepted Step 01; current phase moved to Step 02 Sidecar Readers.
- 2026-07-02: Completed Step 03 implementation: moved final-encode audio preparation into shared Rust mixdown logic, made the temporary FFmpeg backend consume one prepared `f32le` 48 kHz stereo stream, and validated with `cargo test media::audio`, `cargo test encoding::tests`, `cargo test media::sidecar`, and `cargo test`.
- 2026-07-02: Andrew accepted Step 03; current phase moved to Step 04 Transcription WAV.
- 2026-07-02: Completed Step 04 implementation: transcription now writes `transcript-audio.wav` directly from requested local sidecars using the shared mixdown path, removes transcription FFmpeg diagnostics, and validates with `cargo test transcription::tests`, `cargo test media::audio`, `cargo test media::wav`, `cargo test encoding::tests`, `cargo test`, and `deno task check`.
- 2026-07-02: Andrew accepted Step 04; current phase moved to Step 05 Metadata & Thumbnail.
- 2026-07-02: Completed Step 05 implementation: encode completion now derives `MediaInfo` without FFprobe, writes thumbnails from the first raw BGRA sidecar frame without FFmpeg thumbnail extraction, and validates with `cargo test media::metadata`, `cargo test media::thumbnail`, `cargo test encoding::tests`, `cargo test`, and `deno task check`.
- 2026-07-02: Andrew accepted Step 05; current phase moved to Step 06 macOS Native Encoder.
- 2026-07-02: Advanced Step 06 with a macOS AVFoundation H.264/AAC encoder backend, native BGRA frame submission, shared mixed-audio AAC encode, FFprobe-free metadata, FFmpeg-free thumbnails, native synthetic encode tests, full Rust test coverage, and a debug `.app` bundle build. Finder-launched packaged-app validation remains pending.
- 2026-07-02: Andrew accepted Step 06; current phase moved to Step 07 Windows Native Encoder.
- 2026-07-02: Advanced Step 07 with a Windows Media Foundation Sink Writer backend, H.264 MP4 output, RGB32/BGRA input with NV12 fallback conversion, AAC audio from the shared prepared mixdown, FFprobe-free metadata, FFmpeg-free thumbnails, cfg-gated backend selection, full Rust/macOS validation, and isolated Windows-target type-check coverage. Packaged Windows runtime validation remains pending.
- 2026-07-02: Andrew accepted Step 07; current phase moved to Step 08 Linux GStreamer Encoder. Packaged Windows runtime validation remains a follow-up item.
- 2026-07-02: Advanced Step 08 with a Linux-only GStreamer backend implementation, appsrc-fed BGRA video and prepared 48 kHz stereo audio pipelines, H.264/AAC MP4 muxing, plugin readiness diagnostics, FFprobe-free metadata, FFmpeg-free thumbnails, and macOS-host validation. Linux-host compile and GStreamer synthetic encode validation remain pending.
- 2026-07-02: Completed Step 09 implementation: added media backend readiness diagnostics to `app_bootstrap`, routed macOS/Windows/Linux readiness to AVFoundation/Media Foundation/GStreamer, made unsupported-target FFmpeg fallback require the explicit `temporary-ffmpeg-fallback` Cargo feature, removed normal UI/bootstrap FFmpeg setup text, and validated with `cargo fmt --check`, `cargo check`, `cargo test media::readiness::tests`, `cargo test encoding::tests`, and `deno task check`.
- 2026-07-02: Andrew accepted Step 09; current phase moved to Step 10 Remove FFmpeg.
- 2026-07-02: Completed Step 10 implementation: deleted the FFmpeg backend and fallback feature, removed FFprobe metadata parsing, renamed encode result diagnostics to backend-neutral fields, removed FFmpeg/FFprobe binary/download support, updated active product docs to native macOS, native Windows, Linux GStreamer, and whisper.cpp setup, and validated with `cargo fmt --check`, `cargo check`, `cargo test`, `cargo test encoding::tests`, `deno task check`, `deno task build`, and a Whisper-only binary-download dry-run.

## Sign-Off Notes

Update this file whenever a step status changes. The detailed `status.md` inside each step folder remains the source of truth for validation evidence and Andrew's per-step approval.
