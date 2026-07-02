# 07 Whisper Transcription

## Objective

Extract audio from completed recordings, run local Whisper transcription, store raw transcript JSON, persist timestamped transcript segments, and expose transcript status in the UI.

## PRD Coverage

- Local speech-to-text
- whisper.cpp
- Model management
- Timestamped transcript segments
- Transcript failure recovery

## Deliverables

- Audio extraction job from MP4.
- Whisper model directory management.
- Model presence detection.
- Missing model UI state and download/import path.
- Local `whisper.cpp` invocation.
- Chunking configuration for 15-30 second chunks where applicable.
- Raw transcript JSON storage.
- Segment persistence in SQLite.
- Transcript view in recording detail page.
- Retry flow for failed transcription.

## Implementation Notes

- The default model is `small.en`.
- Existing recordings must remain playable when transcription fails.
- Store Whisper output exactly as returned and keep app-specific metadata separately.
- Do not send audio to any network service.

## Acceptance Criteria

- The app can detect whether the selected Whisper model exists locally.
- A completed recording can produce a local transcript.
- Transcript segments include start time, end time, text, and confidence when available.
- Raw transcript JSON is saved under the recording directory.
- Transcript segments render in the UI.
- Failed transcription can be retried with the same or another model.

## Out Of Scope

- Speaker diarization.
- Real-time captions.
- Cloud transcription.
- Semantic embeddings.
