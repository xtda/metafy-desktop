use rusqlite::types::Type;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const DATABASE_FILE_NAME: &str = "app.sqlite";
const RECORDINGS_DIRECTORY_NAME: &str = "recordings";
const MODELS_DIRECTORY_NAME: &str = "models";
const WHISPER_MODELS_DIRECTORY_NAME: &str = "whisper";
const TEMP_DIRECTORY_NAME: &str = "temp";
const RECORDING_SESSIONS_DIRECTORY_NAME: &str = "recording-sessions";
const SCHEMA_VERSION: i64 = 10;
const DEFAULT_AI_PROVIDER: &str = "openai_compatible";
const DEFAULT_AI_MODEL_NAME: &str = "";
const DEFAULT_AI_ENDPOINT_URL: &str = "https://api.openai.com/v1/chat/completions";

const MIGRATION_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS recordings (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'capturing', 'processing', 'completed', 'failed')),
    recording_directory TEXT NOT NULL UNIQUE,
    media_path TEXT,
    thumbnail_path TEXT,
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    captured_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    failure_message TEXT
);

CREATE TABLE IF NOT EXISTS transcripts (
    id TEXT PRIMARY KEY,
    recording_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK (status IN ('not_started', 'processing', 'completed', 'failed')),
    language TEXT,
    model_name TEXT,
    raw_json_path TEXT,
    text TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    failure_message TEXT,
    FOREIGN KEY (recording_id) REFERENCES recordings(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS transcript_segments (
    id TEXT PRIMARY KEY,
    transcript_id TEXT NOT NULL,
    recording_id TEXT NOT NULL,
    segment_index INTEGER NOT NULL CHECK (segment_index >= 0),
    start_ms INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK (end_ms >= start_ms),
    text TEXT NOT NULL,
    confidence REAL CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1)),
    FOREIGN KEY (transcript_id) REFERENCES transcripts(id) ON DELETE CASCADE,
    FOREIGN KEY (recording_id) REFERENCES recordings(id) ON DELETE CASCADE,
    UNIQUE (transcript_id, segment_index)
);

CREATE INDEX IF NOT EXISTS idx_transcript_segments_recording_id
    ON transcript_segments(recording_id, segment_index);

CREATE VIRTUAL TABLE IF NOT EXISTS transcript_segment_search USING fts5(
    segment_id UNINDEXED,
    text,
    tokenize = 'unicode61'
);

CREATE TABLE IF NOT EXISTS ai_summaries (
    id TEXT PRIMARY KEY,
    recording_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK (status IN ('disabled', 'pending', 'processing', 'completed', 'failed')),
    model_name TEXT,
    summary_text TEXT,
    action_items_json TEXT,
    decisions_json TEXT,
    questions_json TEXT,
    risks_json TEXT,
    chapters_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    failure_message TEXT,
    FOREIGN KEY (recording_id) REFERENCES recordings(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS ai_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    provider TEXT NOT NULL DEFAULT 'openai_compatible',
    model_name TEXT NOT NULL DEFAULT '',
    endpoint_url TEXT NOT NULL DEFAULT 'https://api.openai.com/v1/chat/completions',
    api_key TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS processing_jobs (
    id TEXT PRIMARY KEY,
    recording_id TEXT,
    kind TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    priority INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts INTEGER NOT NULL DEFAULT 3 CHECK (max_attempts > 0),
    input_json TEXT,
    output_json TEXT,
    error_message TEXT,
    interrupted INTEGER NOT NULL DEFAULT 0 CHECK (interrupted IN (0, 1)),
    last_error_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT,
    FOREIGN KEY (recording_id) REFERENCES recordings(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_processing_jobs_state
    ON processing_jobs(state, priority DESC, created_at ASC);

CREATE INDEX IF NOT EXISTS idx_processing_jobs_recording_id
    ON processing_jobs(recording_id, created_at DESC);

CREATE TABLE IF NOT EXISTS capture_preferences (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    video_source_id TEXT,
    screen_source_id TEXT,
    microphone_device_id TEXT,
    audio_mode TEXT NOT NULL DEFAULT 'microphone' CHECK (audio_mode IN ('none', 'microphone', 'source', 'microphone_and_source')),
    include_microphone INTEGER NOT NULL DEFAULT 1 CHECK (include_microphone IN (0, 1)),
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS recording_sessions (
    id TEXT PRIMARY KEY,
    recording_id TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK (status IN ('capturing', 'stopped', 'failed')),
    temp_directory TEXT NOT NULL,
    video_path TEXT NOT NULL,
    audio_path TEXT,
    metadata_path TEXT NOT NULL,
    video_source_id TEXT NOT NULL DEFAULT '',
    screen_source_id TEXT NOT NULL,
    video_source_kind TEXT NOT NULL DEFAULT 'display' CHECK (video_source_kind IN ('display', 'application', 'window')),
    video_source_title TEXT NOT NULL DEFAULT '',
    video_source_app_name TEXT,
    video_source_process_id INTEGER CHECK (video_source_process_id IS NULL OR video_source_process_id >= 0),
    video_source_window_id INTEGER CHECK (video_source_window_id IS NULL OR video_source_window_id >= 0),
    microphone_device_id TEXT,
    include_microphone INTEGER NOT NULL CHECK (include_microphone IN (0, 1)),
    audio_mode TEXT NOT NULL DEFAULT 'microphone' CHECK (audio_mode IN ('none', 'microphone', 'source', 'microphone_and_source')),
    microphone_audio_path TEXT,
    source_audio_path TEXT,
    width INTEGER CHECK (width IS NULL OR width > 0),
    height INTEGER CHECK (height IS NULL OR height > 0),
    frame_rate INTEGER NOT NULL CHECK (frame_rate > 0),
    frame_count INTEGER NOT NULL DEFAULT 0 CHECK (frame_count >= 0),
    audio_byte_count INTEGER NOT NULL DEFAULT 0 CHECK (audio_byte_count >= 0),
    audio_sample_rate INTEGER CHECK (audio_sample_rate IS NULL OR audio_sample_rate > 0),
    audio_channels INTEGER CHECK (audio_channels IS NULL OR audio_channels > 0),
    audio_sample_format TEXT,
    microphone_audio_byte_count INTEGER NOT NULL DEFAULT 0 CHECK (microphone_audio_byte_count >= 0),
    microphone_audio_sample_rate INTEGER CHECK (microphone_audio_sample_rate IS NULL OR microphone_audio_sample_rate > 0),
    microphone_audio_channels INTEGER CHECK (microphone_audio_channels IS NULL OR microphone_audio_channels > 0),
    microphone_audio_sample_format TEXT,
    source_audio_byte_count INTEGER NOT NULL DEFAULT 0 CHECK (source_audio_byte_count >= 0),
    source_audio_sample_rate INTEGER CHECK (source_audio_sample_rate IS NULL OR source_audio_sample_rate > 0),
    source_audio_channels INTEGER CHECK (source_audio_channels IS NULL OR source_audio_channels > 0),
    source_audio_sample_format TEXT,
    started_at TEXT NOT NULL,
    stopped_at TEXT,
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    failure_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (recording_id) REFERENCES recordings(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_recording_sessions_status
    ON recording_sessions(status, created_at DESC);
"#;

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Database(rusqlite::Error),
    Tauri(tauri::Error),
    InvalidInput(String),
    NotFound { entity: &'static str, id: String },
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "filesystem error: {error}"),
            Self::Database(error) => write!(formatter, "database error: {error}"),
            Self::Tauri(error) => write!(formatter, "tauri path error: {error}"),
            Self::InvalidInput(message) => write!(formatter, "{message}"),
            Self::NotFound { entity, id } => write!(formatter, "{entity} not found: {id}"),
        }
    }
}

impl Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<tauri::Error> for StorageError {
    fn from(error: tauri::Error) -> Self {
        Self::Tauri(error)
    }
}

#[derive(Debug)]
struct InvalidEnumValue {
    enum_name: &'static str,
    value: String,
}

impl fmt::Display for InvalidEnumValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid {} value: {}",
            self.enum_name, self.value
        )
    }
}

impl Error for InvalidEnumValue {}

macro_rules! local_status_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }

            fn from_db(value: &str) -> Option<Self> {
                match value {
                    $($value => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

local_status_enum!(RecordingStatus {
    Pending => "pending",
    Capturing => "capturing",
    Processing => "processing",
    Completed => "completed",
    Failed => "failed",
});

local_status_enum!(TranscriptStatus {
    NotStarted => "not_started",
    Processing => "processing",
    Completed => "completed",
    Failed => "failed",
});

local_status_enum!(AiStatus {
    Disabled => "disabled",
    Pending => "pending",
    Processing => "processing",
    Completed => "completed",
    Failed => "failed",
});

local_status_enum!(JobState {
    Queued => "queued",
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
    Cancelled => "cancelled",
});

local_status_enum!(RecordingSessionStatus {
    Capturing => "capturing",
    Stopped => "stopped",
    Failed => "failed",
});

local_status_enum!(CaptureAudioMode {
    None => "none",
    Microphone => "microphone",
    Source => "source",
    MicrophoneAndSource => "microphone_and_source",
});

impl CaptureAudioMode {
    pub fn includes_microphone(&self) -> bool {
        matches!(self, Self::Microphone | Self::MicrophoneAndSource)
    }

    pub fn includes_source_audio(&self) -> bool {
        matches!(self, Self::Source | Self::MicrophoneAndSource)
    }

    fn from_include_microphone(include_microphone: bool) -> Self {
        if include_microphone {
            Self::Microphone
        } else {
            Self::None
        }
    }
}

#[derive(Debug, Clone)]
pub struct StorageState {
    paths: StoragePaths,
}

#[derive(Debug, Clone)]
pub struct StoragePaths {
    root: PathBuf,
    database_file: PathBuf,
    recordings_directory: PathBuf,
    whisper_models_directory: PathBuf,
    temp_directory: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoragePathsSnapshot {
    pub root: String,
    pub database_file: String,
    pub recordings_directory: String,
    pub whisper_models_directory: String,
    pub temp_directory: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageOverview {
    pub paths: StoragePathsSnapshot,
    pub schema_version: i64,
    pub sqlite_initialized: bool,
    pub tables: Vec<String>,
    pub recording_count: i64,
    pub processing_job_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    pub id: String,
    pub title: String,
    pub status: RecordingStatus,
    pub recording_directory: String,
    pub media_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub duration_ms: Option<i64>,
    pub captured_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSession {
    pub id: String,
    pub recording_id: String,
    pub status: RecordingSessionStatus,
    pub temp_directory: String,
    pub video_path: String,
    pub audio_path: Option<String>,
    pub metadata_path: String,
    pub video_source_id: String,
    pub screen_source_id: String,
    pub video_source_kind: String,
    pub video_source_title: String,
    pub video_source_app_name: Option<String>,
    pub video_source_process_id: Option<i64>,
    pub video_source_window_id: Option<i64>,
    pub microphone_device_id: Option<String>,
    pub include_microphone: bool,
    pub audio_mode: CaptureAudioMode,
    pub microphone_audio_path: Option<String>,
    pub source_audio_path: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub frame_rate: i64,
    pub frame_count: i64,
    pub audio_byte_count: i64,
    pub audio_sample_rate: Option<i64>,
    pub audio_channels: Option<i64>,
    pub audio_sample_format: Option<String>,
    pub microphone_audio_byte_count: i64,
    pub microphone_audio_sample_rate: Option<i64>,
    pub microphone_audio_channels: Option<i64>,
    pub microphone_audio_sample_format: Option<String>,
    pub source_audio_byte_count: i64,
    pub source_audio_sample_rate: Option<i64>,
    pub source_audio_channels: Option<i64>,
    pub source_audio_sample_format: Option<String>,
    pub started_at: String,
    pub stopped_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub failure_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRecordingInput {
    pub title: Option<String>,
    pub captured_at: Option<String>,
    pub media_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRecordingInput {
    pub id: String,
    pub title: Option<String>,
    pub status: Option<RecordingStatus>,
    pub media_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub duration_ms: Option<i64>,
    pub captured_at: Option<String>,
    pub completed_at: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub id: String,
    pub recording_id: String,
    pub status: TranscriptStatus,
    pub language: Option<String>,
    pub model_name: Option<String>,
    pub raw_json_path: Option<String>,
    pub text: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub id: String,
    pub transcript_id: String,
    pub recording_id: String,
    pub segment_index: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptWithSegments {
    pub transcript: Transcript,
    pub segments: Vec<TranscriptSegment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchTranscriptsInput {
    pub query: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSearchResult {
    pub recording_id: String,
    pub recording_title: String,
    pub transcript_id: String,
    pub segment_id: String,
    pub segment_index: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub snippet: String,
    pub rank: f64,
    pub media_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub captured_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSearchIndexSummary {
    pub indexed_segment_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistTranscriptInput {
    pub recording_id: String,
    pub status: TranscriptStatus,
    pub language: Option<String>,
    pub model_name: Option<String>,
    pub raw_json_path: Option<String>,
    pub text: Option<String>,
    pub completed_at: Option<String>,
    pub failure_message: Option<String>,
    pub segments: Vec<TranscriptSegmentInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegmentInput {
    pub segment_index: Option<i64>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSummary {
    pub id: String,
    pub recording_id: String,
    pub status: AiStatus,
    pub model_name: Option<String>,
    pub summary_text: Option<String>,
    pub action_items_json: Option<String>,
    pub decisions_json: Option<String>,
    pub questions_json: Option<String>,
    pub risks_json: Option<String>,
    pub chapters_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertAiSummaryInput {
    pub recording_id: String,
    pub status: AiStatus,
    pub model_name: Option<String>,
    pub summary_text: Option<String>,
    pub action_items_json: Option<String>,
    pub decisions_json: Option<String>,
    pub questions_json: Option<String>,
    pub risks_json: Option<String>,
    pub chapters_json: Option<String>,
    pub completed_at: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    pub enabled: bool,
    pub provider: String,
    pub model_name: String,
    pub endpoint_url: String,
    pub has_api_key: bool,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AiSettingsRecord {
    pub enabled: bool,
    pub provider: String,
    pub model_name: String,
    pub endpoint_url: String,
    pub api_key: Option<String>,
    pub updated_at: Option<String>,
}

impl AiSettingsRecord {
    pub fn public_settings(&self) -> AiSettings {
        AiSettings {
            enabled: self.enabled,
            provider: self.provider.clone(),
            model_name: self.model_name.clone(),
            endpoint_url: self.endpoint_url.clone(),
            has_api_key: self.api_key.is_some(),
            updated_at: self.updated_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAiSettingsInput {
    pub enabled: bool,
    pub provider: Option<String>,
    pub model_name: Option<String>,
    pub endpoint_url: Option<String>,
    pub api_key: Option<String>,
    pub clear_api_key: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingJob {
    pub id: String,
    pub recording_id: Option<String>,
    pub kind: String,
    pub state: JobState,
    pub priority: i64,
    pub attempts: i64,
    pub max_attempts: i64,
    pub input_json: Option<String>,
    pub output_json: Option<String>,
    pub error_message: Option<String>,
    pub interrupted: bool,
    pub last_error_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSelection {
    pub video_source_id: Option<String>,
    pub screen_source_id: Option<String>,
    pub microphone_device_id: Option<String>,
    pub audio_mode: CaptureAudioMode,
    pub include_microphone: bool,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCaptureSelectionInput {
    pub video_source_id: Option<String>,
    pub screen_source_id: Option<String>,
    pub microphone_device_id: Option<String>,
    pub audio_mode: Option<CaptureAudioMode>,
    pub include_microphone: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRecordingSessionInput {
    pub id: String,
    pub recording_id: String,
    pub temp_directory: String,
    pub video_path: String,
    pub audio_path: Option<String>,
    pub metadata_path: String,
    pub video_source_id: String,
    pub screen_source_id: String,
    pub video_source_kind: String,
    pub video_source_title: String,
    pub video_source_app_name: Option<String>,
    pub video_source_process_id: Option<i64>,
    pub video_source_window_id: Option<i64>,
    pub microphone_device_id: Option<String>,
    pub include_microphone: bool,
    pub audio_mode: CaptureAudioMode,
    pub microphone_audio_path: Option<String>,
    pub source_audio_path: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub frame_rate: i64,
    pub audio_sample_rate: Option<i64>,
    pub audio_channels: Option<i64>,
    pub audio_sample_format: Option<String>,
    pub microphone_audio_sample_rate: Option<i64>,
    pub microphone_audio_channels: Option<i64>,
    pub microphone_audio_sample_format: Option<String>,
    pub source_audio_sample_rate: Option<i64>,
    pub source_audio_channels: Option<i64>,
    pub source_audio_sample_format: Option<String>,
    pub started_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishRecordingSessionInput {
    pub id: String,
    pub status: RecordingSessionStatus,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub frame_count: i64,
    pub audio_byte_count: i64,
    pub audio_sample_rate: Option<i64>,
    pub audio_channels: Option<i64>,
    pub audio_sample_format: Option<String>,
    pub microphone_audio_byte_count: i64,
    pub microphone_audio_sample_rate: Option<i64>,
    pub microphone_audio_channels: Option<i64>,
    pub microphone_audio_sample_format: Option<String>,
    pub source_audio_byte_count: i64,
    pub source_audio_sample_rate: Option<i64>,
    pub source_audio_channels: Option<i64>,
    pub source_audio_sample_format: Option<String>,
    pub stopped_at: String,
    pub duration_ms: i64,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RecordingSessionFiles {
    pub video_path: PathBuf,
    pub microphone_audio_path: Option<PathBuf>,
    pub source_audio_path: Option<PathBuf>,
    pub metadata_path: PathBuf,
    pub temp_directory_relative: String,
    pub video_path_relative: String,
    pub audio_path_relative: Option<String>,
    pub microphone_audio_path_relative: Option<String>,
    pub source_audio_path_relative: Option<String>,
    pub metadata_path_relative: String,
}

#[derive(Debug, Clone)]
pub struct RecordingMediaFiles {
    pub recording_directory: PathBuf,
    pub media_path: PathBuf,
    pub thumbnail_path: PathBuf,
    pub media_path_relative: String,
    pub thumbnail_path_relative: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingAssetPaths {
    pub media_path: Option<String>,
    pub thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProcessingJobInput {
    pub recording_id: Option<String>,
    pub kind: String,
    pub priority: Option<i64>,
    pub input_json: Option<String>,
    pub max_attempts: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProcessingJobInput {
    pub id: String,
    pub state: Option<JobState>,
    pub attempts: Option<i64>,
    pub output_json: Option<String>,
    pub error_message: Option<String>,
    pub interrupted: Option<bool>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

pub fn initialize(app_handle: &AppHandle) -> Result<StorageState, StorageError> {
    let root = app_handle.path().app_data_dir()?;
    initialize_at(root)
}

pub fn initialize_at(root: PathBuf) -> Result<StorageState, StorageError> {
    let paths = StoragePaths::new(root);
    create_required_directories(&paths)?;

    let state = StorageState { paths };
    let connection = state.open_connection()?;
    run_migrations(&connection)?;

    Ok(state)
}

impl StoragePaths {
    fn new(root: PathBuf) -> Self {
        let database_file = root.join(DATABASE_FILE_NAME);
        let recordings_directory = root.join(RECORDINGS_DIRECTORY_NAME);
        let whisper_models_directory = root
            .join(MODELS_DIRECTORY_NAME)
            .join(WHISPER_MODELS_DIRECTORY_NAME);
        let temp_directory = root.join(TEMP_DIRECTORY_NAME);

        Self {
            root,
            database_file,
            recordings_directory,
            whisper_models_directory,
            temp_directory,
        }
    }

    fn snapshot(&self) -> StoragePathsSnapshot {
        StoragePathsSnapshot {
            root: path_to_string(&self.root),
            database_file: path_to_string(&self.database_file),
            recordings_directory: path_to_string(&self.recordings_directory),
            whisper_models_directory: path_to_string(&self.whisper_models_directory),
            temp_directory: path_to_string(&self.temp_directory),
        }
    }
}

impl StorageState {
    pub fn overview(&self) -> Result<StorageOverview, StorageError> {
        let connection = self.open_connection()?;
        let schema_version = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let tables = list_tables(&connection)?;
        let recording_count = table_count(&connection, "recordings")?;
        let processing_job_count = table_count(&connection, "processing_jobs")?;

        Ok(StorageOverview {
            paths: self.paths.snapshot(),
            schema_version,
            sqlite_initialized: self.paths.database_file.exists(),
            tables,
            recording_count,
            processing_job_count,
        })
    }

    pub fn create_recording(&self, input: CreateRecordingInput) -> Result<Recording, StorageError> {
        let connection = self.open_connection()?;
        let id = Uuid::new_v4().to_string();
        let title = normalize_title(input.title);
        let now = now_timestamp();
        let recording_directory = format!("{RECORDINGS_DIRECTORY_NAME}/{id}");

        fs::create_dir_all(self.paths.recordings_directory.join(&id))?;

        connection.execute(
            r#"
            INSERT INTO recordings (
                id, title, status, recording_directory, media_path, captured_at, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                id,
                title,
                RecordingStatus::Pending.as_str(),
                recording_directory,
                input.media_path,
                input.captured_at,
                now,
                now,
            ],
        )?;

        self.get_recording(&id)?.ok_or(StorageError::NotFound {
            entity: "recording",
            id,
        })
    }

    pub fn get_recording(&self, id: &str) -> Result<Option<Recording>, StorageError> {
        let connection = self.open_connection()?;
        connection
            .query_row(
                r#"
                SELECT
                    id, title, status, recording_directory, media_path, thumbnail_path,
                    duration_ms, captured_at, created_at, updated_at, completed_at, failure_message
                FROM recordings
                WHERE id = ?1
                "#,
                params![id],
                map_recording_row,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn list_recordings(&self) -> Result<Vec<Recording>, StorageError> {
        let connection = self.open_connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT
                id, title, status, recording_directory, media_path, thumbnail_path,
                duration_ms, captured_at, created_at, updated_at, completed_at, failure_message
            FROM recordings
            ORDER BY created_at DESC
            "#,
        )?;

        let rows = statement.query_map([], map_recording_row)?;
        let recordings = collect_rows(rows)?;
        Ok(recordings)
    }

    pub fn update_recording(&self, input: UpdateRecordingInput) -> Result<Recording, StorageError> {
        let existing = self
            .get_recording(&input.id)?
            .ok_or_else(|| StorageError::NotFound {
                entity: "recording",
                id: input.id.clone(),
            })?;
        let connection = self.open_connection()?;
        let now = now_timestamp();
        let title = input.title.unwrap_or(existing.title);
        let status = input.status.unwrap_or(existing.status);
        let completed_at = input.completed_at.or(existing.completed_at);

        connection.execute(
            r#"
            UPDATE recordings
            SET
                title = ?2,
                status = ?3,
                media_path = ?4,
                thumbnail_path = ?5,
                duration_ms = ?6,
                captured_at = ?7,
                updated_at = ?8,
                completed_at = ?9,
                failure_message = ?10
            WHERE id = ?1
            "#,
            params![
                input.id,
                title,
                status.as_str(),
                input.media_path.or(existing.media_path),
                input.thumbnail_path.or(existing.thumbnail_path),
                input.duration_ms.or(existing.duration_ms),
                input.captured_at.or(existing.captured_at),
                now,
                completed_at,
                input.failure_message.or(existing.failure_message),
            ],
        )?;

        self.get_recording(&input.id)?
            .ok_or_else(|| StorageError::NotFound {
                entity: "recording",
                id: input.id,
            })
    }

    pub fn mark_recording_processing(&self, id: &str) -> Result<Recording, StorageError> {
        if self.get_recording(id)?.is_none() {
            return Err(StorageError::NotFound {
                entity: "recording",
                id: id.to_owned(),
            });
        }

        let connection = self.open_connection()?;
        let now = now_timestamp();

        connection.execute(
            r#"
            UPDATE recordings
            SET
                status = ?2,
                media_path = NULL,
                thumbnail_path = NULL,
                updated_at = ?3,
                completed_at = NULL,
                failure_message = NULL
            WHERE id = ?1
            "#,
            params![id, RecordingStatus::Processing.as_str(), now],
        )?;

        self.get_recording(id)?
            .ok_or_else(|| StorageError::NotFound {
                entity: "recording",
                id: id.to_owned(),
            })
    }

    pub fn complete_recording_encode(
        &self,
        id: &str,
        media_path: String,
        thumbnail_path: Option<String>,
        duration_ms: Option<i64>,
        completed_at: String,
    ) -> Result<Recording, StorageError> {
        if self.get_recording(id)?.is_none() {
            return Err(StorageError::NotFound {
                entity: "recording",
                id: id.to_owned(),
            });
        }

        let connection = self.open_connection()?;
        let now = now_timestamp();

        connection.execute(
            r#"
            UPDATE recordings
            SET
                status = ?2,
                media_path = ?3,
                thumbnail_path = ?4,
                duration_ms = ?5,
                updated_at = ?6,
                completed_at = ?7,
                failure_message = NULL
            WHERE id = ?1
            "#,
            params![
                id,
                RecordingStatus::Completed.as_str(),
                media_path,
                thumbnail_path,
                duration_ms,
                now,
                completed_at,
            ],
        )?;

        self.get_recording(id)?
            .ok_or_else(|| StorageError::NotFound {
                entity: "recording",
                id: id.to_owned(),
            })
    }

    pub fn fail_recording_encode(
        &self,
        id: &str,
        failure_message: String,
    ) -> Result<Recording, StorageError> {
        if self.get_recording(id)?.is_none() {
            return Err(StorageError::NotFound {
                entity: "recording",
                id: id.to_owned(),
            });
        }

        let connection = self.open_connection()?;
        let now = now_timestamp();

        connection.execute(
            r#"
            UPDATE recordings
            SET
                status = ?2,
                media_path = NULL,
                thumbnail_path = NULL,
                updated_at = ?3,
                completed_at = NULL,
                failure_message = ?4
            WHERE id = ?1
            "#,
            params![id, RecordingStatus::Failed.as_str(), now, failure_message],
        )?;

        self.get_recording(id)?
            .ok_or_else(|| StorageError::NotFound {
                entity: "recording",
                id: id.to_owned(),
            })
    }

    pub fn prepare_recording_session_files(
        &self,
        session_id: &str,
        audio_mode: &CaptureAudioMode,
    ) -> Result<RecordingSessionFiles, StorageError> {
        let temp_directory = self
            .paths
            .temp_directory
            .join(RECORDING_SESSIONS_DIRECTORY_NAME)
            .join(session_id);
        fs::create_dir_all(&temp_directory)?;

        let temp_directory_relative =
            format!("{TEMP_DIRECTORY_NAME}/{RECORDING_SESSIONS_DIRECTORY_NAME}/{session_id}");
        let video_path_relative = if cfg!(target_os = "macos") {
            crate::media::chunked_video::chunked_manifest_relative_path(&temp_directory_relative)
        } else {
            format!("{temp_directory_relative}/screen_frames.mfrv")
        };
        let microphone_audio_path_relative = audio_mode
            .includes_microphone()
            .then(|| format!("{temp_directory_relative}/microphone.pcm"));
        let source_audio_path_relative = audio_mode
            .includes_source_audio()
            .then(|| format!("{temp_directory_relative}/source_audio.pcm"));
        let audio_path_relative = microphone_audio_path_relative.clone();
        let metadata_path_relative = format!("{temp_directory_relative}/session.json");

        if let Some(parent) = self.paths.root.join(&video_path_relative).parent() {
            fs::create_dir_all(parent)?;
        }

        Ok(RecordingSessionFiles {
            video_path: self.paths.root.join(&video_path_relative),
            microphone_audio_path: microphone_audio_path_relative
                .as_ref()
                .map(|path| self.paths.root.join(path)),
            source_audio_path: source_audio_path_relative
                .as_ref()
                .map(|path| self.paths.root.join(path)),
            metadata_path: self.paths.root.join(&metadata_path_relative),
            temp_directory_relative,
            video_path_relative,
            audio_path_relative,
            microphone_audio_path_relative,
            source_audio_path_relative,
            metadata_path_relative,
        })
    }

    pub fn recording_media_files(
        &self,
        recording_id: &str,
    ) -> Result<RecordingMediaFiles, StorageError> {
        let recording =
            self.get_recording(recording_id)?
                .ok_or_else(|| StorageError::NotFound {
                    entity: "recording",
                    id: recording_id.to_owned(),
                })?;
        let recording_directory_relative = recording.recording_directory;
        let recording_directory = self.resolve_path(&recording_directory_relative);
        fs::create_dir_all(&recording_directory)?;

        let media_path_relative = format!(
            "{}/recording.mp4",
            recording_directory_relative.trim_end_matches('/')
        );
        let thumbnail_path_relative = format!(
            "{}/thumbnail.jpg",
            recording_directory_relative.trim_end_matches('/')
        );

        Ok(RecordingMediaFiles {
            recording_directory,
            media_path: self.resolve_path(&media_path_relative),
            thumbnail_path: self.resolve_path(&thumbnail_path_relative),
            media_path_relative,
            thumbnail_path_relative,
        })
    }

    pub fn recording_asset_paths(
        &self,
        recording_id: &str,
    ) -> Result<RecordingAssetPaths, StorageError> {
        let recording =
            self.get_recording(recording_id)?
                .ok_or_else(|| StorageError::NotFound {
                    entity: "recording",
                    id: recording_id.to_owned(),
                })?;

        Ok(RecordingAssetPaths {
            media_path: recording
                .media_path
                .as_deref()
                .map(|path| path_to_string(&self.resolve_path(path))),
            thumbnail_path: recording
                .thumbnail_path
                .as_deref()
                .map(|path| path_to_string(&self.resolve_path(path))),
        })
    }

    pub fn resolve_path(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.paths.root.join(path)
        }
    }

    pub fn whisper_models_directory(&self) -> &Path {
        &self.paths.whisper_models_directory
    }

    pub fn create_recording_session(
        &self,
        input: CreateRecordingSessionInput,
    ) -> Result<RecordingSession, StorageError> {
        if self.get_recording(&input.recording_id)?.is_none() {
            return Err(StorageError::NotFound {
                entity: "recording",
                id: input.recording_id,
            });
        }

        let connection = self.open_connection()?;
        let now = now_timestamp();

        connection.execute(
            r#"
            INSERT INTO recording_sessions (
                id, recording_id, status, temp_directory, video_path, audio_path,
                metadata_path, video_source_id, screen_source_id, video_source_kind, video_source_title,
                video_source_app_name, video_source_process_id, video_source_window_id,
                microphone_device_id, include_microphone, audio_mode,
                microphone_audio_path, source_audio_path,
                width, height, frame_rate, audio_sample_rate, audio_channels,
                audio_sample_format, microphone_audio_sample_rate, microphone_audio_channels,
                microphone_audio_sample_format, source_audio_sample_rate, source_audio_channels,
                source_audio_sample_format, started_at, created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10, ?11,
                ?12, ?13, ?14,
                ?15, ?16, ?17,
                ?18, ?19,
                ?20, ?21, ?22, ?23, ?24,
                ?25, ?26, ?27, ?28, ?29,
                ?30, ?31, ?32, ?33, ?34
            )
            "#,
            params![
                input.id,
                input.recording_id,
                RecordingSessionStatus::Capturing.as_str(),
                input.temp_directory,
                input.video_path,
                input.audio_path,
                input.metadata_path,
                input.video_source_id,
                input.screen_source_id,
                input.video_source_kind,
                input.video_source_title,
                input.video_source_app_name,
                input.video_source_process_id,
                input.video_source_window_id,
                input.microphone_device_id,
                if input.include_microphone { 1 } else { 0 },
                input.audio_mode.as_str(),
                input.microphone_audio_path,
                input.source_audio_path,
                input.width,
                input.height,
                input.frame_rate,
                input.audio_sample_rate,
                input.audio_channels,
                input.audio_sample_format,
                input.microphone_audio_sample_rate,
                input.microphone_audio_channels,
                input.microphone_audio_sample_format,
                input.source_audio_sample_rate,
                input.source_audio_channels,
                input.source_audio_sample_format,
                input.started_at,
                now,
                now,
            ],
        )?;

        self.get_recording_session(&input.id)?
            .ok_or(StorageError::NotFound {
                entity: "recording_session",
                id: input.id,
            })
    }

    pub fn update_recording_session_capture_details(
        &self,
        id: &str,
        width: Option<i64>,
        height: Option<i64>,
        audio_sample_rate: Option<i64>,
        audio_channels: Option<i64>,
        audio_sample_format: Option<String>,
        microphone_audio_sample_rate: Option<i64>,
        microphone_audio_channels: Option<i64>,
        microphone_audio_sample_format: Option<String>,
        source_audio_sample_rate: Option<i64>,
        source_audio_channels: Option<i64>,
        source_audio_sample_format: Option<String>,
    ) -> Result<RecordingSession, StorageError> {
        let existing = self
            .get_recording_session(id)?
            .ok_or_else(|| StorageError::NotFound {
                entity: "recording_session",
                id: id.to_owned(),
            })?;
        let connection = self.open_connection()?;
        let now = now_timestamp();

        connection.execute(
            r#"
            UPDATE recording_sessions
            SET
                width = ?2,
                height = ?3,
                audio_sample_rate = ?4,
                audio_channels = ?5,
                audio_sample_format = ?6,
                microphone_audio_sample_rate = ?7,
                microphone_audio_channels = ?8,
                microphone_audio_sample_format = ?9,
                source_audio_sample_rate = ?10,
                source_audio_channels = ?11,
                source_audio_sample_format = ?12,
                updated_at = ?13
            WHERE id = ?1
            "#,
            params![
                id,
                width.or(existing.width),
                height.or(existing.height),
                audio_sample_rate.or(existing.audio_sample_rate),
                audio_channels.or(existing.audio_channels),
                audio_sample_format.or(existing.audio_sample_format),
                microphone_audio_sample_rate.or(existing.microphone_audio_sample_rate),
                microphone_audio_channels.or(existing.microphone_audio_channels),
                microphone_audio_sample_format.or(existing.microphone_audio_sample_format),
                source_audio_sample_rate.or(existing.source_audio_sample_rate),
                source_audio_channels.or(existing.source_audio_channels),
                source_audio_sample_format.or(existing.source_audio_sample_format),
                now,
            ],
        )?;

        self.get_recording_session(id)?
            .ok_or_else(|| StorageError::NotFound {
                entity: "recording_session",
                id: id.to_owned(),
            })
    }

    pub fn finish_recording_session(
        &self,
        input: FinishRecordingSessionInput,
    ) -> Result<RecordingSession, StorageError> {
        let existing =
            self.get_recording_session(&input.id)?
                .ok_or_else(|| StorageError::NotFound {
                    entity: "recording_session",
                    id: input.id.clone(),
                })?;
        let connection = self.open_connection()?;
        let now = now_timestamp();

        connection.execute(
            r#"
            UPDATE recording_sessions
            SET
                status = ?2,
                width = ?3,
                height = ?4,
                frame_count = ?5,
                audio_byte_count = ?6,
                audio_sample_rate = ?7,
                audio_channels = ?8,
                audio_sample_format = ?9,
                microphone_audio_byte_count = ?10,
                microphone_audio_sample_rate = ?11,
                microphone_audio_channels = ?12,
                microphone_audio_sample_format = ?13,
                source_audio_byte_count = ?14,
                source_audio_sample_rate = ?15,
                source_audio_channels = ?16,
                source_audio_sample_format = ?17,
                stopped_at = ?18,
                duration_ms = ?19,
                failure_message = ?20,
                updated_at = ?21
            WHERE id = ?1
            "#,
            params![
                input.id,
                input.status.as_str(),
                input.width.or(existing.width),
                input.height.or(existing.height),
                input.frame_count,
                input.audio_byte_count,
                input.audio_sample_rate.or(existing.audio_sample_rate),
                input.audio_channels.or(existing.audio_channels),
                input.audio_sample_format.or(existing.audio_sample_format),
                input.microphone_audio_byte_count,
                input
                    .microphone_audio_sample_rate
                    .or(existing.microphone_audio_sample_rate),
                input
                    .microphone_audio_channels
                    .or(existing.microphone_audio_channels),
                input
                    .microphone_audio_sample_format
                    .or(existing.microphone_audio_sample_format),
                input.source_audio_byte_count,
                input
                    .source_audio_sample_rate
                    .or(existing.source_audio_sample_rate),
                input
                    .source_audio_channels
                    .or(existing.source_audio_channels),
                input
                    .source_audio_sample_format
                    .or(existing.source_audio_sample_format),
                input.stopped_at,
                input.duration_ms,
                input.failure_message,
                now,
            ],
        )?;

        self.get_recording_session(&input.id)?
            .ok_or_else(|| StorageError::NotFound {
                entity: "recording_session",
                id: input.id,
            })
    }

    pub fn get_recording_session(
        &self,
        id: &str,
    ) -> Result<Option<RecordingSession>, StorageError> {
        let connection = self.open_connection()?;
        connection
            .query_row(
                r#"
                SELECT
                    id, recording_id, status, temp_directory, video_path, audio_path,
                    metadata_path, video_source_id, screen_source_id, video_source_kind, video_source_title,
                    video_source_app_name, video_source_process_id, video_source_window_id,
                    microphone_device_id, include_microphone, audio_mode,
                    microphone_audio_path, source_audio_path,
                    width, height, frame_rate, frame_count, audio_byte_count,
                    audio_sample_rate, audio_channels, audio_sample_format,
                    microphone_audio_byte_count, microphone_audio_sample_rate,
                    microphone_audio_channels, microphone_audio_sample_format,
                    source_audio_byte_count, source_audio_sample_rate,
                    source_audio_channels, source_audio_sample_format, started_at,
                    stopped_at, duration_ms, failure_message, created_at, updated_at
                FROM recording_sessions
                WHERE id = ?1
                "#,
                params![id],
                map_recording_session_row,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn get_recording_session_by_recording(
        &self,
        recording_id: &str,
    ) -> Result<Option<RecordingSession>, StorageError> {
        let connection = self.open_connection()?;
        connection
            .query_row(
                r#"
                SELECT
                    id, recording_id, status, temp_directory, video_path, audio_path,
                    metadata_path, video_source_id, screen_source_id, video_source_kind, video_source_title,
                    video_source_app_name, video_source_process_id, video_source_window_id,
                    microphone_device_id, include_microphone, audio_mode,
                    microphone_audio_path, source_audio_path,
                    width, height, frame_rate, frame_count, audio_byte_count,
                    audio_sample_rate, audio_channels, audio_sample_format,
                    microphone_audio_byte_count, microphone_audio_sample_rate,
                    microphone_audio_channels, microphone_audio_sample_format,
                    source_audio_byte_count, source_audio_sample_rate,
                    source_audio_channels, source_audio_sample_format, started_at,
                    stopped_at, duration_ms, failure_message, created_at, updated_at
                FROM recording_sessions
                WHERE recording_id = ?1
                "#,
                params![recording_id],
                map_recording_session_row,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn persist_transcript(
        &self,
        input: PersistTranscriptInput,
    ) -> Result<TranscriptWithSegments, StorageError> {
        if self.get_recording(&input.recording_id)?.is_none() {
            return Err(StorageError::NotFound {
                entity: "recording",
                id: input.recording_id,
            });
        }

        let mut connection = self.open_connection()?;
        let transaction = connection.transaction()?;
        let now = now_timestamp();
        let transcript_id = transaction
            .query_row(
                "SELECT id FROM transcripts WHERE recording_id = ?1",
                params![input.recording_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let existing_created_at = transaction
            .query_row(
                "SELECT created_at FROM transcripts WHERE id = ?1",
                params![transcript_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let created_at = existing_created_at.unwrap_or_else(|| now.clone());

        transaction.execute(
            r#"
            INSERT INTO transcripts (
                id, recording_id, status, language, model_name, raw_json_path, text,
                created_at, updated_at, completed_at, failure_message
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT(recording_id) DO UPDATE SET
                status = excluded.status,
                language = excluded.language,
                model_name = excluded.model_name,
                raw_json_path = excluded.raw_json_path,
                text = excluded.text,
                updated_at = excluded.updated_at,
                completed_at = excluded.completed_at,
                failure_message = excluded.failure_message
            "#,
            params![
                transcript_id,
                input.recording_id,
                input.status.as_str(),
                input.language,
                input.model_name,
                input.raw_json_path,
                input.text,
                created_at,
                now,
                input.completed_at,
                input.failure_message,
            ],
        )?;

        clear_transcript_search_index(&transaction, &transcript_id)?;
        transaction.execute(
            "DELETE FROM transcript_segments WHERE transcript_id = ?1",
            params![transcript_id],
        )?;

        for (index, segment) in input.segments.into_iter().enumerate() {
            let segment_index = segment.segment_index.unwrap_or(index as i64);
            transaction.execute(
                r#"
                INSERT INTO transcript_segments (
                    id, transcript_id, recording_id, segment_index, start_ms, end_ms, text, confidence
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
                params![
                    Uuid::new_v4().to_string(),
                    transcript_id,
                    input.recording_id,
                    segment_index,
                    segment.start_ms,
                    segment.end_ms,
                    segment.text,
                    segment.confidence,
                ],
            )?;
        }

        if input.status == TranscriptStatus::Completed {
            index_transcript_segments(&transaction, &transcript_id)?;
        }

        transaction.commit()?;
        self.get_transcript_by_recording(&input.recording_id)?
            .ok_or(StorageError::NotFound {
                entity: "transcript",
                id: input.recording_id,
            })
    }

    pub fn get_transcript_by_recording(
        &self,
        recording_id: &str,
    ) -> Result<Option<TranscriptWithSegments>, StorageError> {
        let connection = self.open_connection()?;
        let transcript = connection
            .query_row(
                r#"
                SELECT
                    id, recording_id, status, language, model_name, raw_json_path, text,
                    created_at, updated_at, completed_at, failure_message
                FROM transcripts
                WHERE recording_id = ?1
                "#,
                params![recording_id],
                map_transcript_row,
            )
            .optional()?;

        let Some(transcript) = transcript else {
            return Ok(None);
        };

        let mut statement = connection.prepare(
            r#"
            SELECT
                id, transcript_id, recording_id, segment_index, start_ms, end_ms, text, confidence
            FROM transcript_segments
            WHERE transcript_id = ?1
            ORDER BY segment_index ASC
            "#,
        )?;
        let rows = statement.query_map(params![transcript.id], map_segment_row)?;
        let segments = collect_rows(rows)?;

        Ok(Some(TranscriptWithSegments {
            transcript,
            segments,
        }))
    }

    pub fn search_transcripts(
        &self,
        input: SearchTranscriptsInput,
    ) -> Result<Vec<TranscriptSearchResult>, StorageError> {
        let Some(match_query) = normalize_transcript_search_query(&input.query) else {
            return Ok(Vec::new());
        };
        let limit = input.limit.unwrap_or(50).clamp(1, 100);
        let connection = self.open_connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT
                recordings.id,
                recordings.title,
                transcripts.id,
                transcript_segments.id,
                transcript_segments.segment_index,
                transcript_segments.start_ms,
                transcript_segments.end_ms,
                transcript_segments.text,
                snippet(transcript_segment_search, 1, '[', ']', '...', 18),
                bm25(transcript_segment_search),
                recordings.media_path,
                recordings.thumbnail_path,
                recordings.captured_at,
                recordings.created_at
            FROM transcript_segment_search
            JOIN transcript_segments
                ON transcript_segments.rowid = transcript_segment_search.rowid
            JOIN transcripts
                ON transcripts.id = transcript_segments.transcript_id
            JOIN recordings
                ON recordings.id = transcript_segments.recording_id
            WHERE
                transcript_segment_search MATCH ?1
                AND transcripts.status = ?2
            ORDER BY bm25(transcript_segment_search) ASC,
                recordings.created_at DESC,
                transcript_segments.start_ms ASC
            LIMIT ?3
            "#,
        )?;
        let rows = statement.query_map(
            params![match_query, TranscriptStatus::Completed.as_str(), limit],
            map_transcript_search_result_row,
        )?;
        collect_rows(rows)
    }

    pub fn reindex_transcript_search(&self) -> Result<TranscriptSearchIndexSummary, StorageError> {
        let mut connection = self.open_connection()?;
        let transaction = connection.transaction()?;

        transaction.execute("DELETE FROM transcript_segment_search", [])?;
        let indexed_segment_count = index_all_completed_transcript_segments(&transaction)?;

        transaction.commit()?;
        Ok(TranscriptSearchIndexSummary {
            indexed_segment_count,
        })
    }

    pub fn upsert_ai_summary(
        &self,
        input: UpsertAiSummaryInput,
    ) -> Result<AiSummary, StorageError> {
        if self.get_recording(&input.recording_id)?.is_none() {
            return Err(StorageError::NotFound {
                entity: "recording",
                id: input.recording_id,
            });
        }

        let connection = self.open_connection()?;
        let now = now_timestamp();
        let summary_id = connection
            .query_row(
                "SELECT id FROM ai_summaries WHERE recording_id = ?1",
                params![input.recording_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let created_at = connection
            .query_row(
                "SELECT created_at FROM ai_summaries WHERE id = ?1",
                params![summary_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| now.clone());

        connection.execute(
            r#"
            INSERT INTO ai_summaries (
                id, recording_id, status, model_name, summary_text, action_items_json,
                decisions_json, questions_json, risks_json, chapters_json, created_at,
                updated_at, completed_at, failure_message
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ON CONFLICT(recording_id) DO UPDATE SET
                status = excluded.status,
                model_name = excluded.model_name,
                summary_text = excluded.summary_text,
                action_items_json = excluded.action_items_json,
                decisions_json = excluded.decisions_json,
                questions_json = excluded.questions_json,
                risks_json = excluded.risks_json,
                chapters_json = excluded.chapters_json,
                updated_at = excluded.updated_at,
                completed_at = excluded.completed_at,
                failure_message = excluded.failure_message
            "#,
            params![
                summary_id,
                input.recording_id,
                input.status.as_str(),
                input.model_name,
                input.summary_text,
                input.action_items_json,
                input.decisions_json,
                input.questions_json,
                input.risks_json,
                input.chapters_json,
                created_at,
                now,
                input.completed_at,
                input.failure_message,
            ],
        )?;

        self.get_ai_summary_by_recording(&input.recording_id)?
            .ok_or(StorageError::NotFound {
                entity: "ai_summary",
                id: input.recording_id,
            })
    }

    pub fn get_ai_summary_by_recording(
        &self,
        recording_id: &str,
    ) -> Result<Option<AiSummary>, StorageError> {
        let connection = self.open_connection()?;
        connection
            .query_row(
                r#"
                SELECT
                    id, recording_id, status, model_name, summary_text, action_items_json,
                    decisions_json, questions_json, risks_json, chapters_json, created_at,
                    updated_at, completed_at, failure_message
                FROM ai_summaries
                WHERE recording_id = ?1
                "#,
                params![recording_id],
                map_ai_summary_row,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn get_ai_settings(&self) -> Result<AiSettings, StorageError> {
        Ok(self.get_ai_settings_record()?.public_settings())
    }

    pub fn get_ai_settings_record(&self) -> Result<AiSettingsRecord, StorageError> {
        let connection = self.open_connection()?;
        connection
            .query_row(
                r#"
                SELECT enabled, provider, model_name, endpoint_url, api_key, updated_at
                FROM ai_settings
                WHERE id = 1
                "#,
                [],
                map_ai_settings_row,
            )
            .optional()
            .map(|settings| settings.unwrap_or_else(default_ai_settings_record))
            .map_err(StorageError::from)
    }

    pub fn save_ai_settings(&self, input: SaveAiSettingsInput) -> Result<AiSettings, StorageError> {
        let existing = self.get_ai_settings_record()?;
        let enabled = input.enabled;
        let provider = normalized_setting(input.provider).unwrap_or(existing.provider);
        let model_name = normalized_setting(input.model_name).unwrap_or(existing.model_name);
        let endpoint_url = normalized_setting(input.endpoint_url).unwrap_or(existing.endpoint_url);
        let api_key = if input.clear_api_key.unwrap_or(false) {
            None
        } else {
            normalized_setting(input.api_key).or(existing.api_key)
        };

        validate_ai_settings(
            enabled,
            &provider,
            &model_name,
            &endpoint_url,
            api_key.as_deref(),
        )?;

        let connection = self.open_connection()?;
        let now = now_timestamp();

        connection.execute(
            r#"
            INSERT INTO ai_settings (
                id, enabled, provider, model_name, endpoint_url, api_key, updated_at
            ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                enabled = excluded.enabled,
                provider = excluded.provider,
                model_name = excluded.model_name,
                endpoint_url = excluded.endpoint_url,
                api_key = excluded.api_key,
                updated_at = excluded.updated_at
            "#,
            params![
                if enabled { 1 } else { 0 },
                provider,
                model_name,
                endpoint_url,
                api_key,
                now,
            ],
        )?;

        self.get_ai_settings()
    }

    pub fn create_processing_job(
        &self,
        input: CreateProcessingJobInput,
    ) -> Result<ProcessingJob, StorageError> {
        if let Some(recording_id) = input.recording_id.as_deref() {
            if self.get_recording(recording_id)?.is_none() {
                return Err(StorageError::NotFound {
                    entity: "recording",
                    id: recording_id.to_owned(),
                });
            }
        }

        let connection = self.open_connection()?;
        let id = Uuid::new_v4().to_string();
        let now = now_timestamp();
        let max_attempts = input
            .max_attempts
            .filter(|attempts| *attempts > 0)
            .unwrap_or(3);

        connection.execute(
            r#"
            INSERT INTO processing_jobs (
                id, recording_id, kind, state, priority, max_attempts, input_json, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                id,
                input.recording_id,
                input.kind,
                JobState::Queued.as_str(),
                input.priority.unwrap_or(0),
                max_attempts,
                input.input_json,
                now,
                now,
            ],
        )?;

        self.get_processing_job(&id)?.ok_or(StorageError::NotFound {
            entity: "processing_job",
            id,
        })
    }

    pub fn get_processing_job(&self, id: &str) -> Result<Option<ProcessingJob>, StorageError> {
        let connection = self.open_connection()?;
        connection
            .query_row(
                r#"
                SELECT
                    id, recording_id, kind, state, priority, attempts, max_attempts,
                    input_json, output_json, error_message, interrupted, last_error_at,
                    created_at, updated_at,
                    started_at, completed_at
                FROM processing_jobs
                WHERE id = ?1
                "#,
                params![id],
                map_processing_job_row,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn list_processing_jobs(
        &self,
        state: Option<JobState>,
        recording_id: Option<String>,
    ) -> Result<Vec<ProcessingJob>, StorageError> {
        let connection = self.open_connection()?;

        match (state, recording_id) {
            (Some(state), Some(recording_id)) => {
                let mut statement = connection.prepare(
                    r#"
                    SELECT
                        id, recording_id, kind, state, priority, attempts, max_attempts,
                        input_json, output_json, error_message, interrupted, last_error_at,
                        created_at, updated_at,
                        started_at, completed_at
                    FROM processing_jobs
                    WHERE state = ?1 AND recording_id = ?2
                    ORDER BY priority DESC, created_at ASC
                    "#,
                )?;
                let rows = statement.query_map(
                    params![state.as_str(), recording_id],
                    map_processing_job_row,
                )?;
                let jobs = collect_rows(rows)?;
                Ok(jobs)
            }
            (Some(state), None) => {
                let mut statement = connection.prepare(
                    r#"
                    SELECT
                        id, recording_id, kind, state, priority, attempts, max_attempts,
                        input_json, output_json, error_message, interrupted, last_error_at,
                        created_at, updated_at,
                        started_at, completed_at
                    FROM processing_jobs
                    WHERE state = ?1
                    ORDER BY priority DESC, created_at ASC
                    "#,
                )?;
                let rows = statement.query_map(params![state.as_str()], map_processing_job_row)?;
                let jobs = collect_rows(rows)?;
                Ok(jobs)
            }
            (None, Some(recording_id)) => {
                let mut statement = connection.prepare(
                    r#"
                    SELECT
                        id, recording_id, kind, state, priority, attempts, max_attempts,
                        input_json, output_json, error_message, interrupted, last_error_at,
                        created_at, updated_at,
                        started_at, completed_at
                    FROM processing_jobs
                    WHERE recording_id = ?1
                    ORDER BY created_at DESC
                    "#,
                )?;
                let rows = statement.query_map(params![recording_id], map_processing_job_row)?;
                let jobs = collect_rows(rows)?;
                Ok(jobs)
            }
            (None, None) => {
                let mut statement = connection.prepare(
                    r#"
                    SELECT
                        id, recording_id, kind, state, priority, attempts, max_attempts,
                        input_json, output_json, error_message, interrupted, last_error_at,
                        created_at, updated_at,
                        started_at, completed_at
                    FROM processing_jobs
                    ORDER BY created_at DESC
                    "#,
                )?;
                let rows = statement.query_map([], map_processing_job_row)?;
                let jobs = collect_rows(rows)?;
                Ok(jobs)
            }
        }
    }

    pub fn update_processing_job(
        &self,
        input: UpdateProcessingJobInput,
    ) -> Result<ProcessingJob, StorageError> {
        let existing =
            self.get_processing_job(&input.id)?
                .ok_or_else(|| StorageError::NotFound {
                    entity: "processing_job",
                    id: input.id.clone(),
                })?;
        let connection = self.open_connection()?;
        let now = now_timestamp();
        let state = input.state.unwrap_or(existing.state);
        let mut started_at = input.started_at.or(existing.started_at);
        let mut completed_at = input.completed_at.or(existing.completed_at);
        let interrupted = input.interrupted.unwrap_or(existing.interrupted);
        let output_json = match state {
            JobState::Queued | JobState::Running | JobState::Failed | JobState::Cancelled => {
                input.output_json
            }
            JobState::Succeeded => input.output_json.or(existing.output_json),
        };
        let error_message = match state {
            JobState::Queued | JobState::Running | JobState::Succeeded => None,
            JobState::Failed | JobState::Cancelled => {
                input.error_message.or(existing.error_message)
            }
        };
        let last_error_at = if error_message.is_some() || interrupted {
            Some(now.clone())
        } else if matches!(
            state,
            JobState::Queued | JobState::Running | JobState::Succeeded
        ) {
            None
        } else {
            existing.last_error_at
        };

        if state == JobState::Running && started_at.is_none() {
            started_at = Some(now.clone());
        }

        if matches!(state, JobState::Queued | JobState::Running) {
            completed_at = None;
        }

        if matches!(
            state,
            JobState::Succeeded | JobState::Failed | JobState::Cancelled
        ) && completed_at.is_none()
        {
            completed_at = Some(now.clone());
        }

        connection.execute(
            r#"
            UPDATE processing_jobs
            SET
                state = ?2,
                attempts = ?3,
                output_json = ?4,
                error_message = ?5,
                updated_at = ?6,
                started_at = ?7,
                completed_at = ?8,
                interrupted = ?9,
                last_error_at = ?10
            WHERE id = ?1
            "#,
            params![
                input.id,
                state.as_str(),
                input.attempts.unwrap_or(existing.attempts),
                output_json,
                error_message,
                now,
                started_at,
                completed_at,
                if interrupted { 1 } else { 0 },
                last_error_at,
            ],
        )?;

        self.get_processing_job(&input.id)?
            .ok_or_else(|| StorageError::NotFound {
                entity: "processing_job",
                id: input.id,
            })
    }

    pub fn mark_running_jobs_interrupted(
        &self,
        message: &str,
    ) -> Result<Vec<ProcessingJob>, StorageError> {
        let connection = self.open_connection()?;
        let now = now_timestamp();

        connection.execute(
            r#"
            UPDATE processing_jobs
            SET
                state = ?2,
                interrupted = 1,
                error_message = ?3,
                last_error_at = ?4,
                updated_at = ?5,
                completed_at = ?6
            WHERE state = ?1
            "#,
            params![
                JobState::Running.as_str(),
                JobState::Failed.as_str(),
                message,
                now,
                now,
                now,
            ],
        )?;

        let mut statement = connection.prepare(
            r#"
            SELECT
                id, recording_id, kind, state, priority, attempts, max_attempts,
                input_json, output_json, error_message, interrupted, last_error_at,
                created_at, updated_at, started_at, completed_at
            FROM processing_jobs
            WHERE interrupted = 1 AND updated_at = ?1
            ORDER BY priority DESC, created_at ASC
            "#,
        )?;
        let rows = statement.query_map(params![now], map_processing_job_row)?;
        collect_rows(rows)
    }

    pub fn reset_processing_job_for_retry(&self, id: &str) -> Result<ProcessingJob, StorageError> {
        if self.get_processing_job(id)?.is_none() {
            return Err(StorageError::NotFound {
                entity: "processing_job",
                id: id.to_owned(),
            });
        }

        let connection = self.open_connection()?;
        let now = now_timestamp();

        connection.execute(
            r#"
            UPDATE processing_jobs
            SET
                state = ?2,
                output_json = NULL,
                error_message = NULL,
                interrupted = 0,
                last_error_at = NULL,
                updated_at = ?3,
                started_at = NULL,
                completed_at = NULL
            WHERE id = ?1
            "#,
            params![id, JobState::Queued.as_str(), now],
        )?;

        self.get_processing_job(id)?
            .ok_or_else(|| StorageError::NotFound {
                entity: "processing_job",
                id: id.to_owned(),
            })
    }

    pub fn claim_next_queued_processing_job(&self) -> Result<Option<ProcessingJob>, StorageError> {
        let mut connection = self.open_connection()?;
        let transaction = connection.transaction()?;
        let next_id = transaction
            .query_row(
                r#"
                SELECT id
                FROM processing_jobs
                WHERE state = ?1
                ORDER BY priority DESC, created_at ASC
                LIMIT 1
                "#,
                params![JobState::Queued.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        let Some(id) = next_id else {
            transaction.commit()?;
            return Ok(None);
        };

        let now = now_timestamp();
        transaction.execute(
            r#"
            UPDATE processing_jobs
            SET
                state = ?2,
                attempts = attempts + 1,
                output_json = NULL,
                error_message = NULL,
                interrupted = 0,
                last_error_at = NULL,
                updated_at = ?3,
                started_at = ?4,
                completed_at = NULL
            WHERE id = ?1 AND state = ?5
            "#,
            params![
                id,
                JobState::Running.as_str(),
                now,
                now,
                JobState::Queued.as_str(),
            ],
        )?;
        transaction.commit()?;

        self.get_processing_job(&id)
    }

    pub fn get_capture_selection(&self) -> Result<CaptureSelection, StorageError> {
        let connection = self.open_connection()?;
        let selection = connection
            .query_row(
                r#"
                SELECT
                    COALESCE(NULLIF(video_source_id, ''), NULLIF(screen_source_id, '')),
                    screen_source_id,
                    microphone_device_id,
                    COALESCE(
                        NULLIF(audio_mode, ''),
                        CASE include_microphone
                            WHEN 1 THEN 'microphone'
                            ELSE 'none'
                        END
                    ),
                    include_microphone,
                    updated_at
                FROM capture_preferences
                WHERE id = 1
                "#,
                [],
                map_capture_selection_row,
            )
            .optional()?
            .unwrap_or_else(default_capture_selection);

        Ok(selection)
    }

    pub fn save_capture_selection(
        &self,
        input: SaveCaptureSelectionInput,
    ) -> Result<CaptureSelection, StorageError> {
        let connection = self.open_connection()?;
        let video_source_id = normalized_setting(input.video_source_id.or(input.screen_source_id));
        let audio_mode = input.audio_mode.unwrap_or_else(|| {
            CaptureAudioMode::from_include_microphone(input.include_microphone.unwrap_or(true))
        });
        let include_microphone = audio_mode.includes_microphone();
        let updated_at = now_timestamp();

        connection.execute(
            r#"
            INSERT INTO capture_preferences (
                id, video_source_id, screen_source_id, microphone_device_id, audio_mode,
                include_microphone, updated_at
            ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                video_source_id = excluded.video_source_id,
                screen_source_id = excluded.screen_source_id,
                microphone_device_id = excluded.microphone_device_id,
                audio_mode = excluded.audio_mode,
                include_microphone = excluded.include_microphone,
                updated_at = excluded.updated_at
            "#,
            params![
                video_source_id.clone(),
                video_source_id,
                input.microphone_device_id,
                audio_mode.as_str(),
                if include_microphone { 1 } else { 0 },
                updated_at,
            ],
        )?;

        self.get_capture_selection()
    }

    fn open_connection(&self) -> Result<Connection, StorageError> {
        let connection = Connection::open(&self.paths.database_file)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(connection)
    }
}

fn create_required_directories(paths: &StoragePaths) -> Result<(), StorageError> {
    fs::create_dir_all(&paths.root)?;
    fs::create_dir_all(&paths.recordings_directory)?;
    fs::create_dir_all(&paths.whisper_models_directory)?;
    fs::create_dir_all(&paths.temp_directory)?;
    Ok(())
}

fn run_migrations(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        "#,
    )?;
    connection.execute_batch(MIGRATION_SQL)?;
    connection.execute(
        r#"
        INSERT INTO transcript_segment_search(rowid, segment_id, text)
        SELECT transcript_segments.rowid, transcript_segments.id, transcript_segments.text
        FROM transcript_segments
        JOIN transcripts ON transcripts.id = transcript_segments.transcript_id
        WHERE transcripts.status = ?1
            AND transcript_segments.rowid NOT IN (
                SELECT rowid FROM transcript_segment_search
            )
        "#,
        params![TranscriptStatus::Completed.as_str()],
    )?;
    add_column_if_missing(
        connection,
        "transcript_segments",
        "confidence",
        "ALTER TABLE transcript_segments ADD COLUMN confidence REAL CHECK (confidence IS NULL OR (confidence >= 0 AND confidence <= 1))",
    )?;
    add_column_if_missing(
        connection,
        "processing_jobs",
        "interrupted",
        "ALTER TABLE processing_jobs ADD COLUMN interrupted INTEGER NOT NULL DEFAULT 0 CHECK (interrupted IN (0, 1))",
    )?;
    add_column_if_missing(
        connection,
        "processing_jobs",
        "last_error_at",
        "ALTER TABLE processing_jobs ADD COLUMN last_error_at TEXT",
    )?;
    add_column_if_missing(
        connection,
        "capture_preferences",
        "video_source_id",
        "ALTER TABLE capture_preferences ADD COLUMN video_source_id TEXT",
    )?;
    add_column_if_missing(
        connection,
        "capture_preferences",
        "audio_mode",
        "ALTER TABLE capture_preferences ADD COLUMN audio_mode TEXT",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "video_source_id",
        "ALTER TABLE recording_sessions ADD COLUMN video_source_id TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "video_source_kind",
        "ALTER TABLE recording_sessions ADD COLUMN video_source_kind TEXT NOT NULL DEFAULT 'display' CHECK (video_source_kind IN ('display', 'application', 'window'))",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "video_source_title",
        "ALTER TABLE recording_sessions ADD COLUMN video_source_title TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "video_source_app_name",
        "ALTER TABLE recording_sessions ADD COLUMN video_source_app_name TEXT",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "video_source_process_id",
        "ALTER TABLE recording_sessions ADD COLUMN video_source_process_id INTEGER CHECK (video_source_process_id IS NULL OR video_source_process_id >= 0)",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "video_source_window_id",
        "ALTER TABLE recording_sessions ADD COLUMN video_source_window_id INTEGER CHECK (video_source_window_id IS NULL OR video_source_window_id >= 0)",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "audio_mode",
        "ALTER TABLE recording_sessions ADD COLUMN audio_mode TEXT NOT NULL DEFAULT 'microphone' CHECK (audio_mode IN ('none', 'microphone', 'source', 'microphone_and_source'))",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "microphone_audio_path",
        "ALTER TABLE recording_sessions ADD COLUMN microphone_audio_path TEXT",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "source_audio_path",
        "ALTER TABLE recording_sessions ADD COLUMN source_audio_path TEXT",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "microphone_audio_byte_count",
        "ALTER TABLE recording_sessions ADD COLUMN microphone_audio_byte_count INTEGER NOT NULL DEFAULT 0 CHECK (microphone_audio_byte_count >= 0)",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "microphone_audio_sample_rate",
        "ALTER TABLE recording_sessions ADD COLUMN microphone_audio_sample_rate INTEGER CHECK (microphone_audio_sample_rate IS NULL OR microphone_audio_sample_rate > 0)",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "microphone_audio_channels",
        "ALTER TABLE recording_sessions ADD COLUMN microphone_audio_channels INTEGER CHECK (microphone_audio_channels IS NULL OR microphone_audio_channels > 0)",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "microphone_audio_sample_format",
        "ALTER TABLE recording_sessions ADD COLUMN microphone_audio_sample_format TEXT",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "source_audio_byte_count",
        "ALTER TABLE recording_sessions ADD COLUMN source_audio_byte_count INTEGER NOT NULL DEFAULT 0 CHECK (source_audio_byte_count >= 0)",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "source_audio_sample_rate",
        "ALTER TABLE recording_sessions ADD COLUMN source_audio_sample_rate INTEGER CHECK (source_audio_sample_rate IS NULL OR source_audio_sample_rate > 0)",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "source_audio_channels",
        "ALTER TABLE recording_sessions ADD COLUMN source_audio_channels INTEGER CHECK (source_audio_channels IS NULL OR source_audio_channels > 0)",
    )?;
    add_column_if_missing(
        connection,
        "recording_sessions",
        "source_audio_sample_format",
        "ALTER TABLE recording_sessions ADD COLUMN source_audio_sample_format TEXT",
    )?;
    connection.execute(
        r#"
        UPDATE capture_preferences
        SET
            video_source_id = COALESCE(NULLIF(video_source_id, ''), NULLIF(screen_source_id, '')),
            audio_mode = CASE
                WHEN audio_mode IS NULL OR audio_mode = '' THEN
                    CASE include_microphone
                        WHEN 1 THEN ?1
                        ELSE ?2
                    END
                ELSE audio_mode
            END
        "#,
        params![
            CaptureAudioMode::Microphone.as_str(),
            CaptureAudioMode::None.as_str()
        ],
    )?;
    connection.execute(
        r#"
        UPDATE recording_sessions
        SET
            video_source_id = COALESCE(NULLIF(video_source_id, ''), NULLIF(screen_source_id, ''), screen_source_id),
            video_source_kind = CASE
                WHEN screen_source_id LIKE 'window:%' THEN 'window'
                WHEN screen_source_id LIKE 'application:%' THEN 'application'
                ELSE 'display'
            END,
            video_source_title = CASE
                WHEN video_source_title IS NULL OR video_source_title = '' THEN screen_source_id
                ELSE video_source_title
            END,
            video_source_window_id = CASE
                WHEN screen_source_id GLOB 'window:[0-9]*' THEN CAST(substr(screen_source_id, 8) AS INTEGER)
                ELSE video_source_window_id
            END
        "#,
        [],
    )?;
    connection.execute(
        r#"
        UPDATE recording_sessions
        SET audio_mode = CASE
            WHEN audio_mode IS NULL
                OR audio_mode = ''
                OR (audio_mode = 'microphone' AND include_microphone = 0)
            THEN
                CASE
                    WHEN include_microphone = 1 AND audio_path LIKE '%source_audio.pcm' THEN 'microphone_and_source'
                    WHEN include_microphone = 1 THEN 'microphone'
                    WHEN audio_path LIKE '%source_audio.pcm' THEN 'source'
                    WHEN audio_path IS NOT NULL AND audio_path != '' THEN 'microphone'
                    ELSE 'none'
                END
            ELSE audio_mode
        END
        "#,
        [],
    )?;
    connection.execute(
        r#"
        UPDATE recording_sessions
        SET
            microphone_audio_path = COALESCE(
                microphone_audio_path,
                CASE
                    WHEN audio_path IS NOT NULL
                        AND audio_path != ''
                        AND audio_mode != 'source'
                    THEN audio_path
                    ELSE NULL
                END
            ),
            source_audio_path = COALESCE(
                source_audio_path,
                CASE
                    WHEN audio_path IS NOT NULL
                        AND audio_path != ''
                        AND audio_mode = 'source'
                    THEN audio_path
                    ELSE NULL
                END
            )
        "#,
        [],
    )?;
    connection.execute(
        r#"
        UPDATE recording_sessions
        SET
            microphone_audio_byte_count = CASE
                WHEN microphone_audio_path IS NOT NULL
                    AND microphone_audio_byte_count = 0
                THEN audio_byte_count
                ELSE microphone_audio_byte_count
            END,
            microphone_audio_sample_rate = COALESCE(microphone_audio_sample_rate, audio_sample_rate),
            microphone_audio_channels = COALESCE(microphone_audio_channels, audio_channels),
            microphone_audio_sample_format = COALESCE(microphone_audio_sample_format, audio_sample_format),
            source_audio_byte_count = CASE
                WHEN source_audio_path IS NOT NULL
                    AND source_audio_byte_count = 0
                THEN audio_byte_count
                ELSE source_audio_byte_count
            END,
            source_audio_sample_rate = CASE
                WHEN source_audio_path IS NOT NULL
                THEN COALESCE(source_audio_sample_rate, audio_sample_rate)
                ELSE source_audio_sample_rate
            END,
            source_audio_channels = CASE
                WHEN source_audio_path IS NOT NULL
                THEN COALESCE(source_audio_channels, audio_channels)
                ELSE source_audio_channels
            END,
            source_audio_sample_format = CASE
                WHEN source_audio_path IS NOT NULL
                THEN COALESCE(source_audio_sample_format, audio_sample_format)
                ELSE source_audio_sample_format
            END
        "#,
        [],
    )?;
    connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    alter_sql: &str,
) -> Result<(), StorageError> {
    if !column_exists(connection, table, column)? {
        connection.execute_batch(alter_sql)?;
    }

    Ok(())
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let columns = collect_rows(rows)?;
    Ok(columns.iter().any(|name| name == column))
}

fn list_tables(connection: &Connection) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(
        r#"
        SELECT name
        FROM sqlite_master
        WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
        ORDER BY name ASC
        "#,
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let tables = collect_rows(rows)?;
    Ok(tables)
}

fn table_count(connection: &Connection, table: &str) -> Result<i64, StorageError> {
    let query = format!("SELECT COUNT(*) FROM {table}");
    connection
        .query_row(&query, [], |row| row.get(0))
        .map_err(StorageError::from)
}

fn clear_transcript_search_index(
    connection: &Connection,
    transcript_id: &str,
) -> Result<(), StorageError> {
    connection.execute(
        r#"
        DELETE FROM transcript_segment_search
        WHERE rowid IN (
            SELECT rowid FROM transcript_segments WHERE transcript_id = ?1
        )
        "#,
        params![transcript_id],
    )?;
    Ok(())
}

fn index_transcript_segments(
    connection: &Connection,
    transcript_id: &str,
) -> Result<i64, StorageError> {
    connection
        .execute(
            r#"
            INSERT INTO transcript_segment_search(rowid, segment_id, text)
            SELECT rowid, id, text
            FROM transcript_segments
            WHERE transcript_id = ?1
            ORDER BY segment_index ASC
            "#,
            params![transcript_id],
        )
        .map(|count| count as i64)
        .map_err(StorageError::from)
}

fn index_all_completed_transcript_segments(connection: &Connection) -> Result<i64, StorageError> {
    connection
        .execute(
            r#"
            INSERT INTO transcript_segment_search(rowid, segment_id, text)
            SELECT transcript_segments.rowid, transcript_segments.id, transcript_segments.text
            FROM transcript_segments
            JOIN transcripts ON transcripts.id = transcript_segments.transcript_id
            JOIN recordings ON recordings.id = transcript_segments.recording_id
            WHERE transcripts.status = ?1
            ORDER BY recordings.created_at DESC, transcript_segments.segment_index ASC
            "#,
            params![TranscriptStatus::Completed.as_str()],
        )
        .map(|count| count as i64)
        .map_err(StorageError::from)
}

fn normalize_transcript_search_query(query: &str) -> Option<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for character in query.chars() {
        if character.is_alphanumeric() || character == '_' {
            current.push(character.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(current);
            current = String::new();
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    if tokens.is_empty() {
        return None;
    }

    Some(
        tokens
            .into_iter()
            .map(|token| format!("\"{token}\""))
            .collect::<Vec<_>>()
            .join(" AND "),
    )
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>, StorageError> {
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(values)
}

fn map_recording_row(row: &Row<'_>) -> rusqlite::Result<Recording> {
    Ok(Recording {
        id: row.get(0)?,
        title: row.get(1)?,
        status: enum_from_column(row, 2, "RecordingStatus", RecordingStatus::from_db)?,
        recording_directory: row.get(3)?,
        media_path: row.get(4)?,
        thumbnail_path: row.get(5)?,
        duration_ms: row.get(6)?,
        captured_at: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        completed_at: row.get(10)?,
        failure_message: row.get(11)?,
    })
}

fn map_recording_session_row(row: &Row<'_>) -> rusqlite::Result<RecordingSession> {
    let legacy_audio_path = row.get::<_, Option<String>>(5)?;
    let video_source_id = row.get::<_, String>(7)?;
    let screen_source_id = row.get::<_, String>(8)?;
    let include_microphone: i64 = row.get(15)?;
    let audio_mode = enum_from_column(row, 16, "CaptureAudioMode", CaptureAudioMode::from_db)?;
    let microphone_audio_path = row.get::<_, Option<String>>(17)?.or_else(|| {
        legacy_audio_path
            .as_ref()
            .filter(|_| audio_mode != CaptureAudioMode::Source)
            .cloned()
    });
    let source_audio_path = row.get::<_, Option<String>>(18)?.or_else(|| {
        legacy_audio_path
            .as_ref()
            .filter(|path| audio_mode == CaptureAudioMode::Source && !path.is_empty())
            .cloned()
    });
    let legacy_audio_byte_count = row.get(23)?;
    let legacy_audio_sample_rate = row.get(24)?;
    let legacy_audio_channels = row.get(25)?;
    let legacy_audio_sample_format = row.get::<_, Option<String>>(26)?;
    let microphone_audio_byte_count = fallback_stream_byte_count(
        row.get(27)?,
        microphone_audio_path.as_deref(),
        legacy_audio_byte_count,
    );
    let source_audio_byte_count = fallback_stream_byte_count(
        row.get(31)?,
        source_audio_path.as_deref(),
        legacy_audio_byte_count,
    );
    let microphone_audio_sample_rate =
        row.get::<_, Option<i64>>(28)?
            .or_else(|| match microphone_audio_path.as_deref() {
                Some(_) => legacy_audio_sample_rate,
                None => None,
            });
    let microphone_audio_channels =
        row.get::<_, Option<i64>>(29)?
            .or_else(|| match microphone_audio_path.as_deref() {
                Some(_) => legacy_audio_channels,
                None => None,
            });
    let microphone_audio_sample_format =
        row.get::<_, Option<String>>(30)?
            .or_else(|| match microphone_audio_path.as_deref() {
                Some(_) => legacy_audio_sample_format.clone(),
                None => None,
            });
    let source_audio_sample_rate =
        row.get::<_, Option<i64>>(32)?
            .or_else(|| match source_audio_path.as_deref() {
                Some(_) => legacy_audio_sample_rate,
                None => None,
            });
    let source_audio_channels = row
        .get::<_, Option<i64>>(33)?
        .or_else(|| match source_audio_path.as_deref() {
            Some(_) => legacy_audio_channels,
            None => None,
        });
    let source_audio_sample_format =
        row.get::<_, Option<String>>(34)?
            .or_else(|| match source_audio_path.as_deref() {
                Some(_) => legacy_audio_sample_format.clone(),
                None => None,
            });
    let audio_path = legacy_audio_path.or_else(|| microphone_audio_path.clone());

    Ok(RecordingSession {
        id: row.get(0)?,
        recording_id: row.get(1)?,
        status: enum_from_column(
            row,
            2,
            "RecordingSessionStatus",
            RecordingSessionStatus::from_db,
        )?,
        temp_directory: row.get(3)?,
        video_path: row.get(4)?,
        audio_path,
        metadata_path: row.get(6)?,
        video_source_id: if video_source_id.is_empty() {
            screen_source_id.clone()
        } else {
            video_source_id
        },
        screen_source_id,
        video_source_kind: row.get(9)?,
        video_source_title: row.get(10)?,
        video_source_app_name: row.get(11)?,
        video_source_process_id: row.get(12)?,
        video_source_window_id: row.get(13)?,
        microphone_device_id: row.get(14)?,
        include_microphone: include_microphone == 1,
        audio_mode,
        microphone_audio_path,
        source_audio_path,
        width: row.get(19)?,
        height: row.get(20)?,
        frame_rate: row.get(21)?,
        frame_count: row.get(22)?,
        audio_byte_count: legacy_audio_byte_count,
        audio_sample_rate: legacy_audio_sample_rate,
        audio_channels: legacy_audio_channels,
        audio_sample_format: legacy_audio_sample_format,
        microphone_audio_byte_count,
        microphone_audio_sample_rate,
        microphone_audio_channels,
        microphone_audio_sample_format,
        source_audio_byte_count,
        source_audio_sample_rate,
        source_audio_channels,
        source_audio_sample_format,
        started_at: row.get(35)?,
        stopped_at: row.get(36)?,
        duration_ms: row.get(37)?,
        failure_message: row.get(38)?,
        created_at: row.get(39)?,
        updated_at: row.get(40)?,
    })
}

fn fallback_stream_byte_count(
    stream_byte_count: i64,
    stream_path: Option<&str>,
    legacy_byte_count: i64,
) -> i64 {
    if stream_byte_count == 0 && stream_path.is_some() && legacy_byte_count > 0 {
        legacy_byte_count
    } else {
        stream_byte_count
    }
}

fn map_transcript_row(row: &Row<'_>) -> rusqlite::Result<Transcript> {
    Ok(Transcript {
        id: row.get(0)?,
        recording_id: row.get(1)?,
        status: enum_from_column(row, 2, "TranscriptStatus", TranscriptStatus::from_db)?,
        language: row.get(3)?,
        model_name: row.get(4)?,
        raw_json_path: row.get(5)?,
        text: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        completed_at: row.get(9)?,
        failure_message: row.get(10)?,
    })
}

fn map_segment_row(row: &Row<'_>) -> rusqlite::Result<TranscriptSegment> {
    Ok(TranscriptSegment {
        id: row.get(0)?,
        transcript_id: row.get(1)?,
        recording_id: row.get(2)?,
        segment_index: row.get(3)?,
        start_ms: row.get(4)?,
        end_ms: row.get(5)?,
        text: row.get(6)?,
        confidence: row.get(7)?,
    })
}

fn map_transcript_search_result_row(row: &Row<'_>) -> rusqlite::Result<TranscriptSearchResult> {
    Ok(TranscriptSearchResult {
        recording_id: row.get(0)?,
        recording_title: row.get(1)?,
        transcript_id: row.get(2)?,
        segment_id: row.get(3)?,
        segment_index: row.get(4)?,
        start_ms: row.get(5)?,
        end_ms: row.get(6)?,
        text: row.get(7)?,
        snippet: row.get(8)?,
        rank: row.get(9)?,
        media_path: row.get(10)?,
        thumbnail_path: row.get(11)?,
        captured_at: row.get(12)?,
        created_at: row.get(13)?,
    })
}

fn map_ai_summary_row(row: &Row<'_>) -> rusqlite::Result<AiSummary> {
    Ok(AiSummary {
        id: row.get(0)?,
        recording_id: row.get(1)?,
        status: enum_from_column(row, 2, "AiStatus", AiStatus::from_db)?,
        model_name: row.get(3)?,
        summary_text: row.get(4)?,
        action_items_json: row.get(5)?,
        decisions_json: row.get(6)?,
        questions_json: row.get(7)?,
        risks_json: row.get(8)?,
        chapters_json: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        completed_at: row.get(12)?,
        failure_message: row.get(13)?,
    })
}

fn map_ai_settings_row(row: &Row<'_>) -> rusqlite::Result<AiSettingsRecord> {
    let enabled: i64 = row.get(0)?;

    Ok(AiSettingsRecord {
        enabled: enabled == 1,
        provider: row.get(1)?,
        model_name: row.get(2)?,
        endpoint_url: row.get(3)?,
        api_key: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn map_processing_job_row(row: &Row<'_>) -> rusqlite::Result<ProcessingJob> {
    let interrupted: i64 = row.get(10)?;

    Ok(ProcessingJob {
        id: row.get(0)?,
        recording_id: row.get(1)?,
        kind: row.get(2)?,
        state: enum_from_column(row, 3, "JobState", JobState::from_db)?,
        priority: row.get(4)?,
        attempts: row.get(5)?,
        max_attempts: row.get(6)?,
        input_json: row.get(7)?,
        output_json: row.get(8)?,
        error_message: row.get(9)?,
        interrupted: interrupted == 1,
        last_error_at: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        started_at: row.get(14)?,
        completed_at: row.get(15)?,
    })
}

fn map_capture_selection_row(row: &Row<'_>) -> rusqlite::Result<CaptureSelection> {
    let audio_mode = enum_from_column(row, 3, "CaptureAudioMode", CaptureAudioMode::from_db)?;

    Ok(CaptureSelection {
        video_source_id: row.get(0)?,
        screen_source_id: row.get(1)?,
        microphone_device_id: row.get(2)?,
        include_microphone: audio_mode.includes_microphone(),
        audio_mode,
        updated_at: row.get(5)?,
    })
}

fn enum_from_column<T>(
    row: &Row<'_>,
    column_index: usize,
    enum_name: &'static str,
    parser: fn(&str) -> Option<T>,
) -> rusqlite::Result<T> {
    let value = row.get::<_, String>(column_index)?;
    parser(&value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            column_index,
            Type::Text,
            Box::new(InvalidEnumValue { enum_name, value }),
        )
    })
}

fn normalize_title(title: Option<String>) -> String {
    title
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Untitled recording".to_owned())
}

fn now_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn default_capture_selection() -> CaptureSelection {
    CaptureSelection {
        video_source_id: None,
        screen_source_id: None,
        microphone_device_id: None,
        audio_mode: CaptureAudioMode::Microphone,
        include_microphone: true,
        updated_at: None,
    }
}

fn default_ai_settings_record() -> AiSettingsRecord {
    AiSettingsRecord {
        enabled: false,
        provider: DEFAULT_AI_PROVIDER.to_owned(),
        model_name: DEFAULT_AI_MODEL_NAME.to_owned(),
        endpoint_url: DEFAULT_AI_ENDPOINT_URL.to_owned(),
        api_key: None,
        updated_at: None,
    }
}

fn normalized_setting(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn validate_ai_settings(
    enabled: bool,
    provider: &str,
    model_name: &str,
    endpoint_url: &str,
    api_key: Option<&str>,
) -> Result<(), StorageError> {
    if provider != "openai_compatible" {
        return Err(StorageError::InvalidInput(
            "Optional AI currently supports OpenAI-compatible chat completion providers."
                .to_owned(),
        ));
    }

    if !endpoint_url.starts_with("https://") && !endpoint_url.starts_with("http://") {
        return Err(StorageError::InvalidInput(
            "Optional AI endpoint must start with http:// or https://.".to_owned(),
        ));
    }

    if enabled && model_name.is_empty() {
        return Err(StorageError::InvalidInput(
            "Optional AI requires a model name before it can be enabled.".to_owned(),
        ));
    }

    if enabled && api_key.is_none() {
        return Err(StorageError::InvalidInput(
            "Optional AI requires an API key before it can be enabled.".to_owned(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_required_layout_and_schema_idempotently() {
        let (state, root) = test_state();

        assert!(state.paths.database_file.exists());
        assert!(state.paths.recordings_directory.is_dir());
        assert!(state.paths.whisper_models_directory.is_dir());
        assert!(state.paths.temp_directory.is_dir());

        let second_state = initialize_at(root.clone()).expect("second initialization");
        let overview = second_state.overview().expect("storage overview");

        assert_eq!(overview.schema_version, SCHEMA_VERSION);
        assert!(overview.tables.contains(&"recordings".to_owned()));
        assert!(overview.tables.contains(&"transcripts".to_owned()));
        assert!(overview.tables.contains(&"transcript_segments".to_owned()));
        assert!(overview.tables.contains(&"ai_summaries".to_owned()));
        assert!(overview.tables.contains(&"ai_settings".to_owned()));
        assert!(overview.tables.contains(&"processing_jobs".to_owned()));
        assert!(overview.tables.contains(&"capture_preferences".to_owned()));

        cleanup(root);
    }

    #[test]
    fn creates_updates_lists_and_loads_recordings() {
        let (state, root) = test_state();
        let recording = state
            .create_recording(CreateRecordingInput {
                title: Some("  Coaching review  ".to_owned()),
                captured_at: Some("2026-07-01T12:00:00Z".to_owned()),
                media_path: None,
            })
            .expect("create recording");

        assert_eq!(recording.title, "Coaching review");
        assert_eq!(recording.status, RecordingStatus::Pending);
        assert!(state
            .paths
            .recordings_directory
            .join(&recording.id)
            .is_dir());

        let updated = state
            .update_recording(UpdateRecordingInput {
                id: recording.id.clone(),
                title: None,
                status: Some(RecordingStatus::Completed),
                media_path: Some(format!("recordings/{}/recording.mp4", recording.id)),
                thumbnail_path: None,
                duration_ms: Some(42_000),
                captured_at: None,
                completed_at: Some("2026-07-01T12:01:00Z".to_owned()),
                failure_message: None,
            })
            .expect("update recording");

        assert_eq!(updated.status, RecordingStatus::Completed);
        assert_eq!(updated.duration_ms, Some(42_000));

        let loaded = state
            .get_recording(&recording.id)
            .expect("load recording")
            .expect("recording exists");
        assert_eq!(loaded.id, recording.id);

        let recordings = state.list_recordings().expect("list recordings");
        assert_eq!(recordings.len(), 1);

        cleanup(root);
    }

    #[test]
    fn persists_transcripts_and_segments_by_recording() {
        let (state, root) = test_state();
        let recording = state
            .create_recording(CreateRecordingInput {
                title: Some("Transcript source".to_owned()),
                captured_at: None,
                media_path: None,
            })
            .expect("create recording");

        let transcript = state
            .persist_transcript(PersistTranscriptInput {
                recording_id: recording.id.clone(),
                status: TranscriptStatus::Completed,
                language: Some("en".to_owned()),
                model_name: Some("whisper-small".to_owned()),
                raw_json_path: Some(format!("recordings/{}/transcript.raw.json", recording.id)),
                text: Some("Hello world. Back to review.".to_owned()),
                completed_at: Some("2026-07-01T12:02:00Z".to_owned()),
                failure_message: None,
                segments: vec![
                    TranscriptSegmentInput {
                        segment_index: None,
                        start_ms: 0,
                        end_ms: 1_250,
                        text: "Hello world.".to_owned(),
                        confidence: Some(0.92),
                    },
                    TranscriptSegmentInput {
                        segment_index: None,
                        start_ms: 1_250,
                        end_ms: 2_500,
                        text: "Back to review.".to_owned(),
                        confidence: None,
                    },
                ],
            })
            .expect("persist transcript");

        assert_eq!(transcript.transcript.recording_id, recording.id);
        assert_eq!(transcript.transcript.status, TranscriptStatus::Completed);
        assert_eq!(transcript.segments.len(), 2);
        assert_eq!(transcript.segments[1].segment_index, 1);
        assert_eq!(transcript.segments[0].confidence, Some(0.92));

        cleanup(root);
    }

    #[test]
    fn searches_completed_transcript_segments_and_reindexes() {
        let (state, root) = test_state();
        let recording = state
            .create_recording(CreateRecordingInput {
                title: Some("Coaching VOD review".to_owned()),
                captured_at: Some("2026-07-01T12:00:00Z".to_owned()),
                media_path: Some("recordings/review/recording.mp4".to_owned()),
            })
            .expect("create recording");

        let transcript = state
            .persist_transcript(PersistTranscriptInput {
                recording_id: recording.id.clone(),
                status: TranscriptStatus::Completed,
                language: Some("en".to_owned()),
                model_name: Some("small.en".to_owned()),
                raw_json_path: None,
                text: Some("First review marker. Rotations and decision making.".to_owned()),
                completed_at: Some("2026-07-01T12:02:00Z".to_owned()),
                failure_message: None,
                segments: vec![
                    TranscriptSegmentInput {
                        segment_index: None,
                        start_ms: 1_000,
                        end_ms: 2_000,
                        text: "First review marker.".to_owned(),
                        confidence: Some(0.88),
                    },
                    TranscriptSegmentInput {
                        segment_index: None,
                        start_ms: 5_000,
                        end_ms: 8_000,
                        text: "Rotations and decision making.".to_owned(),
                        confidence: Some(0.91),
                    },
                ],
            })
            .expect("persist transcript");

        let results = state
            .search_transcripts(SearchTranscriptsInput {
                query: "decision".to_owned(),
                limit: None,
            })
            .expect("search transcripts");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].recording_id, recording.id);
        assert_eq!(results[0].recording_title, "Coaching VOD review");
        assert_eq!(results[0].transcript_id, transcript.transcript.id);
        assert_eq!(results[0].start_ms, 5_000);
        assert!(results[0].snippet.contains("[decision]"));

        state
            .persist_transcript(PersistTranscriptInput {
                recording_id: recording.id.clone(),
                status: TranscriptStatus::Failed,
                language: None,
                model_name: Some("small.en".to_owned()),
                raw_json_path: None,
                text: None,
                completed_at: None,
                failure_message: Some("transcription failed".to_owned()),
                segments: Vec::new(),
            })
            .expect("replace with failed transcript");

        let stale_results = state
            .search_transcripts(SearchTranscriptsInput {
                query: "decision".to_owned(),
                limit: None,
            })
            .expect("search after failed transcript");
        assert!(stale_results.is_empty());

        state
            .persist_transcript(PersistTranscriptInput {
                recording_id: recording.id.clone(),
                status: TranscriptStatus::Completed,
                language: Some("en".to_owned()),
                model_name: Some("small.en".to_owned()),
                raw_json_path: None,
                text: Some("Decision review restored.".to_owned()),
                completed_at: Some("2026-07-01T12:04:00Z".to_owned()),
                failure_message: None,
                segments: vec![TranscriptSegmentInput {
                    segment_index: None,
                    start_ms: 9_000,
                    end_ms: 10_000,
                    text: "Decision review restored.".to_owned(),
                    confidence: Some(0.95),
                }],
            })
            .expect("restore completed transcript");

        let summary = state
            .reindex_transcript_search()
            .expect("reindex transcript search");
        assert_eq!(summary.indexed_segment_count, 1);

        let rebuilt_results = state
            .search_transcripts(SearchTranscriptsInput {
                query: "restored".to_owned(),
                limit: Some(10),
            })
            .expect("search rebuilt index");
        assert_eq!(rebuilt_results.len(), 1);
        assert_eq!(rebuilt_results[0].start_ms, 9_000);

        cleanup(root);
    }

    #[test]
    fn keeps_optional_ai_disabled_until_configured() {
        let (state, root) = test_state();
        let defaults = state.get_ai_settings().expect("default ai settings");

        assert!(!defaults.enabled);
        assert_eq!(defaults.provider, DEFAULT_AI_PROVIDER);
        assert_eq!(defaults.endpoint_url, DEFAULT_AI_ENDPOINT_URL);
        assert!(!defaults.has_api_key);
        assert!(defaults.updated_at.is_none());

        let missing_key = state.save_ai_settings(SaveAiSettingsInput {
            enabled: true,
            provider: Some(DEFAULT_AI_PROVIDER.to_owned()),
            model_name: Some("summary-model".to_owned()),
            endpoint_url: Some(DEFAULT_AI_ENDPOINT_URL.to_owned()),
            api_key: None,
            clear_api_key: None,
        });
        assert!(missing_key.is_err());

        let saved = state
            .save_ai_settings(SaveAiSettingsInput {
                enabled: true,
                provider: Some(DEFAULT_AI_PROVIDER.to_owned()),
                model_name: Some("summary-model".to_owned()),
                endpoint_url: Some(DEFAULT_AI_ENDPOINT_URL.to_owned()),
                api_key: Some("local-test-key".to_owned()),
                clear_api_key: None,
            })
            .expect("save ai settings");

        assert!(saved.enabled);
        assert_eq!(saved.model_name, "summary-model");
        assert!(saved.has_api_key);
        assert!(saved.updated_at.is_some());

        let reinitialized = initialize_at(root.clone()).expect("reinitialize storage");
        let loaded = reinitialized
            .get_ai_settings()
            .expect("load persisted ai settings");

        assert!(loaded.enabled);
        assert_eq!(loaded.model_name, "summary-model");
        assert!(loaded.has_api_key);

        cleanup(root);
    }

    #[test]
    fn processing_jobs_survive_reinitialization() {
        let (state, root) = test_state();
        let recording = state
            .create_recording(CreateRecordingInput {
                title: Some("Job source".to_owned()),
                captured_at: None,
                media_path: None,
            })
            .expect("create recording");
        let job = state
            .create_processing_job(CreateProcessingJobInput {
                recording_id: Some(recording.id),
                kind: "transcribe_recording".to_owned(),
                priority: Some(10),
                input_json: Some(r#"{"model":"whisper-small"}"#.to_owned()),
                max_attempts: Some(5),
            })
            .expect("create job");

        state
            .update_processing_job(UpdateProcessingJobInput {
                id: job.id.clone(),
                state: Some(JobState::Running),
                attempts: Some(1),
                output_json: None,
                error_message: None,
                interrupted: None,
                started_at: None,
                completed_at: None,
            })
            .expect("start job");

        let reinitialized = initialize_at(root.clone()).expect("reinitialize storage");
        let jobs = reinitialized
            .list_processing_jobs(Some(JobState::Running), None)
            .expect("list running jobs");

        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, job.id);
        assert_eq!(jobs[0].attempts, 1);
        assert!(jobs[0].started_at.is_some());

        cleanup(root);
    }

    #[test]
    fn persists_capture_selection() {
        let (state, root) = test_state();
        let default_selection = state
            .get_capture_selection()
            .expect("default capture selection");

        assert!(default_selection.include_microphone);
        assert_eq!(default_selection.audio_mode, CaptureAudioMode::Microphone);
        assert!(default_selection.video_source_id.is_none());
        assert!(default_selection.screen_source_id.is_none());
        assert!(default_selection.microphone_device_id.is_none());
        assert!(default_selection.updated_at.is_none());

        let saved = state
            .save_capture_selection(SaveCaptureSelectionInput {
                video_source_id: Some("display:42".to_owned()),
                screen_source_id: Some("display:42".to_owned()),
                microphone_device_id: Some("coreaudio:default-input".to_owned()),
                audio_mode: Some(CaptureAudioMode::Microphone),
                include_microphone: Some(true),
            })
            .expect("save capture selection");

        assert_eq!(saved.video_source_id.as_deref(), Some("display:42"));
        assert_eq!(saved.screen_source_id.as_deref(), Some("display:42"));
        assert_eq!(
            saved.microphone_device_id.as_deref(),
            Some("coreaudio:default-input")
        );
        assert_eq!(saved.audio_mode, CaptureAudioMode::Microphone);
        assert!(saved.include_microphone);
        assert!(saved.updated_at.is_some());

        let reinitialized = initialize_at(root.clone()).expect("reinitialize storage");
        let loaded = reinitialized
            .get_capture_selection()
            .expect("load capture selection");

        assert_eq!(loaded.video_source_id, saved.video_source_id);
        assert_eq!(loaded.screen_source_id, saved.screen_source_id);
        assert_eq!(loaded.microphone_device_id, saved.microphone_device_id);

        cleanup(root);
    }

    #[test]
    fn reads_legacy_screen_source_selection_as_video_source() {
        let (state, root) = test_state();
        let connection = state.open_connection().expect("open connection");

        connection
            .execute(
                r#"
                INSERT INTO capture_preferences (
                    id, screen_source_id, microphone_device_id, include_microphone, updated_at
                ) VALUES (1, ?1, ?2, 1, ?3)
                "#,
                params!["display:7", "coreaudio:legacy-input", "1234"],
            )
            .expect("insert legacy capture preference");

        let loaded = state
            .get_capture_selection()
            .expect("load legacy capture selection");

        assert_eq!(loaded.video_source_id.as_deref(), Some("display:7"));
        assert_eq!(loaded.screen_source_id.as_deref(), Some("display:7"));
        assert_eq!(
            loaded.microphone_device_id.as_deref(),
            Some("coreaudio:legacy-input")
        );
        assert_eq!(loaded.audio_mode, CaptureAudioMode::Microphone);
        assert!(loaded.include_microphone);

        cleanup(root);
    }

    #[test]
    fn persists_source_audio_mode_without_overloading_microphone() {
        let (state, root) = test_state();
        let saved = state
            .save_capture_selection(SaveCaptureSelectionInput {
                video_source_id: Some("window:9".to_owned()),
                screen_source_id: None,
                microphone_device_id: None,
                audio_mode: Some(CaptureAudioMode::Source),
                include_microphone: Some(true),
            })
            .expect("save source audio selection");

        assert_eq!(saved.video_source_id.as_deref(), Some("window:9"));
        assert_eq!(saved.screen_source_id.as_deref(), Some("window:9"));
        assert_eq!(saved.audio_mode, CaptureAudioMode::Source);
        assert!(!saved.include_microphone);

        cleanup(root);
    }

    #[test]
    fn prepares_separate_audio_files_for_each_audio_mode() {
        let (state, root) = test_state();

        let none = state
            .prepare_recording_session_files("session-none", &CaptureAudioMode::None)
            .expect("prepare no-audio files");
        assert!(none.audio_path_relative.is_none());
        assert!(none.microphone_audio_path_relative.is_none());
        assert!(none.source_audio_path_relative.is_none());

        let microphone = state
            .prepare_recording_session_files("session-mic", &CaptureAudioMode::Microphone)
            .expect("prepare microphone files");
        assert_eq!(
            microphone.audio_path_relative,
            microphone.microphone_audio_path_relative
        );
        assert!(microphone
            .microphone_audio_path_relative
            .as_deref()
            .is_some_and(|path| path.ends_with("/microphone.pcm")));
        assert!(microphone.source_audio_path_relative.is_none());

        let source = state
            .prepare_recording_session_files("session-source", &CaptureAudioMode::Source)
            .expect("prepare source-audio files");
        assert!(source.audio_path_relative.is_none());
        assert!(source.microphone_audio_path_relative.is_none());
        assert!(source
            .source_audio_path_relative
            .as_deref()
            .is_some_and(|path| path.ends_with("/source_audio.pcm")));

        let combined = state
            .prepare_recording_session_files("session-both", &CaptureAudioMode::MicrophoneAndSource)
            .expect("prepare combined audio files");
        assert!(combined
            .microphone_audio_path_relative
            .as_deref()
            .is_some_and(|path| path.ends_with("/microphone.pcm")));
        assert!(combined
            .source_audio_path_relative
            .as_deref()
            .is_some_and(|path| path.ends_with("/source_audio.pcm")));

        cleanup(root);
    }

    #[test]
    fn persists_split_recording_session_audio_metadata() {
        let (state, root) = test_state();
        let recording = state
            .create_recording(CreateRecordingInput {
                title: Some("Split audio".to_owned()),
                captured_at: Some("1234".to_owned()),
                media_path: None,
            })
            .expect("create recording");
        let files = state
            .prepare_recording_session_files(
                "split-session",
                &CaptureAudioMode::MicrophoneAndSource,
            )
            .expect("prepare split session files");

        let session = state
            .create_recording_session(CreateRecordingSessionInput {
                id: "split-session".to_owned(),
                recording_id: recording.id.clone(),
                temp_directory: files.temp_directory_relative,
                video_path: files.video_path_relative,
                audio_path: files.audio_path_relative.clone(),
                metadata_path: files.metadata_path_relative,
                video_source_id: "window:42".to_owned(),
                screen_source_id: "window:42".to_owned(),
                video_source_kind: "window".to_owned(),
                video_source_title: "Match window".to_owned(),
                video_source_app_name: Some("Metafy".to_owned()),
                video_source_process_id: Some(123),
                video_source_window_id: Some(42),
                microphone_device_id: Some("mic:1".to_owned()),
                include_microphone: true,
                audio_mode: CaptureAudioMode::MicrophoneAndSource,
                microphone_audio_path: files.microphone_audio_path_relative.clone(),
                source_audio_path: files.source_audio_path_relative.clone(),
                width: Some(1280),
                height: Some(720),
                frame_rate: 30,
                audio_sample_rate: Some(48_000),
                audio_channels: Some(2),
                audio_sample_format: Some("f32".to_owned()),
                microphone_audio_sample_rate: Some(48_000),
                microphone_audio_channels: Some(2),
                microphone_audio_sample_format: Some("f32".to_owned()),
                source_audio_sample_rate: Some(48_000),
                source_audio_channels: Some(2),
                source_audio_sample_format: Some("f32".to_owned()),
                started_at: "1234".to_owned(),
            })
            .expect("create split session");

        let finished = state
            .finish_recording_session(FinishRecordingSessionInput {
                id: session.id,
                status: RecordingSessionStatus::Stopped,
                width: Some(1280),
                height: Some(720),
                frame_count: 10,
                audio_byte_count: 120,
                audio_sample_rate: Some(48_000),
                audio_channels: Some(2),
                audio_sample_format: Some("f32".to_owned()),
                microphone_audio_byte_count: 120,
                microphone_audio_sample_rate: Some(48_000),
                microphone_audio_channels: Some(2),
                microphone_audio_sample_format: Some("f32".to_owned()),
                source_audio_byte_count: 240,
                source_audio_sample_rate: Some(48_000),
                source_audio_channels: Some(2),
                source_audio_sample_format: Some("f32".to_owned()),
                stopped_at: "1235".to_owned(),
                duration_ms: 1_000,
                failure_message: None,
            })
            .expect("finish split session");

        assert_eq!(finished.video_source_id, "window:42");
        assert_eq!(finished.audio_mode, CaptureAudioMode::MicrophoneAndSource);
        assert_eq!(finished.audio_path, files.audio_path_relative);
        assert_eq!(
            finished.microphone_audio_path,
            files.microphone_audio_path_relative
        );
        assert_eq!(finished.source_audio_path, files.source_audio_path_relative);
        assert_eq!(finished.audio_byte_count, 120);
        assert_eq!(finished.microphone_audio_byte_count, 120);
        assert_eq!(finished.source_audio_byte_count, 240);

        cleanup(root);
    }

    #[test]
    fn reads_legacy_audio_path_as_microphone_audio() {
        let (state, root) = test_state();
        let recording = state
            .create_recording(CreateRecordingInput {
                title: Some("Legacy audio".to_owned()),
                captured_at: Some("1234".to_owned()),
                media_path: None,
            })
            .expect("create recording");
        let connection = state.open_connection().expect("open connection");

        connection
            .execute(
                r#"
                INSERT INTO recording_sessions (
                    id, recording_id, status, temp_directory, video_path, audio_path,
                    metadata_path, screen_source_id, microphone_device_id, include_microphone,
                    width, height, frame_rate, frame_count, audio_byte_count,
                    audio_sample_rate, audio_channels, audio_sample_format,
                    started_at, created_at, updated_at
                ) VALUES (
                    ?1, ?2, 'stopped', ?3, ?4, ?5,
                    ?6, ?7, ?8, 1,
                    640, 480, 30, 5, 96,
                    48000, 2, 'f32',
                    '1234', '1234', '1235'
                )
                "#,
                params![
                    "legacy-session",
                    recording.id,
                    "temp/recording-sessions/legacy-session",
                    "temp/recording-sessions/legacy-session/screen_frames.mfrv",
                    "temp/recording-sessions/legacy-session/audio.pcm",
                    "temp/recording-sessions/legacy-session/session.json",
                    "display:7",
                    "mic:legacy",
                ],
            )
            .expect("insert legacy session row");

        let loaded = state
            .get_recording_session("legacy-session")
            .expect("load legacy session")
            .expect("legacy session");

        assert_eq!(loaded.video_source_id, "display:7");
        assert_eq!(loaded.audio_mode, CaptureAudioMode::Microphone);
        assert_eq!(
            loaded.microphone_audio_path.as_deref(),
            Some("temp/recording-sessions/legacy-session/audio.pcm")
        );
        assert!(loaded.source_audio_path.is_none());
        assert_eq!(loaded.microphone_audio_byte_count, 96);
        assert_eq!(loaded.microphone_audio_sample_rate, Some(48_000));
        assert_eq!(loaded.microphone_audio_channels, Some(2));
        assert_eq!(
            loaded.microphone_audio_sample_format.as_deref(),
            Some("f32")
        );

        cleanup(root);
    }

    fn test_state() -> (StorageState, PathBuf) {
        let root = std::env::temp_dir().join(format!("metafy-storage-test-{}", Uuid::new_v4()));
        let state = initialize_at(root.clone()).expect("initialize test storage");
        (state, root)
    }

    fn cleanup(root: PathBuf) {
        let _ = fs::remove_dir_all(root);
    }
}
