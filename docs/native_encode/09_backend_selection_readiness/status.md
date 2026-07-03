# Status: 09 Backend Selection & Readiness

Current status: Accepted

Progress: 100%

Sign-off: Accepted

Last updated: 2026-07-02

## Checklist

- [x] Backend selection policy exists.
- [x] macOS selects native Apple backend by default.
- [x] Windows selects Media Foundation backend by default.
- [x] Linux selects GStreamer backend by default.
- [x] Readiness diagnostics are backend-neutral.
- [x] UI/bootstrap text no longer points normal users at FFmpeg.
- [x] Temporary FFmpeg fallback is explicit if it still exists.
- [x] Acceptance criteria in `09_backend_selection_readiness.md` are met.

## Validation Evidence

- 2026-07-02: Added `media::readiness::selected_media_backend_readiness` and serialized `mediaBackend` in `app_bootstrap`, covering selected backend, display name, availability, retryability, user action, missing components, and diagnostic messages.
- 2026-07-02: `app_bootstrap` encoding boundary status now reports the selected media backend readiness status instead of `implemented-system-ffmpeg`.
- 2026-07-02: macOS readiness selects `macos-avfoundation`; Windows readiness selects `windows-media-foundation`; Linux readiness selects `linux-gstreamer` and carries GStreamer missing-plugin details and install guidance when unavailable.
- 2026-07-02: Unsupported targets no longer select the temporary FFmpeg backend in normal builds. The temporary command fallback is only available when built with the explicit `temporary-ffmpeg-fallback` Cargo feature.
- 2026-07-02: Updated the Svelte bootstrap type, runtime pill, Settings processing diagnostics, and processing placeholder text to use backend-neutral media processing language.
- 2026-07-02: Svelte autofixer found no issues in the edited runes/markup slice for `src/routes/+page.svelte`.
- 2026-07-02: `rg -n "implemented-system-ffmpeg|FFmpeg is preparing|install FFmpeg|install ffmpeg" src-tauri/src/commands.rs src/routes/+page.svelte` returned no matches.
- 2026-07-02: `cargo fmt --manifest-path src-tauri/Cargo.toml --check` passed.
- 2026-07-02: `cargo check --manifest-path src-tauri/Cargo.toml` passed. Existing warnings remain from vendored `scap` and unused resize-normalization debug fields.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml media::readiness::tests` passed: 6 tests covering macOS, Windows, Linux, unsupported targets, missing GStreamer plugins, and explicit temporary fallback behavior.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml encoding::tests` passed: 10 tests.
- 2026-07-02: `deno task check` passed: `svelte-check found 0 errors and 0 warnings`.

## Open Questions

- None for Step 09. Linux-hosted runtime validation remains tracked by Step 08, and packaged cross-platform validation remains tracked by Step 11.

## Sign-Off

- [x] Approved by Andrew on 2026-07-02.
