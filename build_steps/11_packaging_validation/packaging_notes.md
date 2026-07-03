# Packaging Notes

Last updated: 2026-07-02

## Release Build Configuration

- Source of truth: `src-tauri/tauri.conf.json`.
- Product name: `Metafy Desktop`.
- Identifier: `gg.metafy.desktop`.
- Version: `0.1.0`.
- Frontend build: `deno task build`.
- macOS app bundle build: `deno task bundle:app`.
- Tauri bundle target config: `bundle.active = true`, `bundle.targets = "all"`.
- Local validation command: `deno task tauri build --bundles app`.
- Local macOS app output:
  `src-tauri/target/release/bundle/macos/Metafy Desktop.app`.

The `bundle:app` task intentionally validates the runnable application bundle
without requiring installer generation. Installer formats should be validated in
platform-specific release work.

## macOS Notes

- The release `.app` bundle builds successfully on this machine.
- Current app bundle contents are the native executable, `Info.plist`, and
  `icon.icns`; optional bundled tool archives are copied from
  `src-tauri/resources/binaries` when present.
- `Info.plist` includes microphone and screen capture usage descriptions:
  `NSMicrophoneUsageDescription` and `NSScreenCaptureUsageDescription`.
- Whisper tools can come from the explicit env var, bundled zip archives
  extracted into app data, `PATH`, or common Homebrew/MacPorts locations.
- First manual launch should verify macOS screen recording and microphone
  permissions from the packaged app, not only from `tauri dev`.

## Windows Notes

- Build on a Windows host with the same Tauri config and run
  `deno task tauri build` or a Windows-specific Tauri bundle target.
- Media encoding uses Media Foundation on Windows. whisper.cpp can be bundled
  by adding `whisper.zip` under
  `src-tauri/resources/binaries/windows-x86_64`. Without an archive, install a
  supported whisper.cpp command on `PATH` or set `METAFY_WHISPER_CPP_PATH`.
- Validate Windows display capture permissions, microphone enumeration, MP4
  playback, and local app data path behavior on the target host.
- Installer packaging, signing, and auto-update are out of scope for the MVP
  validation step.

## Linux Notes

- Build on the target Linux distribution with the same Tauri config and run
  `deno task tauri build` or a Linux-specific Tauri bundle target.
- Media encoding uses GStreamer on Linux, so target hosts need GStreamer and
  H.264/AAC plugins installed. whisper.cpp can be bundled by adding
  `whisper.zip` under the matching `src-tauri/resources/binaries/linux-*`
  directory. Without an archive, install a supported whisper.cpp command on
  `PATH`, in a common system location, or set `METAFY_WHISPER_CPP_PATH`.
- Validate capture support under the target display server. Wayland/X11 support
  can vary by distribution, compositor, and portal setup.
- Installer packaging, repository publication, and auto-update are out of scope
  for the MVP validation step.

## Whisper Resource Audit

- Tauri bundles optional zip archives from `src-tauri/resources/binaries`.
  Archives are expected under an `os-arch` directory such as `macos-aarch64`,
  `macos-x86_64`, `windows-x86_64`, or `linux-x86_64`.
- Windows x64 archives can be populated locally with
  `deno task binaries:download` on Windows, or
  `deno task binaries:download:windows` from another platform.
- Downloaded archives are ignored by default so local testing does not
  accidentally stage third-party binary blobs.
- On startup, the app extracts current-platform `.zip` archives into
  `app-data/tools/v1/<os-arch>` when archive metadata changes.
- Whisper lookup order: `METAFY_WHISPER_CPP_PATH`, then extracted bundled tools,
  then `PATH`, then common system install locations.
- Whisper candidate binary names: `whisper-cli`, `main`, `whisper`.
- Whisper models are local user resources under `app-data/models/whisper`.
- Default model name remains `small.en`, with expected file `ggml-small.en.bin`.
- The current download task supports `windows-x86_64` only. macOS still uses
  system/Homebrew-style Whisper discovery unless macOS archives are added
  manually.

## Offline And Privacy Notes

- Core local defaults report `coreNetworkRequired = false` and
  `rawMediaLeavesDevice = false`.
- Source audit found HTTP client usage only in the optional AI summary path.
- Optional AI remains disabled by default and validates transcript-only payloads
  before making provider requests.
- The core recording, encoding, transcription, search, and recovery workflows
  have no backend, S3/R2, or cloud storage configuration.

## Remaining Manual Validation

- Launch the packaged app with network disabled and complete the core workflow.
- Capture a 1080p 30 FPS recording on target hardware.
- Verify playable MP4 output and microphone sync from that recording.
- Run local Whisper with the default model against the captured recording.
- Confirm local search returns timestamped transcript results.
- Exercise failed encode/transcription recovery from the packaged app.
