use serde::Serialize;
use std::path::PathBuf;

use crate::media::metadata::MediaInfo;
use crate::media::thumbnail::BgraFrame;

#[derive(Debug, Clone)]
pub struct EncodeInput {
    pub recording_id: String,
    pub video: EncodeVideoInput,
    pub audio_inputs: Vec<EncodeAudioInput>,
    pub output: EncodeOutputPaths,
    pub duration_hint_ms: Option<i64>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EncodeVideoInput {
    pub path: PathBuf,
    pub format: EncodeVideoFormat,
    pub width: i64,
    pub height: i64,
    pub frame_rate: i64,
    pub frame_count: i64,
    pub thumbnail_frame: BgraFrame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeVideoFormat {
    RawBgra,
    ChunkedH264Segments,
}

#[derive(Debug, Clone)]
pub struct EncodeAudioInput {
    pub path: PathBuf,
    pub sample_rate: i64,
    pub channels: i64,
    pub sample_format: String,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct EncodeOutputPaths {
    pub media_path: PathBuf,
    pub media_path_relative: String,
    pub thumbnail_path: PathBuf,
    pub thumbnail_path_relative: String,
    pub staging_media_path: PathBuf,
    pub staging_thumbnail_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodeOutput {
    pub recording_id: String,
    pub media_path: String,
    pub thumbnail_path: Option<String>,
    pub absolute_media_path: String,
    pub absolute_thumbnail_path: Option<String>,
    pub duration_ms: Option<i64>,
    pub width: i64,
    pub height: i64,
    pub frame_rate: i64,
    pub frame_count: i64,
    pub audio_included: bool,
    pub media_info: MediaInfo,
    pub diagnostics: EncodeDiagnostics,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodeDiagnostics {
    pub backend: String,
    pub commands: Vec<EncodeCommandDiagnostics>,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodeCommandDiagnostics {
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
}

pub trait RecordingEncoder {
    fn encode(&self, input: EncodeInput) -> Result<EncodeOutput, String>;
}
