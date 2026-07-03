# Status: 05 Metadata & Thumbnail

Current status: Accepted

Progress: 100%

Sign-off: Accepted

Last updated: 2026-07-02

## Checklist

- [x] Backend-neutral media metadata struct exists.
- [x] Metadata no longer requires FFprobe.
- [x] Thumbnail generation no longer requires FFmpeg.
- [x] Thumbnail generation uses raw BGRA input or a backend-neutral image path.
- [x] Existing UI receives equivalent recording metadata.
- [x] Acceptance criteria in `05_metadata_thumbnail.md` are met.

## Validation Evidence

- 2026-07-02: Added backend-neutral `MediaInfo` derivation in `src-tauri/src/media/metadata.rs`; duration is derived from the greater of video frame timeline and prepared-audio timeline, falling back to persisted session duration when no derived timeline exists.
- 2026-07-02: Added shared raw BGRA-to-JPEG thumbnail writing in `src-tauri/src/media/thumbnail.rs` using the Rust `image` crate with JPEG-only features. The first raw sidecar frame now feeds thumbnail generation without reopening the MP4.
- 2026-07-02: Updated the temporary FFmpeg backend so successful encode completion only invokes FFmpeg for MP4 encoding. It no longer discovers or runs FFprobe, and it no longer runs an FFmpeg thumbnail command. Legacy `ffprobePath` and `ffprobeJson` job-output fields remain for compatibility and are now `null`.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml media::metadata` passed: 2 tests passed.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml media::thumbnail` passed: 2 tests passed without external media binaries.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml encoding::tests` passed: 9 tests passed, including synthetic MP4 encode with raw-frame thumbnail generation and `ffprobePath`/`ffprobeJson` asserted as absent.
- 2026-07-02: `cargo test --manifest-path src-tauri/Cargo.toml` passed: 68 tests passed.
- 2026-07-02: `deno task check` passed: `svelte-check found 0 errors and 0 warnings`.
- 2026-07-02: `rg -n "METAFY_FFPROBE_PATH|FFprobe metadata|FFmpeg thumbnail|build_thumbnail_command|run_metadata_probe|probe_duration_ms|metadata_probe" src-tauri/src/encoding.rs src-tauri/src/media` returned no matches.

## Open Questions

- None. The implementation uses the Rust `image` crate with JPEG-only features for a shared cross-platform thumbnail path.

## Sign-Off

- [x] Approved by Andrew on 2026-07-02.
