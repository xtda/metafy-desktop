use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ColorType, RgbImage};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

const BGRA_BYTES_PER_PIXEL: usize = 4;
const THUMBNAIL_WIDTH: u32 = 480;
const JPEG_QUALITY: u8 = 85;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BgraFrame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

pub fn write_bgra_thumbnail(frame: &BgraFrame, output_path: &Path) -> Result<(), String> {
    validate_frame(frame)?;
    let rgb = bgra_to_rgb(frame)?;
    let source = RgbImage::from_raw(frame.width, frame.height, rgb)
        .ok_or_else(|| "Unable to create thumbnail image from BGRA frame.".to_owned())?;
    let thumbnail_height = scaled_thumbnail_height(frame.width, frame.height)?;
    let thumbnail = image::imageops::resize(
        &source,
        THUMBNAIL_WIDTH,
        thumbnail_height,
        FilterType::Triangle,
    );
    let file = File::create(output_path)
        .map_err(|error| format!("Unable to create thumbnail image: {error}"))?;
    let mut encoder = JpegEncoder::new_with_quality(BufWriter::new(file), JPEG_QUALITY);
    encoder
        .encode(
            thumbnail.as_raw(),
            thumbnail.width(),
            thumbnail.height(),
            ColorType::Rgb8.into(),
        )
        .map_err(|error| format!("Unable to encode thumbnail JPEG: {error}"))
}

fn validate_frame(frame: &BgraFrame) -> Result<(), String> {
    if frame.width == 0 || frame.height == 0 {
        return Err("Thumbnail source frame dimensions are invalid.".to_owned());
    }

    let expected = usize::try_from(frame.width)
        .ok()
        .and_then(|width| {
            usize::try_from(frame.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(BGRA_BYTES_PER_PIXEL))
        .ok_or_else(|| "Thumbnail source frame is too large.".to_owned())?;
    if frame.bytes.len() != expected {
        return Err(format!(
            "Thumbnail source frame has {} bytes; expected {expected}.",
            frame.bytes.len()
        ));
    }

    Ok(())
}

fn bgra_to_rgb(frame: &BgraFrame) -> Result<Vec<u8>, String> {
    let pixel_count = usize::try_from(frame.width)
        .ok()
        .and_then(|width| {
            usize::try_from(frame.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| "Thumbnail source frame is too large.".to_owned())?;
    let mut rgb = Vec::with_capacity(pixel_count * 3);

    for pixel in frame.bytes.chunks_exact(BGRA_BYTES_PER_PIXEL) {
        rgb.push(pixel[2]);
        rgb.push(pixel[1]);
        rgb.push(pixel[0]);
    }

    Ok(rgb)
}

fn scaled_thumbnail_height(width: u32, height: u32) -> Result<u32, String> {
    let scaled = (u128::from(height) * u128::from(THUMBNAIL_WIDTH) + u128::from(width / 2))
        / u128::from(width);
    u32::try_from(scaled.max(1)).map_err(|_| "Thumbnail output is too tall.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn writes_jpeg_thumbnail_from_bgra_frame_without_external_binaries() {
        let root = std::env::temp_dir().join(format!("metafy-thumbnail-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create test root");
        let thumbnail_path = root.join("thumbnail.jpg");
        let frame = BgraFrame {
            width: 2,
            height: 1,
            bytes: vec![
                0x00, 0x00, 0xff, 0xff, // red
                0xff, 0x00, 0x00, 0xff, // blue
            ],
        };

        write_bgra_thumbnail(&frame, &thumbnail_path).expect("write thumbnail");

        let bytes = std::fs::read(&thumbnail_path).expect("read thumbnail");
        assert_eq!(&bytes[0..2], &[0xff, 0xd8]);
        assert!(bytes.len() > 100);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_misaligned_bgra_input() {
        let frame = BgraFrame {
            width: 2,
            height: 2,
            bytes: vec![0; 4],
        };

        let error = write_bgra_thumbnail(&frame, Path::new("/tmp/unused.jpg"))
            .expect_err("misaligned frame should fail");

        assert!(error.contains("expected 16"), "{error}");
    }
}
