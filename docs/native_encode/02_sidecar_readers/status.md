# Status: 02 Sidecar Readers

Current status: Implemented

Progress: 100%

Sign-off: Approved by Andrew

Last updated: 2026-07-02

## Checklist

- [x] Raw video sidecar reader exists.
- [x] Raw audio sidecar reader exists.
- [x] Encode path uses shared readers.
- [x] Reader tests cover malformed and valid sidecars.
- [x] Readers are streaming and do not load full recordings.
- [x] Acceptance criteria in `02_sidecar_readers.md` are met.

## Validation Evidence

- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml sidecar` passed. Covers valid, empty, malformed, truncated, unsupported, dimension mismatch, byte-count mismatch, audio metadata, and audio mode enumeration through the shared readers.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml encoding` passed. Verifies the migrated encode path and synthetic video-only, microphone-only, source-only, and microphone + source encodes.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml` passed. 50 Rust tests passed.
- 2026-07-02: `deno task check` passed. `svelte-check` reported 0 errors and 0 warnings.

## Open Questions

- Resolved with explicit `next_frame` and `next_block` streaming readers.

## Sign-Off

- [x] Approved by Andrew.
