# Product Requirements Document (PRD)
## AI-Powered Desktop Recording & Knowledge Assistant

### Version
0.1 (MVP)

---

# 1. Overview

The application is a cross-platform desktop application that records the user's screen and microphone, automatically transcribes the recording locally using Whisper, uploads the recording to cloud storage, and uses an LLM to generate searchable summaries, notes, chapters and action items.

A primary design goal is that **raw audio is never sent to the LLM**. Speech recognition is performed locally, and only the transcript and metadata are transmitted for AI analysis.

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

- Cross-platform (Windows/macOS/Linux)
- High-quality screen recording
- Local speech-to-text
- AI summaries
- Searchable transcript history
- Low CPU overhead
- Privacy-first architecture

## Non Goals (MVP)

- Video editing
- Real-time AI assistant
- Multi-user collaboration
- Live streaming
- OCR from video frames
- Face recognition

---

# 3. High Level Architecture

```
                 Desktop Application

┌──────────────────────────────────────────┐
│                                          │
│ Screen Capture (scap)                    │
│ Audio Capture                            │
│ Local Recording                          │
│ Whisper.cpp                              │
│ Upload Manager                           │
│ Local Cache                              │
│                                          │
└────────────────────┬─────────────────────┘
                     │
                     │ HTTPS
                     ▼

              Go Backend API

┌──────────────────────────────────────────┐
│ Authentication                           │
│ Recording Metadata                       │
│ Transcript Storage                       │
│ AI Job Queue                             │
│ Search API                               │
└────────────────────┬─────────────────────┘
                     │
     ┌───────────────┼────────────────┐
     ▼               ▼                ▼

 PostgreSQL        S3/R2          OpenAI
 pgvector         MP4 Files      Transcript
```

---

# 4. Technology Stack

## Desktop

Language

- Rust

UI

- Tauri
- React/Svelte (TBD)

Screen Capture

- scap

Microphone

- cpal

Encoding

- FFmpeg

Speech Recognition

- whisper.cpp

Database

- SQLite (local cache)

Networking

- reqwest

Authentication

- JWT

---

## Backend

Language

- Go

Router

- chi

Database

- PostgreSQL

ORM

- Bun

Cache

- Valkey

Object Storage

- Cloudflare R2
or
- Amazon S3

Background Jobs

- River

Search

- pgvector

Authentication

- JWT

---

## AI

Speech Recognition

- whisper.cpp

LLM

- OpenAI

Embeddings

- OpenAI Embeddings

---

# 5. Desktop Pipeline

```
Screen
      │
      ▼
scap
      │
      ▼
Frames

Microphone
      │
      ▼
cpal

      ▼

ffmpeg

      ▼

recording.mp4
```

Recording occurs entirely locally.

No network connection is required.

---

# 6. Audio Pipeline

```
Microphone

↓

PCM Audio

↓

Voice Activity Detection

↓

Whisper.cpp

↓

Transcript
```

Whisper receives audio in configurable chunks.

Recommended chunk size:

```
15–30 seconds
```

Each chunk produces

```
Text

Start timestamp

End timestamp

Confidence
```

Example

```
00:01

Welcome everyone.

00:06

Today we're discussing...
```

---

# 7. Recording Pipeline

```
User presses Record

↓

Create recording session

↓

Start screen capture

↓

Start microphone capture

↓

Write temporary files

↓

Encode MP4

↓

Finish recording

↓

Extract audio

↓

Run Whisper

↓

Generate transcript

↓

Upload recording

↓

Upload transcript

↓

Generate AI summary
```

---

# 8. Upload Flow

Desktop uploads

```
recording.mp4

transcript.json

metadata.json
```

Metadata

```
Recording ID

Duration

Resolution

Frame Rate

Language

Transcript Version

Created At
```

Video uploads directly to object storage using a presigned URL.

The backend never proxies large video files.

---

# 9. Whisper Processing

Models

```
tiny

base

small

medium
```

Default

```
small.en
```

Optional

```
medium.en
```

Whisper output

```json
{
  "segments":[
      {
          "start":1.24,
          "end":6.40,
          "text":"Welcome everyone."
      }
  ]
}
```

Stored exactly as returned.

---

# 10. AI Processing

OpenAI receives

```
Transcript

Recording metadata

Optional user notes
```

OpenAI does NOT receive

- Video
- Audio

Prompt example

```
Summarize this recording.

Extract:

• Action items

• Decisions

• Important topics

• Risks

• Questions

Return JSON.
```

Example response

```json
{
  "summary":"Weekly engineering meeting",

  "action_items":[...],

  "decisions":[...],

  "chapters":[...]
}
```

---

# 11. Backend Storage

## recordings

```
id

user_id

title

duration

storage_key

created_at
```

---

## transcripts

```
id

recording_id

segment

start_time

end_time

text
```

---

## embeddings

```
id

recording_id

segment_id

vector
```

Uses pgvector.

---

## ai_summaries

```
id

recording_id

summary

action_items

questions

chapters
```

---

# 12. Search

Every transcript segment receives an embedding.

```
Segment

↓

Embedding

↓

pgvector
```

Example

```
"When did we discuss Kubernetes?"
```

Search returns

```
Recording

Timestamp

Transcript

Similarity Score
```

User jumps directly to that point in the recording.

---

# 13. Background Jobs

River workers

```
Generate Embeddings

Generate AI Summary

Retry Uploads

Clean Temporary Files

Thumbnail Generation

Search Index Refresh
```

---

# 14. Security

Video

- encrypted in object storage

JWT authentication

Signed upload URLs

HTTPS only

No audio sent to LLM

No screen capture uploaded until recording finishes

Optional local-only mode

---

# 15. Failure Recovery

If upload fails

```
Recording remains local

Retry later
```

If AI fails

```
Recording still available

Transcript searchable

Retry AI later
```

If Whisper fails

```
Recording uploaded

Transcript marked pending
```

---

# 16. Future Enhancements

## Vision

Sample frames every 5–10 seconds.

Allow AI to identify:

- Slides
- UI changes
- Error dialogs
- Game events
- Whiteboards

Only selected frames are sent for visual analysis rather than the full video.

---

## Speaker Diarization

Identify

```
Speaker A

Speaker B

Speaker C
```

and attach speakers to transcript segments.

---

## Real-Time Transcription

Run Whisper continuously during recording.

Display live captions.

---

## AI Chat

Ask questions such as:

- "What decisions were made?"
- "When did we discuss Kubernetes?"
- "Show every time PostgreSQL was mentioned."
- "Summarize only the deployment discussion."

The backend retrieves the most relevant transcript segments using pgvector and supplies them as context to the LLM.

---

# 17. MVP Success Criteria

- Record 1080p screen at 30 FPS on Windows, macOS, and Linux.
- Record microphone audio with synchronized timestamps.
- Produce a playable MP4 recording.
- Generate a local transcript using whisper.cpp (small.en by default).
- Upload recordings directly to S3-compatible object storage via presigned URLs.
- Store transcript segments and embeddings in PostgreSQL with pgvector.
- Generate AI summaries, action items, and chapters using only transcript text.
- Support semantic search across all recordings with timestamped jump-to-result functionality.
- Recover gracefully from upload, transcription, or AI processing failures without losing the recording.
