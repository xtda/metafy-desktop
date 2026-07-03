use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::media::thumbnail::BgraFrame;

pub const CHUNKED_VIDEO_FORMAT: &str = "metafy_chunked_h264_segments_v1";
pub const CHUNKED_VIDEO_EXTENSION: &str = "mfcv";
const BGRA_BYTES_PER_PIXEL: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChunkedVideoManifest {
    pub format: String,
    pub status: ChunkedVideoStatus,
    pub codec: String,
    pub container: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: u32,
    pub frame_count: u64,
    pub duration_ms: u64,
    pub thumbnail_frame_path: String,
    pub chunks: Vec<ChunkedVideoChunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChunkedVideoStatus {
    Recording,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChunkedVideoChunk {
    pub path: String,
    pub index: u64,
    pub start_frame: u64,
    pub frame_count: u64,
    pub duration_ms: u64,
}

pub fn chunked_manifest_relative_path(temp_directory_relative: &str) -> String {
    format!(
        "{}/screen_video.{CHUNKED_VIDEO_EXTENSION}",
        temp_directory_relative.trim_end_matches('/')
    )
}

pub fn is_chunked_video_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == CHUNKED_VIDEO_EXTENSION)
}

pub fn try_read_manifest(path: impl AsRef<Path>) -> Result<Option<ChunkedVideoManifest>, String> {
    let path = path.as_ref();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Unable to open chunked video manifest {}: {error}",
                path.display()
            ));
        }
    };

    match serde_json::from_reader::<_, ChunkedVideoManifest>(file) {
        Ok(manifest) if manifest.format == CHUNKED_VIDEO_FORMAT => Ok(Some(manifest)),
        Ok(_) => Ok(None),
        Err(error) if looks_like_json_error_from_raw_sidecar(&error) => Ok(None),
        Err(error) => Err(format!(
            "Unable to parse chunked video manifest {}: {error}",
            path.display()
        )),
    }
}

pub fn read_manifest(path: impl AsRef<Path>) -> Result<ChunkedVideoManifest, String> {
    let path = path.as_ref();
    let manifest = try_read_manifest(path)?.ok_or_else(|| {
        format!(
            "Video sidecar {} is not a chunked H.264 manifest.",
            path.display()
        )
    })?;
    validate_manifest(path, manifest)
}

pub fn read_thumbnail_frame(
    manifest_path: impl AsRef<Path>,
    manifest: &ChunkedVideoManifest,
) -> Result<BgraFrame, String> {
    let manifest_path = manifest_path.as_ref();
    let thumbnail_path = manifest_relative_path(manifest_path, &manifest.thumbnail_frame_path);
    let expected_byte_count = bgra_byte_count(manifest.width, manifest.height)?;
    let mut bytes = Vec::new();
    File::open(&thumbnail_path)
        .map_err(|error| {
            format!(
                "Unable to open chunked video thumbnail frame {}: {error}",
                thumbnail_path.display()
            )
        })?
        .read_to_end(&mut bytes)
        .map_err(|error| {
            format!(
                "Unable to read chunked video thumbnail frame {}: {error}",
                thumbnail_path.display()
            )
        })?;
    if bytes.len() != expected_byte_count {
        return Err(format!(
            "Chunked video thumbnail frame {} has {} bytes; expected {expected_byte_count} for {}x{} BGRA.",
            thumbnail_path.display(),
            bytes.len(),
            manifest.width,
            manifest.height
        ));
    }

    Ok(BgraFrame {
        width: manifest.width,
        height: manifest.height,
        bytes,
    })
}

pub fn manifest_relative_path(manifest_path: &Path, relative_path: &str) -> PathBuf {
    manifest_path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(relative_path)
}

fn validate_manifest(
    path: &Path,
    manifest: ChunkedVideoManifest,
) -> Result<ChunkedVideoManifest, String> {
    if manifest.width == 0 || manifest.height == 0 || manifest.frame_rate == 0 {
        return Err(format!(
            "Chunked video manifest {} has invalid video dimensions or frame rate.",
            path.display()
        ));
    }
    if manifest.frame_count == 0 || manifest.chunks.is_empty() {
        return Err(format!(
            "Chunked video manifest {} does not contain any finalized chunks.",
            path.display()
        ));
    }
    let chunk_frame_count = manifest.chunks.iter().try_fold(0_u64, |total, chunk| {
        if chunk.path.trim().is_empty() {
            return Err(format!(
                "Chunked video manifest {} contains a chunk with no path.",
                path.display()
            ));
        }
        if chunk.frame_count == 0 {
            return Err(format!(
                "Chunked video manifest {} contains an empty chunk.",
                path.display()
            ));
        }
        total
            .checked_add(chunk.frame_count)
            .ok_or_else(|| "Chunked video frame count is too large.".to_owned())
    })?;
    if chunk_frame_count != manifest.frame_count {
        return Err(format!(
            "Chunked video manifest {} reports {} frames, but chunks contain {chunk_frame_count}.",
            path.display(),
            manifest.frame_count
        ));
    }

    Ok(manifest)
}

fn looks_like_json_error_from_raw_sidecar(error: &serde_json::Error) -> bool {
    error.is_syntax() || error.is_data() || error.is_eof()
}

fn bgra_byte_count(width: u32, height: u32) -> Result<usize, String> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(BGRA_BYTES_PER_PIXEL))
        .ok_or_else(|| "Chunked video dimensions are too large.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn reads_chunked_manifest_and_thumbnail_frame() {
        let root = std::env::temp_dir().join(format!("metafy-chunked-video-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");
        let manifest_path = root.join("screen_video.mfcv");
        let thumbnail_path = root.join("screen_video-thumbnail.bgra");
        std::fs::write(&thumbnail_path, vec![0x80; 2 * 2 * BGRA_BYTES_PER_PIXEL])
            .expect("write thumbnail");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec(&ChunkedVideoManifest {
                format: CHUNKED_VIDEO_FORMAT.to_owned(),
                status: ChunkedVideoStatus::Completed,
                codec: "h264".to_owned(),
                container: "mp4".to_owned(),
                width: 2,
                height: 2,
                frame_rate: 30,
                frame_count: 3,
                duration_ms: 100,
                thumbnail_frame_path: "screen_video-thumbnail.bgra".to_owned(),
                chunks: vec![ChunkedVideoChunk {
                    path: "screen_video-00000.mp4".to_owned(),
                    index: 0,
                    start_frame: 0,
                    frame_count: 3,
                    duration_ms: 100,
                }],
            })
            .expect("serialize manifest"),
        )
        .expect("write manifest");

        let manifest = read_manifest(&manifest_path).expect("read manifest");
        let thumbnail = read_thumbnail_frame(&manifest_path, &manifest).expect("read thumbnail");

        assert_eq!(manifest.frame_count, 3);
        assert_eq!(thumbnail.width, 2);
        assert_eq!(thumbnail.height, 2);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn raw_sidecar_bytes_are_not_treated_as_chunked_video() {
        let root = std::env::temp_dir().join(format!("metafy-chunked-video-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");
        let manifest_path = root.join("screen_video.mfcv");
        std::fs::write(&manifest_path, b"METAFY_RAW_VIDEO_V1\n").expect("write raw header");

        let manifest = try_read_manifest(&manifest_path).expect("try read manifest");

        assert!(manifest.is_none());

        let _ = std::fs::remove_dir_all(root);
    }
}
