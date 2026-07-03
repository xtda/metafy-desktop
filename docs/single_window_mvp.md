# Single Window And Application Capture MVP

## Goal

Allow the user to select a specific display, application, or window as the capture source on macOS and Windows, record source audio separately from microphone audio, and produce a reliable final recording even when the captured window is resized.

The MVP keeps the raw capture -> local temp files -> final encode flow. The important change is that the capture model becomes source-oriented instead of display-oriented, and audio becomes multi-source instead of microphone-only.

## Current State

- `src-tauri/src/capture.rs` exposes only display sources through `CaptureDisplaySource`.
- `list_display_sources()` already receives all `scap::Target` values, but filters out everything except `Target::Display`.
- `src-tauri/src/recorder.rs` accepts only `display:<id>` source ids and rejects `Target::Window`.
- Audio capture is currently microphone-only through CPAL.
- `src-tauri/src/encoding.rs` expects a single raw BGRA video stream plus an optional raw PCM audio stream.
- The encoder requires all captured frames to have the same width and height.

## MVP Scope

### In Scope

- Select a display, application, or window as the video source on macOS and Windows.
- Capture selected application/window video on macOS and Windows.
- Capture selected application/window audio on macOS and Windows.
- Capture microphone audio as a separate stream from application/window audio.
- Store microphone audio and application/window audio as separate raw sidecars.
- Encode the final MP4 with both audio sources mixed or mapped intentionally.
- Handle window resizes during recording by normalizing output dimensions.
- Keep the existing recording session storage, temporary media layout, encoding, transcription, and local-only behavior.
- Make the source selector resilient when the selected window disappears before recording starts.

### Out Of Scope

- Linux window audio support.
- User-adjustable post-recording audio mixing controls.
- Editing separate audio tracks after recording.
- Switching to direct-to-file encoders.

## Build Step Breakdown

The expanded MVP is tracked as Steps 12-18 under `build_steps/`.

| Step | Focus | Outcome |
| --- | --- | --- |
| [12 Capture Source Model](../build_steps/12_capture_source_model/12_capture_source_model.md) | Source-oriented command payloads, preferences, validation | Displays, applications, and windows can be represented consistently. |
| [13 Source Picker & Audio Mode UI](../build_steps/13_source_picker_ui/13_source_picker_ui.md) | Grouped source selection and explicit audio modes | Users can select source, microphone audio, source audio, both, or no audio. |
| [14 Application & Window Video Capture](../build_steps/14_application_window_video_capture/14_application_window_video_capture.md) | macOS and Windows selected-source video | Application/window video capture works while existing display capture remains intact. |
| [15 Source Audio Backends](../build_steps/15_source_audio_backends/15_source_audio_backends.md) | ScreenCaptureKit and Windows application loopback | Source audio is captured separately from microphone audio. |
| [16 Split Audio Session Storage](../build_steps/16_split_audio_session_storage/16_split_audio_session_storage.md) | SQLite, temp files, session sidecars | Microphone and source audio are recoverable independent artifacts. |
| [17 Resize Normalization](../build_steps/17_resize_normalization/17_resize_normalization.md) | Stable BGRA output canvas | Window resizing does not break encoding. |
| [18 Multi-Input Encoding & Validation](../build_steps/18_multi_input_encoding_validation/18_multi_input_encoding_validation.md) | Mixed-audio encoding and platform validation | All audio modes encode and play correctly on macOS and Windows. |

## Capture Source Model

Replace display-specific names at the command boundary with source-oriented names.

Suggested Rust shape:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureVideoSource {
    pub id: String,
    pub kind: CaptureVideoSourceKind,
    pub title: String,
    pub app_name: Option<String>,
    pub process_id: Option<u32>,
    pub window_id: Option<u32>,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureVideoSourceKind {
    Display,
    Application,
    Window,
}
```

Capture source ids should remain prefixed so old display ids cannot be confused with application or window ids:

- `display:<native_id>`
- `application:<native_id>`
- `window:<native_id>`

For macOS, `application:<native_id>` should map to an `SCRunningApplication` or equivalent app identifier, and `window:<native_id>` should map to a specific `SCWindow`. For Windows, `application:<native_id>` should map to a process tree when available, and `window:<native_id>` should map to an HWND-backed capture target.

## Audio Source Model

Microphone and source audio must be separate capture artifacts.

Suggested Rust shape:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureAudioConfig {
    pub mode: CaptureAudioMode,
    pub microphone: Option<MicrophoneDevice>,
    pub source_audio: Option<SourceAudioCaptureConfig>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureAudioMode {
    None,
    Microphone,
    Source,
    MicrophoneAndSource,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceAudioCaptureConfig {
    pub source_id: String,
    pub backend: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
}
```

The recording runtime should return separate audio configs for microphone and source audio. Avoid overloading the existing `include_microphone` flag to mean any audio.

## Source Video Requirements

### Enumeration

Change the current display-only source enumeration to include applications and windows:

- `scap::Target::Display(display)` -> `display:<id>`
- `scap::Target::Window(window)` -> `window:<id>`
- Platform application entries -> `application:<id>`

Filter out low-quality window targets:

- Empty titles.
- The Metafy Desktop app window, unless explicit self-capture is desired.
- Minimized or zero-sized windows if the platform reports dimensions.
- Utility windows that are unlikely to be useful, if they become noisy in practice.

### Validation

Validation should distinguish these cases:

- No capture permission.
- No sources available.
- Previously selected source no longer exists.
- Selected source exists but cannot be captured.
- Application or window disappeared after recording was configured.
- Source audio requested, but the selected platform backend cannot provide it.

### Recording

Update the target resolver to support each source type:

```text
display:<id>      -> display capture target
application:<id>  -> application capture target, or app-derived window/process target
window:<id>       -> window capture target
```

The rest of `build_screen_capturer()` should stay mostly unchanged for the first video pass:

- `FrameType::BGRAFrame`
- captured output resolution
- cursor capture enabled
- same frame-rate target

## Resize Handling

The current encoder rejects recordings where frame dimensions change. Window capture makes this likely because users can resize windows while recording, so resize handling is required for the MVP.

Required policy:

1. Lock the recording canvas to the initial captured frame size.
2. If a later frame has the same dimensions, write it unchanged.
3. If a later frame is smaller or larger, scale it proportionally to fit inside the locked canvas.
4. Center the scaled frame.
5. Fill unused pixels with black.
6. Continue writing a dimension-stable BGRA stream.

This keeps the raw video file compatible with the final encoder input path and avoids encode-time failures after an otherwise successful recording.

Open implementation detail:

- Normalize inside the frame writer before writing `.mfrv`, or preserve original frame dimensions in `.mfrv` and normalize in `prepare_video_frames()`.

Prefer normalizing in the frame writer so active session dimensions, preview metrics, and final encoding all describe the same output canvas.

## Audio Strategy

### Microphone Audio

Keep the existing CPAL implementation, but treat it as the microphone track only.

This preserves:

- Current microphone permission flow.
- Current audio timing headers.
- Current raw PCM input path for the microphone sidecar.
- Current transcription path after MP4 encoding.

Required changes:

- Rename internal generic audio fields where they actually mean microphone audio.
- Keep microphone stats separate from source-audio stats.
- Allow recordings with source audio and no microphone.
- Allow recordings with microphone and no source audio.

### macOS Application Or System Audio

Use ScreenCaptureKit for source audio.

Relevant platform facts:

- ScreenCaptureKit supports high-performance screen and audio capture.
- `SCContentFilter` is the native mechanism for limiting capture to displays, apps, and windows.
- The current `open-gpui-scap` fork exposes window targets, but the local crate source shows its macOS stream config sets audio capture off.

Required direction:

1. Use a ScreenCaptureKit content filter that matches the selected app/window.
2. Capture source audio from that filtered stream.
3. Exclude Metafy Desktop process audio by default where supported.
4. Write source audio to its own raw sidecar with its own sample format metadata.

Implementation options:

1. Patch or vendor the current `open-gpui-scap` dependency to expose ScreenCaptureKit audio frames.
2. Evaluate switching to a `scap` variant that already exposes `captures_audio`.
3. Add a dedicated macOS ScreenCaptureKit backend that owns both source video and source audio.

For maintainability, prefer one macOS backend that produces both video and source audio once this scope is implemented.

References:

- [Apple ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit/)
- [Apple SCContentFilter](https://developer.apple.com/documentation/screencapturekit/sccontentfilter)

### Windows Application Or System Audio

Use separate APIs for video and audio.

Video:

- Windows Graphics Capture supports capturing a selected display or application window.
- The current `open-gpui-scap` dependency already uses Windows capture internals for video.

Audio:

- For process-tree-specific source audio, use the Windows Application Loopback API when available.
- For a selected window, resolve the owning process and capture that process tree.
- For a selected application, capture the application process tree.
- Fall back to WASAPI system loopback only when process-specific capture is unsupported or explicitly selected.
- Keep source audio separate from microphone audio.

Windows cannot always express "this exact window's audio" if multiple windows share one process. The MVP should document this as process-tree source audio for Windows, while the UI can still present it as application/window source audio.

References:

- [Windows screen capture](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)
- [WASAPI loopback recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- [Application loopback audio capture sample](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/)

### Linux

Defer app/window audio for the MVP.

Linux support likely needs a PipeWire portal based approach for screen/window capture and audio routing. Treat Linux as display/window video plus microphone audio until there is a dedicated platform pass.

## Encoding Requirements

Use the selected native/GStreamer backend as the final muxing and encoding boundary.

Video:

- The raw BGRA video stream must have stable dimensions.
- Resize normalization should happen before final encode sees the video input.
- The final encoder should continue receiving one video input.

Audio:

- Support zero, one, or two raw audio inputs.
- Microphone audio and source audio should remain separate sidecars until shared audio preparation.
- Mix microphone and source audio during final encode preparation when the final MP4 should have one playback track.
- Optionally preserve separate MP4 audio tracks later, but do not require that for the MVP.
- Preserve warning paths if a requested source is missing, unsupported, or silent.

Example direction for two audio inputs:

```text
video raw BGRA -> 0:v
microphone PCM -> 1:a
source PCM -> 2:a
amix=inputs=2 -> encoded AAC
```

Keep audio mixing deterministic and covered by shared Rust tests.

The transcript extraction path can continue extracting from the final MP4. If separate MP4 tracks are preserved later, transcription should use the mixed speech-relevant track or an explicit transcription mixdown.

## Storage Requirements

MVP schema changes:

- Rename `screen_source_id` to `video_source_id`, or add `video_source_id` while keeping backward-compatible reads from `screen_source_id`.
- Add `video_source_kind`.
- Add `audio_mode`.
- Add `source_audio_path`.
- Add source-audio metadata:
  - `source_audio_byte_count`
  - `source_audio_sample_rate`
  - `source_audio_channels`
  - `source_audio_sample_format`
- Preserve microphone-specific metadata separately:
  - `microphone_audio_path`
  - `microphone_audio_byte_count`
  - `microphone_audio_sample_rate`
  - `microphone_audio_channels`
  - `microphone_audio_sample_format`
- Store source metadata useful for recovery and debugging:
  - app name
  - process id
  - native window id
  - platform backend

Temporary media layout:

```text
temp/recording-sessions/{session_id}/
  screen_frames.mfrv
  microphone.pcm
  source_audio.pcm
  metadata.json
```

Backward compatibility:

- Existing recordings with only `audio_path` should be treated as microphone audio.
- Existing capture preferences with `screen_source_id` should be migrated or read as `video_source_id`.

## UI Requirements

Source selection:

- Rename `Display` selector to `Source`.
- Group or label options by kind:
  - Displays
  - Applications
  - Windows
- Show window title, and app/process name when available.
- Disable source changes while recording.
- If the selected app/window disappears, show a validation error and require a new selection.

Audio controls:

- Add audio mode controls:
  - No audio
  - Microphone
  - Source audio
  - Microphone + source audio
- Show source-audio availability for the selected source and platform.
- Keep unsupported modes disabled with concise platform-specific messaging.
- Show separate live byte counts or status indicators for microphone and source audio while recording.

Resize behavior:

- Do not ask users to avoid resizing windows.
- Keep recording active when a captured window resizes.
- The UI can show the locked output resolution and current source size if that helps debugging, but this should not be prominent in the core recording flow.

## Permissions And Packaging

macOS:

- Keep `NSScreenCaptureUsageDescription`.
- Confirm packaged builds appear under Screen & System Audio Recording.
- Validate ScreenCaptureKit source-audio permission behavior on a signed build.
- Validate that Metafy Desktop can exclude its own audio where supported.

Windows:

- Validate Windows Graphics Capture availability before showing window capture.
- For process-loopback audio, require Windows 10 build 20348 or newer.
- Provide fallback messaging when only system-wide WASAPI loopback is available.
- Validate the difference between selected-window video and selected-process audio when multiple windows share one process.

Linux:

- Defer app/window audio permission design.

## Testing Plan

Automated checks:

- Source id parsing for `display:<id>`, `application:<id>`, and `window:<id>`.
- Validation when selected source disappears.
- Recording session creation with a window source id.
- Recording session creation with application/source audio enabled.
- Separate microphone and source-audio sidecar creation.
- Encoder behavior with microphone-only audio.
- Encoder behavior with source-only audio.
- Encoder behavior with microphone + source audio.
- Resize normalization for smaller, larger, wider, and taller frames.
- Encoder behavior for zero, one, and two audio inputs.

Manual checks:

- macOS: record a normal app window.
- macOS: record a browser window with video playback and source audio.
- macOS: record microphone + source audio and verify both are present.
- Windows: record a normal app window.
- Windows: record a browser window with video playback and source audio.
- Windows: record microphone + source audio and verify both are present.
- Start recording, then close the selected window.
- Resize selected window during recording.
- Resize selected window multiple times during recording.
- Record with microphone enabled and source audio disabled.
- Record with microphone disabled and source audio enabled.
- Verify raw media stays under local app data.
- Verify final MP4 plays and has expected audio/video.
- Verify final MP4 does not fail encoding after window resize.
- Verify app audio and microphone audio are stored separately before encode.

## Open Decisions

- Should window capture include child windows/popovers?
- Should the app exclude itself from window enumeration by default?
- Should the final MP4 contain a mixed audio track only, or both mixed and separate microphone/source tracks?
- Should Windows source audio fall back automatically to system loopback, or require explicit user opt-in when process loopback is unavailable?
- Should application capture select all windows for that application, or only the foreground/main window by default?
- Should resize normalization use contain-and-pad only, or offer crop/fill as a future setting?
