# 11 Packaging & Validation

## Objective

Package and validate the local-only MVP across supported desktop targets, with explicit checks against the PRD success criteria.

## PRD Coverage

- Windows/macOS/Linux support
- 1080p 30 FPS recording target
- Playable MP4 output
- Local transcription
- Local search
- Failure recovery
- No backend, no S3/R2, no network dependency for core workflows

## Deliverables

- Development validation checklist.
- Release build configuration.
- Platform packaging notes for macOS, Windows, and Linux.
- Bundled resource/sidecar audit for FFmpeg and Whisper strategy.
- Basic performance validation for 1080p 30 FPS recording.
- Offline-mode validation.
- Privacy validation that confirms audio/video are not uploaded by core workflows.
- Final MVP sign-off checklist.

## Implementation Notes

- Platform-specific packaging issues should be tracked here, not hidden in earlier implementation steps.
- Validate with network disabled for core workflows.
- Keep optional AI validation separate because it is not required for offline local-only operation.
- Record exact commands and evidence in `status.md`.

## Acceptance Criteria

- The app can be built for local testing.
- Core workflows pass with network disabled.
- A 1080p 30 FPS recording can be captured and played back on target hardware.
- Microphone audio is synchronized closely enough for MVP review.
- Local Whisper transcription works with the default model.
- Local search returns timestamped results.
- Failure recovery scenarios are manually verified.
- No backend or cloud storage configuration is required.

## Out Of Scope

- App store publication.
- Automatic updates.
- Cloud sync.
- Production support process.
