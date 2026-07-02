use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::binaries::{find_binary, missing_binary_message};
use crate::storage::{
    PersistTranscriptInput, Recording, RecordingStatus, StorageState, TranscriptSegmentInput,
    TranscriptStatus, TranscriptWithSegments,
};

pub const DEFAULT_WHISPER_MODEL: &str = "small.en";
const WHISPER_MODEL_ENV_VAR: &str = "METAFY_WHISPER_CPP_PATH";
const FFMPEG_ENV_VAR: &str = "METAFY_FFMPEG_PATH";
const TRANSCRIPT_JSON_FILE_NAME: &str = "transcript.json";
const TRANSCRIPT_AUDIO_FILE_NAME: &str = "transcript-audio.wav";
const WHISPER_OUTPUT_PREFIX: &str = "whisper-output";
const AUDIO_SAMPLE_RATE: i64 = 16_000;
const AUDIO_CHANNELS: i64 = 1;
const DEFAULT_CHUNK_DURATION_MS: i64 = 30_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeRecordingInput {
    pub recording_id: String,
    pub model_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportWhisperModelInput {
    pub source_path: String,
    pub model_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModelStatus {
    pub selected_model: String,
    pub default_model: &'static str,
    pub expected_file_name: String,
    pub model_path: String,
    pub models_directory: String,
    pub exists: bool,
    pub available_models: Vec<WhisperLocalModel>,
    pub binary: WhisperBinaryStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperLocalModel {
    pub name: String,
    pub file_name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperBinaryStatus {
    pub available: bool,
    pub path: Option<String>,
    pub env_var: &'static str,
    pub candidates: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioExtractionResult {
    pub recording_id: String,
    pub media_path: String,
    pub audio_path: String,
    pub ffmpeg_path: String,
    pub ffmpeg_args: Vec<String>,
    pub chunk_duration_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionResult {
    pub recording_id: String,
    pub model_name: String,
    pub model_path: String,
    pub media_path: String,
    pub audio_path: String,
    pub raw_json_path: String,
    pub raw_json_path_relative: String,
    pub whisper_path: String,
    pub whisper_args: Vec<String>,
    pub ffmpeg_path: String,
    pub ffmpeg_args: Vec<String>,
    pub chunk_duration_ms: i64,
    pub segment_count: usize,
    pub language: Option<String>,
    pub transcript: TranscriptWithSegments,
}

struct RecordingTranscriptPaths {
    media_path: PathBuf,
    recording_directory: PathBuf,
    audio_path: PathBuf,
    raw_json_path: PathBuf,
    raw_json_path_relative: String,
    temporary_output_prefix: PathBuf,
    temporary_json_path: PathBuf,
}

struct CommandOutput {
    program: PathBuf,
    args: Vec<String>,
}

struct CommandRunResult {
    stdout: Vec<u8>,
}

struct ParsedWhisperJson {
    language: Option<String>,
    text: Option<String>,
    segments: Vec<TranscriptSegmentInput>,
}

pub fn normalize_model_name(model_name: Option<&str>) -> Result<String, String> {
    let raw_name = model_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_WHISPER_MODEL);
    let normalized = raw_name
        .strip_prefix("ggml-")
        .unwrap_or(raw_name)
        .strip_suffix(".bin")
        .unwrap_or_else(|| raw_name.strip_prefix("ggml-").unwrap_or(raw_name))
        .to_owned();

    if normalized.contains('/') || normalized.contains('\\') || normalized.contains("..") {
        return Err("Whisper model names must be local file names only.".to_owned());
    }

    if normalized.is_empty() {
        return Err("Whisper model name is required.".to_owned());
    }

    if !normalized
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_'))
    {
        return Err(
            "Whisper model names may only contain letters, numbers, dots, dashes, and underscores."
                .to_owned(),
        );
    }

    Ok(normalized)
}

pub fn whisper_model_status(
    storage: &StorageState,
    model_name: Option<&str>,
) -> Result<WhisperModelStatus, String> {
    let selected_model = normalize_model_name(model_name)?;
    let model_path = model_path(storage, &selected_model);
    let available_models = available_models(storage)?;
    let whisper_binary = find_binary(WHISPER_MODEL_ENV_VAR, whisper_binary_candidates());

    Ok(WhisperModelStatus {
        selected_model,
        default_model: DEFAULT_WHISPER_MODEL,
        expected_file_name: model_file_name(
            model_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default(),
        ),
        model_path: path_to_string(&model_path),
        models_directory: path_to_string(storage.whisper_models_directory()),
        exists: model_path.is_file(),
        available_models,
        binary: WhisperBinaryStatus {
            available: whisper_binary.is_some(),
            path: whisper_binary.as_ref().map(|path| path_to_string(path)),
            env_var: WHISPER_MODEL_ENV_VAR,
            candidates: whisper_binary_candidates().to_vec(),
        },
    })
}

pub fn import_whisper_model(
    storage: &StorageState,
    input: ImportWhisperModelInput,
) -> Result<WhisperModelStatus, String> {
    let selected_model = normalize_model_name(input.model_name.as_deref())?;
    let source_path = PathBuf::from(input.source_path.trim());

    if !source_path.is_file() {
        return Err("The selected Whisper model file does not exist.".to_owned());
    }

    fs::create_dir_all(storage.whisper_models_directory())
        .map_err(|error| format!("Unable to create Whisper model directory: {error}"))?;
    let destination_path = model_path(storage, &selected_model);

    if source_path.canonicalize().ok() != destination_path.canonicalize().ok() {
        fs::copy(&source_path, &destination_path)
            .map_err(|error| format!("Unable to import Whisper model: {error}"))?;
    }

    whisper_model_status(storage, Some(&selected_model))
}

#[cfg(test)]
fn transcribe_recording(
    storage: &StorageState,
    recording_id: &str,
    model_name: &str,
) -> Result<TranscriptionResult, String> {
    let extraction = extract_recording_audio(storage, recording_id)?;
    run_whisper_recording(storage, recording_id, model_name, extraction)
}

pub fn extract_recording_audio(
    storage: &StorageState,
    recording_id: &str,
) -> Result<AudioExtractionResult, String> {
    let (recording, paths) = recording_transcript_paths(storage, recording_id)?;
    let ffmpeg_path = find_binary(FFMPEG_ENV_VAR, &["ffmpeg"])
        .ok_or_else(|| missing_binary_message("ffmpeg", FFMPEG_ENV_VAR))?;

    remove_file_if_exists(&paths.audio_path)?;
    fs::create_dir_all(&paths.recording_directory)
        .map_err(|error| format!("Unable to prepare transcript directory: {error}"))?;

    let extract_command =
        build_audio_extract_command(&ffmpeg_path, &paths.media_path, &paths.audio_path);
    run_logged_command(&extract_command, "FFmpeg audio extraction")?;

    Ok(AudioExtractionResult {
        recording_id: recording.id,
        media_path: path_to_string(&paths.media_path),
        audio_path: path_to_string(&paths.audio_path),
        ffmpeg_path: path_to_string(&ffmpeg_path),
        ffmpeg_args: extract_command.args,
        chunk_duration_ms: DEFAULT_CHUNK_DURATION_MS,
    })
}

pub fn run_whisper_recording(
    storage: &StorageState,
    recording_id: &str,
    model_name: &str,
    extraction: AudioExtractionResult,
) -> Result<TranscriptionResult, String> {
    if extraction.recording_id != recording_id {
        return Err("Audio extraction result does not match this recording.".to_owned());
    }

    let selected_model = normalize_model_name(Some(model_name))?;
    let model_path = model_path(storage, &selected_model);
    if !model_path.is_file() {
        return Err(format!(
            "Whisper model {selected_model} is missing. Expected {}.",
            path_to_string(&model_path)
        ));
    }

    let whisper_path = find_binary(WHISPER_MODEL_ENV_VAR, whisper_binary_candidates())
        .ok_or_else(|| missing_binary_message("whisper.cpp", WHISPER_MODEL_ENV_VAR))?;
    let (recording, paths) = recording_transcript_paths(storage, recording_id)?;

    remove_file_if_exists(&paths.temporary_json_path)?;
    fs::create_dir_all(&paths.recording_directory)
        .map_err(|error| format!("Unable to prepare transcript directory: {error}"))?;

    let whisper_command = build_whisper_command(
        &whisper_path,
        &model_path,
        &paths.audio_path,
        &paths.temporary_output_prefix,
        &selected_model,
    );
    let whisper_run = run_logged_command(&whisper_command, "whisper.cpp transcription")?;
    let raw_json = read_whisper_json(&paths.temporary_json_path, &whisper_run.stdout)?;

    fs::write(&paths.raw_json_path, &raw_json)
        .map_err(|error| format!("Unable to store raw Whisper JSON: {error}"))?;

    let parsed = parse_whisper_json(&raw_json, &selected_model)?;
    let transcript = storage
        .persist_transcript(PersistTranscriptInput {
            recording_id: recording.id.clone(),
            status: TranscriptStatus::Completed,
            language: parsed.language.clone(),
            model_name: Some(selected_model.clone()),
            raw_json_path: Some(paths.raw_json_path_relative.clone()),
            text: parsed.text,
            completed_at: Some(current_timestamp_string()),
            failure_message: None,
            segments: parsed.segments,
        })
        .map_err(|error| error.to_string())?;

    let _ = remove_file_if_exists(&paths.temporary_json_path);

    Ok(TranscriptionResult {
        recording_id: recording.id,
        model_name: selected_model,
        model_path: path_to_string(&model_path),
        media_path: path_to_string(&paths.media_path),
        audio_path: path_to_string(&paths.audio_path),
        raw_json_path: path_to_string(&paths.raw_json_path),
        raw_json_path_relative: paths.raw_json_path_relative,
        whisper_path: path_to_string(&whisper_path),
        whisper_args: whisper_command.args,
        ffmpeg_path: extraction.ffmpeg_path,
        ffmpeg_args: extraction.ffmpeg_args,
        chunk_duration_ms: DEFAULT_CHUNK_DURATION_MS,
        segment_count: transcript.segments.len(),
        language: transcript.transcript.language.clone(),
        transcript,
    })
}

fn recording_transcript_paths(
    storage: &StorageState,
    recording_id: &str,
) -> Result<(Recording, RecordingTranscriptPaths), String> {
    let recording = storage
        .get_recording(recording_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("recording not found: {recording_id}"))?;

    if recording.status != RecordingStatus::Completed {
        return Err("Only completed recordings can be transcribed.".to_owned());
    }

    let media_path = recording
        .media_path
        .as_deref()
        .map(|path| storage.resolve_path(path))
        .ok_or_else(|| "Recording has no MP4 media path to transcribe.".to_owned())?;

    if !media_path.is_file() {
        return Err(format!(
            "Recording media file is missing: {}",
            path_to_string(&media_path)
        ));
    }

    let recording_directory = storage.resolve_path(&recording.recording_directory);
    let recording_directory_relative = recording.recording_directory.trim_end_matches('/');
    let raw_json_path_relative =
        format!("{recording_directory_relative}/{TRANSCRIPT_JSON_FILE_NAME}");
    let raw_json_path = storage.resolve_path(&raw_json_path_relative);
    let audio_path = recording_directory.join(TRANSCRIPT_AUDIO_FILE_NAME);
    let temporary_output_prefix = recording_directory.join(WHISPER_OUTPUT_PREFIX);
    let temporary_json_path = recording_directory.join(format!("{WHISPER_OUTPUT_PREFIX}.json"));

    Ok((
        recording,
        RecordingTranscriptPaths {
            media_path,
            recording_directory,
            audio_path,
            raw_json_path,
            raw_json_path_relative,
            temporary_output_prefix,
            temporary_json_path,
        },
    ))
}

fn model_path(storage: &StorageState, model_name: &str) -> PathBuf {
    storage
        .whisper_models_directory()
        .join(format!("ggml-{model_name}.bin"))
}

fn available_models(storage: &StorageState) -> Result<Vec<WhisperLocalModel>, String> {
    let directory = storage.whisper_models_directory();
    if !directory.is_dir() {
        return Ok(Vec::new());
    }

    let mut models = Vec::new();
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("Unable to read Whisper model directory: {error}"))?;

    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Unable to read Whisper model directory: {error}"))?;
        let path = entry.path();
        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("bin")
        {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let name = file_name
            .strip_prefix("ggml-")
            .unwrap_or(file_name)
            .strip_suffix(".bin")
            .unwrap_or(file_name)
            .to_owned();

        models.push(WhisperLocalModel {
            name,
            file_name: file_name.to_owned(),
            path: path_to_string(&path),
        });
    }

    models.sort_by(|first, second| first.name.cmp(&second.name));
    Ok(models)
}

fn model_file_name(file_name: &str) -> String {
    file_name.to_owned()
}

fn build_audio_extract_command(
    ffmpeg_path: &Path,
    media_path: &Path,
    audio_path: &Path,
) -> CommandOutput {
    CommandOutput {
        program: ffmpeg_path.to_path_buf(),
        args: vec![
            "-hide_banner".to_owned(),
            "-y".to_owned(),
            "-i".to_owned(),
            path_to_string(media_path),
            "-vn".to_owned(),
            "-ac".to_owned(),
            AUDIO_CHANNELS.to_string(),
            "-ar".to_owned(),
            AUDIO_SAMPLE_RATE.to_string(),
            "-f".to_owned(),
            "wav".to_owned(),
            path_to_string(audio_path),
        ],
    }
}

fn build_whisper_command(
    whisper_path: &Path,
    model_path: &Path,
    audio_path: &Path,
    output_prefix: &Path,
    model_name: &str,
) -> CommandOutput {
    let mut args = vec![
        "-m".to_owned(),
        path_to_string(model_path),
        "-f".to_owned(),
        path_to_string(audio_path),
        "-oj".to_owned(),
        "-of".to_owned(),
        path_to_string(output_prefix),
    ];

    if model_name.ends_with(".en") {
        args.extend(["-l".to_owned(), "en".to_owned()]);
    }

    CommandOutput {
        program: whisper_path.to_path_buf(),
        args,
    }
}

fn run_logged_command(command: &CommandOutput, label: &str) -> Result<CommandRunResult, String> {
    let output = Command::new(&command.program)
        .args(&command.args)
        .output()
        .map_err(|error| format!("Unable to run {label}: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "{label} exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(CommandRunResult {
        stdout: output.stdout,
    })
}

fn read_whisper_json(output_path: &Path, stdout: &[u8]) -> Result<Vec<u8>, String> {
    if output_path.is_file() {
        return fs::read(output_path)
            .map_err(|error| format!("Unable to read Whisper JSON output: {error}"));
    }

    let trimmed_stdout = trim_ascii_whitespace(stdout);
    if trimmed_stdout.starts_with(b"{") || trimmed_stdout.starts_with(b"[") {
        return Ok(stdout.to_vec());
    }

    Err("whisper.cpp did not produce JSON output.".to_owned())
}

fn parse_whisper_json(raw_json: &[u8], model_name: &str) -> Result<ParsedWhisperJson, String> {
    let value: Value = serde_json::from_slice(raw_json)
        .map_err(|error| format!("Unable to parse Whisper JSON output: {error}"))?;
    let segments = parse_segments(&value);
    let text = read_text(&value).or_else(|| joined_segment_text(&segments));
    let language =
        read_language(&value).or_else(|| model_name.ends_with(".en").then(|| "en".to_owned()));

    if segments.is_empty() && text.is_none() {
        return Err(
            "Whisper JSON did not contain transcript text or timestamped segments.".to_owned(),
        );
    }

    Ok(ParsedWhisperJson {
        language,
        text,
        segments,
    })
}

fn parse_segments(value: &Value) -> Vec<TranscriptSegmentInput> {
    let segment_values = value
        .get("segments")
        .and_then(Value::as_array)
        .or_else(|| value.get("transcription").and_then(Value::as_array));
    let Some(segment_values) = segment_values else {
        return Vec::new();
    };

    segment_values
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| parse_segment(index, segment))
        .collect()
}

fn parse_segment(index: usize, segment: &Value) -> Option<TranscriptSegmentInput> {
    let text = segment
        .get("text")
        .and_then(Value::as_str)?
        .trim()
        .to_owned();
    if text.is_empty() {
        return None;
    }

    let start_ms = segment
        .get("start_ms")
        .and_then(number_to_i64)
        .or_else(|| segment.get("start").and_then(seconds_to_ms))
        .or_else(|| segment_offset_ms(segment, "from"))
        .or_else(|| segment_timestamp_ms(segment, "from"))?;
    let end_ms = segment
        .get("end_ms")
        .and_then(number_to_i64)
        .or_else(|| segment.get("end").and_then(seconds_to_ms))
        .or_else(|| segment_offset_ms(segment, "to"))
        .or_else(|| segment_timestamp_ms(segment, "to"))?;

    if start_ms < 0 || end_ms < start_ms {
        return None;
    }

    Some(TranscriptSegmentInput {
        segment_index: Some(index as i64),
        start_ms,
        end_ms,
        text,
        confidence: segment_confidence(segment),
    })
}

fn read_language(value: &Value) -> Option<String> {
    value
        .get("language")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("result")
                .and_then(|result| result.get("language"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|language| !language.is_empty())
        .map(str::to_owned)
}

fn read_text(value: &Value) -> Option<String> {
    value
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("result")
                .and_then(|result| result.get("text"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn joined_segment_text(segments: &[TranscriptSegmentInput]) -> Option<String> {
    let text = segments
        .iter()
        .map(|segment| segment.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    (!text.is_empty()).then_some(text)
}

fn segment_offset_ms(segment: &Value, key: &str) -> Option<i64> {
    segment
        .get("offsets")
        .and_then(|offsets| offsets.get(key))
        .and_then(number_to_i64)
}

fn segment_timestamp_ms(segment: &Value, key: &str) -> Option<i64> {
    segment
        .get("timestamps")
        .and_then(|timestamps| timestamps.get(key))
        .and_then(Value::as_str)
        .and_then(parse_timestamp_ms)
}

fn segment_confidence(segment: &Value) -> Option<f64> {
    ["confidence", "confidence_score", "probability"]
        .iter()
        .find_map(|key| segment.get(*key).and_then(number_to_f64))
        .filter(|value| value.is_finite() && *value >= 0.0 && *value <= 1.0)
}

fn number_to_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_f64().map(|value| value.round() as i64))
        .or_else(|| {
            value
                .as_str()?
                .parse::<f64>()
                .ok()
                .map(|value| value.round() as i64)
        })
}

fn number_to_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse::<f64>().ok())
}

fn seconds_to_ms(value: &Value) -> Option<i64> {
    number_to_f64(value).map(|seconds| (seconds * 1000.0).round() as i64)
}

fn parse_timestamp_ms(value: &str) -> Option<i64> {
    let normalized = value.replace(',', ".");
    let parts = normalized.split(':').collect::<Vec<_>>();

    match parts.as_slice() {
        [minutes, seconds] => {
            let minutes = minutes.parse::<f64>().ok()?;
            let seconds = seconds.parse::<f64>().ok()?;
            Some(((minutes * 60.0 + seconds) * 1000.0).round() as i64)
        }
        [hours, minutes, seconds] => {
            let hours = hours.parse::<f64>().ok()?;
            let minutes = minutes.parse::<f64>().ok()?;
            let seconds = seconds.parse::<f64>().ok()?;
            Some(((hours * 3600.0 + minutes * 60.0 + seconds) * 1000.0).round() as i64)
        }
        _ => None,
    }
}

fn whisper_binary_candidates() -> &'static [&'static str] {
    &["whisper-cli", "main", "whisper"]
}

fn trim_ascii_whitespace(value: &[u8]) -> &[u8] {
    let start = value
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(value.len());
    let end = value
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|position| position + 1)
        .unwrap_or(start);

    &value[start..end]
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Unable to remove stale transcription file: {error}"
        )),
    }
}

fn current_timestamp_string() -> String {
    crate::recorder::current_timestamp_string()
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{initialize_at, CreateRecordingInput, UpdateRecordingInput};
    use std::env;
    use std::ffi::OsString;
    use std::sync::Mutex;
    use uuid::Uuid;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn parses_standard_whisper_segments_with_confidence() {
        let parsed = parse_whisper_json(
            br#"{
                "language": "en",
                "segments": [
                    {"start": 1.24, "end": 6.4, "text": "Welcome everyone.", "confidence": 0.87},
                    {"start": 6.4, "end": 9.0, "text": "Back to review."}
                ]
            }"#,
            DEFAULT_WHISPER_MODEL,
        )
        .expect("parse whisper json");

        assert_eq!(parsed.language.as_deref(), Some("en"));
        assert_eq!(parsed.segments.len(), 2);
        assert_eq!(parsed.segments[0].start_ms, 1_240);
        assert_eq!(parsed.segments[0].end_ms, 6_400);
        assert_eq!(parsed.segments[0].confidence, Some(0.87));
        assert_eq!(
            parsed.text.as_deref(),
            Some("Welcome everyone. Back to review.")
        );
    }

    #[test]
    fn parses_whisper_cpp_transcription_offsets() {
        let parsed = parse_whisper_json(
            br#"{
                "result": {"language": "en"},
                "transcription": [
                    {
                        "timestamps": {"from": "00:00:01,250", "to": "00:00:03,500"},
                        "offsets": {"from": 1250, "to": 3500},
                        "text": "Timestamped text."
                    }
                ]
            }"#,
            DEFAULT_WHISPER_MODEL,
        )
        .expect("parse whisper.cpp json");

        assert_eq!(parsed.language.as_deref(), Some("en"));
        assert_eq!(parsed.segments.len(), 1);
        assert_eq!(parsed.segments[0].start_ms, 1_250);
        assert_eq!(parsed.segments[0].end_ms, 3_500);
    }

    #[cfg(unix)]
    #[test]
    fn runs_local_transcription_with_fake_binaries() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let old_ffmpeg = env::var_os(FFMPEG_ENV_VAR);
        let old_whisper = env::var_os(WHISPER_MODEL_ENV_VAR);
        let root =
            std::env::temp_dir().join(format!("metafy-transcription-test-{}", Uuid::new_v4()));
        let state = initialize_at(root.clone()).expect("initialize test storage");
        let recording = state
            .create_recording(CreateRecordingInput {
                title: Some("Transcription source".to_owned()),
                captured_at: Some(current_timestamp_string()),
                media_path: None,
            })
            .expect("create recording");
        let media_path_relative = format!("recordings/{}/recording.mp4", recording.id);
        fs::write(state.resolve_path(&media_path_relative), b"fake mp4").expect("write media");
        let recording = state
            .update_recording(UpdateRecordingInput {
                id: recording.id,
                title: None,
                status: Some(RecordingStatus::Completed),
                media_path: Some(media_path_relative),
                thumbnail_path: None,
                duration_ms: Some(4_000),
                captured_at: None,
                completed_at: Some(current_timestamp_string()),
                failure_message: None,
            })
            .expect("complete recording");
        fs::write(
            state.whisper_models_directory().join("ggml-small.en.bin"),
            b"fake model",
        )
        .expect("write model");

        let bin_dir = root.join("bin");
        fs::create_dir_all(&bin_dir).expect("create bin dir");
        let fake_ffmpeg = bin_dir.join("ffmpeg");
        let fake_whisper = bin_dir.join("whisper-cli");
        write_executable(
            &fake_ffmpeg,
            r#"#!/bin/sh
for last do :; done
printf 'wav' > "$last"
"#,
        );
        write_executable(
            &fake_whisper,
            r#"#!/bin/sh
prefix=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-of" ] || [ "$1" = "--output-file" ]; then
    shift
    prefix="$1"
  fi
  shift
done
cat > "${prefix}.json" <<'JSON'
{"result":{"language":"en"},"transcription":[{"offsets":{"from":0,"to":2500},"text":"Local transcript."}]}
JSON
"#,
        );
        env::set_var(FFMPEG_ENV_VAR, &fake_ffmpeg);
        env::set_var(WHISPER_MODEL_ENV_VAR, &fake_whisper);

        let result = transcribe_recording(&state, &recording.id, DEFAULT_WHISPER_MODEL)
            .expect("transcribe recording");

        assert_eq!(result.segment_count, 1);
        assert_eq!(
            result.transcript.transcript.status,
            TranscriptStatus::Completed
        );
        assert_eq!(result.transcript.segments[0].text, "Local transcript.");
        assert!(state
            .resolve_path(
                result
                    .transcript
                    .transcript
                    .raw_json_path
                    .as_deref()
                    .unwrap()
            )
            .is_file());

        restore_env(FFMPEG_ENV_VAR, old_ffmpeg);
        restore_env(WHISPER_MODEL_ENV_VAR, old_whisper);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    fn write_executable(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, body).expect("write fake executable");
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod fake executable");
    }

    fn restore_env(name: &str, value: Option<OsString>) {
        match value {
            Some(value) => env::set_var(name, value),
            None => env::remove_var(name),
        }
    }
}
