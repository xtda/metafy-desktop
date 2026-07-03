use std::path::PathBuf;

use crate::storage::{CaptureAudioMode, RecordingSession, StorageState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioSidecarInput {
    pub path: PathBuf,
    pub label: &'static str,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub sample_format: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioSidecarPurpose {
    Encoding,
    Transcription,
}

pub fn select_requested_audio_sidecars(
    storage: &StorageState,
    session: &RecordingSession,
    purpose: AudioSidecarPurpose,
    warnings: &mut Vec<String>,
) -> Vec<AudioSidecarInput> {
    let mut inputs = Vec::new();
    let microphone_path = session
        .microphone_audio_path
        .as_deref()
        .or_else(|| legacy_microphone_audio_path(session));

    if session.audio_mode.includes_microphone() {
        if let Some(path) = microphone_path {
            inputs.push(AudioSidecarInput {
                path: storage.resolve_path(path),
                label: "microphone audio",
                sample_rate: session.microphone_audio_sample_rate,
                channels: session.microphone_audio_channels,
                sample_format: session.microphone_audio_sample_format.clone(),
            });
        } else {
            warnings.push(missing_requested_audio_warning("microphone audio", purpose));
        }
    } else if session.audio_mode == CaptureAudioMode::None {
        if let Some(path) = legacy_microphone_audio_path(session) {
            inputs.push(AudioSidecarInput {
                path: storage.resolve_path(path),
                label: "microphone audio",
                sample_rate: session.audio_sample_rate,
                channels: session.audio_channels,
                sample_format: session.audio_sample_format.clone(),
            });
        }
    }

    if session.audio_mode.includes_source_audio() {
        if let Some(path) = session.source_audio_path.as_deref() {
            inputs.push(AudioSidecarInput {
                path: storage.resolve_path(path),
                label: "source audio",
                sample_rate: session.source_audio_sample_rate,
                channels: session.source_audio_channels,
                sample_format: session.source_audio_sample_format.clone(),
            });
        } else {
            warnings.push(missing_requested_audio_warning("source audio", purpose));
        }
    }

    inputs
}

fn legacy_microphone_audio_path(session: &RecordingSession) -> Option<&str> {
    session
        .audio_path
        .as_deref()
        .filter(|_| session.audio_mode != CaptureAudioMode::Source)
}

fn missing_requested_audio_warning(label: &str, purpose: AudioSidecarPurpose) -> String {
    match purpose {
        AudioSidecarPurpose::Encoding => {
            format!(
                "{label} was requested but no capture file was recorded; encoded without {label}."
            )
        }
        AudioSidecarPurpose::Transcription => {
            format!(
                "{label} was requested but no capture file was recorded; transcription audio prepared without {label}."
            )
        }
    }
}
