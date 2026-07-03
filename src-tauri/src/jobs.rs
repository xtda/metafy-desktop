use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::thread;

use crate::encoding::EncodingResult;
use crate::storage::{
    AiStatus, AiSummary, CreateProcessingJobInput, FinishRecordingSessionInput, JobState,
    PersistTranscriptInput, ProcessingJob, Recording, RecordingSessionStatus, RecordingStatus,
    StorageState, TranscriptSearchIndexSummary, TranscriptStatus, TranscriptWithSegments,
    UpdateProcessingJobInput, UpdateRecordingInput, UpsertAiSummaryInput,
};
use crate::transcription::{AudioExtractionResult, TranscriptionResult};

const KIND_ENCODE_RECORDING: &str = "encode_recording";
const KIND_EXTRACT_AUDIO: &str = "extract_audio";
const KIND_RUN_WHISPER: &str = "run_whisper";
const KIND_INDEX_TRANSCRIPT: &str = "index_transcript";
const KIND_GENERATE_THUMBNAIL: &str = "generate_thumbnail";
const KIND_AI_SUMMARY: &str = "ai_summary";
const KIND_CLEAN_TEMP_FILES: &str = "clean_temp_files";
const KIND_TRANSCRIBE_RECORDING: &str = "transcribe_recording";
const INTERRUPTED_JOB_MESSAGE: &str =
    "Job was interrupted because the app stopped before it completed.";
const INTERRUPTED_CAPTURE_MESSAGE: &str =
    "Recording capture was interrupted because the app stopped before the session finished.";

static JOB_WORKER_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryProcessingJobInput {
    pub job_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupProcessingFilesInput {
    pub recording_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecoverySummary {
    pub interrupted_job_count: usize,
    pub interrupted_recording_count: usize,
    pub rescheduled_job_count: usize,
    pub queued_job_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupTempFilesResult {
    pub job_id: String,
    pub removed_paths: Vec<String>,
    pub preserved_paths: Vec<String>,
}

pub fn recover_on_startup(storage: &StorageState) -> Result<JobRecoverySummary, String> {
    let interrupted_recording_count = mark_interrupted_captures(storage)?;
    let interrupted_jobs = storage
        .mark_running_jobs_interrupted(INTERRUPTED_JOB_MESSAGE)
        .map_err(command_error)?;

    for job in &interrupted_jobs {
        apply_interrupted_job_effect(storage, job)?;
    }

    let rescheduled_job_count = reschedule_processing_recordings(storage)?;
    let queued_job_count = storage
        .list_processing_jobs(Some(JobState::Queued), None)
        .map_err(command_error)?
        .len();

    Ok(JobRecoverySummary {
        interrupted_job_count: interrupted_jobs.len(),
        interrupted_recording_count,
        rescheduled_job_count,
        queued_job_count,
    })
}

pub fn spawn_pending_worker(storage: StorageState) {
    thread::spawn(move || {
        if let Err(error) = run_pending_jobs(&storage) {
            eprintln!("processing job worker failed: {error}");
        }
    });
}

pub fn run_pending_jobs(storage: &StorageState) -> Result<Vec<ProcessingJob>, String> {
    let lock = JOB_WORKER_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock
        .lock()
        .map_err(|_| "Processing job worker lock was poisoned.".to_owned())?;
    let mut finished_jobs = Vec::new();

    loop {
        let Some(job) = storage
            .claim_next_queued_processing_job()
            .map_err(command_error)?
        else {
            break;
        };

        let finished_job = run_claimed_job(storage, job)?;
        finished_jobs.push(finished_job);
    }

    Ok(finished_jobs)
}

pub fn enqueue_encoding_job(
    storage: &StorageState,
    recording_id: &str,
) -> Result<Recording, String> {
    let recording = storage
        .mark_recording_processing(recording_id)
        .map_err(command_error)?;
    let session = storage
        .get_recording_session_by_recording(recording_id)
        .map_err(command_error)?
        .ok_or_else(|| "Recording session metadata is required before encoding.".to_owned())?;

    if !has_incomplete_job(storage, recording_id, KIND_ENCODE_RECORDING)? {
        storage
            .create_processing_job(CreateProcessingJobInput {
                recording_id: Some(recording_id.to_owned()),
                kind: KIND_ENCODE_RECORDING.to_owned(),
                priority: Some(20),
                input_json: Some(
                    json!({
                        "recordingId": recording_id,
                        "sessionId": session.id,
                        "videoPath": session.video_path,
                        "audioPath": session.audio_path,
                        "microphoneAudioPath": session.microphone_audio_path,
                        "sourceAudioPath": session.source_audio_path,
                    })
                    .to_string(),
                ),
                max_attempts: Some(3),
            })
            .map_err(command_error)?;
    }

    Ok(recording)
}

pub fn run_encoding_job(storage: &StorageState, recording_id: &str) -> Result<Recording, String> {
    enqueue_encoding_job(storage, recording_id)?;
    run_pending_jobs(storage)?;
    storage
        .get_recording(recording_id)
        .map_err(command_error)?
        .ok_or_else(|| format!("recording not found: {recording_id}"))
}

pub fn run_transcription_job(
    storage: &StorageState,
    recording_id: &str,
    model_name: Option<&str>,
) -> Result<TranscriptWithSegments, String> {
    let selected_model = crate::transcription::normalize_model_name(model_name)?;
    storage
        .persist_transcript(PersistTranscriptInput {
            recording_id: recording_id.to_owned(),
            status: TranscriptStatus::Processing,
            language: None,
            model_name: Some(selected_model.clone()),
            raw_json_path: None,
            text: None,
            completed_at: None,
            failure_message: None,
            segments: Vec::new(),
        })
        .map_err(command_error)?;

    let job = storage
        .create_processing_job(CreateProcessingJobInput {
            recording_id: Some(recording_id.to_owned()),
            kind: KIND_TRANSCRIBE_RECORDING.to_owned(),
            priority: Some(10),
            input_json: Some(
                json!({
                    "recordingId": recording_id,
                    "modelName": selected_model,
                    "chunkDurationMs": 30_000,
                })
                .to_string(),
            ),
            max_attempts: Some(3),
        })
        .map_err(command_error)?;
    let started_job = start_created_job(storage, job)?;
    let completed_job = run_claimed_job(storage, started_job)?;

    if completed_job.state == JobState::Succeeded {
        return storage
            .get_transcript_by_recording(recording_id)
            .map_err(command_error)?
            .ok_or_else(|| "Transcription completed without persisted transcript.".to_owned());
    }

    Err(completed_job
        .error_message
        .unwrap_or_else(|| "Transcription failed.".to_owned()))
}

pub fn run_ai_summary_job(
    storage: &StorageState,
    recording_id: String,
    user_notes: Option<String>,
) -> Result<AiSummary, String> {
    prepare_ai_summary(storage, &recording_id, user_notes.as_deref())?;
    let job = storage
        .create_processing_job(CreateProcessingJobInput {
            recording_id: Some(recording_id.clone()),
            kind: KIND_AI_SUMMARY.to_owned(),
            priority: Some(5),
            input_json: Some(
                json!({
                    "recordingId": recording_id,
                    "userNotes": user_notes,
                })
                .to_string(),
            ),
            max_attempts: Some(3),
        })
        .map_err(command_error)?;
    let started_job = start_created_job(storage, job)?;
    let completed_job = run_claimed_job(storage, started_job)?;

    if completed_job.state == JobState::Succeeded {
        return storage
            .get_ai_summary_by_recording(
                completed_job
                    .recording_id
                    .as_deref()
                    .ok_or_else(|| "AI job is missing a recording id.".to_owned())?,
            )
            .map_err(command_error)?
            .ok_or_else(|| "AI summary completed without persisted output.".to_owned());
    }

    Err(completed_job
        .error_message
        .unwrap_or_else(|| "Optional AI summary failed.".to_owned()))
}

pub fn run_reindex_transcript_search_job(
    storage: &StorageState,
) -> Result<TranscriptSearchIndexSummary, String> {
    let job = storage
        .create_processing_job(CreateProcessingJobInput {
            recording_id: None,
            kind: KIND_INDEX_TRANSCRIPT.to_owned(),
            priority: Some(4),
            input_json: Some(json!({ "scope": "all_completed_transcripts" }).to_string()),
            max_attempts: Some(2),
        })
        .map_err(command_error)?;
    let started_job = start_created_job(storage, job)?;
    let completed_job = run_claimed_job(storage, started_job)?;

    if completed_job.state != JobState::Succeeded {
        return Err(completed_job
            .error_message
            .unwrap_or_else(|| "Transcript index job failed.".to_owned()));
    }

    let output = completed_job
        .output_json
        .ok_or_else(|| "Transcript index job did not return output.".to_owned())?;
    serde_json::from_str(&output).map_err(json_error)
}

pub fn retry_processing_job(
    storage: &StorageState,
    input: RetryProcessingJobInput,
) -> Result<ProcessingJob, String> {
    let job = storage
        .reset_processing_job_for_retry(&input.job_id)
        .map_err(command_error)?;

    match job.kind.as_str() {
        KIND_ENCODE_RECORDING => {
            if let Some(recording_id) = job.recording_id.as_deref() {
                storage
                    .mark_recording_processing(recording_id)
                    .map_err(command_error)?;
            }
        }
        KIND_TRANSCRIBE_RECORDING | KIND_EXTRACT_AUDIO | KIND_RUN_WHISPER => {
            if let Some(recording_id) = job.recording_id.as_deref() {
                let model_name = job_model_name(&job)?;
                storage
                    .persist_transcript(PersistTranscriptInput {
                        recording_id: recording_id.to_owned(),
                        status: TranscriptStatus::Processing,
                        language: None,
                        model_name: Some(model_name),
                        raw_json_path: None,
                        text: None,
                        completed_at: None,
                        failure_message: None,
                        segments: Vec::new(),
                    })
                    .map_err(command_error)?;
            }
        }
        KIND_AI_SUMMARY => {
            if let Some(recording_id) = job.recording_id.as_deref() {
                let user_notes = job_user_notes(&job);
                prepare_ai_summary(storage, recording_id, user_notes.as_deref())?;
            }
        }
        _ => {}
    }

    spawn_pending_worker(storage.clone());
    Ok(job)
}

pub fn run_cleanup_processing_files_job(
    storage: &StorageState,
    input: CleanupProcessingFilesInput,
) -> Result<CleanupTempFilesResult, String> {
    let job = storage
        .create_processing_job(CreateProcessingJobInput {
            recording_id: input.recording_id.clone(),
            kind: KIND_CLEAN_TEMP_FILES.to_owned(),
            priority: Some(1),
            input_json: Some(json!({ "recordingId": input.recording_id }).to_string()),
            max_attempts: Some(2),
        })
        .map_err(command_error)?;
    let started_job = start_created_job(storage, job)?;
    let completed_job = run_claimed_job(storage, started_job)?;

    if completed_job.state != JobState::Succeeded {
        return Err(completed_job
            .error_message
            .unwrap_or_else(|| "Cleanup job failed.".to_owned()));
    }

    let output = completed_job
        .output_json
        .ok_or_else(|| "Cleanup job did not return output.".to_owned())?;
    serde_json::from_str(&output).map_err(json_error)
}

fn run_claimed_job(storage: &StorageState, job: ProcessingJob) -> Result<ProcessingJob, String> {
    match job.kind.as_str() {
        KIND_ENCODE_RECORDING => run_encode_job(storage, job),
        KIND_TRANSCRIBE_RECORDING => run_transcribe_job(storage, job),
        KIND_AI_SUMMARY => run_ai_job(storage, job),
        KIND_INDEX_TRANSCRIPT => run_index_job(storage, job),
        KIND_CLEAN_TEMP_FILES => run_cleanup_job(storage, job),
        KIND_EXTRACT_AUDIO | KIND_RUN_WHISPER | KIND_GENERATE_THUMBNAIL => fail_job(
            storage,
            &job.id,
            "This substep is retried through its parent recording job.".to_owned(),
            false,
        ),
        _ => fail_job(
            storage,
            &job.id,
            format!("Unsupported processing job kind: {}", job.kind),
            false,
        ),
    }
}

fn run_encode_job(storage: &StorageState, job: ProcessingJob) -> Result<ProcessingJob, String> {
    let recording_id = required_recording_id(&job)?.to_owned();

    if let Err(error) = storage.mark_recording_processing(&recording_id) {
        return fail_job(storage, &job.id, error.to_string(), false);
    }

    match crate::encoding::encode_recording(storage, &recording_id) {
        Ok(result) => complete_encoding_job(storage, &recording_id, &job.id, result),
        Err(error) => fail_encoding_job(storage, &recording_id, &job.id, error),
    }
}

fn complete_encoding_job(
    storage: &StorageState,
    recording_id: &str,
    job_id: &str,
    result: EncodingResult,
) -> Result<ProcessingJob, String> {
    let output_json = json_string(&result)?;
    let job = storage
        .update_processing_job(UpdateProcessingJobInput {
            id: job_id.to_owned(),
            state: Some(JobState::Succeeded),
            attempts: None,
            output_json: Some(output_json),
            error_message: None,
            interrupted: Some(false),
            started_at: None,
            completed_at: None,
        })
        .map_err(command_error)?;

    storage
        .complete_recording_encode(
            recording_id,
            result.media_path.clone(),
            result.thumbnail_path.clone(),
            result.duration_ms,
            crate::recorder::current_timestamp_string(),
        )
        .map_err(command_error)?;

    let _ = record_succeeded_subjob(
        storage,
        Some(recording_id.to_owned()),
        KIND_GENERATE_THUMBNAIL,
        8,
        json!({ "recordingId": recording_id, "mediaPath": result.media_path }),
        json!({ "thumbnailPath": result.thumbnail_path }),
    );
    let _ = run_cleanup_subjob(storage, Some(recording_id.to_owned()));

    Ok(job)
}

fn fail_encoding_job(
    storage: &StorageState,
    recording_id: &str,
    job_id: &str,
    error: String,
) -> Result<ProcessingJob, String> {
    let job = fail_job(storage, job_id, error.clone(), false)?;
    storage
        .fail_recording_encode(recording_id, error)
        .map_err(command_error)?;
    Ok(job)
}

fn run_transcribe_job(storage: &StorageState, job: ProcessingJob) -> Result<ProcessingJob, String> {
    let recording_id = required_recording_id(&job)?.to_owned();
    let selected_model = job_model_name(&job)?;

    if let Err(error) = storage.persist_transcript(PersistTranscriptInput {
        recording_id: recording_id.clone(),
        status: TranscriptStatus::Processing,
        language: None,
        model_name: Some(selected_model.clone()),
        raw_json_path: None,
        text: None,
        completed_at: None,
        failure_message: None,
        segments: Vec::new(),
    }) {
        return fail_job(storage, &job.id, error.to_string(), false);
    }

    let extraction = match run_extract_audio_subjob(storage, &recording_id) {
        Ok(result) => result,
        Err(error) => {
            return fail_transcription_job(storage, &recording_id, &job.id, selected_model, error);
        }
    };

    let transcription =
        match run_whisper_subjob(storage, &recording_id, &selected_model, extraction) {
            Ok(result) => result,
            Err(error) => {
                return fail_transcription_job(
                    storage,
                    &recording_id,
                    &job.id,
                    selected_model,
                    error,
                );
            }
        };

    let _ = record_succeeded_subjob(
        storage,
        Some(recording_id.clone()),
        KIND_INDEX_TRANSCRIPT,
        6,
        json!({
            "recordingId": recording_id,
            "transcriptId": transcription.transcript.transcript.id,
        }),
        json!({
            "indexedSegmentCount": transcription.transcript.segments.len(),
        }),
    );

    storage
        .update_processing_job(UpdateProcessingJobInput {
            id: job.id,
            state: Some(JobState::Succeeded),
            attempts: None,
            output_json: Some(json_string(&transcription)?),
            error_message: None,
            interrupted: Some(false),
            started_at: None,
            completed_at: None,
        })
        .map_err(command_error)
}

fn run_extract_audio_subjob(
    storage: &StorageState,
    recording_id: &str,
) -> Result<AudioExtractionResult, String> {
    let job = start_subjob(
        storage,
        Some(recording_id.to_owned()),
        KIND_EXTRACT_AUDIO,
        9,
        json!({ "recordingId": recording_id }),
    )?;

    match crate::transcription::extract_recording_audio(storage, recording_id) {
        Ok(result) => {
            complete_job(storage, &job.id, json_string(&result)?)?;
            Ok(result)
        }
        Err(error) => {
            let _ = fail_job(storage, &job.id, error.clone(), false);
            Err(error)
        }
    }
}

fn run_whisper_subjob(
    storage: &StorageState,
    recording_id: &str,
    model_name: &str,
    extraction: AudioExtractionResult,
) -> Result<TranscriptionResult, String> {
    let job = start_subjob(
        storage,
        Some(recording_id.to_owned()),
        KIND_RUN_WHISPER,
        8,
        json!({
            "recordingId": recording_id,
            "modelName": model_name,
            "audioPath": extraction.audio_path.clone(),
        }),
    )?;

    match crate::transcription::run_whisper_recording(storage, recording_id, model_name, extraction)
    {
        Ok(result) => {
            complete_job(storage, &job.id, json_string(&result)?)?;
            Ok(result)
        }
        Err(error) => {
            let _ = fail_job(storage, &job.id, error.clone(), false);
            Err(error)
        }
    }
}

fn fail_transcription_job(
    storage: &StorageState,
    recording_id: &str,
    job_id: &str,
    model_name: String,
    error: String,
) -> Result<ProcessingJob, String> {
    let job = fail_job(storage, job_id, error.clone(), false)?;
    storage
        .persist_transcript(PersistTranscriptInput {
            recording_id: recording_id.to_owned(),
            status: TranscriptStatus::Failed,
            language: None,
            model_name: Some(model_name),
            raw_json_path: None,
            text: None,
            completed_at: None,
            failure_message: Some(error),
            segments: Vec::new(),
        })
        .map_err(command_error)?;
    Ok(job)
}

fn run_ai_job(storage: &StorageState, job: ProcessingJob) -> Result<ProcessingJob, String> {
    let recording_id = required_recording_id(&job)?.to_owned();
    let user_notes = job_user_notes(&job);
    let (settings, payload) =
        match prepare_ai_summary(storage, &recording_id, user_notes.as_deref()) {
            Ok(prepared) => prepared,
            Err(error) => return fail_ai_summary_job(storage, &recording_id, &job.id, None, error),
        };

    match crate::ai::request_summary(&settings, &payload) {
        Ok(output) => {
            let job = storage
                .update_processing_job(UpdateProcessingJobInput {
                    id: job.id,
                    state: Some(JobState::Succeeded),
                    attempts: None,
                    output_json: Some(json_string(&output)?),
                    error_message: None,
                    interrupted: Some(false),
                    started_at: None,
                    completed_at: None,
                })
                .map_err(command_error)?;

            storage
                .upsert_ai_summary(UpsertAiSummaryInput {
                    recording_id,
                    status: AiStatus::Completed,
                    model_name: Some(settings.model_name),
                    summary_text: Some(output.summary_text),
                    action_items_json: Some(output.action_items_json),
                    decisions_json: Some(output.decisions_json),
                    questions_json: Some(output.questions_json),
                    risks_json: Some(output.risks_json),
                    chapters_json: Some(output.chapters_json),
                    completed_at: Some(crate::recorder::current_timestamp_string()),
                    failure_message: None,
                })
                .map_err(command_error)?;

            Ok(job)
        }
        Err(error) => fail_ai_summary_job(
            storage,
            &recording_id,
            &job.id,
            Some(settings.model_name),
            error,
        ),
    }
}

fn prepare_ai_summary(
    storage: &StorageState,
    recording_id: &str,
    user_notes: Option<&str>,
) -> Result<(crate::storage::AiSettingsRecord, crate::ai::AiPromptPayload), String> {
    let settings = storage.get_ai_settings_record().map_err(command_error)?;
    if !settings.enabled {
        return Err(
            "Optional AI is disabled. Enable and configure it in Settings first.".to_owned(),
        );
    }

    let recording = storage
        .get_recording(recording_id)
        .map_err(command_error)?
        .ok_or_else(|| format!("recording not found: {recording_id}"))?;
    let transcript = storage
        .get_transcript_by_recording(recording_id)
        .map_err(command_error)?
        .ok_or_else(|| "AI summaries require a completed transcript.".to_owned())?;
    let payload =
        crate::ai::build_summary_payload(&recording, &transcript, user_notes.map(str::to_owned))?;

    storage
        .upsert_ai_summary(UpsertAiSummaryInput {
            recording_id: recording_id.to_owned(),
            status: AiStatus::Processing,
            model_name: Some(settings.model_name.clone()),
            summary_text: None,
            action_items_json: None,
            decisions_json: None,
            questions_json: None,
            risks_json: None,
            chapters_json: None,
            completed_at: None,
            failure_message: None,
        })
        .map_err(command_error)?;

    Ok((settings, payload))
}

fn fail_ai_summary_job(
    storage: &StorageState,
    recording_id: &str,
    job_id: &str,
    model_name: Option<String>,
    error: String,
) -> Result<ProcessingJob, String> {
    let job = fail_job(storage, job_id, error.clone(), false)?;
    let _ = storage.upsert_ai_summary(UpsertAiSummaryInput {
        recording_id: recording_id.to_owned(),
        status: AiStatus::Failed,
        model_name,
        summary_text: None,
        action_items_json: None,
        decisions_json: None,
        questions_json: None,
        risks_json: None,
        chapters_json: None,
        completed_at: None,
        failure_message: Some(error),
    });
    Ok(job)
}

fn run_index_job(storage: &StorageState, job: ProcessingJob) -> Result<ProcessingJob, String> {
    match storage.reindex_transcript_search() {
        Ok(summary) => complete_job(storage, &job.id, json_string(&summary)?),
        Err(error) => fail_job(storage, &job.id, error.to_string(), false),
    }
}

fn run_cleanup_job(storage: &StorageState, job: ProcessingJob) -> Result<ProcessingJob, String> {
    match cleanup_processing_files(storage, job.recording_id.as_deref(), &job.id) {
        Ok(result) => complete_job(storage, &job.id, json_string(&result)?),
        Err(error) => fail_job(storage, &job.id, error, false),
    }
}

fn run_cleanup_subjob(
    storage: &StorageState,
    recording_id: Option<String>,
) -> Result<ProcessingJob, String> {
    let job = start_subjob(
        storage,
        recording_id.clone(),
        KIND_CLEAN_TEMP_FILES,
        1,
        json!({ "recordingId": recording_id }),
    )?;
    run_cleanup_job(storage, job)
}

fn cleanup_processing_files(
    storage: &StorageState,
    recording_id: Option<&str>,
    job_id: &str,
) -> Result<CleanupTempFilesResult, String> {
    let recordings = match recording_id {
        Some(id) => storage
            .get_recording(id)
            .map_err(command_error)?
            .into_iter()
            .collect::<Vec<_>>(),
        None => storage.list_recordings().map_err(command_error)?,
    };
    let mut removed_paths = Vec::new();
    let mut preserved_paths = Vec::new();

    for recording in recordings {
        cleanup_recording_files(
            storage,
            &recording,
            &mut removed_paths,
            &mut preserved_paths,
        )?;
    }

    Ok(CleanupTempFilesResult {
        job_id: job_id.to_owned(),
        removed_paths,
        preserved_paths,
    })
}

fn cleanup_recording_files(
    storage: &StorageState,
    recording: &Recording,
    removed_paths: &mut Vec<String>,
    preserved_paths: &mut Vec<String>,
) -> Result<(), String> {
    let recording_directory = storage.resolve_path(&recording.recording_directory);
    for file_name in [
        "encoding-video.bgra",
        "encoding-audio.raw",
        "encoding-microphone.raw",
        "encoding-source.raw",
        "recording.tmp.mp4",
        "thumbnail.tmp.jpg",
        "transcript-audio.wav",
        "whisper-output.json",
    ] {
        remove_file_if_exists(&recording_directory.join(file_name), removed_paths)?;
    }
    remove_files_with_prefix(&recording_directory, "recording.tmp.mp4.sb-", removed_paths)?;

    let Some(session) = storage
        .get_recording_session_by_recording(&recording.id)
        .map_err(command_error)?
    else {
        return Ok(());
    };

    let temp_directory = storage.resolve_path(&session.temp_directory);
    let media_is_verified = recording
        .media_path
        .as_deref()
        .map(|path| storage.resolve_path(path).is_file())
        .unwrap_or(false);

    if recording.status == RecordingStatus::Completed && media_is_verified {
        remove_dir_if_exists(&temp_directory, removed_paths)?;
    } else {
        preserved_paths.push(path_to_string(&temp_directory));
        preserved_paths.push(path_to_string(&storage.resolve_path(&session.video_path)));
        if let Some(path) = session.microphone_audio_path.as_deref() {
            preserved_paths.push(path_to_string(&storage.resolve_path(path)));
        }
        if let Some(path) = session.source_audio_path.as_deref() {
            preserved_paths.push(path_to_string(&storage.resolve_path(path)));
        }
        if let Some(path) = session.audio_path.as_deref() {
            let legacy_audio_path = path_to_string(&storage.resolve_path(path));
            if !preserved_paths.contains(&legacy_audio_path) {
                preserved_paths.push(legacy_audio_path);
            }
        }
        preserved_paths.push(path_to_string(
            &storage.resolve_path(&session.metadata_path),
        ));
    }

    Ok(())
}

fn mark_interrupted_captures(storage: &StorageState) -> Result<usize, String> {
    let mut interrupted_count = 0;

    for recording in storage.list_recordings().map_err(command_error)? {
        let Some(session) = storage
            .get_recording_session_by_recording(&recording.id)
            .map_err(command_error)?
        else {
            continue;
        };

        if session.status != RecordingSessionStatus::Capturing {
            continue;
        }

        let stopped_at = crate::recorder::current_timestamp_string();
        storage
            .finish_recording_session(FinishRecordingSessionInput {
                id: session.id,
                status: RecordingSessionStatus::Failed,
                width: session.width,
                height: session.height,
                frame_count: session.frame_count,
                audio_byte_count: session.audio_byte_count,
                audio_sample_rate: session.audio_sample_rate,
                audio_channels: session.audio_channels,
                audio_sample_format: session.audio_sample_format,
                microphone_audio_byte_count: session.microphone_audio_byte_count,
                microphone_audio_sample_rate: session.microphone_audio_sample_rate,
                microphone_audio_channels: session.microphone_audio_channels,
                microphone_audio_sample_format: session.microphone_audio_sample_format,
                source_audio_byte_count: session.source_audio_byte_count,
                source_audio_sample_rate: session.source_audio_sample_rate,
                source_audio_channels: session.source_audio_channels,
                source_audio_sample_format: session.source_audio_sample_format,
                stopped_at: stopped_at.clone(),
                duration_ms: session.duration_ms.unwrap_or(0),
                failure_message: Some(INTERRUPTED_CAPTURE_MESSAGE.to_owned()),
            })
            .map_err(command_error)?;
        storage
            .update_recording(UpdateRecordingInput {
                id: recording.id,
                title: None,
                status: Some(RecordingStatus::Failed),
                media_path: None,
                thumbnail_path: None,
                duration_ms: recording.duration_ms,
                captured_at: None,
                completed_at: Some(stopped_at),
                failure_message: Some(INTERRUPTED_CAPTURE_MESSAGE.to_owned()),
            })
            .map_err(command_error)?;
        interrupted_count += 1;
    }

    Ok(interrupted_count)
}

fn apply_interrupted_job_effect(storage: &StorageState, job: &ProcessingJob) -> Result<(), String> {
    let Some(recording_id) = job.recording_id.as_deref() else {
        return Ok(());
    };

    match job.kind.as_str() {
        KIND_ENCODE_RECORDING => {
            storage
                .fail_recording_encode(recording_id, INTERRUPTED_JOB_MESSAGE.to_owned())
                .map_err(command_error)?;
        }
        KIND_TRANSCRIBE_RECORDING | KIND_EXTRACT_AUDIO | KIND_RUN_WHISPER => {
            let model_name = job_model_name(job)
                .unwrap_or_else(|_| crate::transcription::DEFAULT_WHISPER_MODEL.to_owned());
            storage
                .persist_transcript(PersistTranscriptInput {
                    recording_id: recording_id.to_owned(),
                    status: TranscriptStatus::Failed,
                    language: None,
                    model_name: Some(model_name),
                    raw_json_path: None,
                    text: None,
                    completed_at: None,
                    failure_message: Some(INTERRUPTED_JOB_MESSAGE.to_owned()),
                    segments: Vec::new(),
                })
                .map_err(command_error)?;
        }
        KIND_AI_SUMMARY => {
            let _ = storage.upsert_ai_summary(UpsertAiSummaryInput {
                recording_id: recording_id.to_owned(),
                status: AiStatus::Failed,
                model_name: None,
                summary_text: None,
                action_items_json: None,
                decisions_json: None,
                questions_json: None,
                risks_json: None,
                chapters_json: None,
                completed_at: None,
                failure_message: Some(INTERRUPTED_JOB_MESSAGE.to_owned()),
            });
        }
        _ => {}
    }

    Ok(())
}

fn reschedule_processing_recordings(storage: &StorageState) -> Result<usize, String> {
    let mut rescheduled_count = 0;

    for recording in storage.list_recordings().map_err(command_error)? {
        if recording.status != RecordingStatus::Processing || recording.media_path.is_some() {
            continue;
        }

        let Some(session) = storage
            .get_recording_session_by_recording(&recording.id)
            .map_err(command_error)?
        else {
            continue;
        };

        if session.status != RecordingSessionStatus::Stopped
            || has_incomplete_job(storage, &recording.id, KIND_ENCODE_RECORDING)?
        {
            continue;
        }

        enqueue_encoding_job(storage, &recording.id)?;
        rescheduled_count += 1;
    }

    Ok(rescheduled_count)
}

fn has_incomplete_job(
    storage: &StorageState,
    recording_id: &str,
    kind: &str,
) -> Result<bool, String> {
    let jobs = storage
        .list_processing_jobs(None, Some(recording_id.to_owned()))
        .map_err(command_error)?;
    Ok(jobs
        .iter()
        .any(|job| job.kind == kind && matches!(job.state, JobState::Queued | JobState::Running)))
}

fn start_created_job(storage: &StorageState, job: ProcessingJob) -> Result<ProcessingJob, String> {
    storage
        .update_processing_job(UpdateProcessingJobInput {
            id: job.id,
            state: Some(JobState::Running),
            attempts: Some(job.attempts + 1),
            output_json: None,
            error_message: None,
            interrupted: Some(false),
            started_at: None,
            completed_at: None,
        })
        .map_err(command_error)
}

fn start_subjob(
    storage: &StorageState,
    recording_id: Option<String>,
    kind: &str,
    priority: i64,
    input_json: Value,
) -> Result<ProcessingJob, String> {
    let job = storage
        .create_processing_job(CreateProcessingJobInput {
            recording_id,
            kind: kind.to_owned(),
            priority: Some(priority),
            input_json: Some(input_json.to_string()),
            max_attempts: Some(1),
        })
        .map_err(command_error)?;
    start_created_job(storage, job)
}

fn record_succeeded_subjob(
    storage: &StorageState,
    recording_id: Option<String>,
    kind: &str,
    priority: i64,
    input_json: Value,
    output_json: Value,
) -> Result<ProcessingJob, String> {
    let job = start_subjob(storage, recording_id, kind, priority, input_json)?;
    complete_job(storage, &job.id, output_json.to_string())
}

fn complete_job(
    storage: &StorageState,
    job_id: &str,
    output_json: String,
) -> Result<ProcessingJob, String> {
    storage
        .update_processing_job(UpdateProcessingJobInput {
            id: job_id.to_owned(),
            state: Some(JobState::Succeeded),
            attempts: None,
            output_json: Some(output_json),
            error_message: None,
            interrupted: Some(false),
            started_at: None,
            completed_at: None,
        })
        .map_err(command_error)
}

fn fail_job(
    storage: &StorageState,
    job_id: &str,
    error: String,
    interrupted: bool,
) -> Result<ProcessingJob, String> {
    storage
        .update_processing_job(UpdateProcessingJobInput {
            id: job_id.to_owned(),
            state: Some(JobState::Failed),
            attempts: None,
            output_json: None,
            error_message: Some(error),
            interrupted: Some(interrupted),
            started_at: None,
            completed_at: None,
        })
        .map_err(command_error)
}

fn required_recording_id(job: &ProcessingJob) -> Result<&str, String> {
    job.recording_id
        .as_deref()
        .ok_or_else(|| format!("{} job is missing a recording id.", job.kind))
}

fn job_model_name(job: &ProcessingJob) -> Result<String, String> {
    let value = job_input_value(job);
    let model_name = value
        .as_ref()
        .and_then(|value| value.get("modelName"))
        .and_then(Value::as_str);
    crate::transcription::normalize_model_name(model_name)
}

fn job_user_notes(job: &ProcessingJob) -> Option<String> {
    let value = job_input_value(job)?;
    value
        .get("userNotes")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            value
                .pointer("/payload/userNotes")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

fn job_input_value(job: &ProcessingJob) -> Option<Value> {
    job.input_json
        .as_deref()
        .and_then(|input| serde_json::from_str(input).ok())
}

fn remove_file_if_exists(path: &Path, removed_paths: &mut Vec<String>) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => {
            removed_paths.push(path_to_string(path));
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Unable to remove recoverable processing file {}: {error}",
            path_to_string(path)
        )),
    }
}

fn remove_dir_if_exists(path: &Path, removed_paths: &mut Vec<String>) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => {
            removed_paths.push(path_to_string(path));
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Unable to remove completed capture temp directory {}: {error}",
            path_to_string(path)
        )),
    }
}

fn remove_files_with_prefix(
    directory: &Path,
    prefix: &str,
    removed_paths: &mut Vec<String>,
) -> Result<(), String> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "Unable to inspect recoverable processing files in {}: {error}",
                path_to_string(directory)
            ));
        }
    };

    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Unable to inspect recoverable processing file in {}: {error}",
                path_to_string(directory)
            )
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file_name.starts_with(prefix) {
            remove_file_if_exists(&path, removed_paths)?;
        }
    }

    Ok(())
}

fn json_string(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(json_error)
}

fn command_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn json_error(error: impl std::fmt::Display) -> String {
    format!("Unable to serialize processing job output: {error}")
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
