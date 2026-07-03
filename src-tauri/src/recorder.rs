use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::capture::{CaptureVideoSourceKind, SourceAudioCaptureConfig, ValidatedCaptureConfig};
#[cfg(target_os = "macos")]
use crate::media::macos_chunked_video::MacosSegmentedVideoWriter;
#[cfg(not(target_os = "macos"))]
use crate::media::sidecar::VIDEO_FILE_MAGIC;
use crate::media::sidecar::{AUDIO_FILE_MAGIC, BGRA_FORMAT_CODE, BGRA_LZ4_FORMAT_CODE};
use crate::storage::{RecordingSession, RecordingSessionFiles, RecordingSessionStatus};

const CAPTURE_FRAME_RATE: u32 = 30;
const BGRA_BYTES_PER_PIXEL: usize = 4;
const OPAQUE_BLACK_BGRA: [u8; BGRA_BYTES_PER_PIXEL] = [0x00, 0x00, 0x00, 0xff];
const MAX_RAW_VIDEO_WIDTH: u32 = 1280;
const MAX_RAW_VIDEO_HEIGHT: u32 = 720;
const CHUNKED_VIDEO_DURATION_SECONDS: u32 = 5;

#[derive(Default)]
pub struct RecordingRuntime {
    active: Mutex<Option<ActiveSession>>,
}

struct ActiveSession {
    session_id: String,
    recording_id: String,
    stop_signal: Arc<AtomicBool>,
    stats: Arc<Mutex<CaptureStats>>,
    screen_handle: Option<JoinHandle<()>>,
    audio_stream: Option<cpal::Stream>,
    audio_sender: Option<mpsc::Sender<AudioBlock>>,
    audio_writer_handle: Option<JoinHandle<()>>,
    microphone_audio_config: Option<AudioConfig>,
    source_audio_writer_handle: Option<JoinHandle<()>>,
    source_audio_config: Option<Arc<Mutex<Option<AudioConfig>>>>,
    started_instant: Instant,
    files: RecordingSessionFiles,
}

#[derive(Debug, Default, Clone)]
struct CaptureStats {
    frame_count: i64,
    microphone_audio_byte_count: i64,
    source_audio_byte_count: i64,
    width: Option<i64>,
    height: Option<i64>,
    source_width: Option<i64>,
    source_height: Option<i64>,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ActiveRecordingSnapshot {
    pub session_id: String,
    pub frame_count: i64,
    pub microphone_audio_byte_count: i64,
    pub source_audio_byte_count: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: i64,
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoppedCapture {
    pub session_id: String,
    pub recording_id: String,
    pub status: RecordingSessionStatus,
    pub frame_count: i64,
    pub microphone_audio_byte_count: i64,
    pub source_audio_byte_count: i64,
    pub microphone_audio_config: Option<AudioConfig>,
    pub source_audio_config: Option<AudioConfig>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub source_width: Option<i64>,
    pub source_height: Option<i64>,
    pub stopped_at: String,
    pub duration_ms: i64,
    pub failure_message: Option<String>,
    pub files: RecordingSessionFiles,
}

#[derive(Debug, Clone)]
pub struct StartedCapture {
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub microphone_audio_config: Option<AudioConfig>,
    pub source_audio_config: Option<AudioConfig>,
}

#[derive(Debug, Clone, Copy)]
pub struct VideoSourceDimensions {
    pub width: Option<i64>,
    pub height: Option<i64>,
}

struct AudioBlock {
    elapsed_ms: u64,
    callback_stream_ns: u64,
    capture_stream_ns: u64,
    bytes: Vec<u8>,
}

struct AudioRuntime {
    stream: cpal::Stream,
    sender: mpsc::Sender<AudioBlock>,
    writer_handle: JoinHandle<()>,
    sample_rate: i64,
    channels: i64,
    sample_format: String,
}

struct SourceAudioRuntime {
    writer_handle: JoinHandle<()>,
    config: Arc<Mutex<Option<AudioConfig>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordingSessionSidecar<'a> {
    session_id: &'a str,
    recording_id: &'a str,
    status: &'a str,
    created_at: &'a str,
    started_at: &'a str,
    stopped_at: &'a Option<String>,
    duration_ms: &'a Option<i64>,
    resolution: SessionResolution,
    frame_rate: i64,
    frame_count: i64,
    video: SessionVideoSidecar<'a>,
    audio: SessionAudioSidecar<'a>,
    sync: SessionSyncSidecar,
    failure_message: &'a Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionResolution {
    width: Option<i64>,
    height: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionVideoSidecar<'a> {
    path: &'a str,
    format: &'static str,
    source: SessionVideoSourceSidecar<'a>,
    output_dimensions: SessionResolution,
    initial_output_dimensions: SessionResolution,
    current_source_dimensions: SessionResolution,
    normalization: SessionVideoNormalizationSidecar,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionVideoSourceSidecar<'a> {
    id: &'a str,
    kind: &'a str,
    title: &'a str,
    app_name: &'a Option<String>,
    process_id: &'a Option<i64>,
    window_id: &'a Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionVideoNormalizationSidecar {
    policy: &'static str,
    padding: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionAudioSidecar<'a> {
    mode: &'a str,
    microphone: Option<SessionAudioStreamSidecar<'a>>,
    source: Option<SessionAudioStreamSidecar<'a>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionAudioStreamSidecar<'a> {
    path: &'a str,
    format: &'static str,
    sample_rate: &'a Option<i64>,
    channels: &'a Option<i64>,
    sample_format: &'a Option<String>,
    byte_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSyncSidecar {
    video_clock: &'static str,
    audio_clock: &'static str,
    note: &'static str,
}

impl RecordingRuntime {
    pub fn is_active(&self) -> Result<bool, String> {
        self.active
            .lock()
            .map(|active| active.is_some())
            .map_err(|_| "Recording runtime state is unavailable.".to_owned())
    }

    pub fn active_snapshot(&self) -> Result<Option<ActiveRecordingSnapshot>, String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "Recording runtime state is unavailable.".to_owned())?;
        let Some(active) = active.as_ref() else {
            return Ok(None);
        };

        let stats = stats_snapshot(&active.stats);

        Ok(Some(ActiveRecordingSnapshot {
            session_id: active.session_id.clone(),
            frame_count: stats.frame_count,
            microphone_audio_byte_count: stats.microphone_audio_byte_count,
            source_audio_byte_count: stats.source_audio_byte_count,
            width: stats.width,
            height: stats.height,
            duration_ms: duration_ms(active.started_instant.elapsed()),
            failure_message: failure_message(&stats.errors),
        }))
    }

    pub fn start(
        &self,
        session: &RecordingSession,
        config: &ValidatedCaptureConfig,
        files: RecordingSessionFiles,
    ) -> Result<StartedCapture, String> {
        {
            let active = self
                .active
                .lock()
                .map_err(|_| "Recording runtime state is unavailable.".to_owned())?;
            if active.is_some() {
                return Err("A recording session is already active.".to_owned());
            }
        }

        let started_instant = Instant::now();
        let stop_signal = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Mutex::new(CaptureStats::default()));
        let mut capturer = build_video_capturer(&config.video_source.id)?;
        let output_size = capturer.get_output_frame_size();
        let output_canvas = OutputCanvas::bounded(output_size[0], output_size[1])?;
        if let Ok(mut stats) = stats.lock() {
            stats.width = Some(i64::from(output_canvas.width));
            stats.height = Some(i64::from(output_canvas.height));
            stats.source_width = Some(i64::from(output_size[0]));
            stats.source_height = Some(i64::from(output_size[1]));
        }
        let audio_runtime = if config.include_microphone {
            let audio_path = files.microphone_audio_path.as_ref().ok_or_else(|| {
                "Microphone capture was enabled without an audio path.".to_owned()
            })?;
            Some(build_audio_runtime(
                config
                    .microphone
                    .as_ref()
                    .map(|microphone| microphone.id.as_str()),
                audio_path,
                Arc::clone(&stats),
                started_instant,
            )?)
        } else {
            None
        };
        let source_audio_runtime =
            if let Some(source_audio_config) = config.audio.source_audio.as_ref() {
                let audio_path = files.source_audio_path.as_ref().ok_or_else(|| {
                    "Source audio capture was enabled without an audio path.".to_owned()
                })?;
                Some(build_source_audio_runtime(
                    source_audio_config,
                    audio_path,
                    Arc::clone(&stats),
                    Arc::clone(&stop_signal),
                    started_instant,
                )?)
            } else {
                None
            };
        let microphone_audio_config = audio_runtime.as_ref().map(|runtime| AudioConfig {
            sample_rate: runtime.sample_rate,
            channels: runtime.channels,
            sample_format: runtime.sample_format.clone(),
        });

        let mut active = self
            .active
            .lock()
            .map_err(|_| "Recording runtime state is unavailable.".to_owned())?;
        if active.is_some() {
            stop_signal.store(true, Ordering::Relaxed);
            shutdown_audio_runtime(audio_runtime);
            shutdown_source_audio_runtime(source_audio_runtime);
            return Err("A recording session is already active.".to_owned());
        }

        let screen_handle = spawn_screen_writer(
            config.video_source.id.clone(),
            files.video_path.clone(),
            output_canvas,
            Arc::clone(&stop_signal),
            Arc::clone(&stats),
            started_instant,
        );
        let (audio_stream, audio_sender, audio_writer_handle) = split_audio_runtime(audio_runtime);
        let (source_audio_writer_handle, source_audio_config) =
            split_source_audio_runtime(source_audio_runtime);
        let started = StartedCapture {
            width: Some(i64::from(output_canvas.width)),
            height: Some(i64::from(output_canvas.height)),
            microphone_audio_config: microphone_audio_config.clone(),
            source_audio_config: source_audio_config
                .as_ref()
                .and_then(|config| config.lock().ok().and_then(|config| config.clone())),
        };

        active.replace(ActiveSession {
            session_id: session.id.clone(),
            recording_id: session.recording_id.clone(),
            stop_signal,
            stats,
            screen_handle: Some(screen_handle),
            audio_stream,
            audio_sender,
            audio_writer_handle,
            microphone_audio_config,
            source_audio_writer_handle,
            source_audio_config,
            started_instant,
            files,
        });

        Ok(started)
    }

    pub fn stop(&self, recording_id: Option<&str>) -> Result<StoppedCapture, String> {
        let mut active = {
            let mut guard = self
                .active
                .lock()
                .map_err(|_| "Recording runtime state is unavailable.".to_owned())?;
            let Some(active) = guard.as_ref() else {
                return Err("No recording session is active.".to_owned());
            };
            if let Some(recording_id) = recording_id {
                if active.recording_id != recording_id {
                    return Err(
                        "The active recording session does not match this recording.".to_owned(),
                    );
                }
            }
            guard.take().expect("active session exists")
        };

        active.stop_signal.store(true, Ordering::Relaxed);
        active.audio_stream.take();
        active.audio_sender.take();

        if let Some(handle) = active.audio_writer_handle.take() {
            if handle.join().is_err() {
                push_error(
                    &active.stats,
                    "Microphone writer thread stopped unexpectedly.",
                );
            }
        }

        if let Some(handle) = active.source_audio_writer_handle.take() {
            if handle.join().is_err() {
                push_error(
                    &active.stats,
                    "Source audio writer thread stopped unexpectedly.",
                );
            }
        }

        if let Some(handle) = active.screen_handle.take() {
            if handle.join().is_err() {
                push_error(&active.stats, "Screen capture thread stopped unexpectedly.");
            }
        }

        let stats = stats_snapshot(&active.stats);
        let stopped_at = current_timestamp_string();
        let duration_ms = duration_ms(active.started_instant.elapsed());
        let failure_message = failure_message(&stats.errors);
        let status = if failure_message.is_some() {
            RecordingSessionStatus::Failed
        } else {
            RecordingSessionStatus::Stopped
        };
        let source_audio_config = active
            .source_audio_config
            .and_then(|config| config.lock().ok().and_then(|config| config.clone()));

        Ok(StoppedCapture {
            session_id: active.session_id,
            recording_id: active.recording_id,
            status,
            frame_count: stats.frame_count,
            microphone_audio_byte_count: stats.microphone_audio_byte_count,
            source_audio_byte_count: stats.source_audio_byte_count,
            microphone_audio_config: active.microphone_audio_config,
            source_audio_config,
            width: stats.width,
            height: stats.height,
            source_width: stats.source_width,
            source_height: stats.source_height,
            stopped_at,
            duration_ms,
            failure_message,
            files: active.files,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AudioConfig {
    pub sample_rate: i64,
    pub channels: i64,
    pub sample_format: String,
}

pub fn frame_rate() -> i64 {
    i64::from(CAPTURE_FRAME_RATE)
}

pub fn current_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

pub fn write_session_metadata(path: &Path, session: &RecordingSession) -> Result<(), String> {
    write_session_metadata_with_source_dimensions(path, session, None)
}

pub fn write_session_metadata_with_source_dimensions(
    path: &Path,
    session: &RecordingSession,
    source_dimensions: Option<VideoSourceDimensions>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Unable to create session metadata directory: {error}"))?;
    }
    let source_dimensions = source_dimensions.unwrap_or(VideoSourceDimensions {
        width: session.width,
        height: session.height,
    });

    let sidecar = RecordingSessionSidecar {
        session_id: &session.id,
        recording_id: &session.recording_id,
        status: session.status.as_str(),
        created_at: &session.created_at,
        started_at: &session.started_at,
        stopped_at: &session.stopped_at,
        duration_ms: &session.duration_ms,
        resolution: SessionResolution {
            width: session.width,
            height: session.height,
        },
        frame_rate: session.frame_rate,
        frame_count: session.frame_count,
        video: SessionVideoSidecar {
            path: &session.video_path,
            format: video_sidecar_format(&session.video_path),
            source: SessionVideoSourceSidecar {
                id: &session.video_source_id,
                kind: &session.video_source_kind,
                title: &session.video_source_title,
                app_name: &session.video_source_app_name,
                process_id: &session.video_source_process_id,
                window_id: &session.video_source_window_id,
            },
            output_dimensions: SessionResolution {
                width: session.width,
                height: session.height,
            },
            initial_output_dimensions: SessionResolution {
                width: session.width,
                height: session.height,
            },
            current_source_dimensions: SessionResolution {
                width: source_dimensions.width,
                height: source_dimensions.height,
            },
            normalization: SessionVideoNormalizationSidecar {
                policy: "scale_to_fit",
                padding: "centered_opaque_black",
            },
        },
        audio: SessionAudioSidecar {
            mode: session.audio_mode.as_str(),
            microphone: session
                .microphone_audio_path
                .as_ref()
                .map(|path| SessionAudioStreamSidecar {
                    path,
                    format: "raw interleaved PCM bytes with block timing headers v1",
                    sample_rate: &session.microphone_audio_sample_rate,
                    channels: &session.microphone_audio_channels,
                    sample_format: &session.microphone_audio_sample_format,
                    byte_count: session.microphone_audio_byte_count,
                }),
            source: session
                .source_audio_path
                .as_ref()
                .map(|path| SessionAudioStreamSidecar {
                    path,
                    format: "raw interleaved PCM bytes with block timing headers v1",
                    sample_rate: &session.source_audio_sample_rate,
                    channels: &session.source_audio_channels,
                    sample_format: &session.source_audio_sample_format,
                    byte_count: session.source_audio_byte_count,
                }),
        },
        sync: SessionSyncSidecar {
            video_clock: "elapsed milliseconds from local session start, plus frame display epoch",
            audio_clock: "elapsed milliseconds from local session start, plus CPAL stream timestamps",
            note: "Each media record carries timing headers so final encoding can align audio and video.",
        },
        failure_message: &session.failure_message,
    };

    let file =
        File::create(path).map_err(|error| format!("Unable to write session metadata: {error}"))?;
    serde_json::to_writer_pretty(file, &sidecar)
        .map_err(|error| format!("Unable to serialize session metadata: {error}"))
}

fn video_sidecar_format(path: &str) -> &'static str {
    if crate::media::chunked_video::is_chunked_video_path(path) {
        "metafy chunked H.264 segment manifest v1"
    } else {
        "metafy raw BGRA frame stream v1"
    }
}

fn build_video_capturer(source_id: &str) -> Result<scap::capturer::Capturer, String> {
    let target = selected_video_target(source_id)?;
    let options = scap::capturer::Options {
        fps: CAPTURE_FRAME_RATE,
        show_cursor: true,
        show_highlight: false,
        target: Some(target),
        output_type: scap::frame::FrameType::BGRAFrame,
        output_resolution: scap::capturer::Resolution::Captured,
        ..Default::default()
    };

    scap::capturer::Capturer::build(options)
        .map_err(|error| format!("Unable to start video capture: {error}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VideoTargetDescriptor {
    kind: CaptureVideoSourceKind,
    native_id: u32,
}

fn selected_video_target(source_id: &str) -> Result<scap::Target, String> {
    let descriptor = parse_video_target_descriptor(source_id)?;
    let targets = scap::get_all_targets();
    let descriptors = targets
        .iter()
        .map(video_target_descriptor)
        .collect::<Vec<_>>();
    let target_index = selected_video_target_index(descriptor, &descriptors).ok_or_else(|| {
        format!(
            "The selected {} is no longer available.",
            descriptor.kind.label()
        )
    })?;

    targets
        .into_iter()
        .nth(target_index)
        .ok_or_else(|| "Selected video target is no longer available.".to_owned())
}

fn parse_video_target_descriptor(source_id: &str) -> Result<VideoTargetDescriptor, String> {
    let parsed_source_id = crate::capture::parse_capture_video_source_id(source_id)?;
    validate_video_target_kind(parsed_source_id.kind)?;
    let native_id = parsed_source_id
        .native_id
        .parse::<u32>()
        .map_err(|_| format!("Selected {} id is invalid.", parsed_source_id.kind.label()))?;

    Ok(VideoTargetDescriptor {
        kind: parsed_source_id.kind,
        native_id,
    })
}

fn validate_video_target_kind(kind: CaptureVideoSourceKind) -> Result<(), String> {
    match kind {
        CaptureVideoSourceKind::Display => Ok(()),
        CaptureVideoSourceKind::Window if window_video_capture_supported() => Ok(()),
        CaptureVideoSourceKind::Window => {
            Err("Window video capture is not supported on this platform.".to_owned())
        }
        CaptureVideoSourceKind::Application => Err(
            "Application video capture is not available with the current capture backend. Select a window from that application instead."
                .to_owned(),
        ),
    }
}

fn window_video_capture_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

fn video_target_descriptor(target: &scap::Target) -> VideoTargetDescriptor {
    match target {
        scap::Target::Display(display) => VideoTargetDescriptor {
            kind: CaptureVideoSourceKind::Display,
            native_id: display.id,
        },
        scap::Target::Window(window) => VideoTargetDescriptor {
            kind: CaptureVideoSourceKind::Window,
            native_id: window.id,
        },
    }
}

fn selected_video_target_index(
    descriptor: VideoTargetDescriptor,
    targets: &[VideoTargetDescriptor],
) -> Option<usize> {
    targets.iter().position(|target| *target == descriptor)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OutputCanvas {
    width: u32,
    height: u32,
}

impl OutputCanvas {
    fn new(width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("Capture output dimensions are invalid.".to_owned());
        }

        Ok(Self { width, height })
    }

    fn bounded(width: u32, height: u32) -> Result<Self, String> {
        let canvas = Self::new(width, height)?;
        if canvas.width <= MAX_RAW_VIDEO_WIDTH && canvas.height <= MAX_RAW_VIDEO_HEIGHT {
            return Ok(Self {
                width: even_canvas_dimension(canvas.width),
                height: even_canvas_dimension(canvas.height),
            });
        }

        let source_width = u64::from(canvas.width);
        let source_height = u64::from(canvas.height);
        let max_width = u64::from(MAX_RAW_VIDEO_WIDTH);
        let max_height = u64::from(MAX_RAW_VIDEO_HEIGHT);
        let (width, height) = if source_width * max_height > max_width * source_height {
            (
                MAX_RAW_VIDEO_WIDTH,
                rounded_ratio(source_height * max_width, source_width),
            )
        } else {
            (
                rounded_ratio(source_width * max_height, source_height),
                MAX_RAW_VIDEO_HEIGHT,
            )
        };

        Ok(Self {
            width: even_canvas_dimension(width.clamp(1, MAX_RAW_VIDEO_WIDTH)),
            height: even_canvas_dimension(height.clamp(1, MAX_RAW_VIDEO_HEIGHT)),
        })
    }
}

fn spawn_screen_writer(
    source_id: String,
    video_path: PathBuf,
    output_canvas: OutputCanvas,
    stop_signal: Arc<AtomicBool>,
    stats: Arc<Mutex<CaptureStats>>,
    started_instant: Instant,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut capturer = match build_video_capturer(&source_id) {
            Ok(capturer) => capturer,
            Err(error) => {
                push_error(&stats, error);
                return;
            }
        };
        let mut writer = match ScreenFrameWriter::create(&video_path, output_canvas) {
            Ok(writer) => writer,
            Err(error) => {
                push_error(&stats, error);
                return;
            }
        };

        capturer.start_capture();

        while !stop_signal.load(Ordering::Relaxed) {
            match capturer.get_next_frame() {
                Ok(frame) => {
                    if let Err(error) = process_video_frame(
                        &mut writer,
                        frame,
                        output_canvas,
                        &stats,
                        started_instant,
                    ) {
                        push_error(&stats, error);
                        break;
                    }
                }
                Err(error) => {
                    push_error(
                        &stats,
                        format!("Screen capture stopped unexpectedly: {error}"),
                    );
                    break;
                }
            }
        }

        capturer.stop_capture();
        if let Err(error) = writer.finish() {
            push_error(&stats, error);
        }
    })
}

enum ScreenFrameWriter {
    #[cfg(not(target_os = "macos"))]
    Raw(RawScreenFrameWriter),
    #[cfg(target_os = "macos")]
    Chunked(ChunkedScreenFrameWriter),
}

impl ScreenFrameWriter {
    fn create(path: &Path, output_canvas: OutputCanvas) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            return ChunkedScreenFrameWriter::create(path, output_canvas).map(Self::Chunked);
        }

        #[cfg(not(target_os = "macos"))]
        {
            RawScreenFrameWriter::create(path).map(Self::Raw)
        }
    }

    fn append(
        &mut self,
        frame: &NormalizedVideoFrame<'_>,
        elapsed_ms: u64,
        display_time_ms: u64,
    ) -> Result<(), String> {
        match self {
            #[cfg(not(target_os = "macos"))]
            Self::Raw(writer) => writer.append(frame, elapsed_ms, display_time_ms),
            #[cfg(target_os = "macos")]
            Self::Chunked(writer) => writer.append(frame, elapsed_ms, display_time_ms),
        }
    }

    fn finish(&mut self) -> Result<(), String> {
        match self {
            #[cfg(not(target_os = "macos"))]
            Self::Raw(writer) => writer.finish(),
            #[cfg(target_os = "macos")]
            Self::Chunked(writer) => writer.finish(),
        }
    }
}

#[cfg(not(target_os = "macos"))]
struct RawScreenFrameWriter {
    writer: BufWriter<File>,
}

#[cfg(not(target_os = "macos"))]
impl RawScreenFrameWriter {
    fn create(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Unable to create screen frame directory: {error}"))?;
        }
        let mut writer = BufWriter::new(
            File::create(path)
                .map_err(|error| format!("Unable to create screen frame file: {error}"))?,
        );
        writer
            .write_all(VIDEO_FILE_MAGIC)
            .map_err(|error| format!("Unable to initialize screen frame file: {error}"))?;

        Ok(Self { writer })
    }

    fn append(
        &mut self,
        frame: &NormalizedVideoFrame<'_>,
        elapsed_ms: u64,
        display_time_ms: u64,
    ) -> Result<(), String> {
        let (format_code, encoded_bytes) = encode_video_frame_bytes(frame.bytes.as_ref());
        let encoded_byte_count: u32 = encoded_bytes
            .as_ref()
            .len()
            .try_into()
            .map_err(|_| "Encoded BGRA frame is too large to write.".to_owned())?;

        write_u64(&mut self.writer, elapsed_ms)?;
        write_u64(&mut self.writer, display_time_ms)?;
        write_u32(&mut self.writer, format_code)?;
        write_u32(&mut self.writer, frame.output_width)?;
        write_u32(&mut self.writer, frame.output_height)?;
        write_u32(&mut self.writer, encoded_byte_count)?;
        self.writer
            .write_all(encoded_bytes.as_ref())
            .map_err(|error| format!("Unable to write screen frame: {error}"))
    }

    fn finish(&mut self) -> Result<(), String> {
        self.writer
            .flush()
            .map_err(|error| format!("Unable to flush screen frame file: {error}"))
    }
}

#[cfg(target_os = "macos")]
struct ChunkedScreenFrameWriter {
    writer: MacosSegmentedVideoWriter,
    thumbnail_path: PathBuf,
    thumbnail_written: bool,
}

#[cfg(target_os = "macos")]
impl ChunkedScreenFrameWriter {
    fn create(path: &Path, output_canvas: OutputCanvas) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Unable to create chunked video directory: {error}"))?;
        }
        let chunk_frames = i64::from(CAPTURE_FRAME_RATE * CHUNKED_VIDEO_DURATION_SECONDS);
        let writer = MacosSegmentedVideoWriter::create(
            path,
            i64::from(output_canvas.width),
            i64::from(output_canvas.height),
            i64::from(CAPTURE_FRAME_RATE),
            chunk_frames,
        )?;
        let thumbnail_path = chunked_thumbnail_path(path);

        Ok(Self {
            writer,
            thumbnail_path,
            thumbnail_written: false,
        })
    }

    fn append(
        &mut self,
        frame: &NormalizedVideoFrame<'_>,
        elapsed_ms: u64,
        display_time_ms: u64,
    ) -> Result<(), String> {
        if !self.thumbnail_written {
            fs::write(&self.thumbnail_path, frame.bytes.as_ref()).map_err(|error| {
                format!(
                    "Unable to write chunked video thumbnail frame {}: {error}",
                    self.thumbnail_path.display()
                )
            })?;
            self.thumbnail_written = true;
        }
        self.writer.append_frame(
            frame.bytes.as_ref(),
            elapsed_ms
                .try_into()
                .map_err(|_| "Video frame elapsed timestamp is too large.".to_owned())?,
            display_time_ms
                .try_into()
                .map_err(|_| "Video frame display timestamp is too large.".to_owned())?,
        )
    }

    fn finish(&mut self) -> Result<(), String> {
        self.writer.finish()
    }
}

#[cfg(target_os = "macos")]
fn chunked_thumbnail_path(manifest_path: &Path) -> PathBuf {
    let base_name = manifest_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("screen_video");
    manifest_path.with_file_name(format!("{base_name}-thumbnail.bgra"))
}

fn process_video_frame(
    writer: &mut ScreenFrameWriter,
    frame: scap::frame::Frame,
    output_canvas: OutputCanvas,
    stats: &Arc<Mutex<CaptureStats>>,
    started_instant: Instant,
) -> Result<(), String> {
    let payload = video_frame_payload(frame)?;
    let normalized = normalize_bgra_frame(
        payload.width,
        payload.height,
        payload.format_code,
        &payload.bytes,
        output_canvas,
    )?;
    writer.append(
        &normalized,
        duration_ms_u64(started_instant.elapsed()),
        system_time_ms(payload.display_time),
    )?;

    if let Ok(mut stats) = stats.lock() {
        stats.frame_count += 1;
        stats.width = Some(i64::from(normalized.output_width));
        stats.height = Some(i64::from(normalized.output_height));
        stats.source_width = Some(i64::from(normalized.source_width));
        stats.source_height = Some(i64::from(normalized.source_height));
    }

    Ok(())
}

fn encode_video_frame_bytes(bytes: &[u8]) -> (u32, Cow<'_, [u8]>) {
    let compressed = lz4_flex::compress_prepend_size(bytes);
    if compressed.len() < bytes.len() {
        (BGRA_LZ4_FORMAT_CODE, Cow::Owned(compressed))
    } else {
        (BGRA_FORMAT_CODE, Cow::Borrowed(bytes))
    }
}

struct VideoFramePayload {
    display_time: SystemTime,
    width: u32,
    height: u32,
    format_code: u32,
    bytes: Vec<u8>,
}

struct NormalizedVideoFrame<'a> {
    source_width: u32,
    source_height: u32,
    output_width: u32,
    output_height: u32,
    scaled_width: u32,
    scaled_height: u32,
    x_offset: u32,
    y_offset: u32,
    bytes: Cow<'a, [u8]>,
}

fn normalize_bgra_frame(
    source_width: u32,
    source_height: u32,
    format_code: u32,
    bytes: &[u8],
    output_canvas: OutputCanvas,
) -> Result<NormalizedVideoFrame<'_>, String> {
    if format_code != BGRA_FORMAT_CODE {
        return Err(format!(
            "Unsupported captured frame format {format_code}; expected BGRA."
        ));
    }
    if source_width == 0 || source_height == 0 {
        return Err("Captured frame dimensions are invalid.".to_owned());
    }

    let expected_byte_count = bgra_byte_count(source_width, source_height)?;
    if bytes.len() != expected_byte_count {
        return Err(format!(
            "Captured BGRA frame has {} bytes; expected {expected_byte_count} for {source_width}x{source_height}.",
            bytes.len()
        ));
    }

    if source_width == output_canvas.width && source_height == output_canvas.height {
        return Ok(NormalizedVideoFrame {
            source_width,
            source_height,
            output_width: output_canvas.width,
            output_height: output_canvas.height,
            scaled_width: output_canvas.width,
            scaled_height: output_canvas.height,
            x_offset: 0,
            y_offset: 0,
            bytes: Cow::Borrowed(bytes),
        });
    }

    let (scaled_width, scaled_height) =
        scale_to_fit_dimensions(source_width, source_height, output_canvas);
    let x_offset = (output_canvas.width - scaled_width) / 2;
    let y_offset = (output_canvas.height - scaled_height) / 2;
    let mut normalized = black_bgra_frame(output_canvas)?;

    copy_scaled_bgra(
        bytes,
        source_width,
        source_height,
        &mut normalized,
        output_canvas,
        scaled_width,
        scaled_height,
        x_offset,
        y_offset,
    );

    Ok(NormalizedVideoFrame {
        source_width,
        source_height,
        output_width: output_canvas.width,
        output_height: output_canvas.height,
        scaled_width,
        scaled_height,
        x_offset,
        y_offset,
        bytes: Cow::Owned(normalized),
    })
}

fn scale_to_fit_dimensions(
    source_width: u32,
    source_height: u32,
    output_canvas: OutputCanvas,
) -> (u32, u32) {
    let source_width = u64::from(source_width);
    let source_height = u64::from(source_height);
    let canvas_width = u64::from(output_canvas.width);
    let canvas_height = u64::from(output_canvas.height);

    if source_width * canvas_height <= canvas_width * source_height {
        let scaled_width = rounded_ratio(source_width * canvas_height, source_height)
            .clamp(1, output_canvas.width);
        (scaled_width, output_canvas.height)
    } else {
        let scaled_height = rounded_ratio(source_height * canvas_width, source_width)
            .clamp(1, output_canvas.height);
        (output_canvas.width, scaled_height)
    }
}

fn rounded_ratio(numerator: u64, denominator: u64) -> u32 {
    ((numerator + (denominator / 2)) / denominator)
        .try_into()
        .unwrap_or(u32::MAX)
}

fn even_canvas_dimension(value: u32) -> u32 {
    if value <= 2 {
        return value.max(1);
    }

    value - (value % 2)
}

#[allow(clippy::too_many_arguments)]
fn copy_scaled_bgra(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    output: &mut [u8],
    output_canvas: OutputCanvas,
    scaled_width: u32,
    scaled_height: u32,
    x_offset: u32,
    y_offset: u32,
) {
    let source_width = source_width as usize;
    let source_height = source_height as usize;
    let output_width = output_canvas.width as usize;
    let scaled_width = scaled_width as usize;
    let scaled_height = scaled_height as usize;
    let x_offset = x_offset as usize;
    let y_offset = y_offset as usize;

    for target_y in 0..scaled_height {
        let source_y = (target_y * source_height) / scaled_height;
        for target_x in 0..scaled_width {
            let source_x = (target_x * source_width) / scaled_width;
            let source_index = ((source_y * source_width + source_x) * BGRA_BYTES_PER_PIXEL)
                .min(source.len().saturating_sub(BGRA_BYTES_PER_PIXEL));
            let output_index = (((y_offset + target_y) * output_width + (x_offset + target_x))
                * BGRA_BYTES_PER_PIXEL)
                .min(output.len().saturating_sub(BGRA_BYTES_PER_PIXEL));

            output[output_index..output_index + BGRA_BYTES_PER_PIXEL]
                .copy_from_slice(&source[source_index..source_index + BGRA_BYTES_PER_PIXEL]);
        }
    }
}

fn black_bgra_frame(output_canvas: OutputCanvas) -> Result<Vec<u8>, String> {
    let byte_count = bgra_byte_count(output_canvas.width, output_canvas.height)?;
    let mut frame = vec![0_u8; byte_count];
    for pixel in frame.chunks_exact_mut(BGRA_BYTES_PER_PIXEL) {
        pixel.copy_from_slice(&OPAQUE_BLACK_BGRA);
    }
    Ok(frame)
}

fn bgra_byte_count(width: u32, height: u32) -> Result<usize, String> {
    let width = width as usize;
    let height = height as usize;
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(BGRA_BYTES_PER_PIXEL))
        .ok_or_else(|| "Frame dimensions are too large to capture.".to_owned())
}

fn video_frame_payload(frame: scap::frame::Frame) -> Result<VideoFramePayload, String> {
    match frame {
        scap::frame::Frame::Video(frame) => Ok(video_frame_payload_from_video(frame)),
        scap::frame::Frame::Audio(_) => {
            Err("Video capture returned an unexpected audio frame.".to_owned())
        }
    }
}

fn video_frame_payload_from_video(frame: scap::frame::VideoFrame) -> VideoFramePayload {
    match frame {
        scap::frame::VideoFrame::YUVFrame(frame) => {
            let mut bytes =
                Vec::with_capacity(frame.luminance_bytes.len() + frame.chrominance_bytes.len());
            bytes.extend_from_slice(&frame.luminance_bytes);
            bytes.extend_from_slice(&frame.chrominance_bytes);
            VideoFramePayload {
                display_time: frame.display_time,
                width: frame.width.max(0) as u32,
                height: frame.height.max(0) as u32,
                format_code: 1,
                bytes,
            }
        }
        scap::frame::VideoFrame::RGB(frame) => VideoFramePayload {
            display_time: frame.display_time,
            width: frame.width.max(0) as u32,
            height: frame.height.max(0) as u32,
            format_code: 2,
            bytes: frame.data,
        },
        scap::frame::VideoFrame::RGBx(frame) => VideoFramePayload {
            display_time: frame.display_time,
            width: frame.width.max(0) as u32,
            height: frame.height.max(0) as u32,
            format_code: 3,
            bytes: frame.data,
        },
        scap::frame::VideoFrame::XBGR(frame) => VideoFramePayload {
            display_time: frame.display_time,
            width: frame.width.max(0) as u32,
            height: frame.height.max(0) as u32,
            format_code: 4,
            bytes: frame.data,
        },
        scap::frame::VideoFrame::BGRx(frame) => VideoFramePayload {
            display_time: frame.display_time,
            width: frame.width.max(0) as u32,
            height: frame.height.max(0) as u32,
            format_code: 5,
            bytes: frame.data,
        },
        scap::frame::VideoFrame::BGR0(frame) => VideoFramePayload {
            display_time: frame.display_time,
            width: frame.width.max(0) as u32,
            height: frame.height.max(0) as u32,
            format_code: 6,
            bytes: frame.data,
        },
        scap::frame::VideoFrame::BGRA(frame) => VideoFramePayload {
            display_time: frame.display_time,
            width: frame.width.max(0) as u32,
            height: frame.height.max(0) as u32,
            format_code: 7,
            bytes: frame.data,
        },
    }
}

fn build_audio_runtime(
    device_id: Option<&str>,
    audio_path: &Path,
    stats: Arc<Mutex<CaptureStats>>,
    started_instant: Instant,
) -> Result<AudioRuntime, String> {
    let device = selected_microphone_device(device_id)?;
    let supported_config = device
        .default_input_config()
        .map_err(|error| format!("Unable to read microphone input config: {error}"))?;
    let sample_format = supported_config.sample_format();
    let sample_rate = i64::from(supported_config.sample_rate());
    let channels = i64::from(supported_config.channels());
    let file = File::create(audio_path)
        .map_err(|error| format!("Unable to create microphone capture file: {error}"))?;
    let (sender, receiver) = mpsc::channel::<AudioBlock>();
    let writer_stats = Arc::clone(&stats);
    let writer_handle = thread::spawn(move || {
        let mut writer = BufWriter::new(file);
        if let Err(error) = writer.write_all(AUDIO_FILE_MAGIC) {
            push_error(
                &writer_stats,
                format!("Unable to initialize microphone capture file: {error}"),
            );
            return;
        }

        for block in receiver {
            if let Err(error) = write_audio_block(&mut writer, &block) {
                push_error(&writer_stats, error);
                break;
            }

            if let Ok(mut stats) = writer_stats.lock() {
                stats.microphone_audio_byte_count += block.bytes.len() as i64;
            }
        }

        if let Err(error) = writer.flush() {
            push_error(
                &writer_stats,
                format!("Unable to flush microphone capture file: {error}"),
            );
        }
    });

    let callback_sender = sender.clone();
    let error_stats = Arc::clone(&stats);
    let stream = device
        .build_input_stream_raw(
            supported_config.config(),
            sample_format,
            move |data, info| {
                let timestamp = info.timestamp();
                let block = AudioBlock {
                    elapsed_ms: duration_ms_u64(started_instant.elapsed()),
                    callback_stream_ns: stream_nanos(timestamp.callback.as_nanos()),
                    capture_stream_ns: stream_nanos(timestamp.capture.as_nanos()),
                    bytes: data.bytes().to_vec(),
                };
                let _ = callback_sender.send(block);
            },
            move |error| {
                push_error(&error_stats, format!("Microphone capture error: {error}"));
            },
            Some(Duration::from_millis(500)),
        )
        .map_err(|error| format!("Unable to start microphone capture: {error}"))?;

    stream
        .play()
        .map_err(|error| format!("Unable to play microphone input stream: {error}"))?;

    Ok(AudioRuntime {
        stream,
        sender,
        writer_handle,
        sample_rate,
        channels,
        sample_format: sample_format.to_string(),
    })
}

fn build_source_audio_runtime(
    config: &SourceAudioCaptureConfig,
    audio_path: &Path,
    stats: Arc<Mutex<CaptureStats>>,
    stop_signal: Arc<AtomicBool>,
    started_instant: Instant,
) -> Result<SourceAudioRuntime, String> {
    let file = File::create(audio_path)
        .map_err(|error| format!("Unable to create source audio capture file: {error}"))?;
    let runtime_config = Arc::new(Mutex::new(None));
    let writer_config = Arc::clone(&runtime_config);
    let writer_stats = Arc::clone(&stats);
    let capture_config = config.clone();
    let writer_handle = thread::spawn(move || {
        let mut capturer = match crate::source_audio::build_source_audio_capturer(&capture_config) {
            Ok(capturer) => capturer,
            Err(error) => {
                push_error(&writer_stats, error);
                return;
            }
        };
        let mut writer = BufWriter::new(file);
        if let Err(error) = writer.write_all(AUDIO_FILE_MAGIC) {
            push_error(
                &writer_stats,
                format!("Unable to initialize source audio capture file: {error}"),
            );
            return;
        }

        capturer.start_capture();

        while !stop_signal.load(Ordering::Relaxed) {
            match capturer.get_next_frame() {
                Ok(scap::frame::Frame::Audio(frame)) => {
                    let (block, audio_config) = source_audio_block(frame, started_instant);
                    if let Ok(mut config) = writer_config.lock() {
                        if config.is_none() {
                            config.replace(audio_config);
                        }
                    }

                    if let Err(error) = write_audio_block(&mut writer, &block) {
                        push_error(&writer_stats, error);
                        break;
                    }

                    if let Ok(mut stats) = writer_stats.lock() {
                        stats.source_audio_byte_count += block.bytes.len() as i64;
                    }
                }
                Ok(scap::frame::Frame::Video(_)) => {}
                Err(error) => {
                    if !stop_signal.load(Ordering::Relaxed) {
                        push_error(
                            &writer_stats,
                            format!("Source audio capture stopped unexpectedly: {error}"),
                        );
                    }
                    break;
                }
            }
        }

        capturer.stop_capture();
        if let Err(error) = writer.flush() {
            push_error(
                &writer_stats,
                format!("Unable to flush source audio capture file: {error}"),
            );
        }
    });

    Ok(SourceAudioRuntime {
        writer_handle,
        config: runtime_config,
    })
}

fn source_audio_block(
    frame: scap::frame::AudioFrame,
    started_instant: Instant,
) -> (AudioBlock, AudioConfig) {
    let audio_config = AudioConfig {
        sample_rate: i64::from(frame.rate()),
        channels: i64::from(frame.channels()),
        sample_format: source_audio_sample_format(frame.format()),
    };
    let block = AudioBlock {
        elapsed_ms: duration_ms_u64(started_instant.elapsed()),
        callback_stream_ns: 0,
        capture_stream_ns: system_time_ms(frame.time()),
        bytes: frame.raw_data().to_vec(),
    };

    (block, audio_config)
}

fn source_audio_sample_format(format: scap::frame::AudioFormat) -> String {
    match format {
        scap::frame::AudioFormat::I8 => "i8",
        scap::frame::AudioFormat::I16 => "i16",
        scap::frame::AudioFormat::I32 => "i32",
        scap::frame::AudioFormat::I64 => "i64",
        scap::frame::AudioFormat::U8 => "u8",
        scap::frame::AudioFormat::U16 => "u16",
        scap::frame::AudioFormat::U32 => "u32",
        scap::frame::AudioFormat::U64 => "u64",
        scap::frame::AudioFormat::F32 => "f32",
        scap::frame::AudioFormat::F64 => "f64",
        _ => "unknown",
    }
    .to_owned()
}

fn split_audio_runtime(
    runtime: Option<AudioRuntime>,
) -> (
    Option<cpal::Stream>,
    Option<mpsc::Sender<AudioBlock>>,
    Option<JoinHandle<()>>,
) {
    match runtime {
        Some(runtime) => (
            Some(runtime.stream),
            Some(runtime.sender),
            Some(runtime.writer_handle),
        ),
        None => (None, None, None),
    }
}

fn split_source_audio_runtime(
    runtime: Option<SourceAudioRuntime>,
) -> (
    Option<JoinHandle<()>>,
    Option<Arc<Mutex<Option<AudioConfig>>>>,
) {
    match runtime {
        Some(runtime) => (Some(runtime.writer_handle), Some(runtime.config)),
        None => (None, None),
    }
}

fn shutdown_audio_runtime(runtime: Option<AudioRuntime>) {
    if let Some(runtime) = runtime {
        drop(runtime.stream);
        drop(runtime.sender);
        let _ = runtime.writer_handle.join();
    }
}

fn shutdown_source_audio_runtime(runtime: Option<SourceAudioRuntime>) {
    if let Some(runtime) = runtime {
        let _ = runtime.writer_handle.join();
    }
}

fn selected_microphone_device(device_id: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();

    if let Some(device_id) = device_id.filter(|id| !id.is_empty()) {
        let parsed_id = cpal::DeviceId::from_str(device_id)
            .map_err(|error| format!("Selected microphone id is invalid: {error}"))?;
        return host
            .device_by_id(&parsed_id)
            .ok_or_else(|| "Selected microphone is no longer available.".to_owned());
    }

    host.default_input_device()
        .ok_or_else(|| "No default microphone input device is available.".to_owned())
}

fn write_audio_block(writer: &mut BufWriter<File>, block: &AudioBlock) -> Result<(), String> {
    write_u64(writer, block.elapsed_ms)?;
    write_u64(writer, block.callback_stream_ns)?;
    write_u64(writer, block.capture_stream_ns)?;
    write_u32(writer, block.bytes.len().try_into().unwrap_or(u32::MAX))?;
    writer
        .write_all(&block.bytes)
        .map_err(|error| format!("Unable to write captured audio block: {error}"))
}

fn write_u64(writer: &mut BufWriter<File>, value: u64) -> Result<(), String> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|error| format!("Unable to write capture timing data: {error}"))
}

fn write_u32(writer: &mut BufWriter<File>, value: u32) -> Result<(), String> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|error| format!("Unable to write capture metadata: {error}"))
}

fn stream_nanos(value: u128) -> u64 {
    value.try_into().unwrap_or(u64::MAX)
}

fn system_time_ms(time: SystemTime) -> u64 {
    duration_ms_u64(time.duration_since(UNIX_EPOCH).unwrap_or_default())
}

fn duration_ms(duration: Duration) -> i64 {
    duration_ms_u64(duration).try_into().unwrap_or(i64::MAX)
}

fn duration_ms_u64(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn stats_snapshot(stats: &Arc<Mutex<CaptureStats>>) -> CaptureStats {
    stats.lock().map(|stats| stats.clone()).unwrap_or_default()
}

fn push_error(stats: &Arc<Mutex<CaptureStats>>, message: impl Into<String>) {
    if let Ok(mut stats) = stats.lock() {
        stats.errors.push(message.into());
    }
}

fn failure_message(errors: &[String]) -> Option<String> {
    if errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_display_and_window_video_target_descriptors() {
        assert_eq!(
            parse_video_target_descriptor("display:7").expect("display descriptor"),
            VideoTargetDescriptor {
                kind: CaptureVideoSourceKind::Display,
                native_id: 7,
            }
        );
        assert_eq!(
            parse_video_target_descriptor("window:42").expect("window descriptor"),
            VideoTargetDescriptor {
                kind: CaptureVideoSourceKind::Window,
                native_id: 42,
            }
        );
    }

    #[test]
    fn rejects_application_video_target_descriptor() {
        let error = parse_video_target_descriptor("application:com.metafy.app")
            .expect_err("application capture is constrained by the current backend");

        assert!(error.contains("Application video capture is not available"));
    }

    #[test]
    fn rejects_invalid_numeric_video_target_ids() {
        let error =
            parse_video_target_descriptor("window:not-a-number").expect_err("invalid window id");

        assert_eq!(error, "Selected window id is invalid.");
    }

    #[test]
    fn resolves_matching_video_target_index() {
        let targets = vec![
            VideoTargetDescriptor {
                kind: CaptureVideoSourceKind::Display,
                native_id: 1,
            },
            VideoTargetDescriptor {
                kind: CaptureVideoSourceKind::Window,
                native_id: 42,
            },
        ];

        assert_eq!(
            selected_video_target_index(
                VideoTargetDescriptor {
                    kind: CaptureVideoSourceKind::Window,
                    native_id: 42,
                },
                &targets,
            ),
            Some(1)
        );
        assert_eq!(
            selected_video_target_index(
                VideoTargetDescriptor {
                    kind: CaptureVideoSourceKind::Window,
                    native_id: 99,
                },
                &targets,
            ),
            None
        );
    }

    #[test]
    fn bounded_output_canvas_caps_retina_capture() {
        let canvas = OutputCanvas::bounded(3600, 2338).expect("canvas");

        assert_eq!(canvas.width, 1108);
        assert_eq!(canvas.height, 720);
    }

    #[test]
    fn video_frame_encoding_uses_lz4_when_smaller() {
        let source = vec![0x80; 128 * 128 * BGRA_BYTES_PER_PIXEL];

        let (format_code, encoded) = encode_video_frame_bytes(&source);

        assert_eq!(format_code, BGRA_LZ4_FORMAT_CODE);
        assert!(encoded.len() < source.len());
        let decoded =
            lz4_flex::decompress_size_prepended(encoded.as_ref()).expect("decompress frame");
        assert_eq!(decoded, source);
    }

    #[test]
    fn same_size_bgra_frames_pass_through_unchanged() {
        let canvas = OutputCanvas::new(2, 2).expect("canvas");
        let source = bgra_pixels(&[RED, GREEN, BLUE, WHITE]);

        let normalized =
            normalize_bgra_frame(2, 2, BGRA_FORMAT_CODE, &source, canvas).expect("normalize");

        assert!(matches!(&normalized.bytes, Cow::Borrowed(_)));
        assert_eq!(normalized.output_width, 2);
        assert_eq!(normalized.output_height, 2);
        assert_eq!(normalized.scaled_width, 2);
        assert_eq!(normalized.scaled_height, 2);
        assert_eq!(normalized.x_offset, 0);
        assert_eq!(normalized.y_offset, 0);
        assert_eq!(normalized.bytes.as_ref(), source.as_slice());
    }

    #[test]
    fn smaller_bgra_frames_scale_up_to_the_locked_canvas() {
        let canvas = OutputCanvas::new(4, 4).expect("canvas");
        let source = bgra_pixels(&[RED, GREEN, BLUE, WHITE]);

        let normalized =
            normalize_bgra_frame(2, 2, BGRA_FORMAT_CODE, &source, canvas).expect("normalize");

        assert_eq!(normalized.output_width, 4);
        assert_eq!(normalized.output_height, 4);
        assert_eq!(normalized.scaled_width, 4);
        assert_eq!(normalized.scaled_height, 4);
        assert_eq!(normalized.x_offset, 0);
        assert_eq!(normalized.y_offset, 0);
        assert_eq!(pixel(normalized.bytes.as_ref(), 4, 0, 0), RED);
        assert_eq!(pixel(normalized.bytes.as_ref(), 4, 1, 0), RED);
        assert_eq!(pixel(normalized.bytes.as_ref(), 4, 2, 0), GREEN);
        assert_eq!(pixel(normalized.bytes.as_ref(), 4, 3, 3), WHITE);
    }

    #[test]
    fn larger_bgra_frames_scale_down_to_the_locked_canvas() {
        let canvas = OutputCanvas::new(2, 2).expect("canvas");
        let source = gradient_bgra_frame(4, 4);

        let normalized =
            normalize_bgra_frame(4, 4, BGRA_FORMAT_CODE, &source, canvas).expect("normalize");

        assert_eq!(normalized.output_width, 2);
        assert_eq!(normalized.output_height, 2);
        assert_eq!(normalized.scaled_width, 2);
        assert_eq!(normalized.scaled_height, 2);
        assert_eq!(
            pixel(normalized.bytes.as_ref(), 2, 0, 0),
            gradient_pixel(0, 0)
        );
        assert_eq!(
            pixel(normalized.bytes.as_ref(), 2, 1, 0),
            gradient_pixel(2, 0)
        );
        assert_eq!(
            pixel(normalized.bytes.as_ref(), 2, 0, 1),
            gradient_pixel(0, 2)
        );
        assert_eq!(
            pixel(normalized.bytes.as_ref(), 2, 1, 1),
            gradient_pixel(2, 2)
        );
    }

    #[test]
    fn wider_bgra_frames_preserve_aspect_ratio_with_centered_black_padding() {
        let canvas = OutputCanvas::new(4, 4).expect("canvas");
        let source = gradient_bgra_frame(4, 2);

        let normalized =
            normalize_bgra_frame(4, 2, BGRA_FORMAT_CODE, &source, canvas).expect("normalize");

        assert_eq!(normalized.output_width, 4);
        assert_eq!(normalized.output_height, 4);
        assert_eq!(normalized.scaled_width, 4);
        assert_eq!(normalized.scaled_height, 2);
        assert_eq!(normalized.x_offset, 0);
        assert_eq!(normalized.y_offset, 1);
        assert_eq!(pixel(normalized.bytes.as_ref(), 4, 0, 0), OPAQUE_BLACK_BGRA);
        assert_eq!(pixel(normalized.bytes.as_ref(), 4, 3, 3), OPAQUE_BLACK_BGRA);
        assert_eq!(
            pixel(normalized.bytes.as_ref(), 4, 0, 1),
            gradient_pixel(0, 0)
        );
        assert_eq!(
            pixel(normalized.bytes.as_ref(), 4, 3, 2),
            gradient_pixel(3, 1)
        );
    }

    #[test]
    fn taller_bgra_frames_preserve_aspect_ratio_with_centered_black_padding() {
        let canvas = OutputCanvas::new(4, 4).expect("canvas");
        let source = gradient_bgra_frame(2, 4);

        let normalized =
            normalize_bgra_frame(2, 4, BGRA_FORMAT_CODE, &source, canvas).expect("normalize");

        assert_eq!(normalized.output_width, 4);
        assert_eq!(normalized.output_height, 4);
        assert_eq!(normalized.scaled_width, 2);
        assert_eq!(normalized.scaled_height, 4);
        assert_eq!(normalized.x_offset, 1);
        assert_eq!(normalized.y_offset, 0);
        assert_eq!(pixel(normalized.bytes.as_ref(), 4, 0, 0), OPAQUE_BLACK_BGRA);
        assert_eq!(pixel(normalized.bytes.as_ref(), 4, 3, 3), OPAQUE_BLACK_BGRA);
        assert_eq!(
            pixel(normalized.bytes.as_ref(), 4, 1, 0),
            gradient_pixel(0, 0)
        );
        assert_eq!(
            pixel(normalized.bytes.as_ref(), 4, 2, 3),
            gradient_pixel(1, 3)
        );
    }

    const RED: [u8; BGRA_BYTES_PER_PIXEL] = [0x00, 0x00, 0xff, 0xff];
    const GREEN: [u8; BGRA_BYTES_PER_PIXEL] = [0x00, 0xff, 0x00, 0xff];
    const BLUE: [u8; BGRA_BYTES_PER_PIXEL] = [0xff, 0x00, 0x00, 0xff];
    const WHITE: [u8; BGRA_BYTES_PER_PIXEL] = [0xff, 0xff, 0xff, 0xff];

    fn bgra_pixels(pixels: &[[u8; BGRA_BYTES_PER_PIXEL]]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(pixels.len() * BGRA_BYTES_PER_PIXEL);
        for pixel in pixels {
            bytes.extend_from_slice(pixel);
        }
        bytes
    }

    fn gradient_bgra_frame(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity((width * height * BGRA_BYTES_PER_PIXEL as u32) as usize);
        for y in 0..height {
            for x in 0..width {
                bytes.extend_from_slice(&gradient_pixel(x, y));
            }
        }
        bytes
    }

    fn gradient_pixel(x: u32, y: u32) -> [u8; BGRA_BYTES_PER_PIXEL] {
        [x as u8, y as u8, x.wrapping_add(y) as u8, 0xff]
    }

    fn pixel(bytes: &[u8], width: u32, x: u32, y: u32) -> [u8; BGRA_BYTES_PER_PIXEL] {
        let index = ((y * width + x) * BGRA_BYTES_PER_PIXEL as u32) as usize;
        bytes[index..index + BGRA_BYTES_PER_PIXEL]
            .try_into()
            .expect("pixel bytes")
    }
}
