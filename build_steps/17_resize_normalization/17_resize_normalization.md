# 17 Resize Normalization

## Objective

Handle application/window resize during recording by normalizing captured frames into a stable output canvas before encoding.

## PRD Coverage

- Reliable application/window recording
- Stable encoder video input
- Recoverable capture pipeline
- Local video processing

## Deliverables

- Initial output canvas selection when recording starts.
- BGRA frame normalization for changed source dimensions.
- Aspect-ratio-preserving scale-to-fit behavior.
- Centered black padding for unused canvas space.
- Recording metadata for locked output dimensions and current source dimensions when useful.
- Tests for same-size, smaller, larger, wider, and taller frame inputs.

## Implementation Notes

- Prefer normalizing before writing `.mfrv` so downstream encoding stays simple.
- Keep the resize path efficient enough for 1080p at 30 FPS.
- Preserve cursor and frame timing metadata.
- Do not stop recording solely because a selected window resizes.

## Acceptance Criteria

- Resizing a selected window during recording does not break final MP4 encoding.
- The raw BGRA stream remains dimension-stable.
- Aspect ratio is preserved after resize.
- Padding is deterministic and visually acceptable.
- Existing display capture output is unchanged when dimensions are stable.

## Out Of Scope

- User-selectable crop/fill policies.
- Video editing.
- Post-recording resize changes.
- GPU acceleration unless CPU normalization is insufficient.
