use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::ErrorKind;
use serde::Serialize;
use std::panic;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use crate::storage::CaptureSelection;

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDisplaySource {
    pub id: String,
    pub title: String,
    pub primary: bool,
}

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
    pub screen_source: CaptureDisplaySource,
    pub microphone: Option<MicrophoneDevice>,
    pub include_microphone: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatus {
    pub screen: CaptureCapability,
    pub microphone: CaptureCapability,
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
    let displays = if screen.permission_state == CapturePermissionState::Granted {
        match list_display_sources() {
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

    let (microphones, microphone_error) = list_microphone_devices();
    let mut microphone = microphone_capability(&microphones, microphone_error);

    if selection.include_microphone && probe_microphone_permission && microphone.supported {
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

    let (validated_config, validation_errors) =
        validate_status(&selection, &screen, &displays, &microphone, &microphones);

    CaptureStatus {
        screen,
        microphone,
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

fn list_display_sources() -> Result<Vec<CaptureDisplaySource>, String> {
    let main_display_id = if cfg!(target_os = "linux") {
        None
    } else {
        panic::catch_unwind(scap::get_main_display)
            .ok()
            .map(|display| display.id)
    };
    let targets = panic::catch_unwind(scap::get_all_targets)
        .map_err(|_| "Unable to enumerate screen capture targets.".to_owned())?
        .map_err(|error| error.to_string())?;
    let mut displays = Vec::new();

    for target in targets {
        if let scap::Target::Display(display) = target {
            let primary = main_display_id
                .map(|id| id == display.id)
                .unwrap_or_else(|| displays.is_empty());

            displays.push(CaptureDisplaySource {
                id: format!("display:{}", display.id),
                title: display.title,
                primary,
            });
        }
    }

    Ok(displays)
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
    displays: &[CaptureDisplaySource],
    microphone: &CaptureCapability,
    microphones: &[MicrophoneDevice],
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

    let screen_source = selected_display(selection, displays);
    if displays.is_empty() {
        errors.push("No displays are available for screen capture.".to_owned());
    } else if selection.screen_source_id.is_some() && screen_source.is_none() {
        errors.push("The selected display is no longer available.".to_owned());
    }

    let microphone_device = if selection.include_microphone {
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

    let config = if errors.is_empty() {
        screen_source
            .cloned()
            .map(|screen_source| ValidatedCaptureConfig {
                screen_source,
                microphone: microphone_device,
                include_microphone: selection.include_microphone,
            })
    } else {
        None
    };

    (config, errors)
}

fn selected_display<'a>(
    selection: &CaptureSelection,
    displays: &'a [CaptureDisplaySource],
) -> Option<&'a CaptureDisplaySource> {
    selection
        .screen_source_id
        .as_deref()
        .and_then(|selected_id| displays.iter().find(|display| display.id == selected_id))
        .or_else(|| displays.iter().find(|display| display.primary))
        .or_else(|| displays.first())
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
