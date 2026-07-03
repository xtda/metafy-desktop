use crate::media::audio::is_supported_pcm_sample_format;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

pub const VIDEO_FILE_MAGIC: &[u8] = b"METAFY_RAW_VIDEO_V1\n";
pub const AUDIO_FILE_MAGIC: &[u8] = b"METAFY_RAW_AUDIO_V1\n";
pub const BGRA_FORMAT_CODE: u32 = 7;
pub const BGRA_LZ4_FORMAT_CODE: u32 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawVideoFrame {
    pub elapsed_ms: u64,
    pub display_time_ms: u64,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct RawVideoReader {
    path: PathBuf,
    reader: BufReader<File>,
    width: Option<u32>,
    height: Option<u32>,
}

impl RawVideoReader {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).map_err(|error| {
            format!(
                "Unable to open raw video sidecar {}: {error}",
                path.display()
            )
        })?;
        let mut reader = BufReader::new(file);
        expect_magic(&mut reader, VIDEO_FILE_MAGIC, "raw video", &path)?;

        Ok(Self {
            path,
            reader,
            width: None,
            height: None,
        })
    }

    pub fn next_frame(&mut self) -> Result<Option<RawVideoFrame>, String> {
        let Some(elapsed_ms) = read_u64_optional(
            &mut self.reader,
            &self.path,
            "raw video frame elapsed timestamp",
        )?
        else {
            return Ok(None);
        };
        let display_time_ms = read_u64(
            &mut self.reader,
            &self.path,
            "raw video frame display timestamp",
        )?;
        let format_code = read_u32(&mut self.reader, &self.path, "raw video frame format")?;
        let width = read_u32(&mut self.reader, &self.path, "raw video frame width")?;
        let height = read_u32(&mut self.reader, &self.path, "raw video frame height")?;
        let byte_count =
            read_u32(&mut self.reader, &self.path, "raw video frame byte count")? as usize;

        if width == 0 || height == 0 {
            return Err(format!(
                "Raw video sidecar {} has invalid frame dimensions {width}x{height}.",
                self.path.display()
            ));
        }

        match (self.width, self.height) {
            (Some(expected_width), Some(expected_height))
                if width != expected_width || height != expected_height =>
            {
                return Err(format!(
                    "Raw video sidecar {} changed frame dimensions from {expected_width}x{expected_height} to {width}x{height}.",
                    self.path.display()
                ));
            }
            (None, None) => {
                self.width = Some(width);
                self.height = Some(height);
            }
            (Some(_), Some(_)) => {}
            _ => unreachable!("raw video sidecar dimensions are updated together"),
        }

        let expected_byte_count = frame_byte_count(width, height, &self.path)?;
        let mut encoded_bytes = vec![0_u8; byte_count];
        self.reader
            .read_exact(&mut encoded_bytes)
            .map_err(|error| {
                format!(
                    "Unable to read raw video frame bytes from {}: {error}",
                    self.path.display()
                )
            })?;
        let bytes = match format_code {
            BGRA_FORMAT_CODE => {
                if byte_count != expected_byte_count {
                    return Err(format!(
                        "Raw video sidecar {} frame has {byte_count} bytes; expected {expected_byte_count} for {width}x{height} BGRA.",
                        self.path.display()
                    ));
                }
                encoded_bytes
            }
            BGRA_LZ4_FORMAT_CODE => decompress_lz4_bgra_frame(
                &encoded_bytes,
                expected_byte_count,
                width,
                height,
                &self.path,
            )?,
            _ => {
                return Err(format!(
                    "Raw video sidecar {} has unsupported frame format {format_code}; expected BGRA.",
                    self.path.display()
                ));
            }
        };

        Ok(Some(RawVideoFrame {
            elapsed_ms,
            display_time_ms,
            width,
            height,
            bytes,
        }))
    }
}

fn decompress_lz4_bgra_frame(
    bytes: &[u8],
    expected_byte_count: usize,
    width: u32,
    height: u32,
    path: &Path,
) -> Result<Vec<u8>, String> {
    if bytes.len() < std::mem::size_of::<u32>() {
        return Err(format!(
            "Raw video sidecar {} compressed frame is missing its decoded size prefix.",
            path.display()
        ));
    }
    let decoded_byte_count = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if decoded_byte_count != expected_byte_count {
        return Err(format!(
            "Raw video sidecar {} compressed frame decodes to {decoded_byte_count} bytes; expected {expected_byte_count} for {width}x{height} BGRA.",
            path.display()
        ));
    }

    let decoded = lz4_flex::decompress_size_prepended(bytes).map_err(|error| {
        format!(
            "Unable to decompress raw video frame from {}: {error}",
            path.display()
        )
    })?;
    if decoded.len() != expected_byte_count {
        return Err(format!(
            "Raw video sidecar {} decompressed frame has {} bytes; expected {expected_byte_count} for {width}x{height} BGRA.",
            path.display(),
            decoded.len()
        ));
    }

    Ok(decoded)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawAudioFormat {
    pub sample_rate: i64,
    pub channels: i64,
    pub sample_format: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawAudioMetadataError {
    MissingSampleRate,
    MissingChannels,
    MissingSampleFormat,
    UnsupportedSampleFormat,
}

impl RawAudioFormat {
    pub fn from_session_metadata(
        sample_rate: Option<i64>,
        channels: Option<i64>,
        sample_format: Option<String>,
    ) -> Result<Self, RawAudioMetadataError> {
        let sample_rate = sample_rate
            .filter(|value| *value > 0)
            .ok_or(RawAudioMetadataError::MissingSampleRate)?;
        let channels = channels
            .filter(|value| *value > 0)
            .ok_or(RawAudioMetadataError::MissingChannels)?;
        let sample_format = sample_format.ok_or(RawAudioMetadataError::MissingSampleFormat)?;

        if !is_supported_pcm_sample_format(&sample_format) {
            return Err(RawAudioMetadataError::UnsupportedSampleFormat);
        }

        Ok(Self {
            sample_rate,
            channels,
            sample_format,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawAudioBlock {
    pub elapsed_ms: u64,
    pub callback_stream_ns: u64,
    pub capture_stream_ns: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct RawAudioReader {
    path: PathBuf,
    reader: BufReader<File>,
    format: RawAudioFormat,
}

impl RawAudioReader {
    pub fn open(path: impl AsRef<Path>, format: RawAudioFormat) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).map_err(|error| {
            format!(
                "Unable to open raw audio sidecar {}: {error}",
                path.display()
            )
        })?;
        let mut reader = BufReader::new(file);
        expect_magic(&mut reader, AUDIO_FILE_MAGIC, "raw audio", &path)?;

        Ok(Self {
            path,
            reader,
            format,
        })
    }

    pub fn format(&self) -> &RawAudioFormat {
        &self.format
    }

    pub fn next_block(&mut self) -> Result<Option<RawAudioBlock>, String> {
        let Some(elapsed_ms) = read_u64_optional(
            &mut self.reader,
            &self.path,
            "raw audio block elapsed timestamp",
        )?
        else {
            return Ok(None);
        };
        let callback_stream_ns = read_u64(
            &mut self.reader,
            &self.path,
            "raw audio block callback stream timestamp",
        )?;
        let capture_stream_ns = read_u64(
            &mut self.reader,
            &self.path,
            "raw audio block capture stream timestamp",
        )?;
        let byte_count =
            read_u32(&mut self.reader, &self.path, "raw audio block byte count")? as usize;
        let mut bytes = vec![0_u8; byte_count];
        self.reader.read_exact(&mut bytes).map_err(|error| {
            format!(
                "Unable to read raw audio block bytes from {}: {error}",
                self.path.display()
            )
        })?;

        Ok(Some(RawAudioBlock {
            elapsed_ms,
            callback_stream_ns,
            capture_stream_ns,
            bytes,
        }))
    }
}

fn expect_magic(
    reader: &mut BufReader<File>,
    magic: &[u8],
    label: &str,
    path: &Path,
) -> Result<(), String> {
    let mut actual = vec![0_u8; magic.len()];
    reader.read_exact(&mut actual).map_err(|error| {
        format!(
            "Unable to read {label} sidecar header from {}: {error}",
            path.display()
        )
    })?;
    if actual != magic {
        return Err(format!(
            "Captured {label} sidecar {} has an unsupported format.",
            path.display()
        ));
    }
    Ok(())
}

fn read_u64_optional(
    reader: &mut BufReader<File>,
    path: &Path,
    label: &str,
) -> Result<Option<u64>, String> {
    let mut first = [0_u8; 1];
    match reader
        .read(&mut first)
        .map_err(|error| format!("Unable to read {label} from {}: {error}", path.display()))?
    {
        0 => Ok(None),
        1 => {
            let mut bytes = [0_u8; 8];
            bytes[0] = first[0];
            reader.read_exact(&mut bytes[1..]).map_err(|error| {
                format!("Unable to read {label} from {}: {error}", path.display())
            })?;
            Ok(Some(u64::from_le_bytes(bytes)))
        }
        _ => unreachable!("single-byte read returned more than one byte"),
    }
}

fn read_u64(reader: &mut BufReader<File>, path: &Path, label: &str) -> Result<u64, String> {
    let mut bytes = [0_u8; 8];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| format!("Unable to read {label} from {}: {error}", path.display()))?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_u32(reader: &mut BufReader<File>, path: &Path, label: &str) -> Result<u32, String> {
    let mut bytes = [0_u8; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| format!("Unable to read {label} from {}: {error}", path.display()))?;
    Ok(u32::from_le_bytes(bytes))
}

fn frame_byte_count(width: u32, height: u32, path: &Path) -> Result<usize, String> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            format!(
                "Raw video sidecar {} frame dimensions are too large to encode.",
                path.display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{self, BufWriter, Write};
    use uuid::Uuid;

    #[test]
    fn video_reader_streams_valid_frames() {
        let root = test_directory("video-valid");
        fs::create_dir_all(&root).expect("create test root");
        let path = root.join("screen.raw");
        write_video_sidecar(
            &path,
            &[
                TestVideoFrame {
                    elapsed_ms: 0,
                    display_time_ms: 10,
                    format_code: BGRA_FORMAT_CODE,
                    width: 2,
                    height: 1,
                    bytes: vec![1, 2, 3, 4, 5, 6, 7, 8],
                },
                TestVideoFrame {
                    elapsed_ms: 33,
                    display_time_ms: 43,
                    format_code: BGRA_FORMAT_CODE,
                    width: 2,
                    height: 1,
                    bytes: vec![9, 10, 11, 12, 13, 14, 15, 16],
                },
            ],
        )
        .expect("write video sidecar");

        let mut reader = RawVideoReader::open(&path).expect("open video sidecar");
        let first = reader
            .next_frame()
            .expect("read first frame")
            .expect("frame");
        let second = reader
            .next_frame()
            .expect("read second frame")
            .expect("frame");

        assert_eq!(first.elapsed_ms, 0);
        assert_eq!(first.display_time_ms, 10);
        assert_eq!(first.width, 2);
        assert_eq!(first.height, 1);
        assert_eq!(first.bytes, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(second.elapsed_ms, 33);
        assert_eq!(second.display_time_ms, 43);
        assert_eq!(second.bytes, vec![9, 10, 11, 12, 13, 14, 15, 16]);
        assert!(reader.next_frame().expect("end of video").is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn video_reader_decompresses_lz4_bgra_frames() {
        let root = test_directory("video-lz4-valid");
        fs::create_dir_all(&root).expect("create test root");
        let path = root.join("screen.raw");
        let bytes = vec![0x20; 4 * 4 * 4];
        let compressed = lz4_flex::compress_prepend_size(&bytes);

        write_video_sidecar(
            &path,
            &[TestVideoFrame {
                elapsed_ms: 0,
                display_time_ms: 10,
                format_code: BGRA_LZ4_FORMAT_CODE,
                width: 4,
                height: 4,
                bytes: compressed,
            }],
        )
        .expect("write video sidecar");

        let mut reader = RawVideoReader::open(&path).expect("open video sidecar");
        let frame = reader.next_frame().expect("read frame").expect("frame");

        assert_eq!(frame.width, 4);
        assert_eq!(frame.height, 4);
        assert_eq!(frame.bytes, bytes);
        assert!(reader.next_frame().expect("end of video").is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn video_reader_rejects_empty_truncated_malformed_and_unsupported_sidecars() {
        let root = test_directory("video-invalid");
        fs::create_dir_all(&root).expect("create test root");

        let empty_path = root.join("empty.raw");
        File::create(&empty_path).expect("create empty sidecar");
        assert_error_contains(
            RawVideoReader::open(&empty_path),
            &empty_path,
            "Unable to read raw video sidecar header",
        );

        let malformed_path = root.join("malformed.raw");
        fs::write(&malformed_path, b"not a metafy raw video").expect("write malformed sidecar");
        assert_error_contains(
            RawVideoReader::open(&malformed_path),
            &malformed_path,
            "unsupported format",
        );

        let truncated_path = root.join("truncated.raw");
        fs::write(&truncated_path, [VIDEO_FILE_MAGIC, &[0_u8, 1_u8]].concat())
            .expect("write truncated sidecar");
        let mut reader = RawVideoReader::open(&truncated_path).expect("open truncated sidecar");
        assert_error_contains(
            reader.next_frame().map(|_| ()),
            &truncated_path,
            "raw video frame elapsed timestamp",
        );

        let unsupported_path = root.join("unsupported.raw");
        write_video_sidecar(
            &unsupported_path,
            &[TestVideoFrame {
                elapsed_ms: 0,
                display_time_ms: 0,
                format_code: 99,
                width: 1,
                height: 1,
                bytes: vec![0, 0, 0, 0],
            }],
        )
        .expect("write unsupported sidecar");
        let mut reader = RawVideoReader::open(&unsupported_path).expect("open unsupported sidecar");
        assert_error_contains(
            reader.next_frame().map(|_| ()),
            &unsupported_path,
            "unsupported frame format 99",
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn video_reader_rejects_dimension_changes_and_byte_mismatches() {
        let root = test_directory("video-metadata-invalid");
        fs::create_dir_all(&root).expect("create test root");

        let dimension_path = root.join("dimensions.raw");
        write_video_sidecar(
            &dimension_path,
            &[
                TestVideoFrame {
                    elapsed_ms: 0,
                    display_time_ms: 0,
                    format_code: BGRA_FORMAT_CODE,
                    width: 1,
                    height: 1,
                    bytes: vec![0, 0, 0, 0],
                },
                TestVideoFrame {
                    elapsed_ms: 33,
                    display_time_ms: 33,
                    format_code: BGRA_FORMAT_CODE,
                    width: 2,
                    height: 1,
                    bytes: vec![0, 0, 0, 0, 0, 0, 0, 0],
                },
            ],
        )
        .expect("write dimension sidecar");
        let mut reader = RawVideoReader::open(&dimension_path).expect("open dimension sidecar");
        assert!(reader.next_frame().expect("first frame").is_some());
        assert_error_contains(
            reader.next_frame().map(|_| ()),
            &dimension_path,
            "changed frame dimensions",
        );

        let byte_count_path = root.join("byte-count.raw");
        write_video_sidecar(
            &byte_count_path,
            &[TestVideoFrame {
                elapsed_ms: 0,
                display_time_ms: 0,
                format_code: BGRA_FORMAT_CODE,
                width: 2,
                height: 1,
                bytes: vec![0, 0, 0, 0],
            }],
        )
        .expect("write byte count sidecar");
        let mut reader = RawVideoReader::open(&byte_count_path).expect("open byte count sidecar");
        assert_error_contains(
            reader.next_frame().map(|_| ()),
            &byte_count_path,
            "expected 8",
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn audio_reader_streams_blocks_and_handles_empty_sidecars() {
        let root = test_directory("audio-valid");
        fs::create_dir_all(&root).expect("create test root");
        let format = audio_format();

        let empty_path = root.join("empty-audio.raw");
        write_audio_sidecar(&empty_path, &[]).expect("write empty audio sidecar");
        let mut empty_reader =
            RawAudioReader::open(&empty_path, format.clone()).expect("open empty audio sidecar");
        assert!(empty_reader
            .next_block()
            .expect("empty sidecar end")
            .is_none());

        let path = root.join("audio.raw");
        write_audio_sidecar(
            &path,
            &[
                TestAudioBlock {
                    elapsed_ms: 0,
                    callback_stream_ns: 100,
                    capture_stream_ns: 200,
                    bytes: vec![1, 2, 3, 4],
                },
                TestAudioBlock {
                    elapsed_ms: 20,
                    callback_stream_ns: 120,
                    capture_stream_ns: 220,
                    bytes: vec![5, 6],
                },
            ],
        )
        .expect("write audio sidecar");
        let mut reader = RawAudioReader::open(&path, format).expect("open audio sidecar");
        let first = reader
            .next_block()
            .expect("read first block")
            .expect("block");
        let second = reader
            .next_block()
            .expect("read second block")
            .expect("block");

        assert_eq!(reader.format().sample_rate, 48_000);
        assert_eq!(reader.format().channels, 2);
        assert_eq!(reader.format().sample_format, "f32");
        assert_eq!(first.elapsed_ms, 0);
        assert_eq!(first.callback_stream_ns, 100);
        assert_eq!(first.capture_stream_ns, 200);
        assert_eq!(first.bytes, vec![1, 2, 3, 4]);
        assert_eq!(second.elapsed_ms, 20);
        assert_eq!(second.bytes, vec![5, 6]);
        assert!(reader.next_block().expect("end of audio").is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn audio_reader_reports_missing_empty_truncated_and_malformed_sidecars() {
        let root = test_directory("audio-invalid");
        fs::create_dir_all(&root).expect("create test root");
        let format = audio_format();

        let missing_path = root.join("missing.raw");
        assert_error_contains(
            RawAudioReader::open(&missing_path, format.clone()),
            &missing_path,
            "Unable to open raw audio sidecar",
        );

        let empty_path = root.join("empty.raw");
        File::create(&empty_path).expect("create empty sidecar");
        assert_error_contains(
            RawAudioReader::open(&empty_path, format.clone()),
            &empty_path,
            "Unable to read raw audio sidecar header",
        );

        let malformed_path = root.join("malformed.raw");
        fs::write(&malformed_path, b"not a metafy raw audio").expect("write malformed sidecar");
        assert_error_contains(
            RawAudioReader::open(&malformed_path, format.clone()),
            &malformed_path,
            "unsupported format",
        );

        let truncated_path = root.join("truncated.raw");
        fs::write(&truncated_path, [AUDIO_FILE_MAGIC, &[0_u8, 1_u8]].concat())
            .expect("write truncated sidecar");
        let mut reader =
            RawAudioReader::open(&truncated_path, format.clone()).expect("open truncated sidecar");
        assert_error_contains(
            reader.next_block().map(|_| ()),
            &truncated_path,
            "raw audio block elapsed timestamp",
        );

        let short_payload_path = root.join("short-payload.raw");
        let mut writer =
            BufWriter::new(File::create(&short_payload_path).expect("create short payload"));
        writer.write_all(AUDIO_FILE_MAGIC).expect("write magic");
        writer.write_all(&0_u64.to_le_bytes()).expect("elapsed");
        writer.write_all(&0_u64.to_le_bytes()).expect("callback");
        writer.write_all(&0_u64.to_le_bytes()).expect("capture");
        writer.write_all(&4_u32.to_le_bytes()).expect("byte count");
        writer.write_all(&[1, 2]).expect("short payload");
        writer.flush().expect("flush short payload");
        let mut reader =
            RawAudioReader::open(&short_payload_path, format).expect("open short payload sidecar");
        assert_error_contains(
            reader.next_block().map(|_| ()),
            &short_payload_path,
            "raw audio block bytes",
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn audio_metadata_validation_covers_session_fields() {
        assert_eq!(
            RawAudioFormat::from_session_metadata(None, Some(2), Some("f32".to_owned())),
            Err(RawAudioMetadataError::MissingSampleRate)
        );
        assert_eq!(
            RawAudioFormat::from_session_metadata(Some(0), Some(2), Some("f32".to_owned())),
            Err(RawAudioMetadataError::MissingSampleRate)
        );
        assert_eq!(
            RawAudioFormat::from_session_metadata(Some(48_000), None, Some("f32".to_owned())),
            Err(RawAudioMetadataError::MissingChannels)
        );
        assert_eq!(
            RawAudioFormat::from_session_metadata(Some(48_000), Some(2), None),
            Err(RawAudioMetadataError::MissingSampleFormat)
        );
        assert_eq!(
            RawAudioFormat::from_session_metadata(Some(48_000), Some(2), Some("pcm24".to_owned())),
            Err(RawAudioMetadataError::UnsupportedSampleFormat)
        );
        assert_eq!(
            RawAudioFormat::from_session_metadata(Some(48_000), Some(2), Some("f32".to_owned()))
                .expect("valid format"),
            audio_format()
        );
    }

    #[test]
    fn audio_reader_can_enumerate_recording_mode_sidecars() {
        let root = test_directory("audio-modes");
        fs::create_dir_all(&root).expect("create test root");
        let microphone_path = root.join("microphone.raw");
        let source_path = root.join("source.raw");
        write_audio_sidecar(
            &microphone_path,
            &[TestAudioBlock {
                elapsed_ms: 0,
                callback_stream_ns: 0,
                capture_stream_ns: 0,
                bytes: vec![1, 2, 3, 4],
            }],
        )
        .expect("write microphone sidecar");
        write_audio_sidecar(
            &source_path,
            &[TestAudioBlock {
                elapsed_ms: 0,
                callback_stream_ns: 0,
                capture_stream_ns: 0,
                bytes: vec![5, 6, 7, 8],
            }],
        )
        .expect("write source sidecar");

        let cases: &[(&str, &[&Path])] = &[
            ("video-only", &[]),
            ("microphone-only", &[microphone_path.as_path()]),
            ("source-only", &[source_path.as_path()]),
            (
                "microphone-and-source",
                &[microphone_path.as_path(), source_path.as_path()],
            ),
        ];

        for (label, sidecars) in cases {
            let mut blocks = 0;
            for sidecar in *sidecars {
                let mut reader =
                    RawAudioReader::open(sidecar, audio_format()).unwrap_or_else(|error| {
                        panic!("open {label} sidecar {}: {error}", sidecar.display())
                    });
                while reader
                    .next_block()
                    .unwrap_or_else(|error| {
                        panic!("read {label} sidecar {}: {error}", sidecar.display())
                    })
                    .is_some()
                {
                    blocks += 1;
                }
            }
            assert_eq!(blocks, sidecars.len(), "{label}");
        }

        let _ = fs::remove_dir_all(root);
    }

    #[derive(Debug)]
    struct TestVideoFrame {
        elapsed_ms: u64,
        display_time_ms: u64,
        format_code: u32,
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    }

    #[derive(Debug)]
    struct TestAudioBlock {
        elapsed_ms: u64,
        callback_stream_ns: u64,
        capture_stream_ns: u64,
        bytes: Vec<u8>,
    }

    fn write_video_sidecar(path: &Path, frames: &[TestVideoFrame]) -> io::Result<()> {
        let mut writer = BufWriter::new(File::create(path)?);
        writer.write_all(VIDEO_FILE_MAGIC)?;
        for frame in frames {
            writer.write_all(&frame.elapsed_ms.to_le_bytes())?;
            writer.write_all(&frame.display_time_ms.to_le_bytes())?;
            writer.write_all(&frame.format_code.to_le_bytes())?;
            writer.write_all(&frame.width.to_le_bytes())?;
            writer.write_all(&frame.height.to_le_bytes())?;
            writer.write_all(&(frame.bytes.len() as u32).to_le_bytes())?;
            writer.write_all(&frame.bytes)?;
        }
        writer.flush()
    }

    fn write_audio_sidecar(path: &Path, blocks: &[TestAudioBlock]) -> io::Result<()> {
        let mut writer = BufWriter::new(File::create(path)?);
        writer.write_all(AUDIO_FILE_MAGIC)?;
        for block in blocks {
            writer.write_all(&block.elapsed_ms.to_le_bytes())?;
            writer.write_all(&block.callback_stream_ns.to_le_bytes())?;
            writer.write_all(&block.capture_stream_ns.to_le_bytes())?;
            writer.write_all(&(block.bytes.len() as u32).to_le_bytes())?;
            writer.write_all(&block.bytes)?;
        }
        writer.flush()
    }

    fn audio_format() -> RawAudioFormat {
        RawAudioFormat {
            sample_rate: 48_000,
            channels: 2,
            sample_format: "f32".to_owned(),
        }
    }

    fn assert_error_contains<T>(result: Result<T, String>, path: &Path, expected: &str) {
        let error = match result {
            Ok(_) => panic!("expected sidecar error"),
            Err(error) => error,
        };
        assert!(
            error.contains(&path.display().to_string()),
            "{error:?} did not contain {}",
            path.display()
        );
        assert!(
            error.contains(expected),
            "{error:?} did not contain {expected:?}"
        );
    }

    fn test_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("metafy-sidecar-{name}-{}", Uuid::new_v4()))
    }
}
