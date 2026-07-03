# 06 macOS Native Encoder

## Objective

Implement the macOS final MP4 encoder using native Apple media APIs so macOS no longer depends on FFmpeg or FFprobe for recording output.

## Scope

- macOS encode backend.
- AVFoundation MP4 writing.
- H.264 video encoding through AVFoundation/VideoToolbox.
- AAC audio encoding through AVFoundation/CoreAudio.
- CoreMedia timestamp conversion.
- Packaged `.app` behavior without shell `PATH`.

## Deliverables

- `macos` backend implementation behind the shared encoder boundary.
- BGRA frame submission through `CVPixelBuffer` and `AVAssetWriterInputPixelBufferAdaptor`, or a wrapped equivalent.
- Optional AAC audio input fed by the shared 48 kHz stereo mixdown.
- Backend metadata populated without FFprobe.
- macOS synthetic encode tests that do not require FFmpeg.
- Manual packaged-app validation notes in this step's status file.

## Implementation Notes

Use native Apple media APIs:

- AVFoundation for MP4 writing.
- VideoToolbox through AVFoundation for H.264 encode.
- AudioToolbox/CoreAudio through AVFoundation for AAC encode.
- CoreVideo for pixel buffers.
- CoreMedia for presentation timestamps.
- ImageIO or shared raw-frame thumbnail generation for thumbnails.

Video settings:

- Container: MP4.
- Codec: H.264.
- Input: BGRA frames from existing sidecars.
- Output compatibility: browser-playable H.264/AAC MP4.
- Timestamp policy: match the shared cross-platform timeline policy.

Audio settings:

- Codec: AAC.
- Sample rate: 48 kHz.
- Channels: stereo.
- Bitrate target: keep parity with current `160k` unless product requirements change.

Prefer a small Objective-C or Swift shim if direct Rust bindings make the backend fragile. Keep Apple-specific pointer types contained inside the backend module.

## Acceptance Criteria

- macOS synthetic video-only encode passes without FFmpeg.
- macOS synthetic audio encode passes without FFmpeg.
- Video-only, microphone-only, source-only, and microphone + source recordings play in the app.
- Final MP4 opens in QuickTime.
- Packaged `.app` encodes when launched from Finder without FFmpeg or FFprobe on `PATH`.
- Thumbnail and metadata behavior matches earlier steps.

## Out Of Scope

- Windows encoding.
- Linux encoding.
- Removing FFmpeg from non-macOS paths.
- Replacing ScreenCaptureKit/scap capture code.
