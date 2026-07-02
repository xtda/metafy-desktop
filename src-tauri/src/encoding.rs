use serde::Serialize;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::binaries::{find_binary, missing_binary_message};
use crate::storage::{RecordingSession, StorageState};

const VIDEO_FILE_MAGIC: &[u8] = b"METAFY_RAW_VIDEO_V1\n";
const AUDIO_FILE_MAGIC: &[u8] = b"METAFY_RAW_AUDIO_V1\n";
const BGRA_FORMAT_CODE: u32 = 7;

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
    pub ffmpeg_path: String,
    pub ffmpeg_args: Vec<String>,
    pub thumbnail_args: Vec<String>,
    pub ffprobe_path: Option<String>,
    pub ffprobe_json: Option<Value>,
    pub warnings: Vec<String>,
}

struct PreparedVideo {
    width: i64,
    height: i64,
    frame_count: i64,
}

struct PreparedAudio {
    byte_count: i64,
}

struct EncodingAudioInput {
    path: PathBuf,
    label: &'static str,
    sample_rate: Option<i64>,
    channels: Option<i64>,
    sample_format: Option<String>,
}

struct CommandOutput {
    program: PathBuf,
    args: Vec<String>,
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
    let encoding_audio = select_encoding_audio_input(storage, &session, &mut warnings);
    let staging_video_path = media_files.recording_directory.join("encoding-video.bgra");
    let staging_audio_path = media_files.recording_directory.join("encoding-audio.raw");
    let staging_media_path = media_files.recording_directory.join("recording.tmp.mp4");
    let staging_thumbnail_path = media_files.recording_directory.join("thumbnail.tmp.jpg");

    remove_file_if_exists(&staging_video_path)?;
    remove_file_if_exists(&staging_audio_path)?;
    remove_file_if_exists(&staging_media_path)?;
    remove_file_if_exists(&staging_thumbnail_path)?;

    let prepared_video = prepare_video_frames(
        &source_video_path,
        &staging_video_path,
        session.width,
        session.height,
    )?;
    let prepared_audio = match encoding_audio.as_ref() {
        Some(audio_input) if audio_input.path.exists() => {
            let audio =
                prepare_audio_samples(&audio_input.path, &staging_audio_path, audio_input.label)?;
            if audio.byte_count == 0 {
                warnings.push(format!(
                    "{} capture had no audio samples; encoded video only.",
                    audio_input.label
                ));
                None
            } else {
                Some(audio)
            }
        }
        Some(_) => {
            warnings.push(format!(
                "{} capture file was missing; encoded video only.",
                encoding_audio
                    .as_ref()
                    .map(|audio| audio.label)
                    .unwrap_or("Audio")
            ));
            None
        }
        None => None,
    };
    let audio_included = prepared_audio.is_some();
    let encoded_audio = prepared_audio
        .as_ref()
        .and_then(|_| encoding_audio.as_ref());

    let ffmpeg_path = find_binary("METAFY_FFMPEG_PATH", &["ffmpeg"])
        .ok_or_else(|| missing_binary_message("ffmpeg", "METAFY_FFMPEG_PATH"))?;
    let encode_command = build_encode_command(
        &ffmpeg_path,
        &staging_video_path,
        prepared_video.width,
        prepared_video.height,
        session.frame_rate,
        prepared_audio.as_ref().map(|_| &staging_audio_path),
        encoded_audio.and_then(|audio| audio.sample_rate),
        encoded_audio.and_then(|audio| audio.channels),
        encoded_audio.and_then(|audio| audio.sample_format.as_deref()),
        encoded_audio.map(|audio| audio.label).unwrap_or("Audio"),
        &staging_media_path,
    )?;
    run_logged_command(&encode_command, "FFmpeg encode")?;

    rename_replace(&staging_media_path, &media_files.media_path)?;

    let thumbnail_command = build_thumbnail_command(
        &ffmpeg_path,
        &media_files.media_path,
        &staging_thumbnail_path,
    );
    let thumbnail_result = match run_logged_command(&thumbnail_command, "FFmpeg thumbnail") {
        Ok(()) => {
            rename_replace(&staging_thumbnail_path, &media_files.thumbnail_path)?;
            Some(media_files.thumbnail_path_relative.clone())
        }
        Err(error) => {
            let _ = remove_file_if_exists(&staging_thumbnail_path);
            return Err(format!("Thumbnail generation failed: {error}"));
        }
    };

    let ffprobe_path = find_binary("METAFY_FFPROBE_PATH", &["ffprobe"]);
    let ffprobe_json = match ffprobe_path.as_ref() {
        Some(path) => match run_ffprobe(path, &media_files.media_path) {
            Ok(value) => Some(value),
            Err(error) => {
                warnings.push(format!("FFprobe metadata failed: {error}"));
                None
            }
        },
        None => {
            warnings.push(missing_binary_message("ffprobe", "METAFY_FFPROBE_PATH"));
            None
        }
    };

    let _ = remove_file_if_exists(&staging_video_path);
    let _ = remove_file_if_exists(&staging_audio_path);

    Ok(EncodingResult {
        recording_id: recording_id.to_owned(),
        media_path: media_files.media_path_relative,
        thumbnail_path: thumbnail_result,
        absolute_media_path: path_to_string(&media_files.media_path),
        absolute_thumbnail_path: media_files
            .thumbnail_path
            .exists()
            .then(|| path_to_string(&media_files.thumbnail_path)),
        duration_ms: probe_duration_ms(ffprobe_json.as_ref()).or(session.duration_ms),
        width: prepared_video.width,
        height: prepared_video.height,
        frame_rate: session.frame_rate,
        frame_count: prepared_video.frame_count,
        audio_included,
        ffmpeg_path: path_to_string(&ffmpeg_path),
        ffmpeg_args: encode_command.args,
        thumbnail_args: thumbnail_command.args,
        ffprobe_path: ffprobe_path.as_ref().map(|path| path_to_string(path)),
        ffprobe_json,
        warnings,
    })
}

fn select_encoding_audio_input(
    storage: &StorageState,
    session: &RecordingSession,
    warnings: &mut Vec<String>,
) -> Option<EncodingAudioInput> {
    let microphone = session
        .microphone_audio_path
        .as_deref()
        .map(|path| EncodingAudioInput {
            path: storage.resolve_path(path),
            label: "microphone audio",
            sample_rate: session.microphone_audio_sample_rate,
            channels: session.microphone_audio_channels,
            sample_format: session.microphone_audio_sample_format.clone(),
        });
    let source = session
        .source_audio_path
        .as_deref()
        .map(|path| EncodingAudioInput {
            path: storage.resolve_path(path),
            label: "source audio",
            sample_rate: session.source_audio_sample_rate,
            channels: session.source_audio_channels,
            sample_format: session.source_audio_sample_format.clone(),
        });

    match (microphone, source) {
        (Some(microphone), Some(_source)) => {
            warnings.push(
                "Source audio was captured separately; final MP4 encoding currently uses microphone audio only."
                    .to_owned(),
            );
            Some(microphone)
        }
        (Some(microphone), None) => Some(microphone),
        (None, Some(source)) => Some(source),
        (None, None) => session
            .audio_path
            .as_deref()
            .map(|path| EncodingAudioInput {
                path: storage.resolve_path(path),
                label: "microphone audio",
                sample_rate: session.audio_sample_rate,
                channels: session.audio_channels,
                sample_format: session.audio_sample_format.clone(),
            }),
    }
}

fn prepare_video_frames(
    source_path: &Path,
    output_path: &Path,
    expected_width: Option<i64>,
    expected_height: Option<i64>,
) -> Result<PreparedVideo, String> {
    let mut reader = BufReader::new(
        File::open(source_path)
            .map_err(|error| format!("Unable to open captured screen frames: {error}"))?,
    );
    expect_magic(&mut reader, VIDEO_FILE_MAGIC, "screen frame")?;

    let mut writer = BufWriter::new(
        File::create(output_path)
            .map_err(|error| format!("Unable to prepare video encoding input: {error}"))?,
    );
    let mut frame_count = 0_i64;
    let mut width = expected_width.unwrap_or_default();
    let mut height = expected_height.unwrap_or_default();

    while read_u64_optional(&mut reader)
        .map_err(|error| format!("Unable to read screen frame timestamp: {error}"))?
        .is_some()
    {
        let _display_time_ms = read_u64(&mut reader)?;
        let format_code = read_u32(&mut reader)?;
        let frame_width = i64::from(read_u32(&mut reader)?);
        let frame_height = i64::from(read_u32(&mut reader)?);
        let byte_count = read_u32(&mut reader)? as usize;

        if format_code != BGRA_FORMAT_CODE {
            return Err(format!(
                "Unsupported captured frame format {format_code}; expected BGRA."
            ));
        }
        if frame_width <= 0 || frame_height <= 0 {
            return Err("Captured frame dimensions are invalid.".to_owned());
        }

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

        let expected_byte_count = frame_byte_count(width, height)?;
        if byte_count != expected_byte_count {
            return Err(format!(
                "Captured frame has {byte_count} bytes; expected {expected_byte_count} for {width}x{height} BGRA."
            ));
        }

        let mut frame = vec![0_u8; byte_count];
        reader
            .read_exact(&mut frame)
            .map_err(|error| format!("Unable to read captured screen frame: {error}"))?;
        writer
            .write_all(&frame)
            .map_err(|error| format!("Unable to write video encoding input: {error}"))?;
        frame_count += 1;
    }

    writer
        .flush()
        .map_err(|error| format!("Unable to flush video encoding input: {error}"))?;

    if frame_count == 0 {
        return Err("Captured screen frame file had no frames.".to_owned());
    }

    Ok(PreparedVideo {
        width,
        height,
        frame_count,
    })
}

fn prepare_audio_samples(
    source_path: &Path,
    output_path: &Path,
    label: &str,
) -> Result<PreparedAudio, String> {
    let mut reader = BufReader::new(
        File::open(source_path)
            .map_err(|error| format!("Unable to open captured {label}: {error}"))?,
    );
    expect_magic(&mut reader, AUDIO_FILE_MAGIC, label)?;

    let mut writer = BufWriter::new(
        File::create(output_path)
            .map_err(|error| format!("Unable to prepare audio encoding input: {error}"))?,
    );
    let mut byte_count = 0_i64;

    while read_u64_optional(&mut reader)
        .map_err(|error| format!("Unable to read {label} block timestamp: {error}"))?
        .is_some()
    {
        let _callback_stream_ns = read_u64(&mut reader)?;
        let _capture_stream_ns = read_u64(&mut reader)?;
        let block_byte_count = read_u32(&mut reader)? as usize;
        let mut block = vec![0_u8; block_byte_count];
        reader
            .read_exact(&mut block)
            .map_err(|error| format!("Unable to read {label} block: {error}"))?;
        writer
            .write_all(&block)
            .map_err(|error| format!("Unable to write audio encoding input: {error}"))?;
        byte_count += block_byte_count as i64;
    }

    writer
        .flush()
        .map_err(|error| format!("Unable to flush audio encoding input: {error}"))?;

    Ok(PreparedAudio { byte_count })
}

fn build_encode_command(
    ffmpeg_path: &Path,
    video_path: &Path,
    width: i64,
    height: i64,
    frame_rate: i64,
    audio_path: Option<&PathBuf>,
    audio_sample_rate: Option<i64>,
    audio_channels: Option<i64>,
    audio_sample_format: Option<&str>,
    audio_label: &str,
    output_path: &Path,
) -> Result<CommandOutput, String> {
    let mut args = vec![
        "-hide_banner".to_owned(),
        "-y".to_owned(),
        "-f".to_owned(),
        "rawvideo".to_owned(),
        "-pix_fmt".to_owned(),
        "bgra".to_owned(),
        "-video_size".to_owned(),
        format!("{width}x{height}"),
        "-framerate".to_owned(),
        frame_rate.to_string(),
        "-i".to_owned(),
        path_to_string(video_path),
    ];

    if let Some(audio_path) = audio_path {
        let sample_rate = audio_sample_rate
            .filter(|value| *value > 0)
            .ok_or_else(|| format!("{audio_label} sample rate is missing for audio encoding."))?;
        let channels = audio_channels
            .filter(|value| *value > 0)
            .ok_or_else(|| format!("{audio_label} channel count is missing for audio encoding."))?;
        let sample_format = audio_sample_format
            .and_then(ffmpeg_audio_format)
            .ok_or_else(|| format!("{audio_label} sample format is unsupported by the encoder."))?;

        args.extend([
            "-f".to_owned(),
            sample_format.to_owned(),
            "-ar".to_owned(),
            sample_rate.to_string(),
            "-ac".to_owned(),
            channels.to_string(),
            "-i".to_owned(),
            path_to_string(audio_path),
        ]);
    }

    args.extend(["-map".to_owned(), "0:v:0".to_owned()]);

    if audio_path.is_some() {
        args.extend(["-map".to_owned(), "1:a:0".to_owned()]);
    } else {
        args.push("-an".to_owned());
    }

    args.extend([
        "-c:v".to_owned(),
        "libx264".to_owned(),
        "-preset".to_owned(),
        "veryfast".to_owned(),
        "-crf".to_owned(),
        "23".to_owned(),
        "-pix_fmt".to_owned(),
        "yuv420p".to_owned(),
    ]);

    if audio_path.is_some() {
        args.extend([
            "-c:a".to_owned(),
            "aac".to_owned(),
            "-b:a".to_owned(),
            "160k".to_owned(),
        ]);
    }

    args.extend([
        "-movflags".to_owned(),
        "+faststart".to_owned(),
        path_to_string(output_path),
    ]);

    Ok(CommandOutput {
        program: ffmpeg_path.to_path_buf(),
        args,
    })
}

fn build_thumbnail_command(
    ffmpeg_path: &Path,
    media_path: &Path,
    thumbnail_path: &Path,
) -> CommandOutput {
    CommandOutput {
        program: ffmpeg_path.to_path_buf(),
        args: vec![
            "-hide_banner".to_owned(),
            "-y".to_owned(),
            "-i".to_owned(),
            path_to_string(media_path),
            "-frames:v".to_owned(),
            "1".to_owned(),
            "-vf".to_owned(),
            "scale=480:-1".to_owned(),
            path_to_string(thumbnail_path),
        ],
    }
}

fn run_ffprobe(ffprobe_path: &Path, media_path: &Path) -> Result<Value, String> {
    let args = vec![
        "-v".to_owned(),
        "error".to_owned(),
        "-select_streams".to_owned(),
        "v:0".to_owned(),
        "-show_entries".to_owned(),
        "stream=width,height,r_frame_rate,avg_frame_rate,duration,codec_name:format=duration,size"
            .to_owned(),
        "-of".to_owned(),
        "json".to_owned(),
        path_to_string(media_path),
    ];
    let output = Command::new(ffprobe_path)
        .args(&args)
        .output()
        .map_err(|error| format!("Unable to run ffprobe: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "ffprobe exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Unable to parse ffprobe output: {error}"))
}

fn run_logged_command(command: &CommandOutput, label: &str) -> Result<(), String> {
    let output = Command::new(&command.program)
        .args(&command.args)
        .output()
        .map_err(|error| format!("Unable to run {label}: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{label} exited with status {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    Ok(())
}

fn ffmpeg_audio_format(sample_format: &str) -> Option<&'static str> {
    match sample_format {
        "i8" => Some("s8"),
        "u8" => Some("u8"),
        "i16" => Some("s16le"),
        "u16" => Some("u16le"),
        "i24" | "i32" => Some("s32le"),
        "u24" | "u32" => Some("u32le"),
        "f32" => Some("f32le"),
        "f64" => Some("f64le"),
        _ => None,
    }
}

fn expect_magic(reader: &mut BufReader<File>, magic: &[u8], label: &str) -> Result<(), String> {
    let mut actual = vec![0_u8; magic.len()];
    reader
        .read_exact(&mut actual)
        .map_err(|error| format!("Unable to read {label} capture header: {error}"))?;
    if actual != magic {
        return Err(format!("Captured {label} file has an unsupported format."));
    }
    Ok(())
}

fn read_u64_optional(reader: &mut BufReader<File>) -> io::Result<Option<u64>> {
    let mut first = [0_u8; 1];
    match reader.read(&mut first)? {
        0 => Ok(None),
        1 => {
            let mut bytes = [0_u8; 8];
            bytes[0] = first[0];
            reader.read_exact(&mut bytes[1..])?;
            Ok(Some(u64::from_le_bytes(bytes)))
        }
        _ => unreachable!("single-byte read returned more than one byte"),
    }
}

fn read_u64(reader: &mut BufReader<File>) -> Result<u64, String> {
    let mut bytes = [0_u8; 8];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| format!("Unable to read capture timing data: {error}"))?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_u32(reader: &mut BufReader<File>) -> Result<u32, String> {
    let mut bytes = [0_u8; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| format!("Unable to read capture metadata: {error}"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn frame_byte_count(width: i64, height: i64) -> Result<usize, String> {
    let width = usize::try_from(width).map_err(|_| "Frame width is invalid.".to_owned())?;
    let height = usize::try_from(height).map_err(|_| "Frame height is invalid.".to_owned())?;
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Frame dimensions are too large to encode.".to_owned())
}

fn probe_duration_ms(value: Option<&Value>) -> Option<i64> {
    value?
        .get("format")?
        .get("duration")?
        .as_str()?
        .parse::<f64>()
        .ok()
        .map(|seconds| (seconds * 1000.0).round() as i64)
}

fn rename_replace(source: &Path, destination: &Path) -> Result<(), String> {
    remove_file_if_exists(destination)?;
    fs::rename(source, destination)
        .map_err(|error| format!("Unable to move encoded media into the library: {error}"))
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Unable to remove stale encoding file: {error}")),
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder;
    use crate::storage::{
        initialize_at, CreateRecordingInput, CreateRecordingSessionInput,
        FinishRecordingSessionInput, RecordingSessionStatus,
    };
    use uuid::Uuid;

    #[test]
    fn encodes_synthetic_capture_into_mp4_and_thumbnail() {
        if !ffmpeg_supports_libx264() {
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
        assert!(Path::new(&result.absolute_media_path).is_file());
        assert!(result
            .absolute_thumbnail_path
            .as_deref()
            .map(Path::new)
            .is_some_and(Path::is_file));

        let _ = fs::remove_dir_all(root);
    }

    fn ffmpeg_supports_libx264() -> bool {
        Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).contains("libx264"))
            .unwrap_or(false)
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
}
