# Manual MVP Validation Checklist

Last updated: 2026-07-02

Use this checklist from the packaged app, not from `tauri dev`.

## Setup

- [ ] On macOS, build the release app bundle with `deno task bundle:app`.
- [ ] On Windows or Linux, build with the platform-native Tauri bundle target.
- [ ] Launch `src-tauri/target/release/bundle/macos/Metafy Desktop.app`.
- [ ] Confirm FFmpeg and FFprobe are available in Settings or during encode.
- [ ] Confirm whisper.cpp is available in Settings.
- [ ] Import or place `ggml-small.en.bin` under the app data Whisper model
  directory.
- [ ] Disable network access before core workflow validation.

## Core Offline Workflow

- [ ] App starts without a backend, login, cloud storage, or network access.
- [ ] Display capture permission state is visible and actionable.
- [ ] Microphone permission state is visible and actionable.
- [ ] A 1080p 30 FPS recording starts, stops, and persists locally.
- [ ] The completed recording encodes to MP4.
- [ ] The MP4 plays back in the app.
- [ ] Microphone audio is synchronized closely enough for MVP review.
- [ ] Local Whisper transcription completes with `small.en`.
- [ ] Transcript segments display timestamps.
- [ ] Local search returns timestamped results.
- [ ] Jumping from a search result seeks playback to the expected timestamp.

## Failure Recovery

- [ ] Interrupt or fail an encode and confirm temporary media is preserved.
- [ ] Retry the failed encode from the app and confirm it completes.
- [ ] Interrupt or fail a transcription and confirm retry state is preserved.
- [ ] Retry the failed transcription from the app and confirm it completes.
- [ ] Restart the app with pending or failed jobs and confirm recovery state is
  visible.
- [ ] Cleanup removes stale processing files while preserving retry-critical
  files.

## Privacy

- [ ] Confirm raw audio/video files remain under the local app data directory.
- [ ] Confirm no backend, S3/R2, or cloud storage settings are required.
- [ ] Confirm optional AI remains disabled unless explicitly configured.
- [ ] If optional AI is enabled separately, confirm only transcript text and
  recording metadata are sent.

## Sign-Off

- [ ] macOS packaged app validated.
- [ ] Windows packaged app validated.
- [ ] Linux packaged app validated.
- [ ] MVP acceptance criteria approved by Andrew.
