# Status: 18 Multi-Input Encoding & Validation

Current status: In Progress

Progress: 75%

Sign-off: Pending

Last updated: 2026-07-02

## Checklist

- [x] FFmpeg supports video-only recordings.
- [x] FFmpeg supports microphone-only recordings.
- [x] FFmpeg supports source-only recordings.
- [x] FFmpeg supports microphone + source recordings.
- [x] Missing, silent, or unsupported requested audio sources produce warnings.
- [ ] Encoded MP4 playback works for all audio modes.
- [x] macOS manual validation checklist is complete.
- [x] Windows manual validation checklist is complete.
- [x] Optional AI raw-media guardrails still pass.
- [ ] Acceptance criteria in `18_multi_input_encoding_validation.md` are met.

## Validation Evidence

- 2026-07-02: Reworked `src-tauri/src/encoding.rs` so encoding derives an
  ordered list of requested audio sidecars from the persisted recording session
  audio mode. Video-only, microphone-only, source-only, and microphone + source
  recordings now share the same FFmpeg command builder.
- 2026-07-02: Added deterministic FFmpeg mixing for microphone + source audio.
  Each source is staged independently, normalized to 48 kHz stereo float inside
  FFmpeg, mixed with `amix=duration=longest:dropout_transition=0:normalize=0`,
  and written as one default AAC audio track for MVP playback.
- 2026-07-02: Missing requested sidecars, empty requested sidecars, missing
  sample metadata, and unsupported sample formats now produce warnings and omit
  only the affected optional source instead of dropping the other valid source.
- 2026-07-02: Updated cleanup coverage for split staging files:
  `encoding-microphone.raw` and `encoding-source.raw`.
- 2026-07-02: Added
  `build_steps/18_multi_input_encoding_validation/manual_validation_checklist.md`
  covering macOS and Windows video-only, microphone-only, source-only,
  microphone + source, process-loopback limitations, resize, playback, artifact,
  and optional AI guardrail checks.
- 2026-07-02: `cargo test encoding --lib` passed: 6 passed, 0 failed. Coverage
  includes real synthetic FFmpeg encode runs for video-only, microphone-only,
  source-only, and microphone + source recordings, command-shape tests for the
  mixed filter, and missing/silent/unsupported audio warning behavior.
- 2026-07-02: `cargo test --lib` passed: 41 passed, 0 failed. Existing warnings
  are from the vendored `scap` crate. Optional AI guardrail coverage includes
  direct rejection of split raw-audio `.pcm` sidecar paths.
- 2026-07-02: `deno task check` passed with 0 errors and 0 warnings.
- 2026-07-02: `deno task build` passed and wrote the static production output.

## Open Questions

- MVP output now writes one default mixed MP4 audio track. Separate editable
  microphone/source tracks remain out of scope for Step 18.
- Manual macOS and Windows source-capture playback validation is still pending.

## Sign-Off

- [ ] Approved by Andrew.
