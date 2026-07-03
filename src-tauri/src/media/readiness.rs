use serde::Serialize;

#[cfg(target_os = "linux")]
use crate::media::backends::linux_gstreamer::LinuxGstreamerRecordingEncoder;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaBackendReadiness {
    pub backend: String,
    pub display_name: String,
    pub status: String,
    pub available: bool,
    pub retryable: bool,
    pub temporary_fallback: bool,
    pub user_action: Option<String>,
    pub missing_components: Vec<String>,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlatformBackend {
    Macos,
    Windows,
    Linux,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LinuxBackendReadiness {
    available: bool,
    missing_components: Vec<String>,
    messages: Vec<String>,
    install_hint: String,
}

pub fn selected_media_backend_readiness() -> MediaBackendReadiness {
    readiness_for_platform(current_platform_backend(), current_linux_readiness())
}

fn current_platform_backend() -> PlatformBackend {
    if cfg!(target_os = "macos") {
        PlatformBackend::Macos
    } else if cfg!(target_os = "windows") {
        PlatformBackend::Windows
    } else if cfg!(target_os = "linux") {
        PlatformBackend::Linux
    } else {
        PlatformBackend::Unsupported
    }
}

#[cfg(target_os = "linux")]
fn current_linux_readiness() -> Option<LinuxBackendReadiness> {
    let readiness = LinuxGstreamerRecordingEncoder::readiness();

    Some(LinuxBackendReadiness {
        available: readiness.available,
        missing_components: readiness.missing_elements,
        messages: readiness.messages,
        install_hint: readiness.install_hint,
    })
}

#[cfg(not(target_os = "linux"))]
fn current_linux_readiness() -> Option<LinuxBackendReadiness> {
    None
}

fn readiness_for_platform(
    platform: PlatformBackend,
    linux_readiness: Option<LinuxBackendReadiness>,
) -> MediaBackendReadiness {
    match platform {
        PlatformBackend::Macos => native_ready(
            "macos-avfoundation",
            "AVFoundation",
            "selected-macos-avfoundation-ready",
            "Selected AVFoundation for macOS media processing.",
        ),
        PlatformBackend::Windows => native_ready(
            "windows-media-foundation",
            "Media Foundation",
            "selected-windows-media-foundation-ready",
            "Selected Media Foundation for Windows media processing.",
        ),
        PlatformBackend::Linux => linux_gstreamer_readiness(linux_readiness),
        PlatformBackend::Unsupported => unsupported_readiness(),
    }
}

fn native_ready(
    backend: &str,
    display_name: &str,
    status: &str,
    message: &str,
) -> MediaBackendReadiness {
    MediaBackendReadiness {
        backend: backend.to_owned(),
        display_name: display_name.to_owned(),
        status: status.to_owned(),
        available: true,
        retryable: false,
        temporary_fallback: false,
        user_action: None,
        missing_components: Vec::new(),
        messages: vec![message.to_owned()],
    }
}

fn linux_gstreamer_readiness(
    linux_readiness: Option<LinuxBackendReadiness>,
) -> MediaBackendReadiness {
    let readiness = linux_readiness.unwrap_or_else(|| LinuxBackendReadiness {
        available: false,
        missing_components: vec!["gstreamer readiness".to_owned()],
        messages: vec!["GStreamer readiness could not be evaluated.".to_owned()],
        install_hint: "Install GStreamer and the required H.264/AAC plugins, then retry."
            .to_owned(),
    });

    MediaBackendReadiness {
        backend: "linux-gstreamer".to_owned(),
        display_name: "GStreamer".to_owned(),
        status: if readiness.available {
            "selected-linux-gstreamer-ready"
        } else {
            "selected-linux-gstreamer-missing-dependencies"
        }
        .to_owned(),
        available: readiness.available,
        retryable: !readiness.available,
        temporary_fallback: false,
        user_action: if readiness.available {
            None
        } else {
            Some(readiness.install_hint)
        },
        missing_components: readiness.missing_components,
        messages: readiness.messages,
    }
}

fn unsupported_readiness() -> MediaBackendReadiness {
    MediaBackendReadiness {
        backend: "unsupported".to_owned(),
        display_name: "Unsupported platform".to_owned(),
        status: "unsupported-target".to_owned(),
        available: false,
        retryable: false,
        temporary_fallback: false,
        user_action: Some(
            "Media processing is supported on macOS, Windows, and Linux for normal product builds."
                .to_owned(),
        ),
        missing_components: Vec::new(),
        messages: vec!["No native media backend is selected for this target.".to_owned()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_macos_avfoundation_by_default() {
        let readiness = readiness_for_platform(PlatformBackend::Macos, None);

        assert_eq!(readiness.backend, "macos-avfoundation");
        assert_eq!(readiness.display_name, "AVFoundation");
        assert_eq!(readiness.status, "selected-macos-avfoundation-ready");
        assert!(readiness.available);
        assert!(!readiness.temporary_fallback);
        assert!(readiness.user_action.is_none());
    }

    #[test]
    fn selects_windows_media_foundation_by_default() {
        let readiness = readiness_for_platform(PlatformBackend::Windows, None);

        assert_eq!(readiness.backend, "windows-media-foundation");
        assert_eq!(readiness.display_name, "Media Foundation");
        assert_eq!(readiness.status, "selected-windows-media-foundation-ready");
        assert!(readiness.available);
        assert!(!readiness.temporary_fallback);
        assert!(readiness.user_action.is_none());
    }

    #[test]
    fn selects_linux_gstreamer_by_default() {
        let readiness = readiness_for_platform(
            PlatformBackend::Linux,
            Some(LinuxBackendReadiness {
                available: true,
                missing_components: Vec::new(),
                messages: vec![
                    "GStreamer ready with video encoder x264enc and audio encoder voaacenc."
                        .to_owned(),
                ],
                install_hint: "Install GStreamer plugins.".to_owned(),
            }),
        );

        assert_eq!(readiness.backend, "linux-gstreamer");
        assert_eq!(readiness.display_name, "GStreamer");
        assert_eq!(readiness.status, "selected-linux-gstreamer-ready");
        assert!(readiness.available);
        assert!(!readiness.temporary_fallback);
    }

    #[test]
    fn reports_linux_gstreamer_missing_plugins_actionably() {
        let readiness = readiness_for_platform(
            PlatformBackend::Linux,
            Some(LinuxBackendReadiness {
                available: false,
                missing_components: vec!["x264enc".to_owned(), "voaacenc".to_owned()],
                messages: vec![
                    "GStreamer is missing required encoder or muxing elements: x264enc, voaacenc."
                        .to_owned(),
                ],
                install_hint: "Install GStreamer plugins, then retry.".to_owned(),
            }),
        );

        assert_eq!(
            readiness.status,
            "selected-linux-gstreamer-missing-dependencies"
        );
        assert!(!readiness.available);
        assert!(readiness.retryable);
        assert_eq!(readiness.missing_components, vec!["x264enc", "voaacenc"]);
        assert!(readiness
            .user_action
            .as_deref()
            .is_some_and(|action| action.contains("GStreamer")));
    }

    #[test]
    fn unsupported_target_has_no_media_backend() {
        let readiness = readiness_for_platform(PlatformBackend::Unsupported, None);

        assert_eq!(readiness.backend, "unsupported");
        assert_eq!(readiness.status, "unsupported-target");
        assert!(!readiness.available);
        assert!(!readiness.retryable);
        assert!(!readiness.temporary_fallback);
        assert!(readiness
            .user_action
            .as_deref()
            .is_some_and(|action| action.contains("macOS, Windows, and Linux")));
        assert!(readiness
            .messages
            .iter()
            .any(|message| message.contains("No native media backend")));
    }
}
