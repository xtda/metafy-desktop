# Status: 11 Packaging & Validation

Current status: In Progress

Progress: 50%

Sign-off: Pending

Last updated: 2026-07-02

## Checklist

- [x] Release build configuration exists.
- [x] Platform packaging notes exist.
- [x] Whisper resource strategy is audited.
- [ ] Offline core workflow validation is complete.
- [ ] 1080p 30 FPS recording validation is complete.
- [ ] Failure recovery validation is complete.
- [ ] Acceptance criteria in `11_packaging_validation.md` are met.

## Validation Evidence

- Added `deno task bundle:app` / `npm run bundle:app` as the macOS release app
  bundle entrypoint.
- Added `build_steps/11_packaging_validation/packaging_notes.md` with macOS,
  Windows, Linux, native/GStreamer encoding, Whisper, offline, and privacy
  packaging notes.
- Added `build_steps/11_packaging_validation/manual_validation_checklist.md` for
  packaged-app MVP sign-off.
- Added shared runtime binary discovery in `src-tauri/src/binaries.rs`.
  Historical Step 11 evidence covered command encoder tools; current product
  code uses this path for whisper.cpp discovery only.
- Added optional bundled binary archive support. Tauri now includes
  `src-tauri/resources/binaries`, and app startup extracts current-platform
  `.zip` archives into `app-data/tools/v1/<os-arch>` before runtime lookup.
- Added `deno task binaries:download` and `deno task binaries:download:windows`
  to populate local Windows x64 whisper.cpp archives for package testing
  without committing third-party binaries.
- Historical: `deno task binaries:download -- --platform windows-x86_64 --dry-run`
  previously selected a command encoder archive and `whisper-bin-x64.zip`.
  The current download task resolves only `whisper-bin-x64.zip`.
- `cargo check` passed after adding the bundled archive extractor and `zip`
  dependency.
- `cargo test` passed: 43 tests passed, 0 failed.
- `deno task check` passed with 0 Svelte errors and 0 warnings.
- `deno task bundle:app` passed and wrote
  `src-tauri/target/release/bundle/macos/Metafy Desktop.app`.
- Bundle audit: `Contents/Resources/binaries` is present in the generated
  `.app`, with tracked placeholder directories for macOS, Windows, and Linux
  archive drops.
- `deno task build` passed and wrote the static frontend to `build`.
- Bundle audit: the macOS `.app` contains `Info.plist`,
  `Contents/MacOS/metafy-desktop`, `Contents/Resources/icon.icns`, and
  `Contents/Resources/binaries`.
- `Info.plist` includes `CFBundleIdentifier = gg.metafy.desktop`,
  `CFBundleShortVersionString = 0.1.0`, `NSMicrophoneUsageDescription`, and
  `NSScreenCaptureUsageDescription`.
- Runtime tool audit on this machine: `/opt/homebrew/bin/whisper-cli --help`
  exits successfully.
- Source privacy audit: HTTP client usage is confined to optional AI summaries.
  Core local defaults report `coreNetworkRequired = false` and
  `rawMediaLeavesDevice = false`.
- Resource audit: no third-party whisper.cpp or Whisper model assets have been
  committed. The app can consume bundled Whisper zip archives when they are
  added under `src-tauri/resources/binaries/<os-arch>`. Downloaded archives are
  ignored by default and should be intentionally force-added only if the release
  strategy changes to committed binaries.

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
- Which exact whisper.cpp builds should be committed for each target platform?

## Sign-Off

- [ ] Approved by Andrew.
