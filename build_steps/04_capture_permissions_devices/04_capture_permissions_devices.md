# 04 Capture Permissions & Devices

## Objective

Detect screen and microphone capture capability, request platform permissions, enumerate available sources/devices, and expose selected capture inputs to the recording pipeline.

## PRD Coverage

- Screen capture with `scap`
- Microphone capture with `cpal`
- Cross-platform desktop support
- Privacy-first local capture

## Deliverables

- Screen capture capability check.
- Microphone capture capability check.
- Platform permission state detection where available.
- Permission request flow surfaced to the UI.
- Screen/display source enumeration.
- Microphone device enumeration.
- Selected source/device persistence.
- Clear error states for denied/missing permissions.

## Implementation Notes

- Platform-specific permission behavior should be isolated behind a stable Rust interface.
- Avoid starting recording until required permissions and devices are valid.
- Store user-selected devices locally.
- Treat missing microphone as recoverable if screen-only capture is later allowed, but MVP success criteria require microphone recording.

## Acceptance Criteria

- The app can list available displays/screens.
- The app can list available microphone input devices.
- The app can persist selected capture devices.
- Permission failures are visible in the UI and do not crash the app.
- The recording pipeline can receive validated capture configuration.

## Out Of Scope

- Encoding final MP4 output.
- Transcription.
- Search.
- Permission automation for installers.
