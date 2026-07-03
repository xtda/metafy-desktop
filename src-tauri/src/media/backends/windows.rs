#[cfg(target_os = "windows")]
mod platform {
    use std::fs::{self, File};
    use std::io::{self, BufReader, Read};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;

    use windows::core::{Error as WindowsError, PCWSTR};
    use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_FALSE, S_OK};
    use windows::Win32::Media::MediaFoundation::{
        IMFAttributes, IMFByteStream, IMFMediaBuffer, IMFMediaType, IMFSample, IMFSinkWriter,
        MFAudioFormat_AAC, MFAudioFormat_PCM, MFCreateAttributes, MFCreateMediaType,
        MFCreateMemoryBuffer, MFCreateSample, MFCreateSinkWriterFromURL, MFMediaType_Audio,
        MFMediaType_Video, MFShutdown, MFStartup, MFTranscodeContainerType_MPEG4,
        MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoFormat_RGB32, MFVideoInterlace_Progressive,
        MFSTARTUP_FULL, MF_LOW_LATENCY, MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
        MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS,
        MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
        MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
        MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SINK_WRITER_DISABLE_THROTTLING,
        MF_TRANSCODE_CONTAINERTYPE, MF_VERSION,
    };
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    use crate::media::encode::{
        EncodeAudioInput, EncodeDiagnostics, EncodeInput, EncodeOutput, EncodeVideoFormat,
        RecordingEncoder,
    };
    use crate::media::metadata::{derive_media_info, MediaInfoSource};
    use crate::media::thumbnail::write_bgra_thumbnail;

    const BACKEND_NAME: &str = "windows-media-foundation";
    const BACKEND_LABEL: &str = "native-windows-media-foundation";
    const NATIVE_SUCCESS_MESSAGE: &str = "Encoded MP4 with Media Foundation H.264/AAC.";
    const RGB32_DIRECT_MESSAGE: &str = "Media Foundation accepted RGB32/BGRA video input directly.";
    const NV12_FALLBACK_MESSAGE: &str =
        "Media Foundation rejected RGB32 input; converted BGRA frames to NV12 before H.264 encode.";
    const AUDIO_MESSAGE: &str =
        "Converted prepared f32 48 kHz stereo mixdown to PCM16 samples for AAC encode.";
    const HNS_PER_SECOND: i64 = 10_000_000;
    const BGRA_BYTES_PER_PIXEL: usize = 4;
    const PCM16_BYTES_PER_SAMPLE: usize = 2;
    const AUDIO_BITRATE: u32 = 160_000;
    const AUDIO_CHUNK_FRAMES: u64 = 1024;

    trait WindowsResultContext<T> {
        fn context(self, context: &str) -> Result<T, String>;
    }

    impl<T> WindowsResultContext<T> for windows::core::Result<T> {
        fn context(self, context: &str) -> Result<T, String> {
            self.map_err(|error| windows_error(context, error))
        }
    }

    pub struct WindowsRecordingEncoder;

    impl WindowsRecordingEncoder {
        pub fn new() -> Self {
            Self
        }

        pub fn backend_label(&self) -> String {
            BACKEND_LABEL.to_owned()
        }
    }

    impl RecordingEncoder for WindowsRecordingEncoder {
        fn encode(&self, input: EncodeInput) -> Result<EncodeOutput, String> {
            validate_encode_input(&input)?;
            let _session = MediaFoundationSession::start()?;
            let audio_input = input.audio_inputs.first();
            let mut messages = vec![NATIVE_SUCCESS_MESSAGE.to_owned()];
            let mut writer = match SinkWriter::create(&input, VideoInputFormat::Rgb32) {
                Ok(writer) => {
                    messages.push(RGB32_DIRECT_MESSAGE.to_owned());
                    writer
                }
                Err(rgb32_error) => {
                    validate_nv12_dimensions(input.video.width, input.video.height)
                        .map_err(|nv12_error| format!("{rgb32_error}; {nv12_error}"))?;
                    let writer = SinkWriter::create(&input, VideoInputFormat::Nv12)
                        .map_err(|nv12_error| {
                            format!(
                                "Media Foundation rejected RGB32 input ({rgb32_error}) and NV12 fallback failed ({nv12_error})."
                            )
                        })?;
                    messages.push(NV12_FALLBACK_MESSAGE.to_owned());
                    writer
                }
            };

            writer.write_video_samples(
                &input.video.path,
                input.video.width,
                input.video.height,
                input.video.frame_rate,
                input.video.frame_count,
            )?;
            if let Some(audio) = audio_input {
                writer.write_audio_samples(audio)?;
                messages.push(AUDIO_MESSAGE.to_owned());
            }
            writer.finalize()?;

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

    struct MediaFoundationSession {
        _com: ComApartment,
    }

    impl MediaFoundationSession {
        fn start() -> Result<Self, String> {
            let com = ComApartment::initialize()?;
            unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
                .map_err(|error| windows_error("Unable to initialize Media Foundation", error))?;
            Ok(Self { _com: com })
        }
    }

    impl Drop for MediaFoundationSession {
        fn drop(&mut self) {
            let _ = unsafe { MFShutdown() };
        }
    }

    struct ComApartment {
        should_uninitialize: bool,
    }

    impl ComApartment {
        fn initialize() -> Result<Self, String> {
            let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if status == S_OK || status == S_FALSE {
                return Ok(Self {
                    should_uninitialize: true,
                });
            }
            if status == RPC_E_CHANGED_MODE {
                return Ok(Self {
                    should_uninitialize: false,
                });
            }

            status
                .ok()
                .map(|()| Self {
                    should_uninitialize: false,
                })
                .context("Unable to initialize COM for encoding")
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            if self.should_uninitialize {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum VideoInputFormat {
        Rgb32,
        Nv12,
    }

    struct SinkWriter {
        writer: IMFSinkWriter,
        video_stream: u32,
        audio_stream: Option<u32>,
        video_format: VideoInputFormat,
    }

    impl SinkWriter {
        fn create(input: &EncodeInput, video_format: VideoInputFormat) -> Result<Self, String> {
            let attributes = create_writer_attributes()?;
            let output_path = wide_path(&input.output.staging_media_path);
            let writer = unsafe {
                MFCreateSinkWriterFromURL(
                    PCWSTR::from_raw(output_path.as_ptr()),
                    None::<&IMFByteStream>,
                    &attributes,
                )
            }
            .map_err(|error| {
                windows_error("Unable to create Media Foundation sink writer", error)
            })?;

            let video_output_type = create_video_output_type(
                input.video.width,
                input.video.height,
                input.video.frame_rate,
            )?;
            let video_stream = unsafe { writer.AddStream(&video_output_type) }
                .map_err(|error| windows_error("Unable to add H.264 video stream", error))?;
            let video_input_type = create_video_input_type(
                input.video.width,
                input.video.height,
                input.video.frame_rate,
                video_format,
            )?;
            unsafe {
                writer.SetInputMediaType(video_stream, &video_input_type, None::<&IMFAttributes>)
            }
            .map_err(|error| windows_error("Unable to set raw video input type", error))?;

            let audio_stream = input
                .audio_inputs
                .first()
                .map(|audio| configure_audio_stream(&writer, audio))
                .transpose()?;

            unsafe { writer.BeginWriting() }.map_err(|error| {
                windows_error("Unable to start Media Foundation sink writer", error)
            })?;

            Ok(Self {
                writer,
                video_stream,
                audio_stream,
                video_format,
            })
        }

        fn write_video_samples(
            &mut self,
            path: &Path,
            width: i64,
            height: i64,
            frame_rate: i64,
            frame_count: i64,
        ) -> Result<(), String> {
            let frame_size = bgra_frame_size(width, height)?;
            let expected_len = checked_mul_u64(frame_size as u64, frame_count as u64)
                .ok_or_else(|| "Raw video sidecar is too large for Windows encoding.".to_owned())?;
            let actual_len = fs::metadata(path)
                .map_err(|error| format!("Unable to read prepared video input metadata: {error}"))?
                .len();
            if actual_len != expected_len {
                return Err(format!(
                    "Prepared video input has {actual_len} bytes; expected {expected_len}."
                ));
            }

            let mut reader = BufReader::new(
                File::open(path)
                    .map_err(|error| format!("Unable to read prepared video input: {error}"))?,
            );
            let mut bgra = vec![0_u8; frame_size];

            for frame_index in 0..frame_count {
                reader
                    .read_exact(&mut bgra)
                    .map_err(|error| format!("Unable to read prepared video frame: {error}"))?;
                let sample_bytes = match self.video_format {
                    VideoInputFormat::Rgb32 => bgra.as_slice(),
                    VideoInputFormat::Nv12 => {
                        let nv12 = bgra_to_nv12(&bgra, width, height)?;
                        self.write_sample_bytes(self.video_stream, &nv12, frame_index, frame_rate)?;
                        continue;
                    }
                };
                self.write_sample_bytes(self.video_stream, sample_bytes, frame_index, frame_rate)?;
            }

            Ok(())
        }

        fn write_audio_samples(&mut self, audio: &EncodeAudioInput) -> Result<(), String> {
            let stream = self
                .audio_stream
                .ok_or_else(|| "Media Foundation audio stream was not configured.".to_owned())?;
            let channels = usize::try_from(audio.channels)
                .map_err(|_| "Audio channel count is invalid for Windows encoding.".to_owned())?;
            let sample_rate = audio.sample_rate;
            let bytes_per_frame = channels
                .checked_mul(std::mem::size_of::<f32>())
                .ok_or_else(|| "Audio frame size is too large for Windows encoding.".to_owned())?;
            let total_bytes = fs::metadata(&audio.path)
                .map_err(|error| format!("Unable to read prepared audio input metadata: {error}"))?
                .len();
            if total_bytes % bytes_per_frame as u64 != 0 {
                return Err(format!(
                    "Prepared audio input has {total_bytes} bytes, which is not aligned to {channels}-channel f32 frames."
                ));
            }

            let mut reader = BufReader::new(
                File::open(&audio.path)
                    .map_err(|error| format!("Unable to read prepared audio input: {error}"))?,
            );
            let total_frames = total_bytes / bytes_per_frame as u64;
            let mut frame_offset = 0_u64;

            while frame_offset < total_frames {
                let frames_this_sample = (total_frames - frame_offset).min(AUDIO_CHUNK_FRAMES);
                let byte_count = usize::try_from(frames_this_sample)
                    .ok()
                    .and_then(|frames| frames.checked_mul(bytes_per_frame))
                    .ok_or_else(|| {
                        "Audio sample chunk is too large for Windows encoding.".to_owned()
                    })?;
                let mut f32_bytes = vec![0_u8; byte_count];
                reader
                    .read_exact(&mut f32_bytes)
                    .map_err(|error| format!("Unable to read prepared audio samples: {error}"))?;
                let pcm16 = f32le_to_pcm16(&f32_bytes)?;
                let timestamp_frame = i64::try_from(frame_offset)
                    .map_err(|_| "Audio timeline is too long for Windows encoding.".to_owned())?;
                let duration_frames = i64::try_from(frames_this_sample).map_err(|_| {
                    "Audio sample chunk is too long for Windows encoding.".to_owned()
                })?;
                let sample = sample_from_bytes(
                    &pcm16,
                    hns_from_frames(timestamp_frame, sample_rate)?,
                    hns_from_frames(duration_frames, sample_rate)?,
                )?;
                unsafe { self.writer.WriteSample(stream, &sample) }
                    .map_err(|error| windows_error("Unable to write AAC input sample", error))?;
                frame_offset += frames_this_sample;
            }

            Ok(())
        }

        fn write_sample_bytes(
            &self,
            stream: u32,
            bytes: &[u8],
            frame_index: i64,
            frame_rate: i64,
        ) -> Result<(), String> {
            let start = hns_from_frames(frame_index, frame_rate)?;
            let end = hns_from_frames(frame_index + 1, frame_rate)?;
            let sample = sample_from_bytes(bytes, start, end - start)?;
            unsafe { self.writer.WriteSample(stream, &sample) }
                .map_err(|error| windows_error("Unable to write H.264 input sample", error))
        }

        fn finalize(self) -> Result<(), String> {
            unsafe { self.writer.Finalize() }
                .map_err(|error| windows_error("Unable to finalize Media Foundation MP4", error))
        }
    }

    fn configure_audio_stream(
        writer: &IMFSinkWriter,
        audio: &EncodeAudioInput,
    ) -> Result<u32, String> {
        let output_type = create_audio_output_type(audio)?;
        let stream = unsafe { writer.AddStream(&output_type) }
            .map_err(|error| windows_error("Unable to add AAC audio stream", error))?;
        let input_type = create_audio_input_type(audio)?;
        unsafe { writer.SetInputMediaType(stream, &input_type, None::<&IMFAttributes>) }
            .map_err(|error| windows_error("Unable to set PCM audio input type", error))?;

        Ok(stream)
    }

    fn validate_encode_input(input: &EncodeInput) -> Result<(), String> {
        if input.audio_inputs.len() > 1 {
            return Err(
                "Native Windows encoder expects at most one prepared mixed audio input.".to_owned(),
            );
        }
        if input.video.format != EncodeVideoFormat::RawBgra {
            return Err("Native Windows encoder expects prepared raw BGRA video.".to_owned());
        }
        if input.video.width <= 0 || input.video.height <= 0 {
            return Err("Native Windows encoder received invalid video dimensions.".to_owned());
        }
        if input.video.frame_rate <= 0 || input.video.frame_count <= 0 {
            return Err("Native Windows encoder received invalid video timing.".to_owned());
        }

        if let Some(audio) = input.audio_inputs.first() {
            if audio.sample_format != "f32" {
                return Err(format!(
                    "Native Windows encoder expected prepared f32 audio, got {}.",
                    audio.sample_format
                ));
            }
            if audio.sample_rate <= 0 || audio.channels <= 0 {
                return Err(
                    "Native Windows encoder received invalid audio format metadata.".to_owned(),
                );
            }
        }

        Ok(())
    }

    fn create_writer_attributes() -> Result<IMFAttributes, String> {
        let mut attributes = None;
        unsafe { MFCreateAttributes(&mut attributes, 4) }.map_err(|error| {
            windows_error("Unable to create Media Foundation attributes", error)
        })?;
        let attributes = attributes
            .ok_or_else(|| "Media Foundation did not return writer attributes.".to_owned())?;
        unsafe { attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1) }
            .context("Unable to enable Media Foundation hardware transforms")?;
        unsafe { attributes.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1) }
            .context("Unable to disable Media Foundation sink writer throttling")?;
        unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1) }
            .context("Unable to enable Media Foundation low-latency mode")?;
        unsafe { attributes.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4) }
            .context("Unable to set Media Foundation MP4 container type")?;

        Ok(attributes)
    }

    fn create_video_output_type(
        width: i64,
        height: i64,
        frame_rate: i64,
    ) -> Result<IMFMediaType, String> {
        let media_type = create_media_type("video output")?;
        let width = u32_value(width, "video width")?;
        let height = u32_value(height, "video height")?;
        let frame_rate = u32_value(frame_rate, "video frame rate")?;
        unsafe { media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }
            .context("Unable to set video output major type")?;
        unsafe { media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264) }
            .context("Unable to set H.264 output subtype")?;
        unsafe {
            media_type.SetUINT32(&MF_MT_AVG_BITRATE, video_bitrate(width, height, frame_rate))
        }
        .context("Unable to set H.264 output bitrate")?;
        unsafe {
            media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
        }
        .context("Unable to set video output interlace mode")?;
        unsafe { media_type.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32_pair(width, height)) }
            .context("Unable to set video output frame size")?;
        unsafe { media_type.SetUINT64(&MF_MT_FRAME_RATE, pack_u32_pair(frame_rate, 1)) }
            .context("Unable to set video output frame rate")?;
        unsafe { media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u32_pair(1, 1)) }
            .context("Unable to set video output pixel aspect ratio")?;

        Ok(media_type)
    }

    fn create_video_input_type(
        width: i64,
        height: i64,
        frame_rate: i64,
        format: VideoInputFormat,
    ) -> Result<IMFMediaType, String> {
        let media_type = create_media_type("video input")?;
        let width = u32_value(width, "video width")?;
        let height = u32_value(height, "video height")?;
        let frame_rate = u32_value(frame_rate, "video frame rate")?;
        let subtype = match format {
            VideoInputFormat::Rgb32 => &MFVideoFormat_RGB32,
            VideoInputFormat::Nv12 => &MFVideoFormat_NV12,
        };
        unsafe { media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }
            .context("Unable to set video input major type")?;
        unsafe { media_type.SetGUID(&MF_MT_SUBTYPE, subtype) }
            .context("Unable to set raw video input subtype")?;
        unsafe {
            media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
        }
        .context("Unable to set video input interlace mode")?;
        unsafe { media_type.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32_pair(width, height)) }
            .context("Unable to set video input frame size")?;
        unsafe { media_type.SetUINT64(&MF_MT_FRAME_RATE, pack_u32_pair(frame_rate, 1)) }
            .context("Unable to set video input frame rate")?;
        unsafe { media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u32_pair(1, 1)) }
            .context("Unable to set video input pixel aspect ratio")?;

        Ok(media_type)
    }

    fn create_audio_output_type(audio: &EncodeAudioInput) -> Result<IMFMediaType, String> {
        let media_type = create_media_type("audio output")?;
        let sample_rate = u32_value(audio.sample_rate, "audio sample rate")?;
        let channels = u32_value(audio.channels, "audio channels")?;
        unsafe { media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio) }
            .context("Unable to set audio output major type")?;
        unsafe { media_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC) }
            .context("Unable to set AAC output subtype")?;
        unsafe { media_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate) }
            .context("Unable to set AAC output sample rate")?;
        unsafe { media_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels) }
            .context("Unable to set AAC output channel count")?;
        unsafe { media_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16) }
            .context("Unable to set AAC output bits per sample")?;
        unsafe { media_type.SetUINT32(&MF_MT_AVG_BITRATE, AUDIO_BITRATE) }
            .context("Unable to set AAC output bitrate")?;
        unsafe { media_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, AUDIO_BITRATE / 8) }
            .context("Unable to set AAC output byte rate")?;

        Ok(media_type)
    }

    fn create_audio_input_type(audio: &EncodeAudioInput) -> Result<IMFMediaType, String> {
        let media_type = create_media_type("audio input")?;
        let sample_rate = u32_value(audio.sample_rate, "audio sample rate")?;
        let channels = u32_value(audio.channels, "audio channels")?;
        let block_align = channels
            .checked_mul(PCM16_BYTES_PER_SAMPLE as u32)
            .ok_or_else(|| "Audio block alignment is too large for Windows encoding.".to_owned())?;
        unsafe { media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio) }
            .context("Unable to set audio input major type")?;
        unsafe { media_type.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM) }
            .context("Unable to set PCM input subtype")?;
        unsafe { media_type.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate) }
            .context("Unable to set PCM input sample rate")?;
        unsafe { media_type.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels) }
            .context("Unable to set PCM input channel count")?;
        unsafe { media_type.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16) }
            .context("Unable to set PCM input bits per sample")?;
        unsafe { media_type.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, block_align) }
            .context("Unable to set PCM input block alignment")?;
        let byte_rate = sample_rate
            .checked_mul(block_align)
            .ok_or_else(|| "Audio byte rate is too large for Windows encoding.".to_owned())?;
        unsafe { media_type.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, byte_rate) }
            .context("Unable to set PCM input byte rate")?;

        Ok(media_type)
    }

    fn create_media_type(label: &str) -> Result<IMFMediaType, String> {
        unsafe { MFCreateMediaType() }.map_err(|error| {
            windows_error(
                &format!("Unable to create Media Foundation {label} type"),
                error,
            )
        })
    }

    fn sample_from_bytes(
        bytes: &[u8],
        timestamp_hns: i64,
        duration_hns: i64,
    ) -> Result<IMFSample, String> {
        let length = u32::try_from(bytes.len())
            .map_err(|_| "Media Foundation sample is too large.".to_owned())?;
        let buffer = unsafe { MFCreateMemoryBuffer(length) }.map_err(|error| {
            windows_error("Unable to allocate Media Foundation sample buffer", error)
        })?;
        copy_to_media_buffer(&buffer, bytes)?;
        let sample = unsafe { MFCreateSample() }
            .map_err(|error| windows_error("Unable to create Media Foundation sample", error))?;
        unsafe { sample.AddBuffer(&buffer) }
            .context("Unable to attach Media Foundation sample buffer")?;
        unsafe { sample.SetSampleTime(timestamp_hns) }
            .context("Unable to set Media Foundation sample timestamp")?;
        unsafe { sample.SetSampleDuration(duration_hns) }
            .context("Unable to set Media Foundation sample duration")?;

        Ok(sample)
    }

    fn copy_to_media_buffer(buffer: &IMFMediaBuffer, bytes: &[u8]) -> Result<(), String> {
        let mut data = ptr::null_mut();
        unsafe { buffer.Lock(&mut data, None, None) }.map_err(|error| {
            windows_error("Unable to lock Media Foundation sample buffer", error)
        })?;
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
            buffer.Unlock().map_err(|error| {
                windows_error("Unable to unlock Media Foundation sample buffer", error)
            })?;
            buffer
                .SetCurrentLength(bytes.len() as u32)
                .map_err(|error| {
                    windows_error("Unable to set Media Foundation sample length", error)
                })?;
        }

        Ok(())
    }

    fn f32le_to_pcm16(bytes: &[u8]) -> Result<Vec<u8>, String> {
        if bytes.len() % std::mem::size_of::<f32>() != 0 {
            return Err("Prepared f32 audio bytes were not sample-aligned.".to_owned());
        }

        let mut pcm =
            Vec::with_capacity(bytes.len() / std::mem::size_of::<f32>() * PCM16_BYTES_PER_SAMPLE);
        for chunk in bytes.chunks_exact(std::mem::size_of::<f32>()) {
            let sample = f32::from_le_bytes(chunk.try_into().expect("chunk size"));
            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            let value = if sample <= -1.0 {
                i16::MIN
            } else if sample >= 1.0 {
                i16::MAX
            } else {
                (sample * i16::MAX as f32).round() as i16
            };
            pcm.extend_from_slice(&value.to_le_bytes());
        }

        Ok(pcm)
    }

    fn bgra_to_nv12(bgra: &[u8], width: i64, height: i64) -> Result<Vec<u8>, String> {
        validate_nv12_dimensions(width, height)?;
        let width = usize::try_from(width)
            .map_err(|_| "NV12 width is invalid for Windows encoding.".to_owned())?;
        let height = usize::try_from(height)
            .map_err(|_| "NV12 height is invalid for Windows encoding.".to_owned())?;
        let expected_bgra = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(BGRA_BYTES_PER_PIXEL))
            .ok_or_else(|| "BGRA video frame is too large for NV12 conversion.".to_owned())?;
        if bgra.len() != expected_bgra {
            return Err(format!(
                "BGRA video frame has {} bytes; expected {expected_bgra}.",
                bgra.len()
            ));
        }

        let y_plane_len = width
            .checked_mul(height)
            .ok_or_else(|| "NV12 frame is too large for Windows encoding.".to_owned())?;
        let uv_plane_len = y_plane_len / 2;
        let mut output = vec![0_u8; y_plane_len + uv_plane_len];

        for row in 0..height {
            for column in 0..width {
                let offset = (row * width + column) * BGRA_BYTES_PER_PIXEL;
                let b = i32::from(bgra[offset]);
                let g = i32::from(bgra[offset + 1]);
                let r = i32::from(bgra[offset + 2]);
                output[row * width + column] = y_from_rgb(r, g, b);
            }
        }

        let uv_base = y_plane_len;
        for row in (0..height).step_by(2) {
            for column in (0..width).step_by(2) {
                let mut r_sum = 0_i32;
                let mut g_sum = 0_i32;
                let mut b_sum = 0_i32;
                for y in 0..2 {
                    for x in 0..2 {
                        let offset = ((row + y) * width + column + x) * BGRA_BYTES_PER_PIXEL;
                        b_sum += i32::from(bgra[offset]);
                        g_sum += i32::from(bgra[offset + 1]);
                        r_sum += i32::from(bgra[offset + 2]);
                    }
                }
                let r = r_sum / 4;
                let g = g_sum / 4;
                let b = b_sum / 4;
                let uv_offset = uv_base + (row / 2) * width + column;
                output[uv_offset] = u_from_rgb(r, g, b);
                output[uv_offset + 1] = v_from_rgb(r, g, b);
            }
        }

        Ok(output)
    }

    fn y_from_rgb(r: i32, g: i32, b: i32) -> u8 {
        clamp_u8(((66 * r + 129 * g + 25 * b + 128) >> 8) + 16)
    }

    fn u_from_rgb(r: i32, g: i32, b: i32) -> u8 {
        clamp_u8(((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128)
    }

    fn v_from_rgb(r: i32, g: i32, b: i32) -> u8 {
        clamp_u8(((112 * r - 94 * g - 18 * b + 128) >> 8) + 128)
    }

    fn clamp_u8(value: i32) -> u8 {
        value.clamp(0, 255) as u8
    }

    fn validate_nv12_dimensions(width: i64, height: i64) -> Result<(), String> {
        if width <= 0 || height <= 0 || width % 2 != 0 || height % 2 != 0 {
            return Err("NV12 fallback requires positive even video dimensions.".to_owned());
        }
        Ok(())
    }

    fn bgra_frame_size(width: i64, height: i64) -> Result<usize, String> {
        let width = usize::try_from(width)
            .map_err(|_| "Video width is invalid for Windows encoding.".to_owned())?;
        let height = usize::try_from(height)
            .map_err(|_| "Video height is invalid for Windows encoding.".to_owned())?;
        width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(BGRA_BYTES_PER_PIXEL))
            .ok_or_else(|| "Raw video frame is too large for Windows encoding.".to_owned())
    }

    fn hns_from_frames(frame_count: i64, frame_rate: i64) -> Result<i64, String> {
        if frame_count < 0 || frame_rate <= 0 {
            return Err("Invalid Media Foundation timeline inputs.".to_owned());
        }
        let value = (i128::from(frame_count) * i128::from(HNS_PER_SECOND)
            + i128::from(frame_rate / 2))
            / i128::from(frame_rate);
        i64::try_from(value).map_err(|_| "Media Foundation timestamp is too large.".to_owned())
    }

    fn video_bitrate(width: u32, height: u32, frame_rate: u32) -> u32 {
        let pixels_per_second = u64::from(width) * u64::from(height) * u64::from(frame_rate);
        let target = (pixels_per_second / 12).clamp(2_000_000, 16_000_000);
        target as u32
    }

    fn u32_value(value: i64, label: &str) -> Result<u32, String> {
        u32::try_from(value).map_err(|_| format!("{label} is out of range for Windows encoding."))
    }

    fn pack_u32_pair(high: u32, low: u32) -> u64 {
        (u64::from(high) << 32) | u64::from(low)
    }

    fn checked_mul_u64(left: u64, right: u64) -> Option<u64> {
        left.checked_mul(right)
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
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

    fn windows_error(context: &str, error: WindowsError) -> String {
        format!("{context}: {error}")
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
        fn encodes_synthetic_video_only() {
            let root = test_root("video-only");
            let input = synthetic_input(&root, false).expect("synthetic input");

            let output = WindowsRecordingEncoder::new()
                .encode(input)
                .expect("encode video-only");

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
            let root = test_root("with-audio");
            let input = synthetic_input(&root, true).expect("synthetic input");

            let output = WindowsRecordingEncoder::new()
                .encode(input)
                .expect("encode with audio");

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

        #[test]
        fn converts_bgra_to_nv12_with_expected_layout() {
            let bgra = vec![
                0x00, 0x00, 0xff, 0xff, // red
                0x00, 0xff, 0x00, 0xff, // green
                0xff, 0x00, 0x00, 0xff, // blue
                0xff, 0xff, 0xff, 0xff, // white
            ];

            let nv12 = bgra_to_nv12(&bgra, 2, 2).expect("convert");

            assert_eq!(nv12.len(), 6);
            assert_eq!(&nv12[0..4], &[82, 144, 41, 235]);
            assert_eq!(nv12[4], 128);
            assert_eq!(nv12[5], 128);
        }

        fn test_root(label: &str) -> PathBuf {
            let root = std::env::temp_dir().join(format!(
                "metafy-windows-native-encoder-{label}-{}",
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
                        sample_format: "f32".to_owned(),
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

#[cfg(target_os = "windows")]
pub use platform::WindowsRecordingEncoder;

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub struct WindowsRecordingEncoder;

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
impl WindowsRecordingEncoder {
    pub fn new() -> Result<Self, String> {
        Err("Native Windows encoder is only available on Windows.".to_owned())
    }
}
