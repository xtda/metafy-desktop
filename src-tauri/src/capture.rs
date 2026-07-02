use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::ErrorKind;
use serde::Serialize;
use std::panic;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use crate::storage::{CaptureAudioMode, CaptureSelection};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapturePermissionState {
    Granted,
    PromptRequired,
    Denied,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureCapability {
    pub supported: bool,
    pub permission_state: CapturePermissionState,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureVideoSourceKind {
    Display,
    Application,
    Window,
}

impl CaptureVideoSourceKind {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Display => "display",
            Self::Application => "application",
            Self::Window => "window",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCaptureSourceId {
    pub kind: CaptureVideoSourceKind,
    pub native_id: String,
}

impl FromStr for ParsedCaptureSourceId {
    type Err = String;

    fn from_str(source_id: &str) -> Result<Self, Self::Err> {
        let (prefix, native_id) = source_id
            .split_once(':')
            .ok_or_else(|| "Source id must include a kind prefix.".to_owned())?;
        let native_id = native_id.trim();
        if native_id.is_empty() {
            return Err("Source id must include a native id.".to_owned());
        }

        let kind = match prefix {
            "display" => CaptureVideoSourceKind::Display,
            "application" => CaptureVideoSourceKind::Application,
            "window" => CaptureVideoSourceKind::Window,
            _ => {
                return Err(
                    "Source id must use display:, application:, or window: prefix.".to_owned(),
                )
            }
        };

        Ok(Self {
            kind,
            native_id: native_id.to_owned(),
        })
    }
}

pub fn parse_capture_video_source_id(source_id: &str) -> Result<ParsedCaptureSourceId, String> {
    ParsedCaptureSourceId::from_str(source_id)
}

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

pub type CaptureDisplaySource = CaptureVideoSource;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub channels: Option<u16>,
    pub sample_rate: Option<u32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedCaptureConfig {
    pub video_source: CaptureVideoSource,
    pub screen_source: CaptureVideoSource,
    pub audio: CaptureAudioConfig,
    pub microphone: Option<MicrophoneDevice>,
    pub include_microphone: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceAudioCaptureConfig {
    pub source_id: String,
    pub backend: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub sample_format: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureAudioConfig {
    pub mode: CaptureAudioMode,
    pub microphone: Option<MicrophoneDevice>,
    pub source_audio: Option<SourceAudioCaptureConfig>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    pub screen: CaptureCapability,
    pub microphone: CaptureCapability,
    pub source_audio: CaptureCapability,
    pub video_sources: Vec<CaptureVideoSource>,
    pub displays: Vec<CaptureDisplaySource>,
    pub microphones: Vec<MicrophoneDevice>,
    pub selection: CaptureSelection,
    pub validated_config: Option<ValidatedCaptureConfig>,
    pub validation_errors: Vec<String>,
}

pub fn capture_status(selection: CaptureSelection) -> CaptureStatus {
    build_status(selection, false, false)
}

pub fn request_capture_permissions(selection: CaptureSelection) -> CaptureStatus {
    build_status(selection, true, true)
}

pub fn validate_capture_config(
    selection: CaptureSelection,
) -> Result<ValidatedCaptureConfig, String> {
    let status = build_status(selection, false, true);

    status
        .validated_config
        .ok_or_else(|| status.validation_errors.join(" "))
}

fn build_status(
    selection: CaptureSelection,
    request_screen_permission: bool,
    probe_microphone_permission: bool,
) -> CaptureStatus {
    let mut screen = screen_capability(request_screen_permission);
    let video_sources = if screen.permission_state == CapturePermissionState::Granted {
        match list_video_sources() {
            Ok(sources) => sources,
            Err(error) => {
                screen.supported = false;
                screen.permission_state = CapturePermissionState::Unavailable;
                screen.error = Some(error);
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    let displays = video_sources
        .iter()
        .filter(|source| source.kind == CaptureVideoSourceKind::Display)
        .cloned()
        .collect::<Vec<_>>();

    let (microphones, microphone_error) = list_microphone_devices();
    let mut microphone = microphone_capability(&microphones, microphone_error);
    let source_audio = source_audio_capability(&screen);

    if selection.audio_mode.includes_microphone()
        && probe_microphone_permission
        && microphone.supported
    {
        match probe_microphone_input(selection.microphone_device_id.as_deref()) {
            Ok(()) => {
                microphone.permission_state = CapturePermissionState::Granted;
                microphone.error = None;
            }
            Err(error) => {
                microphone.permission_state = match error.kind() {
                    ErrorKind::PermissionDenied => CapturePermissionState::Denied,
                    ErrorKind::DeviceNotAvailable | ErrorKind::HostUnavailable => {
                        CapturePermissionState::Unavailable
                    }
                    _ => CapturePermissionState::Denied,
                };
                microphone.error = Some(error.to_string());
            }
        }
    }

    let (validated_config, validation_errors) = validate_status(
        &selection,
        &screen,
        &video_sources,
        &microphone,
        &microphones,
        &source_audio,
    );

    CaptureStatus {
        screen,
        microphone,
        source_audio,
        video_sources,
        displays,
        microphones,
        selection,
        validated_config,
        validation_errors,
    }
}

fn screen_capability(request_permission: bool) -> CaptureCapability {
    let supported = panic::catch_unwind(scap::is_supported).unwrap_or(false);

    if !supported {
        return CaptureCapability {
            supported: false,
            permission_state: CapturePermissionState::Unavailable,
            error: Some("Screen capture is not supported on this platform.".to_owned()),
        };
    }

    let has_permission = panic::catch_unwind(scap::has_permission).unwrap_or(false);
    if has_permission {
        return CaptureCapability {
            supported: true,
            permission_state: CapturePermissionState::Granted,
            error: None,
        };
    }

    if !request_permission {
        return CaptureCapability {
            supported: true,
            permission_state: CapturePermissionState::PromptRequired,
            error: Some("Screen capture permission has not been granted.".to_owned()),
        };
    }

    let granted = panic::catch_unwind(scap::request_permission).unwrap_or(false);
    CaptureCapability {
        supported: true,
        permission_state: if granted {
            CapturePermissionState::Granted
        } else {
            CapturePermissionState::Denied
        },
        error: (!granted).then(|| {
            "Screen capture permission was denied or still requires OS approval.".to_owned()
        }),
    }
}

fn list_video_sources() -> Result<Vec<CaptureVideoSource>, String> {
    let main_display_id = if cfg!(target_os = "linux") {
        None
    } else {
        panic::catch_unwind(scap::get_main_display)
            .ok()
            .map(|display| display.id)
    };
    let targets = panic::catch_unwind(scap::get_all_targets)
        .map_err(|_| "Unable to enumerate screen capture targets.".to_owned())?;
    let mut sources = Vec::new();
    let mut display_count = 0;

    for target in targets {
        match target {
            scap::Target::Display(display) => {
                let primary = main_display_id
                    .map(|id| id == display.id)
                    .unwrap_or(display_count == 0);
                display_count += 1;

                sources.push(CaptureVideoSource {
                    id: format!("display:{}", display.id),
                    kind: CaptureVideoSourceKind::Display,
                    title: display.title,
                    app_name: None,
                    process_id: None,
                    window_id: None,
                    primary,
                });
            }
            scap::Target::Window(window) if window_sources_supported() => {
                let title = window.title.trim().to_owned();
                if title.is_empty() || title.contains("Metafy Desktop") {
                    continue;
                }

                sources.push(CaptureVideoSource {
                    id: format!("window:{}", window.id),
                    kind: CaptureVideoSourceKind::Window,
                    title,
                    app_name: None,
                    process_id: None,
                    window_id: Some(window.id),
                    primary: false,
                });
            }
            scap::Target::Window(_) => {}
        }
    }

    Ok(sources)
}

fn window_sources_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

fn list_microphone_devices() -> (Vec<MicrophoneDevice>, Option<String>) {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());

    let input_devices = match host.input_devices() {
        Ok(devices) => devices,
        Err(error) => return (Vec::new(), Some(error.to_string())),
    };
    let mut devices = Vec::new();

    for device in input_devices {
        let id = match device.id() {
            Ok(id) => id.to_string(),
            Err(_) => continue,
        };
        let config = device.default_input_config();
        let (channels, sample_rate, error) = match config {
            Ok(config) => (Some(config.channels()), Some(config.sample_rate()), None),
            Err(error) => (None, None, Some(error.to_string())),
        };

        devices.push(MicrophoneDevice {
            is_default: default_id.as_deref() == Some(id.as_str()),
            id,
            name: device.to_string(),
            channels,
            sample_rate,
            error,
        });
    }

    (devices, None)
}

fn microphone_capability(
    microphones: &[MicrophoneDevice],
    error: Option<String>,
) -> CaptureCapability {
    if let Some(error) = error {
        return CaptureCapability {
            supported: false,
            permission_state: CapturePermissionState::Unavailable,
            error: Some(error),
        };
    }

    if microphones.is_empty() {
        return CaptureCapability {
            supported: false,
            permission_state: CapturePermissionState::Unavailable,
            error: Some("No microphone input devices were found.".to_owned()),
        };
    }

    CaptureCapability {
        supported: true,
        permission_state: CapturePermissionState::Unknown,
        error: None,
    }
}

fn source_audio_capability(screen: &CaptureCapability) -> CaptureCapability {
    crate::source_audio::source_audio_capability(screen)
}

fn probe_microphone_input(device_id: Option<&str>) -> Result<(), cpal::Error> {
    let host = cpal::default_host();
    let device = if let Some(device_id) = device_id.filter(|value| !value.is_empty()) {
        let parsed_id = cpal::DeviceId::from_str(device_id)?;
        host.device_by_id(&parsed_id)
            .ok_or_else(|| cpal::Error::new(ErrorKind::DeviceNotAvailable))?
    } else {
        host.default_input_device()
            .ok_or_else(|| cpal::Error::new(ErrorKind::DeviceNotAvailable))?
    };
    let config = device.default_input_config()?;
    let stream = device.build_input_stream_raw(
        config.config(),
        config.sample_format(),
        |_data, _info| {},
        |_error| {},
        Some(Duration::from_millis(500)),
    )?;

    stream.play()?;
    thread::sleep(Duration::from_millis(120));
    drop(stream);

    Ok(())
}

fn validate_status(
    selection: &CaptureSelection,
    screen: &CaptureCapability,
    video_sources: &[CaptureVideoSource],
    microphone: &CaptureCapability,
    microphones: &[MicrophoneDevice],
    source_audio: &CaptureCapability,
) -> (Option<ValidatedCaptureConfig>, Vec<String>) {
    let mut errors = Vec::new();

    if !screen.supported {
        errors.push(
            screen
                .error
                .clone()
                .unwrap_or_else(|| "Screen capture is not supported.".to_owned()),
        );
    } else if screen.permission_state != CapturePermissionState::Granted {
        errors.push(
            screen
                .error
                .clone()
                .unwrap_or_else(|| "Screen capture permission is not granted.".to_owned()),
        );
    }

    let selected_source_id = selected_video_source_id(selection);
    let parsed_source_id = selected_source_id
        .map(parse_capture_video_source_id)
        .transpose();
    if let Err(error) = &parsed_source_id {
        errors.push(format!("Selected video source id is invalid: {error}"));
    }

    let video_source = match parsed_source_id.as_ref() {
        Ok(parsed) => selected_video_source(parsed.as_ref(), video_sources),
        Err(_) => None,
    };

    if video_sources.is_empty() {
        errors.push("No video sources are available for capture.".to_owned());
    } else if let Ok(Some(parsed)) = parsed_source_id.as_ref() {
        if video_source.is_none() {
            errors.push(format!(
                "The selected {} is no longer available.",
                parsed.kind.label()
            ));
        }
    }

    if let Some(source) = video_source {
        if source.kind == CaptureVideoSourceKind::Application {
            errors.push(
                "Application video capture is not available with the current capture backend. Select a window from that application instead."
                    .to_owned(),
            );
        }
    }

    let microphone_device = if selection.audio_mode.includes_microphone() {
        let selected = selected_microphone(selection, microphones);

        if !microphone.supported {
            errors.push(
                microphone
                    .error
                    .clone()
                    .unwrap_or_else(|| "Microphone capture is not available.".to_owned()),
            );
        } else if matches!(
            microphone.permission_state,
            CapturePermissionState::Denied | CapturePermissionState::Unavailable
        ) {
            errors.push(
                microphone
                    .error
                    .clone()
                    .unwrap_or_else(|| "Microphone permission is not granted.".to_owned()),
            );
        }

        if microphones.is_empty() {
            errors.push("No microphone input devices are available.".to_owned());
        } else if selection.microphone_device_id.is_some() && selected.is_none() {
            errors.push("The selected microphone is no longer available.".to_owned());
        }

        selected.cloned()
    } else {
        None
    };

    let mut source_audio_config = None;
    if selection.audio_mode.includes_source_audio() {
        if !source_audio.supported {
            errors.push(
                source_audio
                    .error
                    .clone()
                    .unwrap_or_else(|| "Source audio capture is not available.".to_owned()),
            );
        } else if matches!(
            source_audio.permission_state,
            CapturePermissionState::Denied | CapturePermissionState::Unavailable
        ) {
            errors.push(
                source_audio
                    .error
                    .clone()
                    .unwrap_or_else(|| "Source audio permission is not granted.".to_owned()),
            );
        } else if let Some(source) = video_source {
            match crate::source_audio::source_audio_config(source) {
                Ok(config) => source_audio_config = Some(config),
                Err(error) => errors.push(error),
            }
        }
    }

    let config = if errors.is_empty() {
        video_source.cloned().map(|video_source| {
            let audio = CaptureAudioConfig {
                mode: selection.audio_mode.clone(),
                microphone: microphone_device.clone(),
                source_audio: source_audio_config.clone(),
            };

            ValidatedCaptureConfig {
                screen_source: video_source.clone(),
                video_source,
                audio,
                microphone: microphone_device,
                include_microphone: selection.audio_mode.includes_microphone(),
            }
        })
    } else {
        None
    };

    (config, errors)
}

fn selected_video_source_id(selection: &CaptureSelection) -> Option<&str> {
    selection
        .video_source_id
        .as_deref()
        .or(selection.screen_source_id.as_deref())
}

fn selected_video_source<'a>(
    parsed_source_id: Option<&ParsedCaptureSourceId>,
    video_sources: &'a [CaptureVideoSource],
) -> Option<&'a CaptureVideoSource> {
    if let Some(parsed) = parsed_source_id {
        return video_sources.iter().find(|source| {
            source.kind == parsed.kind
                && parse_capture_video_source_id(&source.id)
                    .map(|source_id| source_id.native_id == parsed.native_id)
                    .unwrap_or(false)
        });
    }

    video_sources
        .iter()
        .find(|source| source.kind == CaptureVideoSourceKind::Display && source.primary)
        .or_else(|| {
            video_sources
                .iter()
                .find(|source| source.kind == CaptureVideoSourceKind::Display)
        })
        .or_else(|| video_sources.first())
}

fn selected_microphone<'a>(
    selection: &CaptureSelection,
    microphones: &'a [MicrophoneDevice],
) -> Option<&'a MicrophoneDevice> {
    selection
        .microphone_device_id
        .as_deref()
        .and_then(|selected_id| {
            microphones
                .iter()
                .find(|microphone| microphone.id == selected_id)
        })
        .or_else(|| microphones.iter().find(|microphone| microphone.is_default))
        .or_else(|| microphones.first())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefixed_video_source_ids() {
        let display = parse_capture_video_source_id("display:1").expect("display source id");
        assert_eq!(display.kind, CaptureVideoSourceKind::Display);
        assert_eq!(display.native_id, "1");

        let application =
            parse_capture_video_source_id("application:com.metafy.app").expect("app source id");
        assert_eq!(application.kind, CaptureVideoSourceKind::Application);
        assert_eq!(application.native_id, "com.metafy.app");

        let window = parse_capture_video_source_id("window:42").expect("window source id");
        assert_eq!(window.kind, CaptureVideoSourceKind::Window);
        assert_eq!(window.native_id, "42");

        assert!(parse_capture_video_source_id("screen:1").is_err());
        assert!(parse_capture_video_source_id("display:").is_err());
        assert!(parse_capture_video_source_id("1").is_err());
    }

    #[test]
    fn validation_accepts_selected_window_video_source() {
        let screen = CaptureCapability {
            supported: true,
            permission_state: CapturePermissionState::Granted,
            error: None,
        };
        let microphone = CaptureCapability {
            supported: false,
            permission_state: CapturePermissionState::Unavailable,
            error: Some("No microphone input devices were found.".to_owned()),
        };
        let source_audio = source_audio_capability(&screen);
        let selection = CaptureSelection {
            video_source_id: Some("window:42".to_owned()),
            screen_source_id: Some("window:42".to_owned()),
            microphone_device_id: None,
            audio_mode: CaptureAudioMode::None,
            include_microphone: false,
            updated_at: None,
        };
        let sources = vec![CaptureVideoSource {
            id: "window:42".to_owned(),
            kind: CaptureVideoSourceKind::Window,
            title: "Game window".to_owned(),
            app_name: None,
            process_id: None,
            window_id: Some(42),
            primary: false,
        }];

        let (config, errors) = validate_status(
            &selection,
            &screen,
            &sources,
            &microphone,
            &[],
            &source_audio,
        );

        assert!(
            errors.is_empty(),
            "unexpected validation errors: {errors:?}"
        );
        let config = config.expect("validated window config");
        assert_eq!(config.video_source.kind, CaptureVideoSourceKind::Window);
        assert_eq!(config.video_source.window_id, Some(42));
    }

    #[test]
    fn validation_reports_stale_selected_video_source() {
        let screen = CaptureCapability {
            supported: true,
            permission_state: CapturePermissionState::Granted,
            error: None,
        };
        let microphone = CaptureCapability {
            supported: false,
            permission_state: CapturePermissionState::Unavailable,
            error: Some("No microphone input devices were found.".to_owned()),
        };
        let source_audio = source_audio_capability(&screen);
        let selection = CaptureSelection {
            video_source_id: Some("window:99".to_owned()),
            screen_source_id: Some("window:99".to_owned()),
            microphone_device_id: None,
            audio_mode: CaptureAudioMode::None,
            include_microphone: false,
            updated_at: None,
        };
        let sources = vec![CaptureVideoSource {
            id: "display:1".to_owned(),
            kind: CaptureVideoSourceKind::Display,
            title: "Display 1".to_owned(),
            app_name: None,
            process_id: None,
            window_id: None,
            primary: true,
        }];

        let (config, errors) = validate_status(
            &selection,
            &screen,
            &sources,
            &microphone,
            &[],
            &source_audio,
        );

        assert!(config.is_none());
        assert!(errors
            .iter()
            .any(|error| error == "The selected window is no longer available."));
    }

    #[test]
    fn validation_reports_unsupported_source_audio() {
        let screen = CaptureCapability {
            supported: true,
            permission_state: CapturePermissionState::Granted,
            error: None,
        };
        let microphone = CaptureCapability {
            supported: false,
            permission_state: CapturePermissionState::Unavailable,
            error: None,
        };
        let source_audio = CaptureCapability {
            supported: false,
            permission_state: CapturePermissionState::Unavailable,
            error: Some("Source audio capture is unavailable.".to_owned()),
        };
        let selection = CaptureSelection {
            video_source_id: Some("display:1".to_owned()),
            screen_source_id: Some("display:1".to_owned()),
            microphone_device_id: None,
            audio_mode: CaptureAudioMode::Source,
            include_microphone: false,
            updated_at: None,
        };
        let sources = vec![CaptureVideoSource {
            id: "display:1".to_owned(),
            kind: CaptureVideoSourceKind::Display,
            title: "Display 1".to_owned(),
            app_name: None,
            process_id: None,
            window_id: None,
            primary: true,
        }];

        let (config, errors) = validate_status(
            &selection,
            &screen,
            &sources,
            &microphone,
            &[],
            &source_audio,
        );

        assert!(config.is_none());
        assert!(errors
            .iter()
            .any(|error| error.contains("Source audio capture")));
    }

    #[test]
    fn validation_builds_source_audio_config_without_microphone() {
        let screen = CaptureCapability {
            supported: true,
            permission_state: CapturePermissionState::Granted,
            error: None,
        };
        let microphone = CaptureCapability {
            supported: false,
            permission_state: CapturePermissionState::Unavailable,
            error: None,
        };
        let source_audio = CaptureCapability {
            supported: true,
            permission_state: CapturePermissionState::Granted,
            error: None,
        };
        let selection = CaptureSelection {
            video_source_id: Some("window:42".to_owned()),
            screen_source_id: Some("window:42".to_owned()),
            microphone_device_id: None,
            audio_mode: CaptureAudioMode::Source,
            include_microphone: false,
            updated_at: None,
        };
        let sources = vec![CaptureVideoSource {
            id: "window:42".to_owned(),
            kind: CaptureVideoSourceKind::Window,
            title: "Game window".to_owned(),
            app_name: None,
            process_id: None,
            window_id: Some(42),
            primary: false,
        }];

        let (config, errors) = validate_status(
            &selection,
            &screen,
            &sources,
            &microphone,
            &[],
            &source_audio,
        );

        assert!(
            errors.is_empty(),
            "unexpected validation errors: {errors:?}"
        );
        let config = config.expect("source audio config");
        assert!(config.audio.source_audio.is_some());
        assert!(!config.include_microphone);
        assert!(config.microphone.is_none());
    }

    #[test]
    fn validation_accepts_combined_audio_with_split_storage() {
        let screen = CaptureCapability {
            supported: true,
            permission_state: CapturePermissionState::Granted,
            error: None,
        };
        let microphone = CaptureCapability {
            supported: true,
            permission_state: CapturePermissionState::Granted,
            error: None,
        };
        let source_audio = CaptureCapability {
            supported: true,
            permission_state: CapturePermissionState::Granted,
            error: None,
        };
        let selection = CaptureSelection {
            video_source_id: Some("window:42".to_owned()),
            screen_source_id: Some("window:42".to_owned()),
            microphone_device_id: Some("mic:1".to_owned()),
            audio_mode: CaptureAudioMode::MicrophoneAndSource,
            include_microphone: true,
            updated_at: None,
        };
        let sources = vec![CaptureVideoSource {
            id: "window:42".to_owned(),
            kind: CaptureVideoSourceKind::Window,
            title: "Game window".to_owned(),
            app_name: None,
            process_id: None,
            window_id: Some(42),
            primary: false,
        }];
        let microphones = vec![MicrophoneDevice {
            id: "mic:1".to_owned(),
            name: "Test mic".to_owned(),
            is_default: true,
            channels: Some(1),
            sample_rate: Some(48_000),
            error: None,
        }];

        let (config, errors) = validate_status(
            &selection,
            &screen,
            &sources,
            &microphone,
            &microphones,
            &source_audio,
        );

        let config = config.expect("combined audio config");

        assert!(errors.is_empty());
        assert!(config.include_microphone);
        assert!(config.microphone.is_some());
        assert!(config.audio.source_audio.is_some());
    }
}
