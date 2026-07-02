use crate::storage::{AiSettingsRecord, Recording, TranscriptStatus, TranscriptWithSegments};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

const AI_REQUEST_TIMEOUT_SECONDS: u64 = 120;
const REQUESTED_OUTPUTS: [&str; 6] = [
    "summary",
    "action_items",
    "decisions",
    "questions",
    "risks",
    "chapters",
];
const FORBIDDEN_KEY_FRAGMENTS: [&str; 7] = [
    "path",
    "directory",
    "media",
    "audio",
    "video",
    "thumbnail",
    "raw_json",
];
const MEDIA_PATH_MARKERS: [&str; 16] = [
    ".mp4",
    ".mov",
    ".mkv",
    ".webm",
    ".avi",
    ".mfrv",
    ".pcm",
    ".wav",
    ".mp3",
    ".m4a",
    ".flac",
    ".jpg",
    ".jpeg",
    ".png",
    ".webp",
    "temp/recording-sessions/",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiPromptPayload {
    pub recording: AiRecordingMetadata,
    pub transcript: AiTranscriptPayload,
    pub user_notes: Option<String>,
    pub requested_outputs: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRecordingMetadata {
    pub id: String,
    pub title: String,
    pub status: String,
    pub captured_at: Option<String>,
    pub duration_ms: Option<i64>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTranscriptPayload {
    pub id: String,
    pub language: Option<String>,
    pub model_name: Option<String>,
    pub text: String,
    pub segments: Vec<AiTranscriptSegmentPayload>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTranscriptSegmentPayload {
    pub index: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSummaryOutput {
    pub summary_text: String,
    pub action_items_json: String,
    pub decisions_json: String,
    pub questions_json: String,
    pub risks_json: String,
    pub chapters_json: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionMessage {
    content: String,
}

pub fn build_summary_payload(
    recording: &Recording,
    transcript: &TranscriptWithSegments,
    user_notes: Option<String>,
) -> Result<AiPromptPayload, String> {
    if transcript.transcript.status != TranscriptStatus::Completed {
        return Err("AI summaries require a completed transcript.".to_owned());
    }

    let text = transcript_text(transcript)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| "AI summaries require transcript text.".to_owned())?;
    let payload = AiPromptPayload {
        recording: AiRecordingMetadata {
            id: recording.id.clone(),
            title: recording.title.clone(),
            status: recording.status.as_str().to_owned(),
            captured_at: recording.captured_at.clone(),
            duration_ms: recording.duration_ms,
            completed_at: recording.completed_at.clone(),
        },
        transcript: AiTranscriptPayload {
            id: transcript.transcript.id.clone(),
            language: transcript.transcript.language.clone(),
            model_name: transcript.transcript.model_name.clone(),
            text,
            segments: transcript
                .segments
                .iter()
                .map(|segment| AiTranscriptSegmentPayload {
                    index: segment.segment_index,
                    start_ms: segment.start_ms,
                    end_ms: segment.end_ms,
                    text: segment.text.clone(),
                })
                .collect(),
        },
        user_notes: user_notes
            .map(|notes| notes.trim().to_owned())
            .filter(|notes| !notes.is_empty()),
        requested_outputs: REQUESTED_OUTPUTS.to_vec(),
    };

    validate_transcript_only_payload(&serde_json::to_value(&payload).map_err(json_error)?)?;
    Ok(payload)
}

pub fn request_summary(
    settings: &AiSettingsRecord,
    payload: &AiPromptPayload,
) -> Result<AiSummaryOutput, String> {
    if !settings.enabled {
        return Err("Optional AI is disabled.".to_owned());
    }

    if settings.provider != "openai_compatible" {
        return Err("Only OpenAI-compatible AI providers are supported.".to_owned());
    }

    if settings.model_name.trim().is_empty() {
        return Err("Optional AI requires a configured model name.".to_owned());
    }

    let api_key = settings
        .api_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| "Optional AI requires a configured API key.".to_owned())?;
    let request_body = chat_completion_body(&settings.model_name, payload)?;
    validate_transcript_only_payload(&request_body)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(AI_REQUEST_TIMEOUT_SECONDS))
        .build()
        .map_err(|error| format!("Unable to initialize AI HTTP client: {error}"))?;
    let response = client
        .post(&settings.endpoint_url)
        .bearer_auth(api_key)
        .json(&request_body)
        .send()
        .map_err(|error| format!("AI provider request failed: {error}"))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| format!("Unable to read AI provider response: {error}"))?;

    if !status.is_success() {
        return Err(format!(
            "AI provider request failed ({status}): {}",
            compact_error_body(&body)
        ));
    }

    let completion: ChatCompletionResponse = serde_json::from_str(&body)
        .map_err(|error| format!("AI provider returned invalid JSON: {error}"))?;
    let content = completion
        .choices
        .first()
        .map(|choice| choice.message.content.trim())
        .filter(|content| !content.is_empty())
        .ok_or_else(|| "AI provider response did not include message content.".to_owned())?;

    parse_summary_content(content)
}

fn chat_completion_body(model_name: &str, payload: &AiPromptPayload) -> Result<Value, String> {
    let body = json!({
        "model": model_name,
        "temperature": 0.2,
        "response_format": { "type": "json_object" },
        "messages": [
            {
                "role": "system",
                "content": "You analyze local recording transcripts. Use only the supplied transcript text, recording metadata, and user notes. Never infer from or request raw audio, raw video, screenshots, thumbnails, or file paths. Return strict JSON only."
            },
            {
                "role": "user",
                "content": user_prompt(payload)?
            }
        ]
    });

    Ok(body)
}

fn user_prompt(payload: &AiPromptPayload) -> Result<String, String> {
    let payload_json = serde_json::to_string_pretty(payload).map_err(json_error)?;

    Ok(format!(
        "Create a transcript-only analysis for this local recording payload.\n\nReturn JSON with these keys:\n- summary: concise paragraph\n- action_items: array of strings or objects\n- decisions: array of strings or objects\n- questions: array of strings or objects\n- risks: array of strings or objects\n- chapters: array of objects with title, start_ms, end_ms, and summary when possible\n\nPayload:\n{payload_json}"
    ))
}

fn transcript_text(transcript: &TranscriptWithSegments) -> Option<String> {
    transcript.transcript.text.clone().or_else(|| {
        let joined = transcript
            .segments
            .iter()
            .map(|segment| segment.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        (!joined.is_empty()).then_some(joined)
    })
}

fn validate_transcript_only_payload(value: &Value) -> Result<(), String> {
    validate_value(value, "$")
}

fn validate_value(value: &Value, location: &str) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let normalized_key = key.to_ascii_lowercase();
                if FORBIDDEN_KEY_FRAGMENTS
                    .iter()
                    .any(|fragment| normalized_key.contains(fragment))
                {
                    return Err(format!(
                        "AI payload guardrail rejected forbidden field `{key}` at {location}."
                    ));
                }

                validate_value(value, &format!("{location}.{key}"))?;
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                validate_value(item, &format!("{location}[{index}]"))?;
            }
        }
        Value::String(text) if contains_likely_media_path(text) => {
            return Err(format!(
                "AI payload guardrail rejected a likely media path at {location}."
            ));
        }
        _ => {}
    }

    Ok(())
}

fn contains_likely_media_path(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase().replace('\\', "/");

    if normalized.starts_with("/users/")
        || normalized.starts_with("/var/")
        || normalized.starts_with("/tmp/")
        || normalized.contains(":/")
        || normalized.contains("recordings/")
    {
        return true;
    }

    MEDIA_PATH_MARKERS
        .iter()
        .any(|marker| normalized.contains(marker))
}

fn parse_summary_content(content: &str) -> Result<AiSummaryOutput, String> {
    let value: Value = serde_json::from_str(strip_json_fence(content))
        .map_err(|error| format!("AI provider summary was not valid JSON: {error}"))?;
    let summary_text = value
        .get("summary")
        .or_else(|| value.get("summary_text"))
        .map(value_to_display_text)
        .filter(|summary| !summary.trim().is_empty())
        .ok_or_else(|| "AI provider summary JSON did not include a summary.".to_owned())?;

    Ok(AiSummaryOutput {
        summary_text,
        action_items_json: array_json(&value, "action_items")?,
        decisions_json: array_json(&value, "decisions")?,
        questions_json: array_json(&value, "questions")?,
        risks_json: array_json(&value, "risks")?,
        chapters_json: array_json(&value, "chapters")?,
    })
}

fn strip_json_fence(content: &str) -> &str {
    let trimmed = content.trim();
    let without_start = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .trim();

    without_start
        .strip_suffix("```")
        .unwrap_or(without_start)
        .trim()
}

fn array_json(value: &Value, key: &str) -> Result<String, String> {
    match value.get(key) {
        Some(Value::Array(items)) => serde_json::to_string(items).map_err(json_error),
        Some(Value::Null) | None => Ok("[]".to_owned()),
        Some(other) => serde_json::to_string(&vec![other.clone()]).map_err(json_error),
    }
}

fn value_to_display_text(value: &Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn compact_error_body(body: &str) -> String {
    const MAX_ERROR_BODY_LENGTH: usize = 500;

    if body.len() <= MAX_ERROR_BODY_LENGTH {
        return body.to_owned();
    }

    format!("{}...", &body[..MAX_ERROR_BODY_LENGTH])
}

fn json_error(error: serde_json::Error) -> String {
    format!("Unable to serialize AI payload: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{RecordingStatus, Transcript, TranscriptSegment};

    #[test]
    fn builds_payload_without_media_paths() {
        let recording = Recording {
            id: "recording-1".to_owned(),
            title: "Review".to_owned(),
            status: RecordingStatus::Completed,
            recording_directory: "recordings/recording-1".to_owned(),
            media_path: Some("recordings/recording-1/recording.mp4".to_owned()),
            thumbnail_path: Some("recordings/recording-1/thumbnail.jpg".to_owned()),
            duration_ms: Some(60_000),
            captured_at: Some("2026-07-01T12:00:00Z".to_owned()),
            created_at: "2026-07-01T12:00:00Z".to_owned(),
            updated_at: "2026-07-01T12:01:00Z".to_owned(),
            completed_at: Some("2026-07-01T12:01:00Z".to_owned()),
            failure_message: None,
        };
        let transcript = TranscriptWithSegments {
            transcript: Transcript {
                id: "transcript-1".to_owned(),
                recording_id: "recording-1".to_owned(),
                status: TranscriptStatus::Completed,
                language: Some("en".to_owned()),
                model_name: Some("small.en".to_owned()),
                raw_json_path: Some("recordings/recording-1/transcript.json".to_owned()),
                text: Some("We decided to update the onboarding flow.".to_owned()),
                created_at: "2026-07-01T12:00:00Z".to_owned(),
                updated_at: "2026-07-01T12:01:00Z".to_owned(),
                completed_at: Some("2026-07-01T12:01:00Z".to_owned()),
                failure_message: None,
            },
            segments: vec![TranscriptSegment {
                id: "segment-1".to_owned(),
                transcript_id: "transcript-1".to_owned(),
                recording_id: "recording-1".to_owned(),
                segment_index: 0,
                start_ms: 0,
                end_ms: 3_000,
                text: "We decided to update the onboarding flow.".to_owned(),
                confidence: Some(0.92),
            }],
        };

        let payload = build_summary_payload(&recording, &transcript, None).expect("payload");
        let serialized = serde_json::to_string(&payload).expect("serialize payload");

        assert!(!serialized.contains("recording.mp4"));
        assert!(!serialized.contains("thumbnail.jpg"));
        assert!(!serialized.contains("transcript.json"));
        assert!(serialized.contains("onboarding flow"));
    }

    #[test]
    fn guardrail_rejects_media_fields_and_paths() {
        let field_payload = json!({
            "recording": {
                "mediaPath": "recordings/1/recording.mp4"
            }
        });
        assert!(validate_transcript_only_payload(&field_payload).is_err());

        let path_payload = json!({
            "recording": {
                "title": "/Users/andrew/recording.mp4"
            }
        });
        assert!(validate_transcript_only_payload(&path_payload).is_err());
    }

    #[test]
    fn parses_summary_json() {
        let output = parse_summary_content(
            r#"{
                "summary": "Short summary.",
                "action_items": ["Follow up"],
                "decisions": [],
                "questions": [{"text":"What changed?"}],
                "risks": null,
                "chapters": [{"title":"Intro","start_ms":0,"end_ms":1000}]
            }"#,
        )
        .expect("parse summary");

        assert_eq!(output.summary_text, "Short summary.");
        assert_eq!(output.action_items_json, r#"["Follow up"]"#);
        assert_eq!(output.risks_json, "[]");
        assert!(output.chapters_json.contains("Intro"));
    }
}
