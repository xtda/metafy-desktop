# Manual Multi-Input Encoding Validation Checklist

Last updated: 2026-07-02

Use this checklist with the selected native/GStreamer media backend ready. The
MVP final MP4 should contain one default mixed AAC audio track when both
microphone and source audio are captured.

## Setup

- [ ] Start from a clean build of the current Step 18 branch.
- [ ] Confirm `cargo test --lib` passes before manual validation.
- [ ] Confirm `deno task check` passes before manual validation.
- [ ] Confirm the selected media backend is ready in Settings or during encode.
- [ ] On Linux, confirm GStreamer and required H.264/AAC plugins are installed.
- [ ] Use local app data only; do not upload raw recording sidecars.

## macOS

- [ ] Display or window video-only recording starts, stops, encodes, and plays.
- [ ] Microphone-only recording starts, stops, encodes, and plays with audible
  microphone audio.
- [ ] Source-only recording starts, stops, encodes, and plays with audible source
  audio.
- [ ] Microphone + source recording starts, stops, encodes, and plays with both
  sources audible in the default MP4 audio track.
- [ ] App/window source video capture remains stable if the captured window is
  resized during recording.
- [ ] Source audio uses the selected display/window capture permission state.
- [ ] Missing, silent, or unsupported requested audio source produces an
  encode/job-output warning while preserving the original capture sidecars for
  retry.
- [ ] Encoding failure preserves `screen_frames.mfrv`, `microphone.pcm`,
  `source_audio.pcm`, and `session.json` when present.

## Windows

- [ ] Display or window video-only recording starts, stops, encodes, and plays.
- [ ] Microphone-only recording starts, stops, encodes, and plays with audible
  microphone audio.
- [ ] Source-only recording starts, stops, encodes, and plays with audible source
  audio.
- [ ] Microphone + source recording starts, stops, encodes, and plays with both
  sources audible in the default MP4 audio track.
- [ ] App/window source video capture remains stable if the captured window is
  resized during recording.
- [ ] The source-audio backend reports `windows_wasapi_system_loopback` when
  process-specific loopback is unavailable.
- [ ] System-loopback behavior is called out in validation notes so it is not
  mistaken for process-tree isolation.
- [ ] Missing, silent, or unsupported requested audio source produces an
  encode/job-output warning while preserving the original capture sidecars for
  retry.
- [ ] Encoding failure preserves `screen_frames.mfrv`, `microphone.pcm`,
  `source_audio.pcm`, and `session.json` when present.

## Playback And Artifacts

- [ ] Each final `recording.mp4` plays from the recording detail view.
- [ ] Playback confirms a video stream for every final MP4.
- [ ] Playback confirms no audible audio for video-only recordings.
- [ ] Playback confirms one default audible track for microphone-only,
  source-only, and microphone + source recordings.
- [ ] The final MP4 path, thumbnail path, duration, dimensions, and completed
  status are persisted after successful encode.
- [ ] Temporary staging files `encoding-video.bgra`, `encoding-audio.raw`,
  `encoding-microphone.raw`, `encoding-source.raw`, `recording.tmp.mp4`, and
  `thumbnail.tmp.jpg` are cleaned after successful processing or cleanup retry.

## Optional AI Guardrails

- [ ] Optional AI payloads still exclude `recording.mp4`, `thumbnail.jpg`, raw
  video paths, microphone PCM paths, source-audio PCM paths, and transcript JSON
  paths.
- [ ] Optional AI remains transcript-only when enabled.

## Sign-Off

- [ ] macOS multi-input capture and playback validated.
- [ ] Windows multi-input capture and playback validated.
- [ ] Andrew approved Step 18.
