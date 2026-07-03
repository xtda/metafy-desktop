# 07 Windows Native Encoder

## Objective

Implement the Windows final MP4 encoder using Media Foundation and WIC so Windows no longer depends on FFmpeg or FFprobe for recording output.

## Scope

- Windows encode backend.
- Media Foundation Sink Writer.
- H.264 video encoding.
- AAC audio encoding.
- WIC thumbnail generation.
- Packaged Windows app behavior without `ffmpeg.exe` or `ffprobe.exe`.

## Deliverables

- `windows` backend implementation behind the shared encoder boundary.
- Media Foundation initialization and shutdown scoped to backend entrypoints.
- H.264 MP4 writer fed by raw BGRA frames or a supported converted format.
- Optional AAC audio stream fed by the shared 48 kHz stereo mixdown.
- Raw-frame thumbnail generation through WIC or shared image code.
- Windows synthetic encode tests that do not require FFmpeg.
- Manual packaged-app validation notes in this step's status file.

## Implementation Notes

Use native Windows media APIs:

- Media Foundation Sink Writer for MP4 muxing.
- Media Foundation H.264 encoder MFT for video.
- Media Foundation AAC encoder MFT for audio.
- WIC for JPEG thumbnail generation.

Video settings:

- Container: MP4.
- Codec: H.264.
- Preferred input: BGRA/RGB32 if accepted.
- Required fallback: convert BGRA to NV12 before sample submission if needed.
- Timestamps: 100 ns units derived from the shared cross-platform timeline policy.

Audio settings:

- Codec: AAC.
- Sample rate: 48 kHz.
- Channels: stereo.
- Bitrate target: keep parity with current `160k`.

Use the `windows` crate for Media Foundation, COM, and WIC bindings unless a narrower helper layer is clearly lower-risk.

## Acceptance Criteria

- Windows synthetic video-only encode passes without FFmpeg.
- Windows synthetic audio encode passes without FFmpeg.
- Video-only, microphone-only, source-only, and microphone + source recordings play in the app.
- Final MP4 opens in the Windows media player.
- Packaged Windows app encodes without `ffmpeg.exe` or `ffprobe.exe`.
- Thumbnail and metadata behavior matches earlier steps.

## Out Of Scope

- macOS encoding.
- Linux encoding.
- Replacing WASAPI/source-audio capture behavior.
- Removing FFmpeg from non-Windows paths.
