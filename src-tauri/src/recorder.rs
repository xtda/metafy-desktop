use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::capture::ValidatedCaptureConfig;
use crate::storage::{RecordingSession, RecordingSessionFiles, RecordingSessionStatus};

const CAPTURE_FRAME_RATE: u32 = 30;
const VIDEO_FILE_MAGIC: &[u8] = b"METAFY_RAW_VIDEO_V1\n";
const AUDIO_FILE_MAGIC: &[u8] = b"METAFY_RAW_AUDIO_V1\n";

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
    audio_config: Option<AudioConfig>,
    started_instant: Instant,
    files: RecordingSessionFiles,
}

#[derive(Debug, Default, Clone)]
struct CaptureStats {
    frame_count: i64,
    audio_byte_count: i64,
    width: Option<i64>,
    height: Option<i64>,
    errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ActiveRecordingSnapshot {
    pub session_id: String,
    pub frame_count: i64,
    pub audio_byte_count: i64,
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
    pub audio_byte_count: i64,
    pub audio_config: Option<AudioConfig>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub stopped_at: String,
    pub duration_ms: i64,
    pub failure_message: Option<String>,
    pub files: RecordingSessionFiles,
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
    audio: Option<SessionAudioSidecar<'a>>,
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionAudioSidecar<'a> {
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
            audio_byte_count: stats.audio_byte_count,
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
    ) -> Result<(Option<i64>, Option<i64>, Option<AudioConfig>), String> {
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
        let mut capturer = build_screen_capturer(&config.screen_source.id)?;
        let output_size = capturer.get_output_frame_size();
        if let Ok(mut stats) = stats.lock() {
            stats.width = Some(i64::from(output_size[0]));
            stats.height = Some(i64::from(output_size[1]));
        }
        let video_file = File::create(&files.video_path)
            .map_err(|error| format!("Unable to create screen frame file: {error}"))?;

        let audio_runtime = if config.include_microphone {
            let audio_path = files.audio_path.as_ref().ok_or_else(|| {
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
        let audio_config = audio_runtime.as_ref().map(|runtime| AudioConfig {
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
            return Err("A recording session is already active.".to_owned());
        }

        let screen_handle = spawn_screen_writer(
            capturer,
            video_file,
            Arc::clone(&stop_signal),
            Arc::clone(&stats),
            started_instant,
        );
        let (audio_stream, audio_sender, audio_writer_handle) = split_audio_runtime(audio_runtime);
        let returned_audio_config = audio_config.clone();

        active.replace(ActiveSession {
            session_id: session.id.clone(),
            recording_id: session.recording_id.clone(),
            stop_signal,
            stats,
            screen_handle: Some(screen_handle),
            audio_stream,
            audio_sender,
            audio_writer_handle,
            audio_config,
            started_instant,
            files,
        });

        Ok((
            Some(i64::from(output_size[0])),
            Some(i64::from(output_size[1])),
            returned_audio_config,
        ))
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

        Ok(StoppedCapture {
            session_id: active.session_id,
            recording_id: active.recording_id,
            status,
            frame_count: stats.frame_count,
            audio_byte_count: stats.audio_byte_count,
            audio_config: active.audio_config,
            width: stats.width,
            height: stats.height,
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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Unable to create session metadata directory: {error}"))?;
    }

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
            format: "metafy raw BGRA frame stream v1",
        },
        audio: session.audio_path.as_ref().map(|path| SessionAudioSidecar {
            path,
            format: "raw interleaved PCM bytes with block timing headers v1",
            sample_rate: &session.audio_sample_rate,
            channels: &session.audio_channels,
            sample_format: &session.audio_sample_format,
            byte_count: session.audio_byte_count,
        }),
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

fn build_screen_capturer(source_id: &str) -> Result<scap::capturer::Capturer, String> {
    let target = selected_display_target(source_id)?;
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
        .map_err(|error| format!("Unable to start screen capture: {error}"))
}

fn selected_display_target(source_id: &str) -> Result<scap::Target, String> {
    let display_id = source_id
        .strip_prefix("display:")
        .ok_or_else(|| "Selected screen source is not a display.".to_owned())?
        .parse::<u32>()
        .map_err(|_| "Selected display id is invalid.".to_owned())?;
    let targets = scap::get_all_targets()
        .map_err(|error| format!("Unable to enumerate screen capture targets: {error}"))?;

    targets
        .into_iter()
        .find(|target| match target {
            scap::Target::Display(display) => display.id == display_id,
            scap::Target::Window(_) => false,
        })
        .ok_or_else(|| "Selected display is no longer available.".to_owned())
}

fn spawn_screen_writer(
    mut capturer: scap::capturer::Capturer,
    file: File,
    stop_signal: Arc<AtomicBool>,
    stats: Arc<Mutex<CaptureStats>>,
    started_instant: Instant,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut writer = BufWriter::new(file);
        if let Err(error) = writer.write_all(VIDEO_FILE_MAGIC) {
            push_error(
                &stats,
                format!("Unable to initialize screen frame file: {error}"),
            );
            return;
        }

        capturer.start_capture();

        while !stop_signal.load(Ordering::Relaxed) {
            match capturer.get_next_frame() {
                Ok(frame) => {
                    if let Err(error) =
                        write_video_frame(&mut writer, frame, &stats, started_instant)
                    {
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
        if let Err(error) = writer.flush() {
            push_error(
                &stats,
                format!("Unable to flush screen frame file: {error}"),
            );
        }
    })
}

fn write_video_frame(
    writer: &mut BufWriter<File>,
    frame: scap::frame::Frame,
    stats: &Arc<Mutex<CaptureStats>>,
    started_instant: Instant,
) -> Result<(), String> {
    let payload = video_frame_payload(frame);

    write_u64(writer, duration_ms_u64(started_instant.elapsed()))?;
    write_u64(writer, system_time_ms(payload.display_time))?;
    write_u32(writer, payload.format_code)?;
    write_u32(writer, payload.width)?;
    write_u32(writer, payload.height)?;
    write_u32(writer, payload.bytes.len().try_into().unwrap_or(u32::MAX))?;
    writer
        .write_all(&payload.bytes)
        .map_err(|error| format!("Unable to write screen frame: {error}"))?;

    if let Ok(mut stats) = stats.lock() {
        stats.frame_count += 1;
        stats.width = Some(i64::from(payload.width));
        stats.height = Some(i64::from(payload.height));
    }

    Ok(())
}

struct VideoFramePayload {
    display_time: SystemTime,
    width: u32,
    height: u32,
    format_code: u32,
    bytes: Vec<u8>,
}

fn video_frame_payload(frame: scap::frame::Frame) -> VideoFramePayload {
    match frame {
        scap::frame::Frame::YUVFrame(frame) => {
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
        scap::frame::Frame::RGB(frame) => VideoFramePayload {
            display_time: frame.display_time,
            width: frame.width.max(0) as u32,
            height: frame.height.max(0) as u32,
            format_code: 2,
            bytes: frame.data,
        },
        scap::frame::Frame::RGBx(frame) => VideoFramePayload {
            display_time: frame.display_time,
            width: frame.width.max(0) as u32,
            height: frame.height.max(0) as u32,
            format_code: 3,
            bytes: frame.data,
        },
        scap::frame::Frame::XBGR(frame) => VideoFramePayload {
            display_time: frame.display_time,
            width: frame.width.max(0) as u32,
            height: frame.height.max(0) as u32,
            format_code: 4,
            bytes: frame.data,
        },
        scap::frame::Frame::BGRx(frame) => VideoFramePayload {
            display_time: frame.display_time,
            width: frame.width.max(0) as u32,
            height: frame.height.max(0) as u32,
            format_code: 5,
            bytes: frame.data,
        },
        scap::frame::Frame::BGR0(frame) => VideoFramePayload {
            display_time: frame.display_time,
            width: frame.width.max(0) as u32,
            height: frame.height.max(0) as u32,
            format_code: 6,
            bytes: frame.data,
        },
        scap::frame::Frame::BGRA(frame) => VideoFramePayload {
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
                stats.audio_byte_count += block.bytes.len() as i64;
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

fn shutdown_audio_runtime(runtime: Option<AudioRuntime>) {
    if let Some(runtime) = runtime {
        drop(runtime.stream);
        drop(runtime.sender);
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
        .map_err(|error| format!("Unable to write microphone audio block: {error}"))
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
