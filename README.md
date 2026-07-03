# Metafy Desktop

Local-only desktop recording and knowledge assistant MVP.

## Stack

- Tauri 2 desktop shell
- Rust native application logic
- SvelteKit frontend
- Deno for frontend dependency installation and tasks

## Local-Only Defaults

The core app does not depend on a backend, account system, cloud storage, uploads,
or network access. Raw audio and video are expected to stay on the user's device.

Rust owns the native domains that touch the filesystem, capture devices, encoding,
transcription, and job orchestration. The Svelte frontend talks to Rust through
narrow Tauri commands only.

The native command boundary exposes local-only storage operations:

- `app_bootstrap`: returns runtime metadata, local-only defaults, initialized
  storage paths, schema status, native module boundaries, and available commands.
- `storage_overview`: returns the resolved app data directory, SQLite schema
  version, table list, recording count, and processing job count.
- `create_recording`, `list_recordings`, `get_recording`, `update_recording`:
  create/read/update local recording metadata and stable recording directories.
- `persist_transcript`, `get_transcript_by_recording`: persist raw transcript
  metadata and indexed timestamp segments by recording.
- `upsert_ai_summary`, `get_ai_summary_by_recording`: store optional
  transcript-only AI outputs locally.
- `get_ai_settings`, `save_ai_settings`: store disabled-by-default optional AI
  provider, model, endpoint, and local API-key configuration.
- `summarize_recording`: run an explicit transcript-only AI summary job with
  guardrails that reject raw media fields and media paths before provider calls.
- `create_processing_job`, `list_processing_jobs`, `update_processing_job`:
  persist durable local job state for restart/recovery flows.
- `start_recording_session`, `stop_recording_session`,
  `active_recording_session`, `get_recording_session_by_recording`: manage
  local capture sessions and recoverable temporary media.
- `encode_recording`: retry local native/GStreamer media encoding for a
  stopped recording session whose temporary media is still present.
- `recording_asset_paths`: resolve stored recording media and thumbnail paths
  to absolute paths for Tauri asset-protocol playback.

## Local Runtime Binary Strategy

Media encoding uses platform media backends: AVFoundation on macOS, Media
Foundation on Windows, and GStreamer on Linux. Linux systems need GStreamer and
the required H.264/AAC plugins installed before recording output can be encoded.

Whisper uses a local whisper.cpp command. Set `METAFY_WHISPER_CPP_PATH` to an
explicit binary path, or install one of `whisper-cli`, `main`, or `whisper`.
Model files stay under the app data directory and can be imported through the
app UI.

Completed encodes write:

```text
app-data/
  recordings/
    {recording_id}/
      recording.mp4
      thumbnail.jpg
```

Capture temp files under `app-data/temp/recording-sessions/{session_id}/` are
preserved after encode failures so the recording can be retried.

Local storage is initialized under the platform app data directory on startup:

```text
app-data/
  app.sqlite
  recordings/
    {recording_id}/
  models/
    whisper/
  temp/
```

## Setup

Install dependencies:

```sh
deno install
```

Check the frontend:

```sh
deno task check
```

Run the Tauri desktop app in development:

```sh
deno task tauri dev
```

Build the frontend:

```sh
deno task build
```

Build the desktop app:

```sh
deno task tauri build
```

Build a local macOS release app bundle:

```sh
deno task bundle:app
```

## Project Shape

```text
src/
  routes/
    +layout.svelte     Svelte app layout
    +layout.ts         SPA rendering mode for Tauri
    +page.svelte       App shell, recording controls, local library, detail, search, and settings UI
src-tauri/
  src/
    ai.rs              Transcript-only AI payload guardrails and provider calls
    commands.rs        Tauri command boundary
    config.rs          Local-only defaults
    encoding.rs        Native/GStreamer encoding, thumbnail generation, and media metadata
    lib.rs             Tauri builder entrypoint
    main.rs            Desktop process entrypoint
    recorder.rs        Capture session runtime and temporary media writers
    storage.rs         App data paths, SQLite schema, models, and DAL
```
