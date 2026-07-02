use std::panic;

use crate::capture::{
    parse_capture_video_source_id, CaptureCapability, CapturePermissionState, CaptureVideoSource,
    CaptureVideoSourceKind, SourceAudioCaptureConfig,
};

const MACOS_BACKEND_ID: &str = "macos_screencapturekit";
const WINDOWS_SYSTEM_LOOPBACK_BACKEND_ID: &str = "windows_wasapi_system_loopback";

pub trait SourceAudioCaptureBackend {
    fn backend_id(&self) -> &'static str;
    fn capability(&self, screen: &CaptureCapability) -> CaptureCapability;
    fn capture_config(
        &self,
        source: &CaptureVideoSource,
    ) -> Result<SourceAudioCaptureConfig, String>;
    fn build_capturer(
        &self,
        config: &SourceAudioCaptureConfig,
    ) -> Result<scap::capturer::Capturer, String>;
}

struct ScapSourceAudioBackend;

static SCAP_SOURCE_AUDIO_BACKEND: ScapSourceAudioBackend = ScapSourceAudioBackend;

pub fn source_audio_capability(screen: &CaptureCapability) -> CaptureCapability {
    SCAP_SOURCE_AUDIO_BACKEND.capability(screen)
}

pub fn source_audio_config(
    source: &CaptureVideoSource,
) -> Result<SourceAudioCaptureConfig, String> {
    SCAP_SOURCE_AUDIO_BACKEND.capture_config(source)
}

pub fn build_source_audio_capturer(
    config: &SourceAudioCaptureConfig,
) -> Result<scap::capturer::Capturer, String> {
    SCAP_SOURCE_AUDIO_BACKEND.build_capturer(config)
}

impl SourceAudioCaptureBackend for ScapSourceAudioBackend {
    fn backend_id(&self) -> &'static str {
        source_audio_backend_id()
    }

    fn capability(&self, screen: &CaptureCapability) -> CaptureCapability {
        if !platform_source_audio_supported() {
            return CaptureCapability {
                supported: false,
                permission_state: CapturePermissionState::Unavailable,
                error: Some(
                    "Source audio capture is only implemented for macOS and Windows.".to_owned(),
                ),
            };
        }

        if !panic::catch_unwind(scap::is_supported).unwrap_or(false) {
            return CaptureCapability {
                supported: false,
                permission_state: CapturePermissionState::Unavailable,
                error: Some(
                    "ScreenCaptureKit/WASAPI source audio is not supported here.".to_owned(),
                ),
            };
        }

        if !screen.supported {
            return CaptureCapability {
                supported: false,
                permission_state: CapturePermissionState::Unavailable,
                error: Some(
                    screen.error.clone().unwrap_or_else(|| {
                        "Source audio requires screen capture support.".to_owned()
                    }),
                ),
            };
        }

        match screen.permission_state {
            CapturePermissionState::Granted => CaptureCapability {
                supported: true,
                permission_state: CapturePermissionState::Granted,
                error: None,
            },
            CapturePermissionState::PromptRequired => CaptureCapability {
                supported: true,
                permission_state: CapturePermissionState::PromptRequired,
                error: Some(
                    "Source audio uses the selected screen/window capture permission.".to_owned(),
                ),
            },
            CapturePermissionState::Denied => CaptureCapability {
                supported: true,
                permission_state: CapturePermissionState::Denied,
                error: screen.error.clone().or_else(|| {
                    Some("Source audio permission is denied with screen capture.".to_owned())
                }),
            },
            CapturePermissionState::Unavailable => CaptureCapability {
                supported: false,
                permission_state: CapturePermissionState::Unavailable,
                error: screen.error.clone().or_else(|| {
                    Some("Source audio is unavailable with screen capture.".to_owned())
                }),
            },
            CapturePermissionState::Unknown => CaptureCapability {
                supported: true,
                permission_state: CapturePermissionState::Unknown,
                error: None,
            },
        }
    }

    fn capture_config(
        &self,
        source: &CaptureVideoSource,
    ) -> Result<SourceAudioCaptureConfig, String> {
        validate_source_audio_target_kind(source.kind)?;

        Ok(SourceAudioCaptureConfig {
            source_id: source.id.clone(),
            backend: self.backend_id().to_owned(),
            sample_rate: None,
            channels: None,
            sample_format: None,
        })
    }

    fn build_capturer(
        &self,
        config: &SourceAudioCaptureConfig,
    ) -> Result<scap::capturer::Capturer, String> {
        let target = selected_source_audio_target(&config.source_id)?;
        let options = scap::capturer::Options {
            fps: 1,
            show_cursor: false,
            show_highlight: false,
            target: Some(target),
            output_type: scap::frame::FrameType::BGRAFrame,
            output_resolution: scap::capturer::Resolution::Captured,
            captures_audio: true,
            exclude_current_process_audio: true,
            ..Default::default()
        };

        scap::capturer::Capturer::build(options)
            .map_err(|error| format!("Unable to start source audio capture: {error}"))
    }
}

fn source_audio_backend_id() -> &'static str {
    if cfg!(target_os = "macos") {
        MACOS_BACKEND_ID
    } else if cfg!(target_os = "windows") {
        WINDOWS_SYSTEM_LOOPBACK_BACKEND_ID
    } else {
        "unsupported"
    }
}

fn platform_source_audio_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

fn validate_source_audio_target_kind(kind: CaptureVideoSourceKind) -> Result<(), String> {
    match kind {
        CaptureVideoSourceKind::Display | CaptureVideoSourceKind::Window => Ok(()),
        CaptureVideoSourceKind::Application => Err(
            "Application source audio is constrained by the current capture backend. Select a concrete window from that application instead."
                .to_owned(),
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceAudioTargetDescriptor {
    kind: CaptureVideoSourceKind,
    native_id: u32,
}

fn selected_source_audio_target(source_id: &str) -> Result<scap::Target, String> {
    let descriptor = parse_source_audio_target_descriptor(source_id)?;
    let targets = panic::catch_unwind(scap::get_all_targets)
        .map_err(|_| "Unable to enumerate source audio targets.".to_owned())?;
    let target_index = targets
        .iter()
        .map(source_audio_target_descriptor)
        .position(|target| target == descriptor)
        .ok_or_else(|| {
            format!(
                "The selected {} is no longer available for source audio.",
                descriptor.kind.label()
            )
        })?;

    targets
        .into_iter()
        .nth(target_index)
        .ok_or_else(|| "Selected source audio target is no longer available.".to_owned())
}

fn parse_source_audio_target_descriptor(
    source_id: &str,
) -> Result<SourceAudioTargetDescriptor, String> {
    let parsed_source_id = parse_capture_video_source_id(source_id)?;
    validate_source_audio_target_kind(parsed_source_id.kind)?;
    let native_id = parsed_source_id
        .native_id
        .parse::<u32>()
        .map_err(|_| format!("Selected {} id is invalid.", parsed_source_id.kind.label()))?;

    Ok(SourceAudioTargetDescriptor {
        kind: parsed_source_id.kind,
        native_id,
    })
}

fn source_audio_target_descriptor(target: &scap::Target) -> SourceAudioTargetDescriptor {
    match target {
        scap::Target::Display(display) => SourceAudioTargetDescriptor {
            kind: CaptureVideoSourceKind::Display,
            native_id: display.id,
        },
        scap::Target::Window(window) => SourceAudioTargetDescriptor {
            kind: CaptureVideoSourceKind::Window,
            native_id: window.id,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_application_source_audio_target() {
        let error = parse_source_audio_target_descriptor("application:com.metafy.app")
            .expect_err("application source audio is constrained");

        assert!(error.contains("Application source audio is constrained"));
    }

    #[test]
    fn parses_display_and_window_source_audio_targets() {
        assert_eq!(
            parse_source_audio_target_descriptor("display:7").expect("display target"),
            SourceAudioTargetDescriptor {
                kind: CaptureVideoSourceKind::Display,
                native_id: 7,
            }
        );
        assert_eq!(
            parse_source_audio_target_descriptor("window:42").expect("window target"),
            SourceAudioTargetDescriptor {
                kind: CaptureVideoSourceKind::Window,
                native_id: 42,
            }
        );
    }
}
