# Status: 17 Resize Normalization

Current status: Implemented

Progress: 100%

Sign-off: Pending

Last updated: 2026-07-02

## Checklist

- [x] Recording canvas locks to initial output dimensions.
- [x] Same-size frames pass through unchanged.
- [x] Changed-size frames are scaled to fit.
- [x] Padding is centered and black.
- [x] Raw video stream remains dimension-stable.
- [x] Resize normalization tests cover core shape changes.
- [x] Acceptance criteria in `17_resize_normalization.md` are met.

## Validation Evidence

- 2026-07-02: Implemented BGRA scale-to-fit normalization before `.mfrv`
  writes. The writer locks the output canvas from the capture backend's initial
  output size, passes same-size frames through unchanged, scales resized frames
  into the locked canvas, and pads unused regions with centered opaque black.
- 2026-07-02: Added session JSON metadata for output dimensions, current source
  dimensions, and the `scale_to_fit`/`centered_opaque_black` normalization
  policy.
- 2026-07-02: `cargo test recorder --lib` passed: 9 passed, 0 failed. Covers
  same-size, smaller, larger, wider, and taller BGRA inputs.
- 2026-07-02: `cargo test --lib` passed: 36 passed, 0 failed. Existing warnings
  are from the vendored `scap` crate.

## Open Questions

- Should crop/fill be offered later as a recording preference?

## Sign-Off

- [ ] Approved by Andrew.
