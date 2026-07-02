# 05 Recording Session Pipeline

## Objective

Implement the local recording session lifecycle from pressing Record through stopping capture and producing recoverable temporary media assets.

## PRD Coverage

- Recording pipeline
- Screen and microphone capture
- Synchronized timestamps
- Temporary files
- Local-only operation

## Deliverables

- Recording session creation.
- Start/stop/pause-safe lifecycle state machine.
- Screen frame capture writing to temp storage.
- Microphone PCM capture writing to temp storage.
- Timestamp tracking for audio/video synchronization.
- Session metadata persistence.
- UI status updates during capture.
- Recoverable temp file handling when capture fails.

## Implementation Notes

- Keep capture hot paths lightweight.
- Persist recording session state early so crash recovery has a record to inspect.
- Capture should not depend on network availability.
- Temporary files should live under the configured local app temp directory.

## Acceptance Criteria

- User can start and stop a local recording session.
- Screen frames are captured to local temporary storage.
- Microphone audio is captured to local temporary storage.
- Recording metadata includes duration, resolution, frame rate, created timestamp, and status.
- Capture failures preserve recoverable temp files when possible.
- The UI reflects active recording state.

## Out Of Scope

- Final MP4 encoding.
- Whisper transcription.
- Search indexing.
- Optional AI summaries.
