use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

const PCM_FORMAT_CODE: u16 = 1;
const PCM_I16_BITS_PER_SAMPLE: u16 = 16;
const PCM_I16_BYTES_PER_SAMPLE: u16 = 2;

pub fn write_mono_i16_wav(
    path: impl AsRef<Path>,
    sample_rate: u32,
    samples: &[i16],
) -> Result<(), String> {
    write_pcm_i16_wav(path, sample_rate, 1, samples)
}

fn write_pcm_i16_wav(
    path: impl AsRef<Path>,
    sample_rate: u32,
    channels: u16,
    samples: &[i16],
) -> Result<(), String> {
    if sample_rate == 0 {
        return Err("WAV sample rate must be greater than zero.".to_owned());
    }
    if channels == 0 {
        return Err("WAV channel count must be greater than zero.".to_owned());
    }
    if samples.len() % channels as usize != 0 {
        return Err("WAV sample data is not aligned to the channel count.".to_owned());
    }

    let data_size = samples
        .len()
        .checked_mul(PCM_I16_BYTES_PER_SAMPLE as usize)
        .and_then(|size| u32::try_from(size).ok())
        .ok_or_else(|| "WAV sample data is too large.".to_owned())?;
    let riff_size = 36_u32
        .checked_add(data_size)
        .ok_or_else(|| "WAV sample data is too large.".to_owned())?;
    let block_align = channels
        .checked_mul(PCM_I16_BYTES_PER_SAMPLE)
        .ok_or_else(|| "WAV block alignment is too large.".to_owned())?;
    let byte_rate = sample_rate
        .checked_mul(u32::from(block_align))
        .ok_or_else(|| "WAV byte rate is too large.".to_owned())?;

    let path = path.as_ref();
    let mut writer = BufWriter::new(
        File::create(path).map_err(|error| format!("Unable to create WAV file: {error}"))?,
    );

    writer
        .write_all(b"RIFF")
        .and_then(|()| writer.write_all(&riff_size.to_le_bytes()))
        .and_then(|()| writer.write_all(b"WAVE"))
        .and_then(|()| writer.write_all(b"fmt "))
        .and_then(|()| writer.write_all(&16_u32.to_le_bytes()))
        .and_then(|()| writer.write_all(&PCM_FORMAT_CODE.to_le_bytes()))
        .and_then(|()| writer.write_all(&channels.to_le_bytes()))
        .and_then(|()| writer.write_all(&sample_rate.to_le_bytes()))
        .and_then(|()| writer.write_all(&byte_rate.to_le_bytes()))
        .and_then(|()| writer.write_all(&block_align.to_le_bytes()))
        .and_then(|()| writer.write_all(&PCM_I16_BITS_PER_SAMPLE.to_le_bytes()))
        .and_then(|()| writer.write_all(b"data"))
        .and_then(|()| writer.write_all(&data_size.to_le_bytes()))
        .map_err(|error| format!("Unable to write WAV header: {error}"))?;

    for sample in samples {
        writer
            .write_all(&sample.to_le_bytes())
            .map_err(|error| format!("Unable to write WAV samples: {error}"))?;
    }
    writer
        .flush()
        .map_err(|error| format!("Unable to flush WAV file: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn writes_mono_i16_pcm_wav() {
        let path = std::env::temp_dir().join(format!("metafy-wav-{}.wav", Uuid::new_v4()));

        write_mono_i16_wav(&path, 16_000, &[-32_768, 0, 32_767]).expect("write wav");

        let bytes = fs::read(&path).expect("read wav");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), PCM_FORMAT_CODE);
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1);
        assert_eq!(
            u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            16_000
        );
        assert_eq!(
            u16::from_le_bytes([bytes[34], bytes[35]]),
            PCM_I16_BITS_PER_SAMPLE
        );
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]),
            6
        );
        assert_eq!(i16::from_le_bytes([bytes[44], bytes[45]]), -32_768);
        assert_eq!(i16::from_le_bytes([bytes[46], bytes[47]]), 0);
        assert_eq!(i16::from_le_bytes([bytes[48], bytes[49]]), 32_767);

        let _ = fs::remove_file(path);
    }
}
