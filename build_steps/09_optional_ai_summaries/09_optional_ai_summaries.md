# 09 Optional AI Summaries

## Objective

Add optional transcript-only AI analysis for summaries, action items, decisions, questions, risks, and chapters, while preserving the local-only core workflow when AI is disabled.

## PRD Coverage

- Optional AI processing
- Transcript-only AI input
- No audio or video sent to LLM providers
- AI outputs stored locally

## Deliverables

- AI disabled-by-default setting.
- Provider/model configuration UI.
- Transcript-only prompt assembly.
- Guardrail that prevents audio/video paths or raw media from entering AI payloads.
- AI summary job.
- Local persistence for summary, action items, decisions, questions, chapters, provider, model, and status.
- AI failure state and retry flow.
- UI for AI output in the recording detail page.

## Implementation Notes

- The core recording, playback, transcription, and search workflows must work with AI disabled.
- AI requests may require network access, but only after the user explicitly enables and configures AI.
- Payloads should include transcript text, recording metadata, and optional user notes only.
- Treat AI output as local derived metadata.

## Acceptance Criteria

- AI is disabled by default.
- Enabling AI requires explicit user configuration.
- AI payload construction excludes raw audio, raw video, and media file paths.
- AI summaries are stored locally.
- AI failure does not affect recording playback or local search.
- User can retry failed AI processing.

## Out Of Scope

- Real-time AI assistant.
- AI chat over search results.
- Vision/frame analysis.
- Backend-hosted AI jobs.
