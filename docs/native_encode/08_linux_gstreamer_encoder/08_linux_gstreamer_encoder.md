# 08 Linux GStreamer Encoder

## Objective

Implement the Linux final MP4 encoder using GStreamer as the Linux media backend. This is not an FFmpeg fallback.

## Scope

- Linux encode backend.
- GStreamer appsrc-driven video and audio pipelines.
- H.264 video encoding.
- AAC audio encoding when audio exists.
- MP4 muxing.
- Missing plugin readiness diagnostics.

## Deliverables

- `linux_gstreamer` backend implementation behind the shared encoder boundary.
- `appsrc` video pipeline accepting BGRA frames with explicit timestamps.
- Optional `appsrc` audio pipeline accepting shared 48 kHz stereo mixdown samples.
- H.264/AAC MP4 output through GStreamer.
- Clear readiness checks for required GStreamer elements/plugins.
- Linux synthetic encode tests on an environment with required plugins.

## Implementation Notes

Recommended Rust crates:

- `gstreamer`
- `gstreamer-app`
- `gstreamer-audio`
- `gstreamer-video`

Video-only pipeline shape:

```text
appsrc name=video_src format=time is-live=false
  ! videoconvert
  ! video/x-raw,format=I420
  ! h264 encoder
  ! h264parse
  ! mp4mux
  ! filesink location=recording.tmp.mp4
```

Video plus audio pipeline shape:

```text
appsrc name=video_src format=time is-live=false
  ! videoconvert
  ! video/x-raw,format=I420
  ! h264 encoder
  ! h264parse
  ! queue
  ! mux.

appsrc name=audio_src format=time is-live=false
  ! audioconvert
  ! audioresample
  ! audio/x-raw,rate=48000,channels=2
  ! AAC encoder
  ! aacparse
  ! queue
  ! mux.

mp4mux name=mux
  ! filesink location=recording.tmp.mp4
```

Encoder element selection should prefer stable platform/hardware encoders when available and provide a deterministic software fallback through common GStreamer plugins.

## Acceptance Criteria

- Linux video-only synthetic encode passes without FFmpeg.
- Linux microphone-only synthetic encode passes without FFmpeg.
- Linux backend reports actionable errors when required GStreamer plugins are missing.
- Linux source-audio cases follow the platform support matrix and do not claim support before capture exists.
- Final MP4 plays in the app and a standard Linux media player.
- Thumbnail and metadata behavior matches earlier steps.

## Out Of Scope

- Adding Linux source-audio capture support.
- macOS or Windows native encoding.
- Depending on `gst-launch` shell commands.
- Using FFmpeg as a Linux fallback.
