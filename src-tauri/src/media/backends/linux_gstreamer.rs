#[cfg(target_os = "linux")]
mod platform {
    use std::fs::{self, File};
    use std::io::{self, Read};
    use std::path::Path;
    use std::sync::mpsc;
    use std::thread;

    use gstreamer::{self as gst, prelude::*};
    use gstreamer_app as gst_app;

    use crate::media::encode::{
        EncodeAudioInput, EncodeDiagnostics, EncodeInput, EncodeOutput, EncodeVideoFormat,
        EncodeVideoInput, RecordingEncoder,
    };
    use crate::media::metadata::{derive_media_info, MediaInfoSource};
    use crate::media::thumbnail::write_bgra_thumbnail;

    const BACKEND_NAME: &str = "linux-gstreamer";
    const BACKEND_LABEL: &str = "native-linux-gstreamer";
    const NATIVE_SUCCESS_MESSAGE: &str = "Encoded MP4 with GStreamer H.264/AAC.";
    const AUDIO_SAMPLE_FORMAT: &str = "f32";
    const AUDIO_GST_FORMAT: &str = "F32LE";
    const AUDIO_CHUNK_FRAMES: usize = 4_096;

    const REQUIRED_ELEMENTS: &[&str] = &[
        "appsrc",
        "videoconvert",
        "h264parse",
        "mp4mux",
        "filesink",
        "audioconvert",
        "audioresample",
        "aacparse",
    ];
    const VIDEO_ENCODERS: &[EncoderCandidate] = &[
        EncoderCandidate {
            factory: "vah264enc",
            description: "VA-API H.264 hardware encoder",
            launch: "vah264enc",
        },
        EncoderCandidate {
            factory: "vaapih264enc",
            description: "legacy VA-API H.264 hardware encoder",
            launch: "vaapih264enc",
        },
        EncoderCandidate {
            factory: "nvh264enc",
            description: "NVIDIA H.264 hardware encoder",
            launch: "nvh264enc",
        },
        EncoderCandidate {
            factory: "x264enc",
            description: "x264 software H.264 encoder",
            launch: "x264enc speed-preset=veryfast tune=zerolatency key-int-max=60 bitrate=4000",
        },
        EncoderCandidate {
            factory: "openh264enc",
            description: "OpenH264 software encoder",
            launch: "openh264enc",
        },
        EncoderCandidate {
            factory: "avenc_h264",
            description: "libav H.264 encoder",
            launch: "avenc_h264",
        },
    ];
    const AUDIO_ENCODERS: &[EncoderCandidate] = &[
        EncoderCandidate {
            factory: "fdkaacenc",
            description: "Fraunhofer FDK AAC encoder",
            launch: "fdkaacenc",
        },
        EncoderCandidate {
            factory: "voaacenc",
            description: "VO AAC encoder",
            launch: "voaacenc",
        },
        EncoderCandidate {
            factory: "avenc_aac",
            description: "libav AAC encoder",
            launch: "avenc_aac",
        },
    ];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct EncoderCandidate {
        factory: &'static str,
        description: &'static str,
        launch: &'static str,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct LinuxGstreamerReadiness {
        pub available: bool,
        pub selected_video_encoder: Option<&'static str>,
        pub selected_audio_encoder: Option<&'static str>,
        pub missing_elements: Vec<String>,
        pub messages: Vec<String>,
        pub install_hint: String,
    }

    pub struct LinuxGstreamerRecordingEncoder {
        video_encoder: EncoderCandidate,
        audio_encoder: EncoderCandidate,
        readiness: LinuxGstreamerReadiness,
    }

    impl LinuxGstreamerRecordingEncoder {
        pub fn new() -> Result<Self, String> {
            let readiness = Self::readiness();
            if !readiness.available {
                return Err(readiness_error(&readiness));
            }

            let video_encoder = encoder_by_factory(
                readiness
                    .selected_video_encoder
                    .ok_or_else(|| readiness_error(&readiness))?,
                VIDEO_ENCODERS,
            )
            .ok_or_else(|| readiness_error(&readiness))?;
            let audio_encoder = encoder_by_factory(
                readiness
                    .selected_audio_encoder
                    .ok_or_else(|| readiness_error(&readiness))?,
                AUDIO_ENCODERS,
            )
            .ok_or_else(|| readiness_error(&readiness))?;

            Ok(Self {
                video_encoder,
                audio_encoder,
                readiness,
            })
        }

        pub fn readiness() -> LinuxGstreamerReadiness {
            if let Err(error) = gst::init() {
                return LinuxGstreamerReadiness {
                    available: false,
                    selected_video_encoder: None,
                    selected_audio_encoder: None,
                    missing_elements: vec!["gstreamer runtime".to_owned()],
                    messages: vec![format!("Unable to initialize GStreamer: {error}")],
                    install_hint: gstreamer_install_hint(),
                };
            }

            let mut missing_elements = REQUIRED_ELEMENTS
                .iter()
                .filter(|element| gst::ElementFactory::find(element).is_none())
                .map(|element| (*element).to_owned())
                .collect::<Vec<_>>();
            let selected_video_encoder = select_encoder(VIDEO_ENCODERS).map(|encoder| {
                missing_elements.retain(|element| element != encoder.factory);
                encoder.factory
            });
            let selected_audio_encoder = select_encoder(AUDIO_ENCODERS).map(|encoder| {
                missing_elements.retain(|element| element != encoder.factory);
                encoder.factory
            });

            if selected_video_encoder.is_none() {
                missing_elements.push(format!(
                    "one of: {}",
                    VIDEO_ENCODERS
                        .iter()
                        .map(|encoder| encoder.factory)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if selected_audio_encoder.is_none() {
                missing_elements.push(format!(
                    "one of: {}",
                    AUDIO_ENCODERS
                        .iter()
                        .map(|encoder| encoder.factory)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }

            let available = missing_elements.is_empty();
            let messages = if available {
                vec![format!(
                    "GStreamer ready with video encoder {} and audio encoder {}.",
                    selected_video_encoder.unwrap_or("unknown"),
                    selected_audio_encoder.unwrap_or("unknown")
                )]
            } else {
                vec![format!(
                    "GStreamer is missing required encoder or muxing elements: {}.",
                    missing_elements.join(", ")
                )]
            };

            LinuxGstreamerReadiness {
                available,
                selected_video_encoder,
                selected_audio_encoder,
                missing_elements,
                messages,
                install_hint: gstreamer_install_hint(),
            }
        }

        pub fn backend_label(&self) -> String {
            BACKEND_LABEL.to_owned()
        }
    }

    impl RecordingEncoder for LinuxGstreamerRecordingEncoder {
        fn encode(&self, input: EncodeInput) -> Result<EncodeOutput, String> {
            validate_encode_input(&input)?;

            encode_with_gstreamer(&input, self.video_encoder, self.audio_encoder)?;
            rename_replace(&input.output.staging_media_path, &input.output.media_path)?;

            write_bgra_thumbnail(
                &input.video.thumbnail_frame,
                &input.output.staging_thumbnail_path,
            )
            .map_err(|error| format!("Thumbnail generation failed: {error}"))?;
            rename_replace(
                &input.output.staging_thumbnail_path,
                &input.output.thumbnail_path,
            )?;

            let mut messages = vec![
                NATIVE_SUCCESS_MESSAGE.to_owned(),
                format!(
                    "GStreamer video encoder: {} ({}).",
                    self.video_encoder.factory, self.video_encoder.description
                ),
                format!(
                    "GStreamer audio encoder: {} ({}).",
                    self.audio_encoder.factory, self.audio_encoder.description
                ),
            ];
            messages.extend(self.readiness.messages.clone());

            let diagnostics = EncodeDiagnostics {
                backend: BACKEND_NAME.to_owned(),
                commands: Vec::new(),
                messages,
            };
            let audio_duration_ms = input
                .audio_inputs
                .iter()
                .filter_map(|audio| audio.duration_ms)
                .max();
            let media_info = derive_media_info(
                MediaInfoSource {
                    width: input.video.width,
                    height: input.video.height,
                    frame_rate: input.video.frame_rate,
                    frame_count: input.video.frame_count,
                    audio_duration_ms,
                    duration_hint_ms: input.duration_hint_ms,
                    audio_included: !input.audio_inputs.is_empty(),
                    backend_id: diagnostics.backend.clone(),
                    backend_diagnostics: diagnostics.messages.clone(),
                },
                &input.output.media_path,
            );

            Ok(EncodeOutput {
                recording_id: input.recording_id,
                media_path: input.output.media_path_relative,
                thumbnail_path: Some(input.output.thumbnail_path_relative.clone()),
                absolute_media_path: path_to_string(&input.output.media_path),
                absolute_thumbnail_path: input
                    .output
                    .thumbnail_path
                    .exists()
                    .then(|| path_to_string(&input.output.thumbnail_path)),
                duration_ms: media_info.duration_ms,
                width: media_info.width,
                height: media_info.height,
                frame_rate: media_info.frame_rate,
                frame_count: media_info.frame_count,
                audio_included: media_info.audio_included,
                media_info,
                diagnostics,
                warnings: input.warnings,
            })
        }
    }

    fn encode_with_gstreamer(
        input: &EncodeInput,
        video_encoder: EncoderCandidate,
        audio_encoder: EncoderCandidate,
    ) -> Result<(), String> {
        let launch = pipeline_description(
            video_encoder,
            input.audio_inputs.first().map(|_| audio_encoder),
            &input.output.staging_media_path,
        );
        let pipeline = gst::parse::launch(&launch)
            .map_err(|error| format!("Unable to build GStreamer encoder pipeline: {error}"))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| "GStreamer parser did not return a pipeline.".to_owned())?;
        let video_src = appsrc_by_name(&pipeline, "video_src")?;
        configure_video_appsrc(&video_src, &input.video)?;
        let audio_src = if input.audio_inputs.is_empty() {
            None
        } else {
            let audio_src = appsrc_by_name(&pipeline, "audio_src")?;
            configure_audio_appsrc(&audio_src, &input.audio_inputs[0])?;
            Some(audio_src)
        };
        let bus = pipeline
            .bus()
            .ok_or_else(|| "GStreamer encoder pipeline has no bus.".to_owned())?;
        let (error_tx, error_rx) = mpsc::channel::<String>();
        let mut workers = Vec::new();

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| format!("Unable to start GStreamer encoder pipeline: {error:?}"))?;

        let video_input = input.video.clone();
        workers.push(spawn_feed_worker(error_tx.clone(), move || {
            push_video_frames(video_src, video_input)
        }));
        if let Some(audio_src) = audio_src {
            let audio_input = input.audio_inputs[0].clone();
            workers.push(spawn_feed_worker(error_tx, move || {
                push_audio_samples(audio_src, audio_input)
            }));
        }

        let wait_result = wait_for_pipeline(&bus, &error_rx);
        let _ = pipeline.set_state(gst::State::Null);
        let worker_result = join_workers(workers);

        wait_result.and(worker_result)
    }

    fn pipeline_description(
        video_encoder: EncoderCandidate,
        audio_encoder: Option<EncoderCandidate>,
        output_path: &Path,
    ) -> String {
        let output_location = gst_launch_escape(&path_to_string(output_path));
        let video_branch = format!(
            "appsrc name=video_src format=time is-live=false do-timestamp=false \
             ! videoconvert \
             ! video/x-raw,format=I420 \
             ! {} \
             ! h264parse \
             ! queue \
             ! mux.",
            video_encoder.launch
        );

        if let Some(audio_encoder) = audio_encoder {
            format!(
                "mp4mux name=mux \
                 ! filesink name=output_sink location=\"{output_location}\" \
                 {video_branch} \
                 appsrc name=audio_src format=time is-live=false do-timestamp=false \
                 ! audioconvert \
                 ! audioresample \
                 ! audio/x-raw,rate=48000,channels=2 \
                 ! {} \
                 ! aacparse \
                 ! queue \
                 ! mux.",
                audio_encoder.launch
            )
        } else {
            format!(
                "mp4mux name=mux \
                 ! filesink name=output_sink location=\"{output_location}\" \
                 {video_branch}"
            )
        }
    }

    fn appsrc_by_name(pipeline: &gst::Pipeline, name: &str) -> Result<gst_app::AppSrc, String> {
        pipeline
            .by_name(name)
            .ok_or_else(|| format!("GStreamer pipeline is missing appsrc {name}."))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| format!("GStreamer element {name} is not an appsrc."))
    }

    fn configure_video_appsrc(
        appsrc: &gst_app::AppSrc,
        video: &EncodeVideoInput,
    ) -> Result<(), String> {
        let width = i32::try_from(video.width)
            .map_err(|_| "Linux GStreamer encoder received invalid video width.".to_owned())?;
        let height = i32::try_from(video.height)
            .map_err(|_| "Linux GStreamer encoder received invalid video height.".to_owned())?;
        let frame_rate = i32::try_from(video.frame_rate)
            .map_err(|_| "Linux GStreamer encoder received invalid video frame rate.".to_owned())?;
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "BGRA")
            .field("width", width)
            .field("height", height)
            .field("framerate", gst::Fraction::new(frame_rate, 1))
            .build();

        appsrc.set_caps(Some(&caps));
        appsrc.set_format(gst::Format::Time);
        appsrc.set_is_live(false);
        appsrc.set_block(true);
        appsrc.set_size(expected_video_bytes(video)? as i64);
        appsrc.set_max_bytes(expected_video_frame_bytes(video)? as u64 * 4);

        Ok(())
    }

    fn configure_audio_appsrc(
        appsrc: &gst_app::AppSrc,
        audio: &EncodeAudioInput,
    ) -> Result<(), String> {
        let sample_rate = i32::try_from(audio.sample_rate).map_err(|_| {
            "Linux GStreamer encoder received invalid audio sample rate.".to_owned()
        })?;
        let channels = i32::try_from(audio.channels).map_err(|_| {
            "Linux GStreamer encoder received invalid audio channel count.".to_owned()
        })?;
        let caps = gst::Caps::builder("audio/x-raw")
            .field("format", AUDIO_GST_FORMAT)
            .field("layout", "interleaved")
            .field("rate", sample_rate)
            .field("channels", channels)
            .build();

        appsrc.set_caps(Some(&caps));
        appsrc.set_format(gst::Format::Time);
        appsrc.set_is_live(false);
        appsrc.set_block(true);
        appsrc.set_size(audio_file_size(audio)? as i64);
        appsrc.set_max_bytes((AUDIO_CHUNK_FRAMES * audio_frame_bytes(audio)?) as u64 * 4);

        Ok(())
    }

    fn spawn_feed_worker<F>(
        error_tx: mpsc::Sender<String>,
        feed: F,
    ) -> thread::JoinHandle<Result<(), String>>
    where
        F: FnOnce() -> Result<(), String> + Send + 'static,
    {
        thread::spawn(move || {
            let result = feed();
            if let Err(error) = &result {
                let _ = error_tx.send(error.clone());
            }
            result
        })
    }

    fn wait_for_pipeline(bus: &gst::Bus, error_rx: &mpsc::Receiver<String>) -> Result<(), String> {
        loop {
            if let Ok(error) = error_rx.try_recv() {
                return Err(error);
            }

            let Some(message) = bus.timed_pop(gst::ClockTime::from_mseconds(100)) else {
                continue;
            };

            match message.view() {
                gst::MessageView::Eos(_) => return Ok(()),
                gst::MessageView::Error(error) => {
                    let source = error
                        .src()
                        .map(|source| source.path_string())
                        .unwrap_or_else(|| "unknown source".into());
                    let debug = error
                        .debug()
                        .map(|debug| format!(" ({debug})"))
                        .unwrap_or_default();
                    return Err(format!(
                        "GStreamer encode failed in {source}: {error}{debug}",
                        error = error.error()
                    ));
                }
                _ => {}
            }
        }
    }

    fn join_workers(workers: Vec<thread::JoinHandle<Result<(), String>>>) -> Result<(), String> {
        let mut first_error = None;
        for worker in workers {
            match worker.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
                Err(_) => {
                    if first_error.is_none() {
                        first_error = Some("GStreamer appsrc feed thread panicked.".to_owned());
                    }
                }
            }
        }

        first_error.map_or(Ok(()), Err)
    }

    fn push_video_frames(appsrc: gst_app::AppSrc, video: EncodeVideoInput) -> Result<(), String> {
        let frame_bytes = expected_video_frame_bytes(&video)?;
        let mut file = File::open(&video.path)
            .map_err(|error| format!("Unable to open prepared video input: {error}"))?;
        let mut bytes = vec![0_u8; frame_bytes];

        for frame_index in 0..video.frame_count {
            file.read_exact(&mut bytes)
                .map_err(|error| format!("Unable to read prepared video frame: {error}"))?;
            let mut buffer = gst::Buffer::with_size(frame_bytes)
                .map_err(|error| format!("Unable to allocate GStreamer video buffer: {error}"))?;
            {
                let buffer_ref = buffer
                    .get_mut()
                    .ok_or_else(|| "Unable to make GStreamer video buffer writable.".to_owned())?;
                buffer_ref.set_pts(clock_time_for_frame(frame_index, video.frame_rate)?);
                buffer_ref.set_duration(frame_duration(frame_index, video.frame_rate)?);
                let mut map = buffer_ref
                    .map_writable()
                    .map_err(|_| "Unable to map GStreamer video buffer.".to_owned())?;
                map.as_mut_slice().copy_from_slice(&bytes);
            }
            appsrc
                .push_buffer(buffer)
                .map_err(|error| format!("Unable to push video frame to GStreamer: {error:?}"))?;
        }

        appsrc
            .end_of_stream()
            .map_err(|error| format!("Unable to finish GStreamer video stream: {error:?}"))?;
        Ok(())
    }

    fn push_audio_samples(appsrc: gst_app::AppSrc, audio: EncodeAudioInput) -> Result<(), String> {
        let frame_bytes = audio_frame_bytes(&audio)?;
        let chunk_bytes = AUDIO_CHUNK_FRAMES
            .checked_mul(frame_bytes)
            .ok_or_else(|| "Linux GStreamer audio chunk is too large.".to_owned())?;
        let mut file = File::open(&audio.path)
            .map_err(|error| format!("Unable to open prepared audio input: {error}"))?;
        let mut frame_offset = 0_i64;

        loop {
            let mut bytes = vec![0_u8; chunk_bytes];
            let bytes_read = file
                .read(&mut bytes)
                .map_err(|error| format!("Unable to read prepared audio input: {error}"))?;
            if bytes_read == 0 {
                break;
            }
            bytes.truncate(bytes_read);
            if bytes.len() % frame_bytes != 0 {
                return Err("Prepared audio input ended mid-frame.".to_owned());
            }
            let chunk_frames = i64::try_from(bytes.len() / frame_bytes)
                .map_err(|_| "Prepared audio input is too large.".to_owned())?;
            let mut buffer = gst::Buffer::with_size(bytes.len())
                .map_err(|error| format!("Unable to allocate GStreamer audio buffer: {error}"))?;
            {
                let buffer_ref = buffer
                    .get_mut()
                    .ok_or_else(|| "Unable to make GStreamer audio buffer writable.".to_owned())?;
                buffer_ref.set_pts(clock_time_for_audio_frame(frame_offset, audio.sample_rate)?);
                buffer_ref.set_duration(audio_duration(chunk_frames, audio.sample_rate)?);
                let mut map = buffer_ref
                    .map_writable()
                    .map_err(|_| "Unable to map GStreamer audio buffer.".to_owned())?;
                map.as_mut_slice().copy_from_slice(&bytes);
            }
            appsrc
                .push_buffer(buffer)
                .map_err(|error| format!("Unable to push audio samples to GStreamer: {error:?}"))?;
            frame_offset += chunk_frames;
        }

        appsrc
            .end_of_stream()
            .map_err(|error| format!("Unable to finish GStreamer audio stream: {error:?}"))?;
        Ok(())
    }

    fn validate_encode_input(input: &EncodeInput) -> Result<(), String> {
        if input.audio_inputs.len() > 1 {
            return Err(
                "Linux GStreamer encoder expects at most one prepared mixed audio input."
                    .to_owned(),
            );
        }
        if input.video.format != EncodeVideoFormat::RawBgra {
            return Err("Linux GStreamer encoder expects prepared raw BGRA video.".to_owned());
        }
        if input.video.width <= 0 || input.video.height <= 0 {
            return Err("Linux GStreamer encoder received invalid video dimensions.".to_owned());
        }
        if input.video.frame_rate <= 0 || input.video.frame_count <= 0 {
            return Err("Linux GStreamer encoder received invalid video timing.".to_owned());
        }
        expected_video_bytes(&input.video)?;

        for audio in &input.audio_inputs {
            if audio.sample_format != AUDIO_SAMPLE_FORMAT {
                return Err(format!(
                    "Linux GStreamer encoder expected prepared f32 audio, got {}.",
                    audio.sample_format
                ));
            }
            if audio.sample_rate <= 0 || audio.channels <= 0 {
                return Err(
                    "Linux GStreamer encoder received invalid audio format metadata.".to_owned(),
                );
            }
            audio_file_size(audio)?;
        }

        Ok(())
    }

    fn expected_video_frame_bytes(video: &EncodeVideoInput) -> Result<usize, String> {
        let width = usize::try_from(video.width)
            .map_err(|_| "Linux GStreamer encoder received invalid video width.".to_owned())?;
        let height = usize::try_from(video.height)
            .map_err(|_| "Linux GStreamer encoder received invalid video height.".to_owned())?;

        width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| "Linux GStreamer video frame is too large.".to_owned())
    }

    fn expected_video_bytes(video: &EncodeVideoInput) -> Result<usize, String> {
        let frame_bytes = expected_video_frame_bytes(video)?;
        let frame_count = usize::try_from(video.frame_count).map_err(|_| {
            "Linux GStreamer encoder received invalid video frame count.".to_owned()
        })?;
        let expected = frame_bytes
            .checked_mul(frame_count)
            .ok_or_else(|| "Linux GStreamer video input is too large.".to_owned())?;
        let actual = fs::metadata(&video.path)
            .map_err(|error| format!("Unable to read prepared video input metadata: {error}"))?
            .len();
        if actual != expected as u64 {
            return Err(format!(
                "Linux GStreamer encoder expected prepared video input to be {expected} bytes, got {actual}."
            ));
        }

        Ok(expected)
    }

    fn audio_frame_bytes(audio: &EncodeAudioInput) -> Result<usize, String> {
        let channels = usize::try_from(audio.channels).map_err(|_| {
            "Linux GStreamer encoder received invalid audio channel count.".to_owned()
        })?;
        channels
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or_else(|| "Linux GStreamer audio frame is too large.".to_owned())
    }

    fn audio_file_size(audio: &EncodeAudioInput) -> Result<u64, String> {
        let frame_bytes = audio_frame_bytes(audio)?;
        let size = fs::metadata(&audio.path)
            .map_err(|error| format!("Unable to read prepared audio input metadata: {error}"))?
            .len();
        if frame_bytes == 0 || size % frame_bytes as u64 != 0 {
            return Err(format!(
                "Linux GStreamer encoder expected prepared audio to be aligned to {} bytes per frame, got {size} bytes.",
                frame_bytes
            ));
        }
        Ok(size)
    }

    fn clock_time_for_frame(frame_index: i64, frame_rate: i64) -> Result<gst::ClockTime, String> {
        if frame_index < 0 || frame_rate <= 0 {
            return Err("Linux GStreamer encoder received invalid video timing.".to_owned());
        }
        Ok(gst::ClockTime::from_nseconds(scaled_time_nanos(
            frame_index,
            frame_rate,
        )?))
    }

    fn frame_duration(frame_index: i64, frame_rate: i64) -> Result<gst::ClockTime, String> {
        let start = scaled_time_nanos(frame_index, frame_rate)?;
        let end = scaled_time_nanos(frame_index + 1, frame_rate)?;
        Ok(gst::ClockTime::from_nseconds(end.saturating_sub(start)))
    }

    fn clock_time_for_audio_frame(
        frame_offset: i64,
        sample_rate: i64,
    ) -> Result<gst::ClockTime, String> {
        if frame_offset < 0 || sample_rate <= 0 {
            return Err("Linux GStreamer encoder received invalid audio timing.".to_owned());
        }
        Ok(gst::ClockTime::from_nseconds(scaled_time_nanos(
            frame_offset,
            sample_rate,
        )?))
    }

    fn audio_duration(frame_count: i64, sample_rate: i64) -> Result<gst::ClockTime, String> {
        if frame_count < 0 || sample_rate <= 0 {
            return Err("Linux GStreamer encoder received invalid audio timing.".to_owned());
        }
        Ok(gst::ClockTime::from_nseconds(scaled_time_nanos(
            frame_count,
            sample_rate,
        )?))
    }

    fn scaled_time_nanos(units: i64, units_per_second: i64) -> Result<u64, String> {
        if units < 0 || units_per_second <= 0 {
            return Err("Linux GStreamer encoder received invalid timestamp input.".to_owned());
        }
        let nanos = (i128::from(units) * 1_000_000_000_i128) / i128::from(units_per_second);
        u64::try_from(nanos).map_err(|_| "Linux GStreamer timestamp is too large.".to_owned())
    }

    fn select_encoder(candidates: &[EncoderCandidate]) -> Option<EncoderCandidate> {
        candidates
            .iter()
            .copied()
            .find(|candidate| gst::ElementFactory::find(candidate.factory).is_some())
    }

    fn encoder_by_factory(
        factory: &str,
        candidates: &[EncoderCandidate],
    ) -> Option<EncoderCandidate> {
        candidates
            .iter()
            .copied()
            .find(|candidate| candidate.factory == factory)
    }

    fn readiness_error(readiness: &LinuxGstreamerReadiness) -> String {
        format!(
            "Linux GStreamer encoder is not ready. {} Install required GStreamer plugins and retry. {}",
            readiness
                .messages
                .first()
                .cloned()
                .unwrap_or_else(|| "No readiness detail was reported.".to_owned()),
            readiness.install_hint
        )
    }

    fn gstreamer_install_hint() -> String {
        "Ubuntu/Debian packages usually include gstreamer1.0-tools, gstreamer1.0-plugins-base, gstreamer1.0-plugins-good, gstreamer1.0-plugins-bad, gstreamer1.0-plugins-ugly, and gstreamer1.0-libav; Fedora packages usually include gstreamer1, gstreamer1-plugins-base, gstreamer1-plugins-good, gstreamer1-plugins-bad-free, gstreamer1-plugins-ugly-free, and gstreamer1-libav.".to_owned()
    }

    fn gst_launch_escape(value: &str) -> String {
        value.replace('\\', "\\\\").replace('"', "\\\"")
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
        use crate::media::encode::{
            EncodeAudioInput, EncodeOutputPaths, EncodeVideoInput, RecordingEncoder,
        };
        use crate::media::thumbnail::BgraFrame;
        use std::fs::File;
        use std::io::Write;
        use std::path::{Path, PathBuf};
        use uuid::Uuid;

        #[test]
        fn readiness_reports_missing_plugins_actionably() {
            let readiness = LinuxGstreamerRecordingEncoder::readiness();

            assert!(readiness.install_hint.contains("GStreamer"));
            if !readiness.available {
                assert!(!readiness.missing_elements.is_empty());
                assert!(readiness.messages.iter().any(|message| {
                    message.contains("GStreamer")
                        && message.contains("missing required encoder or muxing elements")
                }));
            }
        }

        #[test]
        fn encodes_synthetic_video_only() {
            let Ok(encoder) = LinuxGstreamerRecordingEncoder::new() else {
                return;
            };
            let root = test_root("video-only");
            let input = synthetic_input(&root, false).expect("synthetic input");

            let output = encoder.encode(input).expect("encode video-only");

            assert_eq!(output.diagnostics.backend, BACKEND_NAME);
            assert!(output.diagnostics.commands.is_empty());
            assert!(!output.audio_included);
            assert!(Path::new(&output.absolute_media_path).is_file());
            assert!(output
                .absolute_thumbnail_path
                .as_deref()
                .map(Path::new)
                .is_some_and(Path::is_file));

            let _ = fs::remove_dir_all(root);
        }

        #[test]
        fn encodes_synthetic_audio() {
            let Ok(encoder) = LinuxGstreamerRecordingEncoder::new() else {
                return;
            };
            let root = test_root("with-audio");
            let input = synthetic_input(&root, true).expect("synthetic input");

            let output = encoder.encode(input).expect("encode with audio");

            assert_eq!(output.diagnostics.backend, BACKEND_NAME);
            assert!(output.diagnostics.commands.is_empty());
            assert!(output.audio_included);
            assert_eq!(output.duration_ms, Some(100));
            assert!(
                fs::metadata(&output.absolute_media_path)
                    .expect("media metadata")
                    .len()
                    > 0
            );

            let _ = fs::remove_dir_all(root);
        }

        fn test_root(label: &str) -> PathBuf {
            let root = std::env::temp_dir().join(format!(
                "metafy-linux-gstreamer-encoder-{label}-{}",
                Uuid::new_v4()
            ));
            fs::create_dir_all(&root).expect("create test root");
            root
        }

        fn synthetic_input(root: &Path, include_audio: bool) -> Result<EncodeInput, String> {
            let width = 2;
            let height = 2;
            let frame_count = 3;
            let frame_rate = 30;
            let video_path = root.join("video.bgra");
            let audio_path = root.join("audio.raw");
            let media_path = root.join("recording.mp4");
            let thumbnail_path = root.join("thumbnail.jpg");
            let staging_media_path = root.join("recording.tmp.mp4");
            let staging_thumbnail_path = root.join("thumbnail.tmp.jpg");

            write_synthetic_video(&video_path, frame_count, width, height)?;
            if include_audio {
                write_synthetic_audio(&audio_path, 4_800)?;
            }

            let thumbnail_frame = BgraFrame {
                width: width as u32,
                height: height as u32,
                bytes: synthetic_frame_bytes(0, width, height),
            };

            Ok(EncodeInput {
                recording_id: "synthetic".to_owned(),
                video: EncodeVideoInput {
                    path: video_path,
                    format: EncodeVideoFormat::RawBgra,
                    width,
                    height,
                    frame_rate,
                    frame_count,
                    thumbnail_frame,
                },
                audio_inputs: include_audio
                    .then(|| EncodeAudioInput {
                        path: audio_path,
                        sample_rate: 48_000,
                        channels: 2,
                        sample_format: AUDIO_SAMPLE_FORMAT.to_owned(),
                        duration_ms: Some(100),
                    })
                    .into_iter()
                    .collect(),
                output: EncodeOutputPaths {
                    media_path,
                    media_path_relative: "recording.mp4".to_owned(),
                    thumbnail_path,
                    thumbnail_path_relative: "thumbnail.jpg".to_owned(),
                    staging_media_path,
                    staging_thumbnail_path,
                },
                duration_hint_ms: None,
                warnings: Vec::new(),
            })
        }

        fn write_synthetic_video(
            path: &Path,
            frame_count: i64,
            width: i64,
            height: i64,
        ) -> Result<(), String> {
            let mut file = File::create(path)
                .map_err(|error| format!("Unable to create test video: {error}"))?;
            for frame_index in 0..frame_count {
                file.write_all(&synthetic_frame_bytes(frame_index, width, height))
                    .map_err(|error| format!("Unable to write test video: {error}"))?;
            }
            Ok(())
        }

        fn synthetic_frame_bytes(frame_index: i64, width: i64, height: i64) -> Vec<u8> {
            let mut bytes = Vec::new();
            for pixel in 0..(width * height) {
                bytes.push((frame_index * 40 + pixel * 15) as u8);
                bytes.push((80 + pixel * 20) as u8);
                bytes.push((160 + frame_index * 15) as u8);
                bytes.push(255);
            }
            bytes
        }

        fn write_synthetic_audio(path: &Path, frame_count: usize) -> Result<(), String> {
            let mut file = File::create(path)
                .map_err(|error| format!("Unable to create test audio: {error}"))?;
            for frame_index in 0..frame_count {
                let phase = frame_index as f32 / 32.0;
                let sample = phase.sin() * 0.2;
                file.write_all(&sample.to_le_bytes())
                    .map_err(|error| format!("Unable to write test audio: {error}"))?;
                file.write_all(&sample.to_le_bytes())
                    .map_err(|error| format!("Unable to write test audio: {error}"))?;
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
pub use platform::LinuxGstreamerRecordingEncoder;

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
pub struct LinuxGstreamerRecordingEncoder;

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
impl LinuxGstreamerRecordingEncoder {
    pub fn new() -> Result<Self, String> {
        Err("Linux GStreamer encoder is only available on Linux.".to_owned())
    }
}
