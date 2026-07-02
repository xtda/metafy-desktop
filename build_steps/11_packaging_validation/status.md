# Status: 11 Packaging & Validation

Current status: In Progress

Progress: 50%

Sign-off: Pending

Last updated: 2026-07-02

## Checklist

- [x] Release build configuration exists.
- [x] Platform packaging notes exist.
- [x] FFmpeg/Whisper resource strategy is audited.
- [ ] Offline core workflow validation is complete.
- [ ] 1080p 30 FPS recording validation is complete.
- [ ] Failure recovery validation is complete.
- [ ] Acceptance criteria in `11_packaging_validation.md` are met.

## Validation Evidence

- Added `deno task bundle:app` / `npm run bundle:app` as the macOS release app
  bundle entrypoint.
- Added `build_steps/11_packaging_validation/packaging_notes.md` with macOS,
  Windows, Linux, FFmpeg, Whisper, offline, and privacy packaging notes.
- Added `build_steps/11_packaging_validation/manual_validation_checklist.md`
  for packaged-app MVP sign-off.
- Added shared runtime binary discovery in `src-tauri/src/binaries.rs`.
  FFmpeg, FFprobe, and whisper.cpp now resolve via explicit env var, `PATH`,
  then common system install locations. This covers Finder-launched macOS apps
  that do not inherit the Homebrew PATH.
- `deno task check` passed with 0 Svelte errors and 0 warnings.
- `cargo test` passed: 14 tests passed, 0 failed. Coverage includes synthetic
  FFmpeg MP4/thumbnail encode, local Whisper with fake binaries, transcript
  parsing, local search, durable job state, storage initialization, and optional
  AI media guardrails.
- `deno task build` passed and wrote the static frontend to `build`.
- `deno task bundle:app` passed and wrote
  `src-tauri/target/release/bundle/macos/Metafy Desktop.app`.
- Bundle audit: the macOS `.app` is 14M and contains `Info.plist`,
  `Contents/MacOS/metafy-desktop`, and `Contents/Resources/icon.icns`.
- `Info.plist` includes `CFBundleIdentifier = gg.metafy.desktop`,
  `CFBundleShortVersionString = 0.1.0`,
  `NSMicrophoneUsageDescription`, and `NSScreenCaptureUsageDescription`.
- Runtime tool audit on this machine:
  `/opt/homebrew/bin/ffmpeg` is FFmpeg 8.1.2,
  `/opt/homebrew/bin/ffprobe` is FFprobe 8.1.2, and
  `/opt/homebrew/bin/whisper-cli --help` exits successfully.
- Source privacy audit: HTTP client usage is confined to optional AI summaries.
  Core local defaults report `coreNetworkRequired = false` and
  `rawMediaLeavesDevice = false`.
- Resource audit: the current `.app` does not bundle FFmpeg, FFprobe,
  whisper.cpp, or Whisper model files. These remain local system/user resources.

Not completed in this environment:

- Packaged-app workflow with network disabled.
- Hardware validation for 1080p 30 FPS capture.
- Microphone sync review from a real captured recording.
- Real default-model Whisper transcription against a captured recording.
- Packaged-app failure/retry validation.
- Windows and Linux package validation.

## Open Questions

- Which macOS, Windows, and Linux machines should be used for final packaged-app
  validation?
- Is the MVP distribution expected to keep FFmpeg, FFprobe, and whisper.cpp as
  system prerequisites, or should later release work bundle sidecars?

## Sign-Off

- [ ] Approved by Andrew.
