# 10 Remove FFmpeg

## Objective

Delete FFmpeg and FFprobe from product code, runtime binary discovery, packaging scripts, tests, and active documentation.

## Scope

- FFmpeg backend code.
- `METAFY_FFMPEG_PATH` and `METAFY_FFPROBE_PATH`.
- Bundled binary extraction lists.
- Binary download scripts.
- README and packaging notes.
- Build-step docs that currently describe FFmpeg as active product behavior.
- FFmpeg-specific tests.

## Deliverables

- FFmpeg backend deleted.
- FFprobe integration deleted.
- FFmpeg/FFprobe binary discovery deleted.
- FFmpeg archive download support deleted.
- Product docs updated to native macOS, native Windows, and Linux GStreamer.
- FFmpeg-specific result fields removed or renamed to backend-neutral diagnostics.
- Tests updated to assert native/GStreamer behavior or shared logic.

## Implementation Notes

Search targets:

```text
ffmpeg
ffprobe
METAFY_FFMPEG_PATH
METAFY_FFPROBE_PATH
implemented-system-ffmpeg
```

Historical docs can keep references only when clearly marked historical. Active setup, packaging, validation, and runtime docs should not tell users to install or configure FFmpeg.

Do not delete `whisper.cpp` binary discovery as part of this step. Whisper is a separate dependency.

## Acceptance Criteria

- Product code has no FFmpeg or FFprobe execution path.
- Packaged apps do not include FFmpeg or FFprobe artifacts.
- README no longer documents FFmpeg setup.
- Build and validation docs reflect native/GStreamer media processing.
- `rg -n "ffmpeg|ffprobe|METAFY_FFMPEG|METAFY_FFPROBE" .` returns no active product references.
- Full relevant test suite passes after deletion.

## Out Of Scope

- Replacing `whisper.cpp`.
- Rewriting old historical records that are explicitly marked historical.
- Adding new media features.
