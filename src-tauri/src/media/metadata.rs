#![cfg_attr(target_os = "macos", allow(dead_code))]

use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub width: i64,
    pub height: i64,
    pub frame_rate: i64,
    pub frame_count: i64,
    pub duration_ms: Option<i64>,
    pub audio_included: bool,
    pub file_size_bytes: Option<u64>,
    pub backend_id: String,
    pub backend_diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInfoSource {
    pub width: i64,
    pub height: i64,
    pub frame_rate: i64,
    pub frame_count: i64,
    pub audio_duration_ms: Option<i64>,
    pub duration_hint_ms: Option<i64>,
    pub audio_included: bool,
    pub backend_id: String,
    pub backend_diagnostics: Vec<String>,
}

pub fn derive_media_info(source: MediaInfoSource, media_path: &Path) -> MediaInfo {
    let video_duration_ms = duration_ms_from_frames(source.frame_count, source.frame_rate);
    let duration_ms = [video_duration_ms, source.audio_duration_ms]
        .into_iter()
        .flatten()
        .max()
        .or(source.duration_hint_ms);

    MediaInfo {
        width: source.width,
        height: source.height,
        frame_rate: source.frame_rate,
        frame_count: source.frame_count,
        duration_ms,
        audio_included: source.audio_included,
        file_size_bytes: fs::metadata(media_path).ok().map(|metadata| metadata.len()),
        backend_id: source.backend_id,
        backend_diagnostics: source.backend_diagnostics,
    }
}

pub fn duration_ms_from_frames(frame_count: i64, frame_rate: i64) -> Option<i64> {
    if frame_count <= 0 || frame_rate <= 0 {
        return None;
    }

    let duration_ms =
        (i128::from(frame_count) * 1_000 + i128::from(frame_rate / 2)) / i128::from(frame_rate);
    i64::try_from(duration_ms).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use uuid::Uuid;

    #[test]
    fn derives_duration_from_longest_video_or_audio_timeline() {
        let root = std::env::temp_dir().join(format!("metafy-media-info-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create test root");
        let media_path = root.join("recording.mp4");
        fs::File::create(&media_path)
            .expect("create media")
            .write_all(&[1, 2, 3, 4])
            .expect("write media");

        let info = derive_media_info(
            MediaInfoSource {
                width: 1280,
                height: 720,
                frame_rate: 30,
                frame_count: 60,
                audio_duration_ms: Some(2_500),
                duration_hint_ms: Some(3_000),
                audio_included: true,
                backend_id: "test-backend".to_owned(),
                backend_diagnostics: vec!["encoded".to_owned()],
            },
            &media_path,
        );

        assert_eq!(info.duration_ms, Some(2_500));
        assert_eq!(info.file_size_bytes, Some(4));
        assert_eq!(info.backend_id, "test-backend");
        assert_eq!(info.backend_diagnostics, vec!["encoded"]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn falls_back_to_session_duration_when_timelines_are_unavailable() {
        let info = derive_media_info(
            MediaInfoSource {
                width: 0,
                height: 0,
                frame_rate: 0,
                frame_count: 0,
                audio_duration_ms: None,
                duration_hint_ms: Some(1_234),
                audio_included: false,
                backend_id: "test-backend".to_owned(),
                backend_diagnostics: Vec::new(),
            },
            Path::new("/does/not/exist.mp4"),
        );

        assert_eq!(info.duration_ms, Some(1_234));
        assert_eq!(info.file_size_bytes, None);
    }
}
