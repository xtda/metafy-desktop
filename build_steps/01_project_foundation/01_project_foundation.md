# 01 Project Foundation

## Objective

Create the base desktop application using the locked local-only stack:

- Tauri
- Svelte
- Deno for frontend package/task management
- Rust for native application logic

This step establishes the project shape, development commands, native command boundary, and baseline app shell needed by all later work.

## PRD Coverage

- Technology stack
- Cross-platform desktop support
- Local-only architecture
- No backend or cloud dependency for core workflows

## Deliverables

- Tauri project scaffolded with a Svelte frontend.
- Deno configured as the frontend package manager and task runner.
- Rust app entrypoint and Tauri command structure in place.
- Baseline frontend route/layout structure.
- Documented local development commands.
- Minimal "app is running" UI that can call a native command.
- Initial local-only configuration defaults.

## Implementation Notes

- Keep recorder/transcription logic out of UI components.
- Treat Rust native modules as the source of truth for filesystem, capture, encoding, transcription, and job orchestration.
- Expose only narrow commands to the Svelte frontend.
- Avoid committing any backend, auth, upload, or sync assumptions into the project skeleton.

## Acceptance Criteria

- The desktop app runs locally in development mode.
- The frontend renders from Svelte.
- Deno tasks can install, check, and run the frontend workflow.
- A frontend action can invoke a Rust/Tauri command and render the result.
- The app does not require network access to start.
- The repository contains clear setup instructions for local development.

## Out Of Scope

- Actual screen/audio recording.
- SQLite schema implementation.
- Media encoding and Whisper integration.
- Packaging installers.
