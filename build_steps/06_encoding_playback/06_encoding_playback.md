# 06 Encoding & Playback

## Objective

Convert captured temporary media into a playable local MP4, move it into the local recording library, generate a thumbnail, and support local playback in the app.

## PRD Coverage

- FFmpeg encoding
- Playable MP4 recording
- Local library storage
- Thumbnail generation
- No upload or cloud dependency

## Deliverables

- FFmpeg availability strategy:
  - bundled sidecar, system binary, or documented MVP assumption
- Encoding job from temp capture files to `recording.mp4`.
- Final recording directory move.
- Metadata update with file path, duration, resolution, frame rate, and status.
- Thumbnail generation.
- Local video playback in the recording detail view.
- Encoding failure state and retry hook.

## Implementation Notes

- Keep FFmpeg invocation deterministic and logged.
- Never delete source temp files before successful encode and library move.
- Use local file URLs or safe media serving compatible with Tauri.
- Store thumbnail path in SQLite.

## Acceptance Criteria

- A completed capture can be encoded into a playable MP4.
- The MP4 is stored under `app-data/recordings/{recording_id}/recording.mp4`.
- The UI can play the local MP4.
- A thumbnail is generated or the recording is marked with a clear thumbnail failure.
- Encoding failure keeps enough data to retry when possible.

## Out Of Scope

- Video editing.
- Uploading recordings.
- Transcription.
- Search.
