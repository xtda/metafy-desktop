# Product Requirements Document (PRD)
## Local-Only Desktop Recording & Knowledge Assistant

### Version
0.1 (Local-Only MVP)

---

# 1. Overview

The application is a cross-platform desktop application that records the user's screen and microphone, stores recordings locally, automatically transcribes audio locally using Whisper, and creates a searchable local history of recordings, transcripts, notes, chapters, and action items.

For this MVP, the application has **no backend, no S3/R2 upload, and no cloud storage requirement**. The product must remain useful without a network connection.

A primary design goal is that **raw audio and video stay on the user's device**. Speech recognition is performed locally, and local transcript data is stored in the desktop application's local database.

AI analysis is optional and configurable. If enabled, only transcript text and recording metadata may be sent to a configured LLM provider. Raw audio and video are never sent for AI analysis.

The system is designed to support future use cases including:

- Meeting recording
- Gameplay recording
- Coaching sessions
- Educational videos
- Product demonstrations
- Customer support sessions
- Personal knowledge management

---

# 2. Goals

## Primary Goals

- Cross-platform desktop support (Windows/macOS/Linux)
- High-quality local screen recording
- Local microphone recording
- Local speech-to-text
- Playable local MP4 recordings
- Searchable local transcript history
- Low CPU overhead during recording
- Privacy-first local storage model
- Graceful recovery from recording, encoding, transcription, or AI failures

## Non Goals (Local-Only MVP)

- Backend API
- Cloud account system
- JWT authentication
- S3/R2 upload
- Presigned URLs
- PostgreSQL storage
- pgvector search
- Server-side background jobs
- Multi-device sync
- Multi-user collaboration
- Video editing
- Real-time AI assistant
- Live streaming
- OCR from video frames
- Face recognition

---

# 3. High Level Architecture

```text
                 Desktop Application

+------------------------------------------------+
|                                                |
| Screen Capture (scap)                          |
| Audio Capture (cpal)                           |
| Local Recording / Encoding (native/GStreamer)  |
| Local Transcription (whisper.cpp)              |
| Local Search Index                             |
| Local Cache / SQLite                           |
| Optional Transcript-Only AI Analysis           |
|                                                |
+------------------------+-----------------------+
                         |
                         v

                 Local Filesystem

+------------------------------------------------+
| recordings/*.mp4                               |
| transcripts/*.json                             |
| thumbnails/*                                   |
| app.sqlite                                     |
| temp/*                                         |
+------------------------------------------------+
```

No backend service is required for the local-only MVP.

---

# 4. Technology Stack

## Desktop

Language

- Rust

UI

- Tauri
- Svelte

Frontend Package Manager / Task Runner

- Deno

Screen Capture

- scap

Microphone

- cpal

Encoding

- AVFoundation on macOS
- Media Foundation on Windows
- GStreamer on Linux

Speech Recognition

- whisper.cpp

Database

- SQLite
- SQLite FTS5 for local transcript search

Local Storage

- Application data directory
- Local filesystem for MP4, transcript JSON, thumbnails, and temporary files

Networking

- No networking required for core MVP
- Optional direct LLM provider calls for transcript-only AI analysis

---

# 5. Desktop Pipeline

```text
Screen
  |
  v
scap
  |
  v
Frames

Microphone
  |
  v
cpal

  |
  v

Native/GStreamer media backend

  |
  v

recording.mp4

  |
  v

Local Library
```

Recording occurs entirely locally.

No network connection is required for recording, playback, transcription, or local search.

---

# 6. Audio Pipeline

```text
Microphone
  |
  v
PCM Audio
  |
  v
Voice Activity Detection
  |
  v
whisper.cpp
  |
  v
Transcript
```

Whisper receives audio in configurable chunks.

Recommended chunk size:

```text
15-30 seconds
```

Each chunk produces:

```text
Text
Start timestamp
End timestamp
Confidence
```

Example:

```text
00:01
Welcome everyone.

00:06
Today we're discussing...
```

---

# 7. Recording Pipeline

```text
User presses Record
  |
  v
Create local recording session
  |
  v
Start screen capture
  |
  v
Start microphone capture
  |
  v
Write temporary files
  |
  v
Encode MP4
  |
  v
Finish recording
  |
  v
Move MP4 into local library
  |
  v
Extract audio
  |
  v
Run Whisper
  |
  v
Generate transcript JSON
  |
  v
Index transcript locally
  |
  v
Generate optional AI summary
```

---

# 8. Local Storage Flow

The desktop app stores:

```text
recording.mp4
transcript.json
metadata.json
thumbnail.jpg
```

Metadata:

```text
Recording ID
Title
Duration
Resolution
Frame Rate
Language
Transcript Version
Created At
Local File Path
Processing Status
```

Recommended local directory layout:

```text
app-data/
  recordings/
    {recording_id}/
      recording.mp4
      transcript.json
      metadata.json
      thumbnail.jpg
  models/
    whisper/
  temp/
  app.sqlite
```

The app should avoid deleting source recording files until all local processing steps have either completed or been marked failed with a retry option.

---

# 9. Whisper Processing

Models:

```text
tiny
base
small
medium
```

Default:

```text
small.en
```

Optional:

```text
medium.en
```

Whisper output:

```json
{
  "segments": [
    {
      "start": 1.24,
      "end": 6.40,
      "text": "Welcome everyone."
    }
  ]
}
```

Stored exactly as returned, with any app-specific metadata stored separately.

Model management requirements:

- The app can detect whether the selected Whisper model exists locally.
- The app can prompt the user to download a missing model.
- Downloaded models are stored in the app data directory.
- Existing recordings remain playable even if transcription fails or a model is missing.

---

# 10. AI Processing

AI processing is optional for the local-only MVP.

If AI is disabled:

- Recording remains available.
- Transcript remains searchable locally.
- User can add manual notes.
- Summary, action items, decisions, questions, and chapters remain empty or pending.

If AI is enabled, the desktop app may send:

```text
Transcript
Recording metadata
Optional user notes
```

The configured LLM provider must not receive:

- Video
- Audio

Prompt example:

```text
Summarize this recording.

Extract:
- Action items
- Decisions
- Important topics
- Risks
- Questions

Return JSON.
```

Example response:

```json
{
  "summary": "Weekly engineering meeting",
  "action_items": [],
  "decisions": [],
  "chapters": []
}
```

AI outputs are stored locally.

---

# 11. Local Database Storage

## recordings

```text
id
title
duration
resolution_width
resolution_height
frame_rate
file_path
thumbnail_path
created_at
updated_at
status
```

## transcripts

```text
id
recording_id
transcript_version
language
raw_json_path
created_at
status
```

## transcript_segments

```text
id
recording_id
transcript_id
segment_index
start_time
end_time
text
confidence
```

## ai_summaries

```text
id
recording_id
summary
action_items
decisions
questions
chapters
provider
model
created_at
status
```

## processing_jobs

```text
id
recording_id
job_type
status
attempt_count
last_error
created_at
updated_at
```

---

# 12. Search

The local-only MVP uses local transcript search.

Minimum requirement:

```text
Transcript segment
  |
  v
SQLite FTS5 index
  |
  v
Local search results
```

Search returns:

```text
Recording
Timestamp
Transcript segment
Match score/rank
```

User can jump directly to that timestamp in the local recording.

Optional future local search enhancement:

- Local embeddings
- On-device vector index
- Hybrid keyword + semantic search

---

# 13. Local Background Tasks

The desktop app manages local processing tasks:

```text
Encode Recording
Extract Audio
Run Whisper
Index Transcript
Generate Thumbnail
Generate Optional AI Summary
Clean Temporary Files
Retry Failed Local Jobs
```

Tasks should be persisted in SQLite so interrupted work can resume after app restart.

---

# 14. Security & Privacy

- Raw audio stays on device.
- Raw video stays on device.
- No recording is uploaded automatically.
- No backend authentication is required.
- No cloud account is required.
- AI analysis is opt-in and transcript-only.
- Local files should be stored in the app data directory by default.
- Optional local-only mode disables all network calls.
- Optional local encryption can be added after MVP.

---

# 15. Failure Recovery

If recording fails:

```text
Keep any recoverable temporary files.
Mark recording as failed.
Show retry or cleanup option.
```

If encoding fails:

```text
Keep source temp files when possible.
Mark encoding as failed.
Allow retry.
```

If Whisper fails:

```text
Recording remains playable.
Transcript marked pending or failed.
Allow retry with same or different model.
```

If AI fails:

```text
Recording remains playable.
Transcript remains searchable.
AI summary marked pending or failed.
Allow retry.
```

If the app closes during processing:

```text
Persist job state.
Resume or mark interrupted jobs on next launch.
```

---

# 16. Future Enhancements

## Backend Sync

Optional future backend support may add:

- User accounts
- Cross-device sync
- Direct object storage upload
- Server-side embeddings
- Web library access

## Vision

Sample frames every 5-10 seconds.

Allow AI to identify:

- Slides
- UI changes
- Error dialogs
- Game events
- Whiteboards

Only selected frames should be used for visual analysis rather than the full video.

## Speaker Diarization

Identify:

```text
Speaker A
Speaker B
Speaker C
```

and attach speakers to transcript segments.

## Real-Time Transcription

Run Whisper continuously during recording.

Display live captions.

## AI Chat

Ask questions such as:

- "What decisions were made?"
- "When did we discuss Kubernetes?"
- "Show every time PostgreSQL was mentioned."
- "Summarize only the deployment discussion."

For the local-only product, chat should retrieve relevant local transcript segments and supply them as context to either a local model or an explicitly configured transcript-only LLM provider.

---

# 17. MVP Success Criteria

- Record 1080p screen at 30 FPS on Windows, macOS, and Linux.
- Record microphone audio with synchronized timestamps.
- Produce a playable local MP4 recording.
- Store recordings, transcripts, metadata, thumbnails, and processing state locally.
- Generate a local transcript using whisper.cpp, with small.en as the default model.
- Support local keyword search across transcript segments.
- Support timestamped jump-to-result playback from search.
- Keep recordings playable even when transcription or AI processing fails.
- Recover gracefully from recording, encoding, transcription, AI, or app-restart failures without losing the recording.
- Require no backend, no S3/R2 bucket, and no network connection for core recording, transcription, playback, and search workflows.
