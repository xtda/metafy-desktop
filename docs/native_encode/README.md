# Native Encoding Steps

This folder breaks the FFmpeg removal plan into targeted implementation steps. Each numbered folder contains:

- A step brief named `NN_<step_name>.md`
- A `status.md` file for progress tracking and sign-off

Use [status.md](status.md) for the overall native encoding migration rollup.

## Goal

Remove FFmpeg and FFprobe completely from Metafy Desktop.

The target state is:

- macOS encodes, muxes, derives metadata, and creates thumbnails through native Apple media APIs.
- Windows encodes, muxes, derives metadata, and creates thumbnails through native Windows media APIs.
- Linux encodes, muxes, derives metadata, and creates thumbnails through GStreamer.
- Transcription writes its Whisper WAV directly from local sidecars instead of demuxing the final MP4 with FFmpeg.
- Core recording, recovery, retry, local-only privacy, and library playback behavior remains intact.

Whisper itself can remain a local `whisper.cpp` dependency for this migration. These steps only remove FFmpeg and FFprobe.

## Current Pipeline

The existing capture pipeline is already a good migration boundary:

- `src-tauri/src/recorder.rs` writes a raw BGRA video sidecar with timing headers.
- `src-tauri/src/recorder.rs` writes microphone PCM and source-audio PCM sidecars independently.
- `src-tauri/src/encoding.rs` reads those sidecars and currently shells out to FFmpeg for MP4 encode, AAC encode, audio mixing, muxing, thumbnails, and FFprobe metadata.
- `src-tauri/src/transcription.rs` currently shells out to FFmpeg to extract `transcript-audio.wav` from the final MP4 before running `whisper.cpp`.

The migration keeps the raw sidecar model and replaces the processing backend behind it.

## Step Index

| Step | Area | Outcome |
| --- | --- | --- |
| [01 Backend Boundary](01_backend_boundary/01_backend_boundary.md) | Encoder contracts, module scaffold, FFmpeg isolation | FFmpeg becomes a temporary backend behind a platform-neutral API. |
| [02 Sidecar Readers](02_sidecar_readers/02_sidecar_readers.md) | Raw video/audio parsing | Encoding and transcription share streaming readers for existing local sidecars. |
| [03 Audio Mixdown](03_audio_mixdown/03_audio_mixdown.md) | PCM conversion, resampling, source mixing | Audio policy moves out of FFmpeg and becomes deterministic app-owned logic. |
| [04 Transcription WAV](04_transcription_wav/04_transcription_wav.md) | Direct Whisper audio prep | Transcription no longer invokes FFmpeg or depends on MP4 demuxing. |
| [05 Metadata & Thumbnail](05_metadata_thumbnail/05_metadata_thumbnail.md) | Probe replacement, JPEG generation | FFprobe and FFmpeg thumbnail extraction are replaced by backend-neutral outputs. |
| [06 macOS Native Encoder](06_macos_native_encoder/06_macos_native_encoder.md) | AVFoundation, VideoToolbox, CoreMedia | macOS can produce final MP4s without FFmpeg or FFprobe. |
| [07 Windows Native Encoder](07_windows_native_encoder/07_windows_native_encoder.md) | Media Foundation, WIC | Windows can produce final MP4s without FFmpeg or FFprobe. |
| [08 Linux GStreamer Encoder](08_linux_gstreamer_encoder/08_linux_gstreamer_encoder.md) | GStreamer appsrc pipelines | Linux uses GStreamer as its media backend, not FFmpeg. |
| [09 Backend Selection & Readiness](09_backend_selection_readiness/09_backend_selection_readiness.md) | Platform routing, diagnostics, UI text | The app reports native/GStreamer readiness with no FFmpeg-facing setup path. |
| [10 Remove FFmpeg](10_remove_ffmpeg/10_remove_ffmpeg.md) | Code, scripts, docs, binary discovery cleanup | FFmpeg and FFprobe references are removed from product code and packaging. |
| [11 Cross-Platform Validation](11_cross_platform_validation/11_cross_platform_validation.md) | Packaged app validation, playback, privacy | All target platforms prove encode, playback, transcription, and local-only behavior. |

## Sign-Off Flow

1. Work through the steps in order unless a later step is explicitly unblocked.
2. Update each step's `status.md` as implementation progresses.
3. Attach concrete validation evidence in the step `status.md` before asking for sign-off.
4. Mark the sign-off checkbox only after the step has been reviewed and approved.
5. Do not remove FFmpeg support until native macOS, native Windows, Linux GStreamer, direct transcription WAV, and replacement metadata/thumbnail paths are validated.
