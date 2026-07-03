#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::{CStr, CString};
    use std::fs;
    use std::io;
    use std::os::raw::{c_char, c_int};
    use std::path::Path;
    use std::ptr;

    use crate::media::encode::{
        EncodeDiagnostics, EncodeInput, EncodeOutput, EncodeVideoFormat, RecordingEncoder,
    };
    use crate::media::metadata::{derive_media_info, MediaInfoSource};
    use crate::media::thumbnail::write_bgra_thumbnail;

    const BACKEND_NAME: &str = "macos-avfoundation";
    const BACKEND_LABEL: &str = "native-macos-avfoundation";
    const NATIVE_SUCCESS_MESSAGE: &str = "Encoded MP4 with AVFoundation H.264/AAC.";

    unsafe extern "C" {
        fn metafy_macos_encode_mp4(
            video_path: *const c_char,
            audio_path: *const c_char,
            output_path: *const c_char,
            width: i64,
            height: i64,
            frame_rate: i64,
            frame_count: i64,
            audio_sample_rate: i64,
            audio_channels: i64,
            error_out: *mut *mut c_char,
        ) -> c_int;
        fn metafy_macos_mux_chunked_mp4(
            manifest_path: *const c_char,
            audio_path: *const c_char,
            output_path: *const c_char,
            audio_sample_rate: i64,
            audio_channels: i64,
            error_out: *mut *mut c_char,
        ) -> c_int;
        fn metafy_macos_free_string(value: *mut c_char);
    }

    pub struct MacosRecordingEncoder;

    impl MacosRecordingEncoder {
        pub fn new() -> Self {
            Self
        }

        pub fn backend_label(&self) -> String {
            BACKEND_LABEL.to_owned()
        }
    }

    impl RecordingEncoder for MacosRecordingEncoder {
        fn encode(&self, input: EncodeInput) -> Result<EncodeOutput, String> {
            if input.audio_inputs.len() > 1 {
                return Err(
                    "Native macOS encoder expects at most one prepared mixed audio input."
                        .to_owned(),
                );
            }

            let video_path = path_c_string(&input.video.path)?;
            let output_path = path_c_string(&input.output.staging_media_path)?;
            let audio_input = input.audio_inputs.first();
            let audio_path = audio_input
                .map(|audio| {
                    if audio.sample_format != "f32" {
                        return Err(format!(
                            "Native macOS encoder expected prepared f32 audio, got {}.",
                            audio.sample_format
                        ));
                    }
                    if audio.sample_rate <= 0 || audio.channels <= 0 {
                        return Err(
                            "Native macOS encoder received invalid audio format metadata."
                                .to_owned(),
                        );
                    }
                    path_c_string(&audio.path)
                })
                .transpose()?;
            let audio_path_ptr = audio_path
                .as_ref()
                .map_or(ptr::null(), |path| path.as_ptr());
            let audio_sample_rate = audio_input.map_or(0, |audio| audio.sample_rate);
            let audio_channels = audio_input.map_or(0, |audio| audio.channels);

            let mut native_error: *mut c_char = ptr::null_mut();
            let status = unsafe {
                match input.video.format {
                    EncodeVideoFormat::RawBgra => metafy_macos_encode_mp4(
                        video_path.as_ptr(),
                        audio_path_ptr,
                        output_path.as_ptr(),
                        input.video.width,
                        input.video.height,
                        input.video.frame_rate,
                        input.video.frame_count,
                        audio_sample_rate,
                        audio_channels,
                        &mut native_error,
                    ),
                    EncodeVideoFormat::ChunkedH264Segments => metafy_macos_mux_chunked_mp4(
                        video_path.as_ptr(),
                        audio_path_ptr,
                        output_path.as_ptr(),
                        audio_sample_rate,
                        audio_channels,
                        &mut native_error,
                    ),
                }
            };

            if status != 0 {
                return Err(format!(
                    "Native macOS encode failed: {}",
                    take_native_error(native_error)
                ));
            }

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

            let messages = vec![match input.video.format {
                EncodeVideoFormat::RawBgra => NATIVE_SUCCESS_MESSAGE.to_owned(),
                EncodeVideoFormat::ChunkedH264Segments => {
                    "Muxed chunked H.264 recording with AVFoundation/AAC.".to_owned()
                }
            }];
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

    fn path_c_string(path: &Path) -> Result<CString, String> {
        CString::new(path_to_string(path))
            .map_err(|_| format!("Path contains an unsupported NUL byte: {}", path.display()))
    }

    fn take_native_error(error: *mut c_char) -> String {
        if error.is_null() {
            return "native encoder returned no error detail".to_owned();
        }

        let message = unsafe { CStr::from_ptr(error).to_string_lossy().into_owned() };
        unsafe {
            metafy_macos_free_string(error);
        }
        message
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
        use crate::media::chunked_video::{read_manifest, read_thumbnail_frame};
        use crate::media::encode::{
            EncodeAudioInput, EncodeOutputPaths, EncodeVideoFormat, EncodeVideoInput,
            RecordingEncoder,
        };
        use crate::media::macos_chunked_video::MacosSegmentedVideoWriter;
        use crate::media::thumbnail::BgraFrame;
        use std::fs::File;
        use std::io::Write;
        use std::path::{Path, PathBuf};
        use uuid::Uuid;

        #[test]
        fn encodes_synthetic_video_only() {
            let root = test_root("video-only");
            let input = synthetic_input(&root, false).expect("synthetic input");

            let output = MacosRecordingEncoder::new()
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

            let output = MacosRecordingEncoder::new()
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
        fn encodes_synthetic_long_audio() {
            let root = test_root("long-audio");
            let input = synthetic_input_with_options(&root, true, 160, 90, 334, 556_800)
                .expect("synthetic input");

            let output = MacosRecordingEncoder::new()
                .encode(input)
                .expect("encode long audio");

            assert!(output.audio_included);
            assert_eq!(output.frame_count, 334);
            assert_eq!(output.duration_ms, Some(11_600));

            let _ = fs::remove_dir_all(root);
        }

        #[test]
        fn muxes_synthetic_chunked_video() {
            let root = test_root("chunked-video");
            let manifest_path = root.join("screen_video.mfcv");
            let thumbnail_path = root.join("screen_video-thumbnail.bgra");
            let width = 2;
            let height = 2;
            let frame_rate = 30;
            let frame_count = 3;
            let first_frame = synthetic_frame_bytes(0, width, height);
            fs::write(&thumbnail_path, &first_frame).expect("write thumbnail frame");
            let mut chunk_writer =
                MacosSegmentedVideoWriter::create(&manifest_path, width, height, frame_rate, 2)
                    .expect("create chunked writer");
            for frame_index in 0..frame_count {
                chunk_writer
                    .append_frame(
                        &synthetic_frame_bytes(frame_index, width, height),
                        frame_index * 33,
                        frame_index * 33,
                    )
                    .expect("append frame");
            }
            chunk_writer.finish().expect("finish chunks");

            let manifest = read_manifest(&manifest_path).expect("read manifest");
            assert_eq!(manifest.frame_count, frame_count as u64);
            assert_eq!(manifest.chunks.len(), 2);
            let thumbnail_frame =
                read_thumbnail_frame(&manifest_path, &manifest).expect("read thumbnail");
            let input = EncodeInput {
                recording_id: "synthetic-chunked".to_owned(),
                video: EncodeVideoInput {
                    path: manifest_path,
                    format: EncodeVideoFormat::ChunkedH264Segments,
                    width,
                    height,
                    frame_rate,
                    frame_count,
                    thumbnail_frame,
                },
                audio_inputs: Vec::new(),
                output: EncodeOutputPaths {
                    media_path: root.join("recording.mp4"),
                    media_path_relative: "recording.mp4".to_owned(),
                    thumbnail_path: root.join("thumbnail.jpg"),
                    thumbnail_path_relative: "thumbnail.jpg".to_owned(),
                    staging_media_path: root.join("recording.tmp.mp4"),
                    staging_thumbnail_path: root.join("thumbnail.tmp.jpg"),
                },
                duration_hint_ms: Some(100),
                warnings: Vec::new(),
            };

            let output = MacosRecordingEncoder::new()
                .encode(input)
                .expect("mux chunked video");

            assert_eq!(output.frame_count, frame_count);
            assert!(Path::new(&output.absolute_media_path).is_file());
            assert!(output
                .diagnostics
                .messages
                .iter()
                .any(|message| message.contains("chunked H.264")));

            let _ = fs::remove_dir_all(root);
        }

        fn test_root(label: &str) -> PathBuf {
            let root = std::env::temp_dir().join(format!(
                "metafy-macos-native-encoder-{label}-{}",
                Uuid::new_v4()
            ));
            fs::create_dir_all(&root).expect("create test root");
            root
        }

        fn synthetic_input(root: &Path, include_audio: bool) -> Result<EncodeInput, String> {
            synthetic_input_with_options(root, include_audio, 2, 2, 3, 4_800)
        }

        fn synthetic_input_with_options(
            root: &Path,
            include_audio: bool,
            width: i64,
            height: i64,
            frame_count: i64,
            audio_frame_count: usize,
        ) -> Result<EncodeInput, String> {
            let frame_rate = 30;
            let video_path = root.join("video.bgra");
            let audio_path = root.join("audio.raw");
            let media_path = root.join("recording.mp4");
            let thumbnail_path = root.join("thumbnail.jpg");
            let staging_media_path = root.join("recording.tmp.mp4");
            let staging_thumbnail_path = root.join("thumbnail.tmp.jpg");

            write_synthetic_video(&video_path, frame_count, width, height)?;
            if include_audio {
                write_synthetic_audio(&audio_path, audio_frame_count)?;
            }
            let audio_duration_ms = (audio_frame_count as i64 * 1_000) / 48_000;

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
                        duration_ms: Some(audio_duration_ms),
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

#[cfg(target_os = "macos")]
pub use platform::MacosRecordingEncoder;

#[cfg(not(target_os = "macos"))]
pub struct MacosRecordingEncoder;

#[cfg(not(target_os = "macos"))]
impl MacosRecordingEncoder {
    pub fn new() -> Result<Self, String> {
        Err("Native macOS encoder is only available on macOS.".to_owned())
    }
}
