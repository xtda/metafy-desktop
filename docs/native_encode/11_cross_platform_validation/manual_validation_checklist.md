# Native Encoding Manual Validation Checklist

Use this checklist after Steps 01-10 are implemented. The goal is to prove the app no longer requires FFmpeg or FFprobe on any supported platform.

## Global Preconditions

- [ ] FFmpeg is not bundled with the app.
- [ ] FFprobe is not bundled with the app.
- [ ] FFmpeg is not available on `PATH`.
- [ ] FFprobe is not available on `PATH`.
- [ ] App is launched like a normal user would launch it, not from a development shell.
- [ ] Whisper model and `whisper.cpp` setup are available for transcription validation.
- [ ] Raw media remains local to app storage.

## macOS

- [ ] Packaged `.app` launches from Finder.
- [ ] Display video-only recording starts, stops, encodes, and plays.
- [ ] Display + microphone recording starts, stops, encodes, and plays with audible microphone audio.
- [ ] Window/source-only recording starts, stops, encodes, and plays where source audio is supported.
- [ ] Microphone + source recording starts, stops, encodes, and plays with both sources audible.
- [ ] Captured window resize during recording does not break encode.
- [ ] Final MP4 opens in QuickTime.
- [ ] Thumbnail is generated and visible in the app.
- [ ] Local Whisper transcription completes from direct WAV generation.
- [ ] Retry after a forced encode failure preserves sidecars and can complete.

## Windows

- [ ] Packaged app launches from the normal desktop shell.
- [ ] Display video-only recording starts, stops, encodes, and plays.
- [ ] Display + microphone recording starts, stops, encodes, and plays with audible microphone audio.
- [ ] Window/source-only recording starts, stops, encodes, and plays where source audio is supported.
- [ ] Microphone + source recording starts, stops, encodes, and plays with both sources audible.
- [ ] Captured window resize during recording does not break encode.
- [ ] Final MP4 opens in the Windows media player.
- [ ] Thumbnail is generated and visible in the app.
- [ ] Local Whisper transcription completes from direct WAV generation.
- [ ] Retry after a forced encode failure preserves sidecars and can complete.

## Linux

- [ ] App launches with required GStreamer runtime/plugins installed.
- [ ] App reports actionable readiness errors when required GStreamer plugins are missing.
- [ ] Display video-only recording starts, stops, encodes, and plays.
- [ ] Display + microphone recording starts, stops, encodes, and plays with audible microphone audio.
- [ ] Linux source-audio cases are validated only if source-audio capture is implemented for Linux.
- [ ] Captured window resize during recording does not break encode where window capture is supported.
- [ ] Final MP4 opens in a standard Linux media player.
- [ ] Thumbnail is generated and visible in the app.
- [ ] Local Whisper transcription completes from direct WAV generation.
- [ ] Retry after a forced encode failure preserves sidecars and can complete.

## Artifact Audit

- [ ] `rg -n "ffmpeg|ffprobe|METAFY_FFMPEG|METAFY_FFPROBE" .` has no active product references.
- [ ] Packaged macOS artifact contains no FFmpeg or FFprobe binaries.
- [ ] Packaged Windows artifact contains no FFmpeg or FFprobe binaries.
- [ ] Linux package/runtime docs require GStreamer, not FFmpeg.
- [ ] Optional AI payload guardrails still reject raw audio/video paths.
