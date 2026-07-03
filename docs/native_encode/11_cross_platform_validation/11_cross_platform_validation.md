# 11 Cross-Platform Validation

## Objective

Validate the completed native/GStreamer media pipeline across packaged macOS, Windows, and Linux builds before marking FFmpeg removal complete.

## Scope

- Packaged app launch behavior.
- Recording and encode across all supported audio modes.
- Playback in app and platform media players.
- Thumbnail generation.
- Local Whisper transcription through direct WAV.
- Failure/retry behavior.
- Local-only privacy guardrails.

## Deliverables

- macOS packaged-app validation evidence.
- Windows packaged-app validation evidence.
- Linux packaged-app or dev-package validation evidence.
- Automated test evidence for shared media logic and available platform backends.
- Manual validation notes for media playback and transcription.
- A completed [manual validation checklist](manual_validation_checklist.md).
- Final sign-off that FFmpeg and FFprobe are not required, bundled, or invoked.

## Automated Validation Matrix

| Case | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Video-only synthetic encode | Required | Required | Required |
| Microphone-only synthetic encode | Required | Required | Required |
| Source-only synthetic encode | Required | Required | Platform-dependent until Linux source audio exists |
| Microphone + source synthetic encode | Required | Required | Platform-dependent until Linux source audio exists |
| Thumbnail generation | Required | Required | Required |
| Direct transcription WAV | Required | Required | Required |
| Missing audio sidecar warnings | Required | Required | Required |
| Silent audio sidecar warnings | Required | Required | Required |
| Unsupported sample format warnings | Required | Required | Required |
| Retry after encode failure | Required | Required | Required |

## Manual Validation Checklist

Use [manual_validation_checklist.md](manual_validation_checklist.md) for the detailed run log. The minimum checklist is:

- [ ] Start packaged app from the normal desktop shell, not the development terminal.
- [ ] Confirm FFmpeg and FFprobe are not installed, not on `PATH`, and not bundled.
- [ ] Record display video-only.
- [ ] Record display with microphone.
- [ ] Record window/app source with source audio where supported.
- [ ] Record microphone + source audio where supported.
- [ ] Resize the captured window during recording.
- [ ] Play the final MP4 in the app.
- [ ] Open the final MP4 in the platform media player.
- [ ] Verify thumbnail display.
- [ ] Run local Whisper transcription.
- [ ] Confirm raw media still does not leave the device.

## Acceptance Criteria

- macOS validates without FFmpeg or FFprobe.
- Windows validates without FFmpeg or FFprobe.
- Linux validates through GStreamer without FFmpeg or FFprobe.
- Direct transcription WAV works across platforms.
- Failure recovery still preserves sidecars and supports retry.
- Optional AI remains transcript-only.
- The overall [status.md](../status.md) can be marked complete.

## Out Of Scope

- New editing features.
- Post-recording mixer UI.
- Replacing `whisper.cpp`.
