use crate::media::sidecar::RawAudioReader;

pub const FINAL_ENCODE_SAMPLE_RATE: i64 = 48_000;
pub const FINAL_ENCODE_CHANNELS: i64 = 2;
pub const FINAL_ENCODE_SAMPLE_FORMAT: &str = "f32";
pub const TRANSCRIPTION_SAMPLE_RATE: i64 = 16_000;

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedPcmAudio {
    pub samples: Vec<f32>,
    pub sample_rate: i64,
    pub channels: i64,
}

impl PreparedPcmAudio {
    pub fn frame_count(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn is_silent(&self) -> bool {
        !self.samples.is_empty()
            && self
                .samples
                .iter()
                .all(|sample| sample.abs() <= f32::EPSILON)
    }
}

pub fn is_supported_pcm_sample_format(sample_format: &str) -> bool {
    matches!(
        sample_format,
        "i8" | "u8" | "i16" | "u16" | "i24" | "i32" | "u24" | "u32" | "f32" | "f64"
    )
}

pub fn prepare_audio_source(reader: &mut RawAudioReader) -> Result<PreparedPcmAudio, String> {
    let format = reader.format().clone();
    let input_sample_rate = usize::try_from(format.sample_rate)
        .map_err(|_| "Audio sample rate is invalid.".to_owned())?;
    let mut source_samples = Vec::new();

    while let Some(block) = reader.next_block()? {
        let start_frame = timestamp_to_frame(block.elapsed_ms, input_sample_rate)?;
        let block_samples =
            decode_pcm_block_to_stereo(&block.bytes, &format.sample_format, format.channels)?;
        let current_frame = source_samples.len() / FINAL_ENCODE_CHANNELS as usize;

        if start_frame > current_frame {
            let missing_frames = start_frame - current_frame;
            source_samples.extend(
                std::iter::repeat(0.0_f32).take(missing_frames * FINAL_ENCODE_CHANNELS as usize),
            );
            source_samples.extend_from_slice(&block_samples);
        } else {
            let overlapping_frames = current_frame - start_frame;
            let skip_samples = overlapping_frames.saturating_mul(FINAL_ENCODE_CHANNELS as usize);
            if skip_samples < block_samples.len() {
                source_samples.extend_from_slice(&block_samples[skip_samples..]);
            }
        }
    }

    Ok(PreparedPcmAudio {
        samples: resample_stereo(
            &source_samples,
            format.sample_rate,
            FINAL_ENCODE_SAMPLE_RATE,
        )?,
        sample_rate: FINAL_ENCODE_SAMPLE_RATE,
        channels: FINAL_ENCODE_CHANNELS,
    })
}

pub fn mix_audio_sources(sources: &[PreparedPcmAudio]) -> PreparedPcmAudio {
    let valid_sources = sources
        .iter()
        .filter(|source| !source.is_empty())
        .collect::<Vec<_>>();

    if valid_sources.is_empty() {
        return PreparedPcmAudio {
            samples: Vec::new(),
            sample_rate: FINAL_ENCODE_SAMPLE_RATE,
            channels: FINAL_ENCODE_CHANNELS,
        };
    }

    if valid_sources.len() == 1 {
        return PreparedPcmAudio {
            samples: valid_sources[0].samples.clone(),
            sample_rate: FINAL_ENCODE_SAMPLE_RATE,
            channels: FINAL_ENCODE_CHANNELS,
        };
    }

    let output_frames = valid_sources
        .iter()
        .map(|source| source.frame_count())
        .max()
        .unwrap_or_default();
    let mut mixed = vec![0.0_f32; output_frames * FINAL_ENCODE_CHANNELS as usize];
    let gain = 1.0_f32 / valid_sources.len() as f32;

    for source in valid_sources {
        for (output, sample) in mixed.iter_mut().zip(source.samples.iter()) {
            *output += *sample * gain;
        }
    }

    PreparedPcmAudio {
        samples: mixed,
        sample_rate: FINAL_ENCODE_SAMPLE_RATE,
        channels: FINAL_ENCODE_CHANNELS,
    }
}

pub fn write_f32le_samples(samples: &[f32], output: &mut Vec<u8>) {
    output.reserve(samples.len() * std::mem::size_of::<f32>());
    for sample in samples {
        output.extend_from_slice(&sample.to_le_bytes());
    }
}

pub fn prepare_transcription_samples(source: &PreparedPcmAudio) -> Result<Vec<i16>, String> {
    let mono = downmix_to_mono(&source.samples, source.channels)?;
    let resampled = resample_mono(&mono, source.sample_rate, TRANSCRIPTION_SAMPLE_RATE)?;
    Ok(resampled.into_iter().map(f32_sample_to_i16).collect())
}

fn timestamp_to_frame(elapsed_ms: u64, sample_rate: usize) -> Result<usize, String> {
    if sample_rate == 0 {
        return Err("Audio sample rate is invalid.".to_owned());
    }
    let frame = (u128::from(elapsed_ms) * sample_rate as u128 + 500) / 1000;
    usize::try_from(frame).map_err(|_| "Audio timeline offset is too large.".to_owned())
}

fn decode_pcm_block_to_stereo(
    bytes: &[u8],
    sample_format: &str,
    channels: i64,
) -> Result<Vec<f32>, String> {
    let channels =
        usize::try_from(channels).map_err(|_| "Audio channel count is invalid.".to_owned())?;
    if channels == 0 {
        return Err("Audio channel count is invalid.".to_owned());
    }

    let sample_size = pcm_sample_size(sample_format)
        .ok_or_else(|| format!("Audio sample format {sample_format} is unsupported."))?;
    let frame_size = sample_size
        .checked_mul(channels)
        .ok_or_else(|| "Audio frame size is too large.".to_owned())?;
    if frame_size == 0 || bytes.len() % frame_size != 0 {
        return Err(format!(
            "Audio block has {} bytes, which is not aligned to {channels} channel(s) of {sample_format} samples.",
            bytes.len()
        ));
    }

    let frame_count = bytes.len() / frame_size;
    let mut samples = Vec::with_capacity(frame_count * FINAL_ENCODE_CHANNELS as usize);

    for frame in bytes.chunks_exact(frame_size) {
        if channels == 1 {
            let sample = pcm_sample_to_f32(sample_format, &frame[..sample_size])?;
            samples.push(sample);
            samples.push(sample);
        } else if channels == 2 {
            samples.push(pcm_sample_to_f32(sample_format, &frame[..sample_size])?);
            samples.push(pcm_sample_to_f32(
                sample_format,
                &frame[sample_size..sample_size * 2],
            )?);
        } else {
            let mut sum = 0.0_f32;
            for sample in frame.chunks_exact(sample_size) {
                sum += pcm_sample_to_f32(sample_format, sample)?;
            }
            let mono = sum / channels as f32;
            samples.push(mono);
            samples.push(mono);
        }
    }

    Ok(samples)
}

fn pcm_sample_size(sample_format: &str) -> Option<usize> {
    match sample_format {
        "i8" | "u8" => Some(1),
        "i16" | "u16" => Some(2),
        "i24" | "u24" | "i32" | "u32" | "f32" => Some(4),
        "f64" => Some(8),
        _ => None,
    }
}

fn pcm_sample_to_f32(sample_format: &str, bytes: &[u8]) -> Result<f32, String> {
    match sample_format {
        "i8" => Ok(i8::from_le_bytes([bytes[0]]) as f32 / 128.0),
        "u8" => Ok((u8::from_le_bytes([bytes[0]]) as f32 - 128.0) / 128.0),
        "i16" => Ok(i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32_768.0),
        "u16" => Ok((u16::from_le_bytes([bytes[0], bytes[1]]) as f32 - 32_768.0) / 32_768.0),
        "i24" => {
            Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 / 8_388_608.0)
        }
        "u24" => Ok(
            (i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 - 8_388_608.0)
                / 8_388_608.0,
        ),
        "i32" => Ok(
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 / 2_147_483_648.0,
        ),
        "u32" => Ok(
            ((u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64
                - 2_147_483_648.0)
                / 2_147_483_648.0) as f32,
        ),
        "f32" => Ok(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        "f64" => Ok(f64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]) as f32),
        _ => Err(format!(
            "Audio sample format {sample_format} is unsupported."
        )),
    }
}

fn resample_stereo(samples: &[f32], input_rate: i64, output_rate: i64) -> Result<Vec<f32>, String> {
    resample_interleaved(
        samples,
        FINAL_ENCODE_CHANNELS as usize,
        input_rate,
        output_rate,
    )
}

fn resample_mono(samples: &[f32], input_rate: i64, output_rate: i64) -> Result<Vec<f32>, String> {
    resample_interleaved(samples, 1, input_rate, output_rate)
}

fn resample_interleaved(
    samples: &[f32],
    channels: usize,
    input_rate: i64,
    output_rate: i64,
) -> Result<Vec<f32>, String> {
    if input_rate <= 0 || output_rate <= 0 {
        return Err("Audio sample rate is invalid.".to_owned());
    }
    if channels == 0 {
        return Err("Audio channel count is invalid.".to_owned());
    }
    if samples.is_empty() || input_rate == output_rate {
        return Ok(samples.to_vec());
    }
    if samples.len() % channels != 0 {
        return Err("Audio sample data is malformed.".to_owned());
    }

    let input_frames = samples.len() / channels;
    let output_frames = rounded_resampled_frame_count(input_frames, input_rate, output_rate)?;
    let mut output = Vec::with_capacity(output_frames * channels);

    for output_frame in 0..output_frames {
        let source_position = output_frame as f64 * input_rate as f64 / output_rate as f64;
        let lower_frame = source_position.floor() as usize;
        let upper_frame = (lower_frame + 1).min(input_frames.saturating_sub(1));
        let fraction = (source_position - lower_frame as f64) as f32;

        for channel in 0..channels {
            let lower = samples[lower_frame * channels + channel];
            let upper = samples[upper_frame * channels + channel];
            output.push(lower + (upper - lower) * fraction);
        }
    }

    Ok(output)
}

fn rounded_resampled_frame_count(
    input_frames: usize,
    input_rate: i64,
    output_rate: i64,
) -> Result<usize, String> {
    let input_rate = u128::try_from(input_rate).map_err(|_| "Audio sample rate is invalid.")?;
    let output_rate = u128::try_from(output_rate).map_err(|_| "Audio sample rate is invalid.")?;
    let frames = (input_frames as u128 * output_rate + input_rate / 2) / input_rate;
    usize::try_from(frames).map_err(|_| "Resampled audio is too large.".to_owned())
}

fn downmix_to_mono(samples: &[f32], channels: i64) -> Result<Vec<f32>, String> {
    let channels =
        usize::try_from(channels).map_err(|_| "Audio channel count is invalid.".to_owned())?;
    if channels == 0 {
        return Err("Audio channel count is invalid.".to_owned());
    }
    if samples.len() % channels != 0 {
        return Err("Audio sample data is malformed.".to_owned());
    }

    Ok(samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
        .collect())
}

fn f32_sample_to_i16(sample: f32) -> i16 {
    let sample = if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    };

    if sample < 0.0 {
        (sample * 32_768.0).round() as i16
    } else {
        (sample * 32_767.0).round() as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::sidecar::{RawAudioBlock, RawAudioFormat};

    #[test]
    fn converts_supported_pcm_formats_to_f32_stereo() {
        let cases = [
            ("i8", vec![i8::MIN as u8], -1.0_f32),
            ("u8", vec![128_u8], 0.0_f32),
            ("i16", i16::MIN.to_le_bytes().to_vec(), -1.0_f32),
            ("u16", 32_768_u16.to_le_bytes().to_vec(), 0.0_f32),
            ("i24", (-4_194_304_i32).to_le_bytes().to_vec(), -0.5_f32),
            ("u24", 12_582_912_i32.to_le_bytes().to_vec(), 0.5_f32),
            ("i32", i32::MIN.to_le_bytes().to_vec(), -1.0_f32),
            ("u32", 2_147_483_648_u32.to_le_bytes().to_vec(), 0.0_f32),
            ("f32", 0.25_f32.to_le_bytes().to_vec(), 0.25_f32),
            ("f64", 0.75_f64.to_le_bytes().to_vec(), 0.75_f32),
        ];

        for (sample_format, bytes, expected) in cases {
            let samples =
                decode_pcm_block_to_stereo(&bytes, sample_format, 1).expect(sample_format);

            assert_approx(samples[0], expected);
            assert_approx(samples[1], expected);
        }
    }

    #[test]
    fn normalizes_multichannel_audio_by_averaging_to_stereo() {
        let mut bytes = Vec::new();
        for sample in [0.0_f32, 0.5_f32, 1.0_f32] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }

        let samples = decode_pcm_block_to_stereo(&bytes, "f32", 3).expect("decode");

        assert_eq!(samples.len(), 2);
        assert_approx(samples[0], 0.5);
        assert_approx(samples[1], 0.5);
    }

    #[test]
    fn resamples_source_audio_to_48khz_stereo() {
        let source = build_prepared_source(
            RawAudioFormat {
                sample_rate: 24_000,
                channels: 1,
                sample_format: "f32".to_owned(),
            },
            vec![RawAudioBlock {
                elapsed_ms: 0,
                callback_stream_ns: 0,
                capture_stream_ns: 0,
                bytes: f32_samples(&[0.0, 1.0]),
            }],
        )
        .expect("prepare source");

        assert_eq!(source.sample_rate, FINAL_ENCODE_SAMPLE_RATE);
        assert_eq!(source.channels, FINAL_ENCODE_CHANNELS);
        assert_eq!(source.frame_count(), 4);
        assert_approx(source.samples[0], 0.0);
        assert_approx(source.samples[2], 0.5);
        assert_approx(source.samples[4], 1.0);
        assert_approx(source.samples[6], 1.0);
    }

    #[test]
    fn applies_elapsed_timing_gaps_from_audio_blocks() {
        let source = build_prepared_source(
            RawAudioFormat {
                sample_rate: 48_000,
                channels: 2,
                sample_format: "f32".to_owned(),
            },
            vec![RawAudioBlock {
                elapsed_ms: 1,
                callback_stream_ns: 0,
                capture_stream_ns: 0,
                bytes: f32_samples(&[1.0, 1.0]),
            }],
        )
        .expect("prepare source");

        assert_eq!(source.frame_count(), 49);
        assert!(source.samples[..96].iter().all(|sample| *sample == 0.0));
        assert_approx(source.samples[96], 1.0);
        assert_approx(source.samples[97], 1.0);
    }

    #[test]
    fn mixes_sources_with_equal_gain_and_longest_duration() {
        let short = PreparedPcmAudio {
            samples: f32_samples_raw(&[1.0, 1.0]),
            sample_rate: FINAL_ENCODE_SAMPLE_RATE,
            channels: FINAL_ENCODE_CHANNELS,
        };
        let long = PreparedPcmAudio {
            samples: f32_samples_raw(&[0.5, 0.5, 0.5, 0.5]),
            sample_rate: FINAL_ENCODE_SAMPLE_RATE,
            channels: FINAL_ENCODE_CHANNELS,
        };

        let mixed = mix_audio_sources(&[short, long]);

        assert_eq!(mixed.frame_count(), 2);
        assert_approx(mixed.samples[0], 0.75);
        assert_approx(mixed.samples[1], 0.75);
        assert_approx(mixed.samples[2], 0.25);
        assert_approx(mixed.samples[3], 0.25);
    }

    #[test]
    fn prepares_transcription_samples_as_16khz_mono_i16() {
        let source = PreparedPcmAudio {
            samples: std::iter::repeat([0.5_f32, 0.25_f32])
                .take(480)
                .flatten()
                .collect(),
            sample_rate: FINAL_ENCODE_SAMPLE_RATE,
            channels: FINAL_ENCODE_CHANNELS,
        };

        let samples = prepare_transcription_samples(&source).expect("prepare transcription");

        assert_eq!(samples.len(), 160);
        assert_eq!(samples[0], 12_288);
    }

    fn build_prepared_source(
        format: RawAudioFormat,
        blocks: Vec<RawAudioBlock>,
    ) -> Result<PreparedPcmAudio, String> {
        let mut source_samples = Vec::new();
        for block in blocks {
            let start_frame = timestamp_to_frame(block.elapsed_ms, format.sample_rate as usize)?;
            let block_samples =
                decode_pcm_block_to_stereo(&block.bytes, &format.sample_format, format.channels)?;
            let current_frame = source_samples.len() / FINAL_ENCODE_CHANNELS as usize;
            if start_frame > current_frame {
                source_samples.extend(
                    std::iter::repeat(0.0_f32)
                        .take((start_frame - current_frame) * FINAL_ENCODE_CHANNELS as usize),
                );
            }
            source_samples.extend_from_slice(&block_samples);
        }

        Ok(PreparedPcmAudio {
            samples: resample_stereo(
                &source_samples,
                format.sample_rate,
                FINAL_ENCODE_SAMPLE_RATE,
            )?,
            sample_rate: FINAL_ENCODE_SAMPLE_RATE,
            channels: FINAL_ENCODE_CHANNELS,
        })
    }

    fn f32_samples(samples: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_f32le_samples(samples, &mut bytes);
        bytes
    }

    fn f32_samples_raw(samples: &[f32]) -> Vec<f32> {
        samples.to_vec()
    }

    fn assert_approx(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.0001,
            "{actual} did not match {expected}"
        );
    }
}
