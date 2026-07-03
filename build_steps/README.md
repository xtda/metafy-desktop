# Build Steps

This folder breaks `prd_local_only.md` into targeted implementation steps. Each numbered folder contains:

- A step brief named `NN_<step_name>.md`
- A `status.md` file for progress tracking and sign-off

Use [status.md](status.md) for the overall build rollup across all steps.

All steps assume the local-only MVP constraints:

- No backend service
- No cloud storage
- No account system
- No network dependency for recording, playback, transcription, or local search
- Optional AI is transcript-only and must never receive raw audio or video

Steps 12-18 extend the MVP scope to support macOS and Windows
application/window capture, separate microphone and source audio, and resize-safe
window recording.

## Step Index

| Step | Area | Outcome |
| --- | --- | --- |
| [01 Project Foundation](01_project_foundation/01_project_foundation.md) | Tauri, Svelte, Deno, Rust workspace | A runnable desktop app shell with the agreed stack and baseline architecture. |
| [02 Local Storage Schema](02_local_storage_schema/02_local_storage_schema.md) | App data directory, SQLite, filesystem layout | A durable local data model for recordings, transcripts, jobs, thumbnails, and AI outputs. |
| [03 App Shell & Library UI](03_app_shell_library_ui/03_app_shell_library_ui.md) | Svelte UI, navigation, library views | A usable local app shell for recording, browsing, playback, search, and settings. |
| [04 Capture Permissions & Devices](04_capture_permissions_devices/04_capture_permissions_devices.md) | Screen/audio permissions, device selection | The app can detect capture capability, request permissions, and select screen/audio sources. |
| [05 Recording Session Pipeline](05_recording_session_pipeline/05_recording_session_pipeline.md) | Session lifecycle, temporary files, synchronized capture | The app can start, track, stop, and persist a local recording session. |
| [06 Encoding & Playback](06_encoding_playback/06_encoding_playback.md) | MP4 output, thumbnails, local playback | Captured media becomes a playable MP4 stored in the local library. |
| [07 Whisper Transcription](07_whisper_transcription/07_whisper_transcription.md) | Model management, audio extraction, transcript segments | Recordings can be transcribed locally with timestamped segments. |
| [08 Local Search](08_local_search/08_local_search.md) | SQLite FTS5, timestamped search results | Transcript segments are locally searchable with jump-to-timestamp playback. |
| [09 Optional AI Summaries](09_optional_ai_summaries/09_optional_ai_summaries.md) | Transcript-only AI analysis | Optional summaries, action items, decisions, questions, and chapters are generated and stored locally. |
| [10 Failure Recovery & Jobs](10_failure_recovery_jobs/10_failure_recovery_jobs.md) | Persisted jobs, retries, interruption handling | Interrupted or failed work can resume or retry without losing recordings. |
| [11 Packaging & Validation](11_packaging_validation/11_packaging_validation.md) | Cross-platform packaging, performance checks | The MVP is packaged and verified against the PRD success criteria. |
| [12 Capture Source Model](12_capture_source_model/12_capture_source_model.md) | Source-oriented capture config, preferences, validation | Displays, applications, and windows can be represented consistently across the command boundary. |
| [13 Source Picker & Audio Mode UI](13_source_picker_ui/13_source_picker_ui.md) | Svelte capture controls, source grouping, audio modes | Users can choose source type and microphone/source audio modes clearly. |
| [14 Application & Window Video Capture](14_application_window_video_capture/14_application_window_video_capture.md) | macOS/Windows app and window video capture | Selected applications and windows can be captured as local video sources. |
| [15 Source Audio Backends](15_source_audio_backends/15_source_audio_backends.md) | ScreenCaptureKit, Windows application loopback, source audio capability | Selected source audio can be captured separately from microphone audio. |
| [16 Split Audio Session Storage](16_split_audio_session_storage/16_split_audio_session_storage.md) | SQLite schema, temp files, recording session metadata | Microphone audio and source audio are stored as separate recoverable sidecars. |
| [17 Resize Normalization](17_resize_normalization/17_resize_normalization.md) | Frame normalization, stable BGRA stream | Window resizes no longer break rawvideo encoding. |
| [18 Multi-Input Encoding & Validation](18_multi_input_encoding_validation/18_multi_input_encoding_validation.md) | Multi-audio input, mixed playback, macOS/Windows validation | Recordings encode and play correctly across all audio modes and required platforms. |

## Sign-Off Flow

1. Work through the steps in order unless a later step is explicitly unblocked.
2. Update the step's `status.md` as implementation progresses.
3. Attach validation evidence in `status.md` before asking for sign-off.
4. Mark the sign-off checkbox only after the step has been reviewed and approved.
5. Keep future enhancements out of these MVP steps unless they are required to satisfy the local-only PRD.
