# 02 Local Storage Schema

## Objective

Implement the local persistence foundation for recordings, transcripts, transcript segments, optional AI summaries, processing jobs, and filesystem paths.

## PRD Coverage

- Local storage flow
- Local database storage
- Local files in the app data directory
- Processing state persistence
- Privacy-first local storage model

## Deliverables

- App data directory resolver.
- Local directory layout creation:
  - `recordings/{recording_id}/`
  - `models/whisper/`
  - `temp/`
- SQLite database initialization.
- Migrations for:
  - `recordings`
  - `transcripts`
  - `transcript_segments`
  - `ai_summaries`
  - `processing_jobs`
- Rust data access layer for create/read/update operations.
- Status enums for recording, transcript, AI, and job states.

## Implementation Notes

- Store large binary media as files, not SQLite blobs.
- Store raw Whisper output as JSON on disk and indexed segment rows in SQLite.
- Use stable IDs for recording directories so filesystem and database records stay aligned.
- Avoid deleting temporary capture files until the related job is complete or safely recoverable.

## Acceptance Criteria

- The app creates its local data directory on first launch.
- SQLite migrations run idempotently.
- A recording record can be created, updated, listed, and loaded.
- Transcript and segment records can be persisted and queried by recording.
- Processing jobs survive app restart.
- No backend, account, or cloud storage fields are required.

## Out Of Scope

- Actual media capture.
- Search indexing.
- AI summary generation.
- Cross-device sync.
