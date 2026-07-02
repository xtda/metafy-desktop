use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::capture::{CaptureStatus, ValidatedCaptureConfig};
use crate::config::{local_only_defaults, LocalOnlyDefaults};
use crate::jobs::{
    CleanupProcessingFilesInput, CleanupTempFilesResult, JobRecoverySummary,
    RetryProcessingJobInput,
};
use crate::recorder::RecordingRuntime;
use crate::storage::{
    AiSettings, AiSummary, CaptureSelection, CreateProcessingJobInput, CreateRecordingInput,
    CreateRecordingSessionInput, FinishRecordingSessionInput, JobState, PersistTranscriptInput,
    ProcessingJob, Recording, RecordingAssetPaths, RecordingSession, RecordingSessionStatus,
    SaveAiSettingsInput, SaveCaptureSelectionInput, SearchTranscriptsInput, StorageOverview,
    StorageState, TranscriptSearchIndexSummary, TranscriptSearchResult, TranscriptWithSegments,
    UpdateProcessingJobInput, UpdateRecordingInput, UpsertAiSummaryInput,
};
use crate::transcription::{ImportWhisperModelInput, TranscribeRecordingInput, WhisperModelStatus};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrap {
    pub app_name: &'static str,
    pub runtime: RuntimeInfo,
    pub local_only: LocalOnlyDefaults,
    pub storage: StorageOverview,
    pub native_boundaries: Vec<NativeBoundary>,
    pub available_commands: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub shell: &'static str,
    pub native: &'static str,
    pub frontend: &'static str,
    pub package_manager: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeBoundary {
    pub domain: &'static str,
    pub owner: &'static str,
    pub status: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRecordingSessionInput {
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopRecordingSessionInput {
    pub recording_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummarizeRecordingInput {
    pub recording_id: String,
    pub user_notes: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSessionEnvelope {
    pub recording: Recording,
    pub session: RecordingSession,
}

#[tauri::command]
pub fn app_bootstrap(storage: State<'_, StorageState>) -> Result<AppBootstrap, String> {
    Ok(AppBootstrap {
        app_name: "Metafy Desktop",
        runtime: RuntimeInfo {
            shell: "Tauri 2",
            native: "Rust",
            frontend: "SvelteKit",
            package_manager: "Deno",
        },
        local_only: local_only_defaults(),
        storage: storage.overview().map_err(command_error)?,
        native_boundaries: vec![
            NativeBoundary {
                domain: "filesystem",
                owner: "Rust",
                status: "implemented",
            },
            NativeBoundary {
                domain: "capture",
                owner: "Rust",
                status: "recording-session-implemented",
            },
            NativeBoundary {
                domain: "encoding",
                owner: "Rust",
                status: "implemented-system-ffmpeg",
            },
            NativeBoundary {
                domain: "transcription",
                owner: "Rust",
                status: "implemented-local-whisper",
            },
            NativeBoundary {
                domain: "jobs",
                owner: "Rust",
                status: "implemented",
            },
            NativeBoundary {
                domain: "optional_ai",
                owner: "Rust",
                status: "implemented-transcript-only",
            },
        ],
        available_commands: vec![
            "app_bootstrap",
            "storage_overview",
            "create_recording",
            "list_recordings",
            "get_recording",
            "update_recording",
            "persist_transcript",
            "get_transcript_by_recording",
            "search_transcripts",
            "reindex_transcript_search",
            "whisper_model_status",
            "import_whisper_model",
            "transcribe_recording",
            "upsert_ai_summary",
            "get_ai_summary_by_recording",
            "get_ai_settings",
            "save_ai_settings",
            "summarize_recording",
            "create_processing_job",
            "list_processing_jobs",
            "update_processing_job",
            "recover_processing_jobs",
            "retry_processing_job",
            "cleanup_processing_files",
            "capture_status",
            "request_capture_permissions",
            "get_capture_selection",
            "save_capture_selection",
            "validate_capture_config",
            "start_recording_session",
            "stop_recording_session",
            "active_recording_session",
            "get_recording_session_by_recording",
            "encode_recording",
            "recording_asset_paths",
        ],
    })
}

#[tauri::command]
pub fn storage_overview(storage: State<'_, StorageState>) -> Result<StorageOverview, String> {
    storage.overview().map_err(command_error)
}

#[tauri::command]
pub fn create_recording(
    input: CreateRecordingInput,
    storage: State<'_, StorageState>,
) -> Result<Recording, String> {
    storage.create_recording(input).map_err(command_error)
}

#[tauri::command]
pub fn list_recordings(storage: State<'_, StorageState>) -> Result<Vec<Recording>, String> {
    storage.list_recordings().map_err(command_error)
}

#[tauri::command]
pub fn get_recording(
    id: String,
    storage: State<'_, StorageState>,
) -> Result<Option<Recording>, String> {
    storage.get_recording(&id).map_err(command_error)
}

#[tauri::command]
pub fn update_recording(
    input: UpdateRecordingInput,
    storage: State<'_, StorageState>,
) -> Result<Recording, String> {
    storage.update_recording(input).map_err(command_error)
}

#[tauri::command]
pub fn persist_transcript(
    input: PersistTranscriptInput,
    storage: State<'_, StorageState>,
) -> Result<TranscriptWithSegments, String> {
    storage.persist_transcript(input).map_err(command_error)
}

#[tauri::command]
pub fn get_transcript_by_recording(
    recording_id: String,
    storage: State<'_, StorageState>,
) -> Result<Option<TranscriptWithSegments>, String> {
    storage
        .get_transcript_by_recording(&recording_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn search_transcripts(
    input: SearchTranscriptsInput,
    storage: State<'_, StorageState>,
) -> Result<Vec<TranscriptSearchResult>, String> {
    storage.search_transcripts(input).map_err(command_error)
}

#[tauri::command]
pub fn reindex_transcript_search(
    storage: State<'_, StorageState>,
) -> Result<TranscriptSearchIndexSummary, String> {
    crate::jobs::run_reindex_transcript_search_job(&storage)
}

#[tauri::command]
pub fn whisper_model_status(
    model_name: Option<String>,
    storage: State<'_, StorageState>,
) -> Result<WhisperModelStatus, String> {
    crate::transcription::whisper_model_status(&storage, model_name.as_deref())
        .map_err(command_error)
}

#[tauri::command]
pub fn import_whisper_model(
    input: ImportWhisperModelInput,
    storage: State<'_, StorageState>,
) -> Result<WhisperModelStatus, String> {
    crate::transcription::import_whisper_model(&storage, input).map_err(command_error)
}

#[tauri::command]
pub fn transcribe_recording(
    input: TranscribeRecordingInput,
    storage: State<'_, StorageState>,
) -> Result<TranscriptWithSegments, String> {
    crate::jobs::run_transcription_job(&storage, &input.recording_id, input.model_name.as_deref())
}

#[tauri::command]
pub fn upsert_ai_summary(
    input: UpsertAiSummaryInput,
    storage: State<'_, StorageState>,
) -> Result<AiSummary, String> {
    storage.upsert_ai_summary(input).map_err(command_error)
}

#[tauri::command]
pub fn get_ai_summary_by_recording(
    recording_id: String,
    storage: State<'_, StorageState>,
) -> Result<Option<AiSummary>, String> {
    storage
        .get_ai_summary_by_recording(&recording_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn get_ai_settings(storage: State<'_, StorageState>) -> Result<AiSettings, String> {
    storage.get_ai_settings().map_err(command_error)
}

#[tauri::command]
pub fn save_ai_settings(
    input: SaveAiSettingsInput,
    storage: State<'_, StorageState>,
) -> Result<AiSettings, String> {
    storage.save_ai_settings(input).map_err(command_error)
}

#[tauri::command]
pub fn summarize_recording(
    input: SummarizeRecordingInput,
    storage: State<'_, StorageState>,
) -> Result<AiSummary, String> {
    crate::jobs::run_ai_summary_job(&storage, input.recording_id, input.user_notes)
}

#[tauri::command]
pub fn create_processing_job(
    input: CreateProcessingJobInput,
    storage: State<'_, StorageState>,
) -> Result<ProcessingJob, String> {
    storage.create_processing_job(input).map_err(command_error)
}

#[tauri::command]
pub fn list_processing_jobs(
    state: Option<JobState>,
    recording_id: Option<String>,
    storage: State<'_, StorageState>,
) -> Result<Vec<ProcessingJob>, String> {
    storage
        .list_processing_jobs(state, recording_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn update_processing_job(
    input: UpdateProcessingJobInput,
    storage: State<'_, StorageState>,
) -> Result<ProcessingJob, String> {
    storage.update_processing_job(input).map_err(command_error)
}

#[tauri::command]
pub fn recover_processing_jobs(
    storage: State<'_, StorageState>,
) -> Result<JobRecoverySummary, String> {
    let summary = crate::jobs::recover_on_startup(&storage)?;
    crate::jobs::spawn_pending_worker(storage.inner().clone());
    Ok(summary)
}

#[tauri::command]
pub fn retry_processing_job(
    input: RetryProcessingJobInput,
    storage: State<'_, StorageState>,
) -> Result<ProcessingJob, String> {
    crate::jobs::retry_processing_job(&storage, input)
}

#[tauri::command]
pub fn cleanup_processing_files(
    input: CleanupProcessingFilesInput,
    storage: State<'_, StorageState>,
) -> Result<CleanupTempFilesResult, String> {
    crate::jobs::run_cleanup_processing_files_job(&storage, input)
}

#[tauri::command]
pub fn capture_status(storage: State<'_, StorageState>) -> Result<CaptureStatus, String> {
    let selection = storage.get_capture_selection().map_err(command_error)?;
    Ok(crate::capture::capture_status(selection))
}

#[tauri::command]
pub fn request_capture_permissions(
    storage: State<'_, StorageState>,
) -> Result<CaptureStatus, String> {
    let selection = storage.get_capture_selection().map_err(command_error)?;
    Ok(crate::capture::request_capture_permissions(selection))
}

#[tauri::command]
pub fn get_capture_selection(storage: State<'_, StorageState>) -> Result<CaptureSelection, String> {
    storage.get_capture_selection().map_err(command_error)
}

#[tauri::command]
pub fn save_capture_selection(
    input: SaveCaptureSelectionInput,
    storage: State<'_, StorageState>,
) -> Result<CaptureStatus, String> {
    let selection = storage
        .save_capture_selection(input)
        .map_err(command_error)?;
    Ok(crate::capture::capture_status(selection))
}

#[tauri::command]
pub fn validate_capture_config(
    storage: State<'_, StorageState>,
) -> Result<ValidatedCaptureConfig, String> {
    let selection = storage.get_capture_selection().map_err(command_error)?;
    crate::capture::validate_capture_config(selection)
}

#[tauri::command]
pub fn start_recording_session(
    input: StartRecordingSessionInput,
    storage: State<'_, StorageState>,
    runtime: State<'_, RecordingRuntime>,
) -> Result<RecordingSessionEnvelope, String> {
    if runtime.is_active()? {
        return Err("A recording session is already active.".to_owned());
    }

    let selection = storage.get_capture_selection().map_err(command_error)?;
    let capture_config = crate::capture::validate_capture_config(selection)?;
    let session_id = Uuid::new_v4().to_string();
    let files = storage
        .prepare_recording_session_files(&session_id, capture_config.include_microphone)
        .map_err(command_error)?;
    let started_at = crate::recorder::current_timestamp_string();
    let recording = storage
        .create_recording(CreateRecordingInput {
            title: input.title,
            captured_at: Some(started_at.clone()),
            media_path: None,
        })
        .map_err(command_error)?;
    let recording = storage
        .update_recording(UpdateRecordingInput {
            id: recording.id.clone(),
            title: None,
            status: Some(crate::storage::RecordingStatus::Capturing),
            media_path: None,
            thumbnail_path: None,
            duration_ms: None,
            captured_at: recording.captured_at.clone(),
            completed_at: None,
            failure_message: None,
        })
        .map_err(command_error)?;
    let mut session = storage
        .create_recording_session(CreateRecordingSessionInput {
            id: session_id.clone(),
            recording_id: recording.id.clone(),
            temp_directory: files.temp_directory_relative.clone(),
            video_path: files.video_path_relative.clone(),
            audio_path: files.audio_path_relative.clone(),
            metadata_path: files.metadata_path_relative.clone(),
            screen_source_id: capture_config.screen_source.id.clone(),
            microphone_device_id: capture_config
                .microphone
                .as_ref()
                .map(|microphone| microphone.id.clone()),
            include_microphone: capture_config.include_microphone,
            width: None,
            height: None,
            frame_rate: crate::recorder::frame_rate(),
            audio_sample_rate: None,
            audio_channels: None,
            audio_sample_format: None,
            started_at,
        })
        .map_err(command_error)?;

    if let Err(error) = crate::recorder::write_session_metadata(&files.metadata_path, &session) {
        let _ = fail_created_recording_session(&storage, &recording.id, &session.id, &files, error);
        return Err("Unable to persist recording session metadata.".to_owned());
    }

    match runtime.start(&session, &capture_config, files.clone()) {
        Ok((width, height, audio_config)) => {
            session = storage
                .update_recording_session_capture_details(
                    &session.id,
                    width,
                    height,
                    audio_config.as_ref().map(|config| config.sample_rate),
                    audio_config.as_ref().map(|config| config.channels),
                    audio_config.map(|config| config.sample_format),
                )
                .map_err(command_error)?;
            crate::recorder::write_session_metadata(&files.metadata_path, &session)?;

            Ok(RecordingSessionEnvelope { recording, session })
        }
        Err(error) => {
            fail_created_recording_session(
                &storage,
                &recording.id,
                &session.id,
                &files,
                error.clone(),
            )?;
            Err(error)
        }
    }
}

#[tauri::command]
pub fn stop_recording_session(
    input: StopRecordingSessionInput,
    storage: State<'_, StorageState>,
    runtime: State<'_, RecordingRuntime>,
) -> Result<RecordingSessionEnvelope, String> {
    let stopped = runtime.stop(input.recording_id.as_deref())?;
    let audio_config = stopped.audio_config.clone();
    let session = storage
        .finish_recording_session(FinishRecordingSessionInput {
            id: stopped.session_id,
            status: stopped.status.clone(),
            width: stopped.width,
            height: stopped.height,
            frame_count: stopped.frame_count,
            audio_byte_count: stopped.audio_byte_count,
            audio_sample_rate: audio_config.as_ref().map(|config| config.sample_rate),
            audio_channels: audio_config.as_ref().map(|config| config.channels),
            audio_sample_format: audio_config.map(|config| config.sample_format),
            stopped_at: stopped.stopped_at.clone(),
            duration_ms: stopped.duration_ms,
            failure_message: stopped.failure_message.clone(),
        })
        .map_err(command_error)?;
    let recording_status = if stopped.failure_message.is_some() {
        crate::storage::RecordingStatus::Failed
    } else {
        crate::storage::RecordingStatus::Processing
    };
    let mut recording = storage
        .update_recording(UpdateRecordingInput {
            id: stopped.recording_id,
            title: None,
            status: Some(recording_status),
            media_path: None,
            thumbnail_path: None,
            duration_ms: Some(stopped.duration_ms),
            captured_at: None,
            completed_at: stopped.failure_message.as_ref().map(|_| stopped.stopped_at),
            failure_message: stopped.failure_message,
        })
        .map_err(command_error)?;

    crate::recorder::write_session_metadata(&stopped.files.metadata_path, &session)?;

    if session.status == RecordingSessionStatus::Stopped {
        recording = crate::jobs::enqueue_encoding_job(&storage, &recording.id)?;
        crate::jobs::spawn_pending_worker(storage.inner().clone());
    }

    Ok(RecordingSessionEnvelope { recording, session })
}

#[tauri::command]
pub fn active_recording_session(
    storage: State<'_, StorageState>,
    runtime: State<'_, RecordingRuntime>,
) -> Result<Option<RecordingSession>, String> {
    let Some(snapshot) = runtime.active_snapshot()? else {
        return Ok(None);
    };
    let mut session = storage
        .get_recording_session(&snapshot.session_id)
        .map_err(command_error)?
        .ok_or_else(|| "The active recording session metadata is missing.".to_owned())?;

    session.frame_count = snapshot.frame_count;
    session.audio_byte_count = snapshot.audio_byte_count;
    session.width = snapshot.width.or(session.width);
    session.height = snapshot.height.or(session.height);
    session.duration_ms = Some(snapshot.duration_ms);
    session.failure_message = snapshot.failure_message;

    Ok(Some(session))
}

#[tauri::command]
pub fn get_recording_session_by_recording(
    recording_id: String,
    storage: State<'_, StorageState>,
) -> Result<Option<RecordingSession>, String> {
    storage
        .get_recording_session_by_recording(&recording_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn encode_recording(
    recording_id: String,
    storage: State<'_, StorageState>,
) -> Result<Recording, String> {
    crate::jobs::run_encoding_job(&storage, &recording_id)
}

#[tauri::command]
pub fn recording_asset_paths(
    recording_id: String,
    storage: State<'_, StorageState>,
) -> Result<RecordingAssetPaths, String> {
    storage
        .recording_asset_paths(&recording_id)
        .map_err(command_error)
}

fn command_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn fail_created_recording_session(
    storage: &StorageState,
    recording_id: &str,
    session_id: &str,
    files: &crate::storage::RecordingSessionFiles,
    error: String,
) -> Result<(), String> {
    let stopped_at = crate::recorder::current_timestamp_string();
    let session = storage
        .finish_recording_session(FinishRecordingSessionInput {
            id: session_id.to_owned(),
            status: RecordingSessionStatus::Failed,
            width: None,
            height: None,
            frame_count: 0,
            audio_byte_count: 0,
            audio_sample_rate: None,
            audio_channels: None,
            audio_sample_format: None,
            stopped_at: stopped_at.clone(),
            duration_ms: 0,
            failure_message: Some(error.clone()),
        })
        .map_err(command_error)?;
    storage
        .update_recording(UpdateRecordingInput {
            id: recording_id.to_owned(),
            title: None,
            status: Some(crate::storage::RecordingStatus::Failed),
            media_path: None,
            thumbnail_path: None,
            duration_ms: Some(0),
            captured_at: None,
            completed_at: Some(stopped_at),
            failure_message: Some(error),
        })
        .map_err(command_error)?;
    crate::recorder::write_session_metadata(&files.metadata_path, &session)
}
