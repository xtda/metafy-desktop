# 05 Metadata & Thumbnail

## Objective

Replace FFprobe metadata extraction and FFmpeg thumbnail generation with backend-neutral metadata derivation and raw-frame thumbnail generation.

## Scope

- Final encode metadata currently read through FFprobe.
- Thumbnail currently generated with FFmpeg.
- Recording completion metadata written after encode.
- Synthetic encode tests that currently assert thumbnail output.

## Deliverables

- Backend-neutral `MediaInfo` or equivalent output struct.
- Metadata derivation from session, sidecar, prepared audio, backend success, and filesystem metadata.
- Thumbnail generation from the first raw BGRA frame.
- JPEG thumbnail writer shared by macOS, Windows, and Linux paths where practical.
- Removal of the need to run FFprobe for successful encode completion.

## Implementation Notes

Derived metadata should include:

- Width.
- Height.
- Frame rate.
- Frame count.
- Duration.
- Audio included.
- File size if useful.
- Backend id and backend diagnostics.

Duration should be derived from the greater of video timeline duration and mixed audio duration, falling back to persisted session duration where appropriate.

Thumbnail generation should prefer the first raw BGRA frame, scaled to the current thumbnail size policy. This avoids reopening or decoding the final MP4.

## Acceptance Criteria

- Successful encode completion does not require FFprobe.
- Thumbnail generation does not require FFmpeg.
- Final recording metadata remains sufficient for the existing UI.
- Thumbnail files are generated for synthetic captures.
- Metadata and thumbnail tests do not require FFmpeg or FFprobe.

## Out Of Scope

- Native MP4 writing.
- Changing library UI layout.
- Codec-level inspection beyond what the UI currently needs.
