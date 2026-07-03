# Status: 08 Linux GStreamer Encoder

Current status: In Progress

Progress: 65%

Sign-off: Pending

Last updated: 2026-07-02

## Checklist

- [ ] Linux GStreamer backend compiles behind platform cfg.
- [x] GStreamer readiness checks exist.
- [x] App-fed BGRA video encodes to H.264 MP4.
- [x] Shared mixed audio encodes to AAC when present.
- [x] Missing plugin errors are actionable.
- [x] Metadata is populated without FFprobe.
- [x] Thumbnail path works without FFmpeg.
- [ ] Acceptance criteria in `08_linux_gstreamer_encoder.md` are met.

## Validation Evidence

- 2026-07-02: Added Linux-only GStreamer Rust dependencies in `src-tauri/Cargo.toml`: `gstreamer`, `gstreamer-app`, `gstreamer-audio`, and `gstreamer-video`.
- 2026-07-02: Added `media::backends::linux_gstreamer::LinuxGstreamerRecordingEncoder` behind `target_os = "linux"`.
- 2026-07-02: Linux encode routing now selects `LinuxGstreamerRecordingEncoder` on Linux; the temporary FFmpeg backend remains only for non-macOS, non-Windows, non-Linux fallback targets.
- 2026-07-02: Implemented GStreamer readiness checks for required appsrc/video/audio/mux elements plus H.264 and AAC encoder candidate selection. Missing-plugin errors include distro package guidance for Ubuntu/Debian and Fedora.
- 2026-07-02: Implemented appsrc-fed BGRA video pipeline with explicit PTS/duration per frame, H.264 parse, MP4 mux, and filesink output.
- 2026-07-02: Implemented optional appsrc-fed prepared 48 kHz stereo `f32` audio pipeline with explicit timestamps, AAC parse, and MP4 muxing when shared mixed audio exists.
- 2026-07-02: Metadata remains populated through backend-neutral `derive_media_info`; `ffprobePath`, `ffprobeJson`, and command diagnostics stay empty for the Linux GStreamer path.
- 2026-07-02: Thumbnail generation uses the accepted shared raw-BGRA JPEG path from Step 05, so Linux encode completion does not invoke FFmpeg for thumbnails.
- 2026-07-02: `pkg-config --modversion gstreamer-1.0 gstreamer-app-1.0 gstreamer-audio-1.0 gstreamer-video-1.0` failed on this macOS host because GStreamer development packages are not installed locally.
- 2026-07-02: `rustup target list --installed` showed no `x86_64-unknown-linux-gnu` target on this macOS host, so Linux target compilation was not run.
- 2026-07-02: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` passed.
- 2026-07-02: `cargo check --manifest-path src-tauri/Cargo.toml` passed on the current macOS host. Existing warnings remain from vendored `scap` and unused resize-normalization debug fields.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml encoding::tests` passed: 10 tests.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml` passed: 72 tests.
- 2026-07-02: `deno task check` passed: `svelte-check found 0 errors and 0 warnings`.

## Open Questions

- Linux runtime validation remains required on an environment with GStreamer development/runtime packages and H.264/AAC plugins installed.
- Baseline package guidance currently documented in readiness errors: Ubuntu/Debian `gstreamer1.0-tools`, `gstreamer1.0-plugins-base`, `gstreamer1.0-plugins-good`, `gstreamer1.0-plugins-bad`, `gstreamer1.0-plugins-ugly`, `gstreamer1.0-libav`; Fedora `gstreamer1`, `gstreamer1-plugins-base`, `gstreamer1-plugins-good`, `gstreamer1-plugins-bad-free`, `gstreamer1-plugins-ugly-free`, `gstreamer1-libav`.
- Linux video-only and microphone-only synthetic encode tests need to run on Linux before marking the compile and acceptance criteria complete.
- Final MP4 playback in the app and a standard Linux media player remains unvalidated.

## Sign-Off

- [ ] Approved by Andrew.
