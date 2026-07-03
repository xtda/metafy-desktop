# 09 Backend Selection & Readiness

## Objective

Route each platform to its intended backend and expose product-ready readiness diagnostics without directing users to install FFmpeg.

## Scope

- Backend selection policy.
- Startup/readiness diagnostics.
- Tauri bootstrap native boundary status.
- Settings or error text that currently mentions FFmpeg or FFprobe.
- Platform-specific dependency messaging.

## Deliverables

- macOS selects the native Apple backend by default.
- Windows selects the native Media Foundation backend by default.
- Linux selects the GStreamer backend by default.
- FFmpeg fallback is allowed only behind an explicit temporary migration flag, if still needed.
- Readiness diagnostics report native/GStreamer state.
- UI and bootstrap text use backend-neutral media processing language.
- Linux reports missing GStreamer plugins clearly.

## Implementation Notes

The target product should not have an FFmpeg setup path. During migration, any FFmpeg fallback must be explicit and temporary so accidental production use is visible.

Readiness diagnostics should answer:

- Which backend is selected?
- Is the backend available?
- If not available, what user or package action is needed?
- Is the failure retryable after installing dependencies?

For macOS and Windows, dependency errors should usually mean platform API failure rather than missing user-installed tools. For Linux, GStreamer/plugin readiness is expected to be part of the support surface.

## Acceptance Criteria

- `app_bootstrap` no longer reports `implemented-system-ffmpeg` as the normal encoding status.
- User-facing retry/readiness text does not instruct macOS or Windows users to install FFmpeg.
- Linux readiness text names GStreamer and missing plugins where available.
- Backend selection tests cover macOS, Windows, Linux, and unsupported targets where practical.
- Temporary FFmpeg fallback cannot be selected silently in normal product builds.

## Out Of Scope

- Deleting FFmpeg code.
- Platform encoder implementation.
- Packaging cleanup.
