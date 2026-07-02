# 08 Local Search

## Objective

Implement local keyword search across transcript segments using SQLite FTS5, return timestamped results, and support jump-to-result playback.

## PRD Coverage

- Searchable local transcript history
- SQLite FTS5 local search
- Timestamped search results
- Jump directly to recording timestamp

## Deliverables

- FTS5 virtual table or equivalent local index.
- Indexing job for transcript segments.
- Search command exposed to the frontend.
- Search UI with empty/loading/result states.
- Result rows containing recording, timestamp, transcript segment, and rank/match score.
- Jump-to-timestamp behavior in playback view.
- Reindex path for interrupted or stale indexes.

## Implementation Notes

- Keep this MVP to local keyword search.
- Do not introduce pgvector or a backend search service.
- Store enough metadata with results for navigation without extra fragile lookups.
- Search should work offline.

## Acceptance Criteria

- Transcript segments are indexed locally.
- User can search across all local recordings.
- Search results show recording, timestamp, matching text, and ranking information.
- Clicking a result opens the recording and seeks to the timestamp.
- Search remains functional without network access.

## Out Of Scope

- Semantic search.
- Local embeddings.
- AI chat.
- OCR over video frames.
