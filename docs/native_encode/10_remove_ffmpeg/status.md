# Status: 10 Remove FFmpeg

Current status: Ready for Review

Progress: 100%

Sign-off: Pending

Last updated: 2026-07-02

## Checklist

- [x] FFmpeg backend code is deleted.
- [x] FFprobe code is deleted.
- [x] FFmpeg/FFprobe binary discovery is deleted.
- [x] FFmpeg archive download support is deleted.
- [x] Product docs no longer instruct users to install FFmpeg.
- [x] FFmpeg-specific tests are replaced or removed.
- [x] Search audit has no active FFmpeg/FFprobe references.
- [x] Acceptance criteria in `10_remove_ffmpeg.md` are met.

## Validation Evidence

- 2026-07-02: Deleted `src-tauri/src/media/backends/ffmpeg.rs`, removed the backend module export, and removed the `temporary-ffmpeg-fallback` Cargo feature.
- 2026-07-02: Removed FFprobe parsing and command-probe metadata from the shared media metadata/diagnostics contract.
- 2026-07-02: Renamed encoding command output diagnostics to backend-neutral `backendId`, `backendLabel`, `backendCommands`, and `backendMessages`.
- 2026-07-02: Updated bundled binary extraction and `scripts/download-binaries.ts` so bundled archives cover whisper.cpp only; the Windows dry-run now resolves only `whisper-bin-x64.zip`.
- 2026-07-02: Updated active README, PRD, packaging, resource, and build-step docs to describe native macOS, native Windows, Linux GStreamer, and whisper.cpp setup instead of FFmpeg/FFprobe setup.
- 2026-07-02: `rg -n -i "ffmpeg|ffprobe|METAFY_FFMPEG|METAFY_FFPROBE|implemented-system-ffmpeg" src-tauri/src src-tauri/Cargo.toml scripts deno.json README.md prd.md prd_local_only.md build_steps docs/single_window_mvp.md src-tauri/resources/binaries/README.md` returned no matches.
- 2026-07-02: `rg -n -i "ffmpeg|ffprobe|METAFY_FFMPEG|METAFY_FFPROBE|implemented-system-ffmpeg" . | rg -v '^\./docs/native_encode/'` returned no matches; remaining matches are in the native-encoding migration docs and validation checklists.
- 2026-07-02: `find src-tauri/resources/binaries -maxdepth 2 -type f -print | sort` found only `.DS_Store`, `.gitignore`, `README.md`, and platform `.gitkeep` placeholders under the bundled binary resource tree.
- 2026-07-02: `cargo fmt --check` passed.
- 2026-07-02: `cargo check` passed. Existing warnings remain from vendored `scap` and unused resize-normalization debug fields.
- 2026-07-02: `cargo test` passed: 72 tests passed.
- 2026-07-02: `cargo test encoding::tests` passed: 6 tests passed.
- 2026-07-02: `deno task check` passed: `svelte-check found 0 errors and 0 warnings`.
- 2026-07-02: `deno task build` passed and wrote the static production output to `build`.
- 2026-07-02: `deno task binaries:download -- --platform windows-x86_64 --dry-run` passed and selected `whisper-bin-x64.zip`.

## Open Questions

- None. Historical references remain only inside the native-encoding migration docs and validation checklists.

## Sign-Off

- [ ] Approved by Andrew.
