# 03 App Shell & Library UI

## Objective

Build the first usable Svelte interface for the local recorder: navigation, recording controls, recording library, playback view, transcript panel, search entry points, and settings.

## PRD Coverage

- Local recording library
- Playable local MP4 recordings
- Searchable local transcript history
- Optional AI controls
- Local-only privacy model

## Deliverables

- Main application layout.
- Recording control area with idle/recording/processing states.
- Local recordings list.
- Recording detail view with video playback placeholder.
- Transcript panel placeholder.
- Search view placeholder.
- Settings view for:
  - Storage location display
  - Whisper model selection placeholder
  - Optional AI enablement placeholder
  - Local-only network-disable preference placeholder

## Implementation Notes

- Keep UI state separate from native processing state.
- Use Tauri commands for all filesystem/database access.
- Design screens around empty, loading, processing, failed, and complete states from the beginning.
- The UI should make local-only behavior obvious through settings and state, not marketing copy.

## Acceptance Criteria

- The app has a navigable Svelte UI.
- The recording library can render persisted recordings from SQLite.
- The recording detail view can show metadata and reserve space for playback/transcript/search results.
- The UI exposes clear status states for pending, processing, failed, and complete recordings.
- Settings reflect local-only defaults.

## Out Of Scope

- Actual capture implementation.
- Real transcript search.
- Real AI summary generation.
- Final visual polish.
