use serde::Serialize;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::media::audio::{
    mix_audio_sources, prepare_audio_source, write_f32le_samples, FINAL_ENCODE_CHANNELS,
    FINAL_ENCODE_SAMPLE_FORMAT, FINAL_ENCODE_SAMPLE_RATE,
};
#[cfg(target_os = "linux")]
use crate::media::backends::linux_gstreamer::LinuxGstreamerRecordingEncoder;
#[cfg(target_os = "macos")]
use crate::media::backends::macos::MacosRecordingEncoder;
#[cfg(target_os = "windows")]
use crate::media::backends::windows::WindowsRecordingEncoder;
use crate::media::chunked_video::{
    read_manifest as read_chunked_video_manifest, read_thumbnail_frame as read_chunked_thumbnail,
    try_read_manifest as try_read_chunked_video_manifest, ChunkedVideoStatus,
};
use crate::media::encode::{
    EncodeAudioInput, EncodeCommandDiagnostics, EncodeInput, EncodeOutput, EncodeOutputPaths,
    EncodeVideoFormat, EncodeVideoInput, RecordingEncoder,
};
use crate::media::metadata::{duration_ms_from_frames, MediaInfo};
use crate::media::sidecar::{
    RawAudioFormat, RawAudioMetadataError, RawAudioReader, RawVideoReader,
};
use crate::media::sidecar_selection::{
    select_requested_audio_sidecars, AudioSidecarInput, AudioSidecarPurpose,
};
use crate::media::thumbnail::BgraFrame;
use crate::storage::StorageState;

const STAGING_AUDIO_FILE_NAME: &str = "encoding-audio.raw";
const MICROPHONE_STAGING_AUDIO_FILE_NAME: &str = "encoding-microphone.raw";
const SOURCE_STAGING_AUDIO_FILE_NAME: &str = "encoding-source.raw";
const MAX_ENCODE_WIDTH: u32 = 1920;
const MAX_ENCODE_HEIGHT: u32 = 1080;
const BGRA_BYTES_PER_PIXEL: usize = 4;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodingResult {
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
    pub backend_id: String,
    pub backend_label: String,
    pub backend_commands: Vec<EncodeCommandDiagnostics>,
    pub backend_messages: Vec<String>,
    pub warnings: Vec<String>,
}

struct PreparedVideo {
    path: PathBuf,
    format: EncodeVideoFormat,
    temporary_path: Option<PathBuf>,
    width: i64,
    height: i64,
    frame_count: i64,
    thumbnail_frame: BgraFrame,
}

struct PreparedAudioInput {
    path: PathBuf,
    sample_rate: i64,
    channels: i64,
    sample_format: String,
    duration_ms: Option<i64>,
}

pub fn encode_recording(
    storage: &StorageState,
    recording_id: &str,
) -> Result<EncodingResult, String> {
    let session = storage
        .get_recording_session_by_recording(recording_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Recording session metadata is required before encoding.".to_owned())?;

    if session.frame_count <= 0 {
        return Err("Recording has no captured frames to encode.".to_owned());
    }

    let media_files = storage
        .recording_media_files(recording_id)
        .map_err(|error| error.to_string())?;
    let source_video_path = storage.resolve_path(&session.video_path);
    let mut warnings = Vec::new();
    let encoding_audio_inputs = select_requested_audio_sidecars(
        storage,
        &session,
        AudioSidecarPurpose::Encoding,
        &mut warnings,
    );
    let staging_video_path = media_files.recording_directory.join("encoding-video.bgra");
    let staging_audio_path = media_files
        .recording_directory
        .join(STAGING_AUDIO_FILE_NAME);
    let staging_media_path = media_files.recording_directory.join("recording.tmp.mp4");
    let staging_thumbnail_path = media_files.recording_directory.join("thumbnail.tmp.jpg");

    remove_file_if_exists(&staging_video_path)?;
    for file_name in [
        STAGING_AUDIO_FILE_NAME,
        MICROPHONE_STAGING_AUDIO_FILE_NAME,
        SOURCE_STAGING_AUDIO_FILE_NAME,
    ] {
        remove_file_if_exists(&media_files.recording_directory.join(file_name))?;
    }
    remove_file_if_exists(&staging_media_path)?;
    remove_file_if_exists(&staging_thumbnail_path)?;

    let prepared_video = prepare_video_input(
        &source_video_path,
        &staging_video_path,
        session.width,
        session.height,
        &mut warnings,
    )?;
    let prepared_audio_input =
        prepare_encoding_audio_inputs(encoding_audio_inputs, staging_audio_path, &mut warnings)?;
    let prepared_audio_inputs = prepared_audio_input.into_iter().collect::<Vec<_>>();
    let encode_input = EncodeInput {
        recording_id: recording_id.to_owned(),
        video: EncodeVideoInput {
            path: prepared_video.path.clone(),
            format: prepared_video.format,
            width: prepared_video.width,
            height: prepared_video.height,
            frame_rate: session.frame_rate,
            frame_count: prepared_video.frame_count,
            thumbnail_frame: prepared_video.thumbnail_frame.clone(),
        },
        audio_inputs: prepared_audio_inputs
            .iter()
            .map(|audio_input| EncodeAudioInput {
                path: audio_input.path.clone(),
                sample_rate: audio_input.sample_rate,
                channels: audio_input.channels,
                sample_format: audio_input.sample_format.clone(),
                duration_ms: audio_input.duration_ms,
            })
            .collect(),
        output: EncodeOutputPaths {
            media_path: media_files.media_path,
            media_path_relative: media_files.media_path_relative,
            thumbnail_path: media_files.thumbnail_path,
            thumbnail_path_relative: media_files.thumbnail_path_relative,
            staging_media_path,
            staging_thumbnail_path,
        },
        duration_hint_ms: session.duration_ms,
        warnings,
    };
    let (backend_output, backend_label) = encode_with_platform_backend(encode_input)?;

    if let Some(path) = prepared_video.temporary_path.as_ref() {
        let _ = remove_file_if_exists(path);
    }
    for audio_input in &prepared_audio_inputs {
        let _ = remove_file_if_exists(&audio_input.path);
    }

    Ok(encoding_result_from_backend(backend_output, backend_label))
}

#[cfg(target_os = "macos")]
fn encode_with_platform_backend(input: EncodeInput) -> Result<(EncodeOutput, String), String> {
    let encoder = MacosRecordingEncoder::new();
    let backend_label = encoder.backend_label();
    let output = encoder.encode(input)?;

    Ok((output, backend_label))
}

#[cfg(target_os = "windows")]
fn encode_with_platform_backend(input: EncodeInput) -> Result<(EncodeOutput, String), String> {
    let encoder = WindowsRecordingEncoder::new();
    let backend_label = encoder.backend_label();
    let output = encoder.encode(input)?;

    Ok((output, backend_label))
}

#[cfg(target_os = "linux")]
fn encode_with_platform_backend(input: EncodeInput) -> Result<(EncodeOutput, String), String> {
    let encoder = LinuxGstreamerRecordingEncoder::new()?;
    let backend_label = encoder.backend_label();
    let output = encoder.encode(input)?;

    Ok((output, backend_label))
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn encode_with_platform_backend(_input: EncodeInput) -> Result<(EncodeOutput, String), String> {
    let readiness = crate::media::readiness::selected_media_backend_readiness();
    let action = readiness.user_action.unwrap_or_else(|| {
        "Run Metafy Desktop on macOS, Windows, or Linux for media processing.".to_owned()
    });

    Err(format!(
        "No media encoding backend is available for this platform. {action}"
    ))
}

fn encoding_result_from_backend(output: EncodeOutput, backend_label: String) -> EncodingResult {
    let backend_id = output.diagnostics.backend.clone();
    let backend_commands = output.diagnostics.commands.clone();
    let backend_messages = output.diagnostics.messages.clone();

    EncodingResult {
        recording_id: output.recording_id,
        media_path: output.media_path,
        thumbnail_path: output.thumbnail_path,
        absolute_media_path: output.absolute_media_path,
        absolute_thumbnail_path: output.absolute_thumbnail_path,
        duration_ms: output.duration_ms,
        width: output.width,
        height: output.height,
        frame_rate: output.frame_rate,
        frame_count: output.frame_count,
        audio_included: output.audio_included,
        media_info: output.media_info,
        backend_id,
        backend_label,
        backend_commands,
        backend_messages,
        warnings: output.warnings,
    }
}

fn prepare_video_input(
    source_path: &Path,
    raw_output_path: &Path,
    expected_width: Option<i64>,
    expected_height: Option<i64>,
    warnings: &mut Vec<String>,
) -> Result<PreparedVideo, String> {
    if try_read_chunked_video_manifest(source_path)?.is_some() {
        return prepare_chunked_video(source_path, warnings);
    }

    let mut prepared = prepare_video_frames(
        source_path,
        raw_output_path,
        expected_width,
        expected_height,
    )?;
    prepared.path = raw_output_path.to_path_buf();
    prepared.format = EncodeVideoFormat::RawBgra;
    prepared.temporary_path = Some(raw_output_path.to_path_buf());

    Ok(prepared)
}

fn prepare_chunked_video(
    source_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<PreparedVideo, String> {
    let manifest = read_chunked_video_manifest(source_path)?;
    if manifest.status != ChunkedVideoStatus::Completed {
        warnings.push(
            "Encoding recovered finalized video chunks from an interrupted recording manifest."
                .to_owned(),
        );
    }
    let thumbnail_frame = read_chunked_thumbnail(source_path, &manifest)?;

    Ok(PreparedVideo {
        path: source_path.to_path_buf(),
        format: EncodeVideoFormat::ChunkedH264Segments,
        temporary_path: None,
        width: i64::from(manifest.width),
        height: i64::from(manifest.height),
        frame_count: manifest
            .frame_count
            .try_into()
            .map_err(|_| "Chunked video frame count is too large.".to_owned())?,
        thumbnail_frame,
    })
}

fn prepare_video_frames(
    source_path: &Path,
    output_path: &Path,
    expected_width: Option<i64>,
    expected_height: Option<i64>,
) -> Result<PreparedVideo, String> {
    let mut reader = RawVideoReader::open(source_path)?;
    let mut writer = BufWriter::new(
        File::create(output_path)
            .map_err(|error| format!("Unable to prepare video encoding input: {error}"))?,
    );
    let mut frame_count = 0_i64;
    let mut width = expected_width.unwrap_or_default();
    let mut height = expected_height.unwrap_or_default();
    let mut output_dimensions = None;
    let mut thumbnail_frame = None;

    while let Some(frame) = reader.next_frame()? {
        let frame_width = i64::from(frame.width);
        let frame_height = i64::from(frame.height);

        if width == 0 {
            width = frame_width;
        }
        if height == 0 {
            height = frame_height;
        }
        if frame_width != width || frame_height != height {
            return Err(format!(
                "Captured frame dimensions changed from {width}x{height} to {frame_width}x{frame_height}."
            ));
        }

        let (output_width, output_height) = *output_dimensions
            .get_or_insert_with(|| bounded_encode_dimensions(frame.width, frame.height));
        let output_bytes = if frame.width == output_width && frame.height == output_height {
            frame.bytes
        } else {
            resize_bgra_nearest(
                &frame.bytes,
                frame.width,
                frame.height,
                output_width,
                output_height,
            )?
        };

        if thumbnail_frame.is_none() {
            thumbnail_frame = Some(BgraFrame {
                width: output_width,
                height: output_height,
                bytes: output_bytes.clone(),
            });
        }

        writer
            .write_all(&output_bytes)
            .map_err(|error| format!("Unable to write video encoding input: {error}"))?;
        frame_count += 1;
    }

    writer
        .flush()
        .map_err(|error| format!("Unable to flush video encoding input: {error}"))?;

    if frame_count == 0 {
        return Err("Captured screen frame file had no frames.".to_owned());
    }

    let (output_width, output_height) =
        output_dimensions.ok_or_else(|| "Captured screen frame file had no frames.".to_owned())?;

    Ok(PreparedVideo {
        path: output_path.to_path_buf(),
        format: EncodeVideoFormat::RawBgra,
        temporary_path: Some(output_path.to_path_buf()),
        width: i64::from(output_width),
        height: i64::from(output_height),
        frame_count,
        thumbnail_frame: thumbnail_frame
            .ok_or_else(|| "Captured screen frame file had no thumbnail frame.".to_owned())?,
    })
}

fn bounded_encode_dimensions(width: u32, height: u32) -> (u32, u32) {
    if width <= MAX_ENCODE_WIDTH && height <= MAX_ENCODE_HEIGHT {
        return (even_encode_dimension(width), even_encode_dimension(height));
    }

    let source_width = u64::from(width);
    let source_height = u64::from(height);
    let max_width = u64::from(MAX_ENCODE_WIDTH);
    let max_height = u64::from(MAX_ENCODE_HEIGHT);

    let (scaled_width, scaled_height) = if source_width * max_height > max_width * source_height {
        (
            MAX_ENCODE_WIDTH,
            rounded_ratio_u32(source_height * max_width, source_width),
        )
    } else {
        (
            rounded_ratio_u32(source_width * max_height, source_height),
            MAX_ENCODE_HEIGHT,
        )
    };

    (
        even_encode_dimension(scaled_width.clamp(1, MAX_ENCODE_WIDTH)),
        even_encode_dimension(scaled_height.clamp(1, MAX_ENCODE_HEIGHT)),
    )
}

fn rounded_ratio_u32(numerator: u64, denominator: u64) -> u32 {
    if denominator == 0 {
        return 1;
    }

    ((numerator + (denominator / 2)) / denominator)
        .max(1)
        .try_into()
        .unwrap_or(u32::MAX)
}

fn even_encode_dimension(value: u32) -> u32 {
    if value <= 2 {
        return value.max(1);
    }

    value - (value % 2)
}

fn resize_bgra_nearest(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    output_width: u32,
    output_height: u32,
) -> Result<Vec<u8>, String> {
    let expected_source_byte_count = bgra_byte_count(source_width, source_height)?;
    if source.len() != expected_source_byte_count {
        return Err(format!(
            "Captured BGRA frame has {} bytes; expected {expected_source_byte_count} for {source_width}x{source_height}.",
            source.len()
        ));
    }

    let mut output = vec![0_u8; bgra_byte_count(output_width, output_height)?];
    let source_width = source_width as usize;
    let source_height = source_height as usize;
    let output_width = output_width as usize;
    let output_height = output_height as usize;

    for target_y in 0..output_height {
        let source_y = (target_y * source_height) / output_height;
        for target_x in 0..output_width {
            let source_x = (target_x * source_width) / output_width;
            let source_index = (source_y * source_width + source_x) * BGRA_BYTES_PER_PIXEL;
            let output_index = (target_y * output_width + target_x) * BGRA_BYTES_PER_PIXEL;

            output[output_index..output_index + BGRA_BYTES_PER_PIXEL]
                .copy_from_slice(&source[source_index..source_index + BGRA_BYTES_PER_PIXEL]);
        }
    }

    Ok(output)
}

fn bgra_byte_count(width: u32, height: u32) -> Result<usize, String> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(BGRA_BYTES_PER_PIXEL))
        .ok_or_else(|| "BGRA frame dimensions are too large.".to_owned())
}

fn prepare_encoding_audio_inputs(
    inputs: Vec<AudioSidecarInput>,
    staging_path: PathBuf,
    warnings: &mut Vec<String>,
) -> Result<Option<PreparedAudioInput>, String> {
    let mut sources = Vec::new();

    for input in inputs {
        if !input.path.exists() {
            warnings.push(format!(
                "{} was requested but the capture file was missing; encoded without this source.",
                input.label
            ));
            continue;
        }

        let format = match RawAudioFormat::from_session_metadata(
            input.sample_rate,
            input.channels,
            input.sample_format,
        ) {
            Ok(format) => format,
            Err(error) => {
                warnings.push(audio_metadata_warning(input.label, error));
                continue;
            }
        };
        let mut reader = match RawAudioReader::open(&input.path, format) {
            Ok(reader) => reader,
            Err(error) => {
                warnings.push(format!(
                    "Unable to read captured {}: {error}; encoded without this source.",
                    input.label
                ));
                continue;
            }
        };
        let source = match prepare_audio_source(&mut reader) {
            Ok(source) => source,
            Err(error) => {
                warnings.push(format!(
                    "Unable to prepare captured {}: {error}; encoded without this source.",
                    input.label
                ));
                continue;
            }
        };

        if source.is_empty() {
            warnings.push(format!(
                "{} capture had no audio samples; encoded without this source.",
                input.label
            ));
            continue;
        }
        if source.is_silent() {
            warnings.push(format!(
                "{} capture contained only silence; encoded without this source.",
                input.label
            ));
            continue;
        }

        sources.push(source);
    }

    let mixed = mix_audio_sources(&sources);
    if mixed.is_empty() {
        let _ = remove_file_if_exists(&staging_path);
        return Ok(None);
    }
    let duration_ms = i64::try_from(mixed.frame_count())
        .ok()
        .and_then(|frame_count| duration_ms_from_frames(frame_count, mixed.sample_rate));

    let mut bytes = Vec::new();
    write_f32le_samples(&mixed.samples, &mut bytes);
    let mut writer = BufWriter::new(
        File::create(&staging_path)
            .map_err(|error| format!("Unable to prepare audio encoding input: {error}"))?,
    );
    writer
        .write_all(&bytes)
        .map_err(|error| format!("Unable to write audio encoding input: {error}"))?;
    writer
        .flush()
        .map_err(|error| format!("Unable to flush audio encoding input: {error}"))?;

    Ok(Some(PreparedAudioInput {
        path: staging_path,
        sample_rate: FINAL_ENCODE_SAMPLE_RATE,
        channels: FINAL_ENCODE_CHANNELS,
        sample_format: FINAL_ENCODE_SAMPLE_FORMAT.to_owned(),
        duration_ms,
    }))
}

fn audio_metadata_warning(label: &str, error: RawAudioMetadataError) -> String {
    match error {
        RawAudioMetadataError::MissingSampleRate => {
            format!("{label} sample rate was missing; encoded without this source.")
        }
        RawAudioMetadataError::MissingChannels => {
            format!("{label} channel count was missing; encoded without this source.")
        }
        RawAudioMetadataError::MissingSampleFormat
        | RawAudioMetadataError::UnsupportedSampleFormat => {
            format!("{label} sample format was unsupported by the encoder; encoded without this source.")
        }
    }
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Unable to remove stale encoding file: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::sidecar::{AUDIO_FILE_MAGIC, BGRA_FORMAT_CODE, VIDEO_FILE_MAGIC};
    use crate::recorder;
    use crate::storage::{
        initialize_at, CaptureAudioMode, CreateRecordingInput, CreateRecordingSessionInput,
        FinishRecordingSessionInput, RecordingSessionStatus, StorageState,
    };
    use uuid::Uuid;

    #[test]
    fn encodes_synthetic_capture_into_mp4_and_thumbnail() {
        if should_skip_synthetic_encode_tests() {
            return;
        }

        let root = std::env::temp_dir().join(format!("metafy-encoding-test-{}", Uuid::new_v4()));
        let state = initialize_at(root.clone()).expect("initialize test storage");
        let recording = state
            .create_recording(CreateRecordingInput {
                title: Some("Synthetic encode".to_owned()),
                captured_at: Some(recorder::current_timestamp_string()),
                media_path: None,
            })
            .expect("create recording");
        let files = state
            .prepare_recording_session_files(
                "synthetic-session",
                &crate::storage::CaptureAudioMode::None,
            )
            .expect("prepare session files");
        write_synthetic_video(&files.video_path, 3, 2, 2).expect("write synthetic video");

        let session = state
            .create_recording_session(CreateRecordingSessionInput {
                id: "synthetic-session".to_owned(),
                recording_id: recording.id.clone(),
                temp_directory: files.temp_directory_relative,
                video_path: files.video_path_relative,
                audio_path: None,
                metadata_path: files.metadata_path_relative,
                video_source_id: "display:1".to_owned(),
                screen_source_id: "display:1".to_owned(),
                video_source_kind: "display".to_owned(),
                video_source_title: "Display 1".to_owned(),
                video_source_app_name: None,
                video_source_process_id: None,
                video_source_window_id: None,
                microphone_device_id: None,
                include_microphone: false,
                audio_mode: crate::storage::CaptureAudioMode::None,
                microphone_audio_path: None,
                source_audio_path: None,
                width: Some(2),
                height: Some(2),
                frame_rate: recorder::frame_rate(),
                audio_sample_rate: None,
                audio_channels: None,
                audio_sample_format: None,
                microphone_audio_sample_rate: None,
                microphone_audio_channels: None,
                microphone_audio_sample_format: None,
                source_audio_sample_rate: None,
                source_audio_channels: None,
                source_audio_sample_format: None,
                started_at: recorder::current_timestamp_string(),
            })
            .expect("create session");
        state
            .finish_recording_session(FinishRecordingSessionInput {
                id: session.id,
                status: RecordingSessionStatus::Stopped,
                width: Some(2),
                height: Some(2),
                frame_count: 3,
                audio_byte_count: 0,
                audio_sample_rate: None,
                audio_channels: None,
                audio_sample_format: None,
                microphone_audio_byte_count: 0,
                microphone_audio_sample_rate: None,
                microphone_audio_channels: None,
                microphone_audio_sample_format: None,
                source_audio_byte_count: 0,
                source_audio_sample_rate: None,
                source_audio_channels: None,
                source_audio_sample_format: None,
                stopped_at: recorder::current_timestamp_string(),
                duration_ms: 100,
                failure_message: None,
            })
            .expect("finish session");

        let result = encode_recording(&state, &recording.id).expect("encode recording");

        assert_eq!(result.frame_count, 3);
        assert_eq!(result.width, 2);
        assert_eq!(result.height, 2);
        assert_eq!(result.duration_ms, Some(100));
        assert_eq!(result.media_info.duration_ms, Some(100));
        assert_eq!(
            result.media_info.file_size_bytes,
            fs::metadata(&result.absolute_media_path)
                .ok()
                .map(|metadata| metadata.len())
        );
        assert!(result.backend_commands.is_empty());
        assert_expected_backend(&result);
        assert!(Path::new(&result.absolute_media_path).is_file());
        assert!(result
            .absolute_thumbnail_path
            .as_deref()
            .map(Path::new)
            .is_some_and(Path::is_file));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn encodes_synthetic_capture_across_audio_modes() {
        if should_skip_synthetic_encode_tests() {
            return;
        }

        let root = std::env::temp_dir().join(format!("metafy-encoding-test-{}", Uuid::new_v4()));
        let state = initialize_at(root.clone()).expect("initialize test storage");
        let cases = [
            ("video-only", CaptureAudioMode::None, false, false),
            ("microphone-only", CaptureAudioMode::Microphone, true, false),
            ("source-only", CaptureAudioMode::Source, false, true),
            (
                "microphone-and-source",
                CaptureAudioMode::MicrophoneAndSource,
                true,
                true,
            ),
        ];

        for (session_id, audio_mode, write_microphone, write_source) in cases {
            let recording_id = create_finished_synthetic_recording(
                &state,
                session_id,
                audio_mode,
                write_microphone,
                write_source,
            );

            let result = encode_recording(&state, &recording_id).expect("encode recording");

            assert_eq!(result.audio_included, write_microphone || write_source);
            assert!(Path::new(&result.absolute_media_path).is_file());
            assert!(
                !result
                    .warnings
                    .iter()
                    .any(|warning| warning.contains("encoded without")),
                "{:?}",
                result.warnings
            );
            assert_expected_backend(&result);
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_encode_dimensions_preserves_safe_even_frames() {
        assert_eq!(bounded_encode_dimensions(1280, 720), (1280, 720));
        assert_eq!(bounded_encode_dimensions(1279, 719), (1278, 718));
    }

    #[test]
    fn bounded_encode_dimensions_downscales_retina_display_capture() {
        assert_eq!(bounded_encode_dimensions(3600, 2338), (1662, 1080));
    }

    #[test]
    fn resize_bgra_nearest_samples_source_pixels() {
        let source = bgra_pixels(&[
            [1, 0, 0, 255],
            [2, 0, 0, 255],
            [3, 0, 0, 255],
            [4, 0, 0, 255],
            [5, 0, 0, 255],
            [6, 0, 0, 255],
            [7, 0, 0, 255],
            [8, 0, 0, 255],
        ]);

        let output = resize_bgra_nearest(&source, 4, 2, 2, 2).expect("resize");

        assert_eq!(
            output,
            bgra_pixels(&[
                [1, 0, 0, 255],
                [3, 0, 0, 255],
                [5, 0, 0, 255],
                [7, 0, 0, 255],
            ])
        );
    }

    #[test]
    fn prepare_video_frames_downscales_oversized_sidecar() {
        let root = std::env::temp_dir().join(format!("metafy-video-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create test root");
        let source_path = root.join("oversized.mfrv");
        let output_path = root.join("encoding-video.bgra");
        write_synthetic_video(&source_path, 1, 1930, 1080).expect("write oversized video");

        let prepared = prepare_video_frames(&source_path, &output_path, Some(1930), Some(1080))
            .expect("prepare video");

        assert_eq!(prepared.width, 1920);
        assert_eq!(prepared.height, 1074);
        assert_eq!(prepared.thumbnail_frame.width, 1920);
        assert_eq!(prepared.thumbnail_frame.height, 1074);
        assert_eq!(
            fs::metadata(&output_path).expect("output metadata").len(),
            u64::from(1920_u32 * 1074 * 4)
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_encoding_audio_inputs_warns_for_missing_empty_silent_and_unsupported_sources() {
        let root = std::env::temp_dir().join(format!("metafy-audio-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create test root");

        let mut warnings = Vec::new();
        let missing = prepare_encoding_audio_inputs(
            vec![encoding_audio_input(
                root.join("missing.pcm"),
                "microphone audio",
                Some(48_000),
                Some(2),
                Some("f32"),
            )],
            root.join("missing.raw"),
            &mut warnings,
        )
        .expect("prepare missing audio");
        assert!(missing.is_none());
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("capture file was missing")));

        let empty_path = root.join("empty.pcm");
        write_synthetic_audio(&empty_path, &[]).expect("write empty audio");
        let empty_staging_path = root.join("empty.raw");
        let empty = prepare_encoding_audio_inputs(
            vec![encoding_audio_input(
                empty_path,
                "source audio",
                Some(48_000),
                Some(2),
                Some("f32"),
            )],
            empty_staging_path.clone(),
            &mut warnings,
        )
        .expect("prepare empty audio");
        assert!(empty.is_none());
        assert!(!empty_staging_path.exists());
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("had no audio samples")));

        let silent_path = root.join("silent.pcm");
        write_synthetic_audio(&silent_path, &[&synthetic_f32_stereo_block(1, 0.0)])
            .expect("write silent audio");
        let silent_staging_path = root.join("silent.raw");
        let silent = prepare_encoding_audio_inputs(
            vec![encoding_audio_input(
                silent_path,
                "source audio",
                Some(48_000),
                Some(2),
                Some("f32"),
            )],
            silent_staging_path.clone(),
            &mut warnings,
        )
        .expect("prepare silent audio");
        assert!(silent.is_none());
        assert!(!silent_staging_path.exists());
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("contained only silence")));

        let unsupported_path = root.join("unsupported.pcm");
        write_synthetic_audio(&unsupported_path, &[&[0, 1, 2, 3]]).expect("write audio");
        let unsupported_staging_path = root.join("unsupported.raw");
        let unsupported = prepare_encoding_audio_inputs(
            vec![encoding_audio_input(
                unsupported_path,
                "source audio",
                Some(48_000),
                Some(2),
                Some("pcm24"),
            )],
            unsupported_staging_path.clone(),
            &mut warnings,
        )
        .expect("prepare unsupported audio");
        assert!(unsupported.is_none());
        assert!(!unsupported_staging_path.exists());
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("sample format was unsupported")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_encoding_audio_inputs_writes_prepared_equal_gain_mix() {
        let root = std::env::temp_dir().join(format!("metafy-audio-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create test root");
        let microphone_path = root.join("microphone.pcm");
        let source_path = root.join("source.pcm");
        let staging_path = root.join(STAGING_AUDIO_FILE_NAME);
        write_synthetic_audio(&microphone_path, &[&synthetic_f32_stereo_block(2, 0.2)])
            .expect("write microphone");
        write_synthetic_audio(&source_path, &[&synthetic_f32_stereo_block(1, 0.4)])
            .expect("write source");

        let mut warnings = Vec::new();
        let prepared = prepare_encoding_audio_inputs(
            vec![
                encoding_audio_input(
                    microphone_path,
                    "microphone audio",
                    Some(48_000),
                    Some(2),
                    Some("f32"),
                ),
                encoding_audio_input(
                    source_path,
                    "source audio",
                    Some(48_000),
                    Some(2),
                    Some("f32"),
                ),
            ],
            staging_path.clone(),
            &mut warnings,
        )
        .expect("prepare audio")
        .expect("prepared audio");

        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(prepared.sample_rate, FINAL_ENCODE_SAMPLE_RATE);
        assert_eq!(prepared.channels, FINAL_ENCODE_CHANNELS);
        assert_eq!(prepared.sample_format, FINAL_ENCODE_SAMPLE_FORMAT);
        assert_eq!(prepared.path, staging_path);
        assert_eq!(
            fs::metadata(&prepared.path)
                .expect("prepared metadata")
                .len(),
            4 * std::mem::size_of::<f32>() as u64
        );
        let samples = read_f32le_samples(&prepared.path).expect("read prepared samples");
        assert_eq!(samples.len(), 4);
        assert_approx(samples[0], 0.3);
        assert_approx(samples[1], 0.3);
        assert_approx(samples[2], 0.1);
        assert_approx(samples[3], 0.1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_encoding_audio_inputs_keeps_valid_source_when_another_is_incomplete() {
        let root = std::env::temp_dir().join(format!("metafy-audio-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create test root");
        let microphone_path = root.join("microphone.pcm");
        let source_path = root.join("truncated-source.pcm");
        let staging_path = root.join(STAGING_AUDIO_FILE_NAME);
        write_synthetic_audio(&microphone_path, &[&synthetic_f32_stereo_block(1, 0.25)])
            .expect("write microphone");
        fs::write(&source_path, [AUDIO_FILE_MAGIC, &[0_u8, 1_u8]].concat())
            .expect("write truncated source");

        let mut warnings = Vec::new();
        let prepared = prepare_encoding_audio_inputs(
            vec![
                encoding_audio_input(
                    microphone_path,
                    "microphone audio",
                    Some(48_000),
                    Some(2),
                    Some("f32"),
                ),
                encoding_audio_input(
                    source_path,
                    "source audio",
                    Some(48_000),
                    Some(2),
                    Some("f32"),
                ),
            ],
            staging_path,
            &mut warnings,
        )
        .expect("prepare audio")
        .expect("valid source still prepares");

        assert!(warnings
            .iter()
            .any(|warning| warning.contains("Unable to prepare captured source audio")));
        let samples = read_f32le_samples(&prepared.path).expect("read prepared samples");
        assert_eq!(samples.len(), 2);
        assert_approx(samples[0], 0.25);
        assert_approx(samples[1], 0.25);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_encoding_audio_inputs_omits_silent_source_without_attenuating_valid_source() {
        let root = std::env::temp_dir().join(format!("metafy-audio-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create test root");
        let microphone_path = root.join("microphone.pcm");
        let source_path = root.join("silent-source.pcm");
        let staging_path = root.join(STAGING_AUDIO_FILE_NAME);
        write_synthetic_audio(&microphone_path, &[&synthetic_f32_stereo_block(1, 0.5)])
            .expect("write microphone");
        write_synthetic_audio(&source_path, &[&synthetic_f32_stereo_block(1, 0.0)])
            .expect("write silent source");

        let mut warnings = Vec::new();
        let prepared = prepare_encoding_audio_inputs(
            vec![
                encoding_audio_input(
                    microphone_path,
                    "microphone audio",
                    Some(48_000),
                    Some(2),
                    Some("f32"),
                ),
                encoding_audio_input(
                    source_path,
                    "source audio",
                    Some(48_000),
                    Some(2),
                    Some("f32"),
                ),
            ],
            staging_path,
            &mut warnings,
        )
        .expect("prepare audio")
        .expect("valid source still prepares");

        assert!(warnings
            .iter()
            .any(|warning| warning.contains("source audio capture contained only silence")));
        let samples = read_f32le_samples(&prepared.path).expect("read prepared samples");
        assert_eq!(samples.len(), 2);
        assert_approx(samples[0], 0.5);
        assert_approx(samples[1], 0.5);

        let _ = fs::remove_dir_all(root);
    }

    fn should_skip_synthetic_encode_tests() -> bool {
        let readiness = crate::media::readiness::selected_media_backend_readiness();

        !readiness.available
    }

    fn assert_expected_backend(result: &EncodingResult) {
        if cfg!(target_os = "macos") {
            assert_eq!(result.media_info.backend_id, "macos-avfoundation");
            assert_eq!(result.backend_id, "macos-avfoundation");
            assert_eq!(result.backend_label, "native-macos-avfoundation");
            assert!(result
                .backend_messages
                .iter()
                .any(|message| message.contains("AVFoundation")));
        } else if cfg!(target_os = "windows") {
            assert_eq!(result.media_info.backend_id, "windows-media-foundation");
            assert_eq!(result.backend_id, "windows-media-foundation");
            assert_eq!(result.backend_label, "native-windows-media-foundation");
            assert!(result
                .backend_messages
                .iter()
                .any(|message| message.contains("Media Foundation")));
        } else if cfg!(target_os = "linux") {
            assert_eq!(result.media_info.backend_id, "linux-gstreamer");
            assert_eq!(result.backend_id, "linux-gstreamer");
            assert_eq!(result.backend_label, "native-linux-gstreamer");
            assert!(result
                .backend_messages
                .iter()
                .any(|message| message.contains("GStreamer")));
        }
    }

    fn create_finished_synthetic_recording(
        state: &StorageState,
        session_id: &str,
        audio_mode: CaptureAudioMode,
        write_microphone: bool,
        write_source: bool,
    ) -> String {
        let recording = state
            .create_recording(CreateRecordingInput {
                title: Some(session_id.to_owned()),
                captured_at: Some(recorder::current_timestamp_string()),
                media_path: None,
            })
            .expect("create recording");
        let files = state
            .prepare_recording_session_files(session_id, &audio_mode)
            .expect("prepare session files");
        write_synthetic_video(&files.video_path, 3, 2, 2).expect("write synthetic video");

        let microphone_block = synthetic_f32_stereo_block(480, 0.2);
        let source_block = synthetic_f32_stereo_block(480, 0.4);
        if write_microphone {
            write_synthetic_audio(
                files
                    .microphone_audio_path
                    .as_deref()
                    .expect("microphone path"),
                &[microphone_block.as_slice()],
            )
            .expect("write microphone audio");
        }
        if write_source {
            write_synthetic_audio(
                files.source_audio_path.as_deref().expect("source path"),
                &[source_block.as_slice()],
            )
            .expect("write source audio");
        }

        let session = state
            .create_recording_session(CreateRecordingSessionInput {
                id: session_id.to_owned(),
                recording_id: recording.id.clone(),
                temp_directory: files.temp_directory_relative,
                video_path: files.video_path_relative,
                audio_path: files.audio_path_relative.clone(),
                metadata_path: files.metadata_path_relative,
                video_source_id: "display:1".to_owned(),
                screen_source_id: "display:1".to_owned(),
                video_source_kind: "display".to_owned(),
                video_source_title: "Display 1".to_owned(),
                video_source_app_name: None,
                video_source_process_id: None,
                video_source_window_id: None,
                microphone_device_id: write_microphone.then(|| "mic:1".to_owned()),
                include_microphone: audio_mode.includes_microphone(),
                audio_mode,
                microphone_audio_path: files.microphone_audio_path_relative.clone(),
                source_audio_path: files.source_audio_path_relative.clone(),
                width: Some(2),
                height: Some(2),
                frame_rate: recorder::frame_rate(),
                audio_sample_rate: write_microphone.then_some(48_000),
                audio_channels: write_microphone.then_some(2),
                audio_sample_format: write_microphone.then(|| "f32".to_owned()),
                microphone_audio_sample_rate: write_microphone.then_some(48_000),
                microphone_audio_channels: write_microphone.then_some(2),
                microphone_audio_sample_format: write_microphone.then(|| "f32".to_owned()),
                source_audio_sample_rate: write_source.then_some(48_000),
                source_audio_channels: write_source.then_some(2),
                source_audio_sample_format: write_source.then(|| "f32".to_owned()),
                started_at: recorder::current_timestamp_string(),
            })
            .expect("create session");
        state
            .finish_recording_session(FinishRecordingSessionInput {
                id: session.id,
                status: RecordingSessionStatus::Stopped,
                width: Some(2),
                height: Some(2),
                frame_count: 3,
                audio_byte_count: if write_microphone {
                    microphone_block.len() as i64
                } else {
                    0
                },
                audio_sample_rate: write_microphone.then_some(48_000),
                audio_channels: write_microphone.then_some(2),
                audio_sample_format: write_microphone.then(|| "f32".to_owned()),
                microphone_audio_byte_count: if write_microphone {
                    microphone_block.len() as i64
                } else {
                    0
                },
                microphone_audio_sample_rate: write_microphone.then_some(48_000),
                microphone_audio_channels: write_microphone.then_some(2),
                microphone_audio_sample_format: write_microphone.then(|| "f32".to_owned()),
                source_audio_byte_count: if write_source {
                    source_block.len() as i64
                } else {
                    0
                },
                source_audio_sample_rate: write_source.then_some(48_000),
                source_audio_channels: write_source.then_some(2),
                source_audio_sample_format: write_source.then(|| "f32".to_owned()),
                stopped_at: recorder::current_timestamp_string(),
                duration_ms: 100,
                failure_message: None,
            })
            .expect("finish session");

        recording.id
    }

    fn synthetic_f32_stereo_block(frame_count: usize, sample: f32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(frame_count * 2 * std::mem::size_of::<f32>());
        for _frame in 0..frame_count {
            bytes.extend_from_slice(&sample.to_le_bytes());
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    fn encoding_audio_input(
        path: PathBuf,
        label: &'static str,
        sample_rate: Option<i64>,
        channels: Option<i64>,
        sample_format: Option<&str>,
    ) -> AudioSidecarInput {
        AudioSidecarInput {
            path,
            label,
            sample_rate,
            channels,
            sample_format: sample_format.map(str::to_owned),
        }
    }

    fn write_synthetic_video(
        path: &Path,
        frame_count: u32,
        width: u32,
        height: u32,
    ) -> io::Result<()> {
        let mut writer = BufWriter::new(File::create(path)?);
        writer.write_all(VIDEO_FILE_MAGIC)?;

        for frame_index in 0..frame_count {
            let elapsed_ms = u64::from(frame_index) * 33;
            let mut bytes = Vec::with_capacity((width * height * 4) as usize);
            for _pixel in 0..(width * height) {
                bytes.extend_from_slice(&[0x20, 0x80, 0xc0, 0xff]);
            }

            writer.write_all(&elapsed_ms.to_le_bytes())?;
            writer.write_all(&elapsed_ms.to_le_bytes())?;
            writer.write_all(&BGRA_FORMAT_CODE.to_le_bytes())?;
            writer.write_all(&width.to_le_bytes())?;
            writer.write_all(&height.to_le_bytes())?;
            writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
            writer.write_all(&bytes)?;
        }

        writer.flush()
    }

    fn write_synthetic_audio(path: &Path, blocks: &[&[u8]]) -> io::Result<()> {
        let mut writer = BufWriter::new(File::create(path)?);
        writer.write_all(AUDIO_FILE_MAGIC)?;

        for (block_index, block) in blocks.iter().enumerate() {
            let elapsed_ms = block_index as u64 * 10;
            writer.write_all(&elapsed_ms.to_le_bytes())?;
            writer.write_all(&elapsed_ms.to_le_bytes())?;
            writer.write_all(&elapsed_ms.to_le_bytes())?;
            writer.write_all(&(block.len() as u32).to_le_bytes())?;
            writer.write_all(block)?;
        }

        writer.flush()
    }

    fn read_f32le_samples(path: &Path) -> io::Result<Vec<f32>> {
        let bytes = fs::read(path)?;
        Ok(bytes
            .chunks_exact(std::mem::size_of::<f32>())
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect())
    }

    fn bgra_pixels(pixels: &[[u8; BGRA_BYTES_PER_PIXEL]]) -> Vec<u8> {
        pixels.iter().flatten().copied().collect()
    }

    fn assert_approx(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.0001,
            "{actual} did not match {expected}"
        );
    }
}
