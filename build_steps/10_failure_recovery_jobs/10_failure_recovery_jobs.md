# 10 Failure Recovery & Jobs

## Objective

Make processing durable by persisting local jobs, recovering interrupted work after restart, exposing retry/cleanup actions, and preventing data loss across recording, encoding, transcription, search indexing, AI, and cleanup failures.

## PRD Coverage

- Local background tasks
- Failure recovery
- Processing state persistence
- Keep recordings playable even when transcription or AI fails

## Deliverables

- Durable local job runner backed by SQLite.
- Job types for:
  - Encode recording
  - Extract audio
  - Run Whisper
  - Index transcript
  - Generate thumbnail
  - Generate optional AI summary
  - Clean temporary files
- Attempt counts and last error tracking.
- Retry action for failed jobs.
- App-start recovery scan.
- Cleanup flow that avoids deleting needed recoverable files.
- UI surfaces for interrupted, failed, retrying, and complete states.

## Implementation Notes

- Every destructive cleanup should be explicit or tied to a completed, verified successor artifact.
- Job retries should be idempotent where practical.
- A failed optional job should not mark the whole recording unusable.
- Recovery should prefer preserving user data over aggressive cleanup.

## Acceptance Criteria

- Jobs persist across app restart.
- Interrupted jobs are resumed or marked interrupted on next launch.
- Failed jobs show last error and retry option.
- Playback remains available when transcription/search/AI jobs fail.
- Cleanup does not remove source files needed for retry.
- Failure behavior matches the PRD's recording, encoding, Whisper, AI, and app-close recovery expectations.

## Out Of Scope

- Server-side workers.
- Cloud retry queues.
- Cross-device recovery.
