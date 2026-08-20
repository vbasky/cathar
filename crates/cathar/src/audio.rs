//! Decoded audio: decode any input, encode WAV/FLAC/AIFF, channel ops.

use crate::{Error, integrated_loudness, resample, true_peak_dbtp};
use hound::{WavSpec, WavWriter};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia_common::mpeg::audio::{AudioObjectType, AudioSpecificConfig};

/// Decoded audio: a sample rate plus one `f32` PCM buffer per channel
/// (de-interleaved, sample values in `[-1.0, 1.0]`).
#[derive(Debug, Clone)]
pub struct AudioData {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// One buffer of `f32` samples per channel (channel-major, equal length).
    pub channels: Vec<Vec<f32>>,
}

impl AudioData {
    /// Decode any [`symphonia`]-supported file (WAV/MP3/FLAC/OGG/M4A/MP4/MKV…)
    /// to de-interleaved `f32` PCM. The container is detected from the
    /// extension and content.
    ///
    /// [`symphonia`]: https://crates.io/crates/symphonia
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        // Take `AsRef<Path>` so Windows paths from `rfd` / `PathBuf` are opened
        // without a lossy `display()` round-trip (which can break File Open).
        let path = path.as_ref();
        let mut last_err = None;
        for (params, upsample_to) in decoder_attempts(path)? {
            match decode_pcm(path, &params, upsample_to) {
                Ok(audio) if audio.channels.iter().any(|c| !c.is_empty()) => return Ok(audio),
                Ok(_) => last_err = Some(Error::Decode("no audio samples decoded".into())),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or(Error::UnsupportedFormat))
    }

    /// Write to `path`, choosing the container from its extension: `.flac`
    /// (24-bit lossless FLAC), `.aif`/`.aiff` (24-bit big-endian PCM), and
    /// anything else — including `.wav` — as 32-bit float WAV.
    pub fn to_file(&self, path: &str) -> Result<(), Error> {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("flac") => self.write_flac(path),
            Some("aif" | "aiff") => self.write_aiff(path),
            _ => self.write_wav(path),
        }
    }

    /// Interleave channels into signed integers at `bits` bits per sample.
    fn interleaved_int(&self, bits: u32) -> Vec<i32> {
        let peak = ((1i64 << (bits - 1)) - 1) as f32;
        let len = self.channels.first().map_or(0, |c| c.len());
        let mut out = Vec::with_capacity(len * self.channels.len());
        for i in 0..len {
            for ch in &self.channels {
                out.push((ch[i].clamp(-1.0, 1.0) * peak).round() as i32);
            }
        }
        out
    }

    /// 32-bit float WAV (the lossless default).
    fn write_wav(&self, path: &str) -> Result<(), Error> {
        let spec = WavSpec {
            channels: self.channels.len() as u16,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = WavWriter::create(path, spec)?;
        let len = self.channels.first().map_or(0, |c| c.len());
        for i in 0..len {
            for ch in &self.channels {
                writer.write_sample(ch[i])?;
            }
        }
        writer.finalize()?;
        if self.channels.len() == 1 {
            fix_mono_wav_channel_mask(path)?;
        }
        Ok(())
    }

    /// 24-bit lossless FLAC via the pure-Rust `flacenc` encoder.
    fn write_flac(&self, path: &str) -> Result<(), Error> {
        use flacenc::component::BitRepr;
        use flacenc::error::Verify;

        let bits = 24u32;
        let channels = self.channels.len().max(1);
        let samples = self.interleaved_int(bits);
        let config = flacenc::config::Encoder::default()
            .into_verified()
            .map_err(|e| Error::Encode(format!("flac config: {e:?}")))?;
        let source = flacenc::source::MemSource::from_samples(
            &samples,
            channels,
            bits as usize,
            self.sample_rate as usize,
        );
        let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
            .map_err(|e| Error::Encode(format!("flac encode: {e:?}")))?;
        let mut sink = flacenc::bitsink::ByteSink::new();
        stream.write(&mut sink).map_err(|e| Error::Encode(format!("flac write: {e:?}")))?;

        // flacenc records the (smaller) final partial block as the stream's
        // `min_block_size`, leaving it ≠ `max_block_size`. Some decoders read
        // that as a variable-block-size stream and then misparse the
        // fixed-block-size frames. The convention for fixed block size is
        // min == max, so copy max over min. STREAMINFO is the first metadata
        // block — `fLaC`(4) + block header(4) — so min is at bytes 8..10 and
        // max at 10..12. The block sizes are not covered by the audio MD5, so
        // this is a safe in-place edit.
        let mut bytes = sink.as_slice().to_vec();
        if bytes.len() >= 12 {
            bytes[8] = bytes[10];
            bytes[9] = bytes[11];
        }
        std::fs::write(path, &bytes)?;
        Ok(())
    }

    /// 24-bit big-endian PCM AIFF, written directly (no AIFF dependency).
    fn write_aiff(&self, path: &str) -> Result<(), Error> {
        let channels = self.channels.len().max(1) as u16;
        let frames = self.channels.first().map_or(0, |c| c.len()) as u32;
        let samples = self.interleaved_int(24);

        // Sample data: 24-bit signed, big-endian, interleaved.
        let mut ssnd = Vec::with_capacity(8 + samples.len() * 3);
        ssnd.extend_from_slice(&0u32.to_be_bytes()); // offset
        ssnd.extend_from_slice(&0u32.to_be_bytes()); // block size
        for s in &samples {
            let v = *s;
            ssnd.push((v >> 16) as u8);
            ssnd.push((v >> 8) as u8);
            ssnd.push(v as u8);
        }

        // COMM chunk (18 bytes): channels, frames, bit depth, 80-bit rate.
        let mut comm = Vec::with_capacity(18);
        comm.extend_from_slice(&(channels as i16).to_be_bytes());
        comm.extend_from_slice(&frames.to_be_bytes());
        comm.extend_from_slice(&24i16.to_be_bytes());
        comm.extend_from_slice(&ieee754_extended(self.sample_rate as f64));

        let mut form = Vec::new();
        form.extend_from_slice(b"AIFF");
        form.extend_from_slice(b"COMM");
        form.extend_from_slice(&(comm.len() as u32).to_be_bytes());
        form.extend_from_slice(&comm);
        form.extend_from_slice(b"SSND");
        form.extend_from_slice(&(ssnd.len() as u32).to_be_bytes());
        form.extend_from_slice(&ssnd);

        let mut out = Vec::with_capacity(8 + form.len());
        out.extend_from_slice(b"FORM");
        out.extend_from_slice(&(form.len() as u32).to_be_bytes());
        out.extend_from_slice(&form);
        std::fs::write(path, out)?;
        Ok(())
    }

    /// Map a single-channel operation across all channels.
    pub fn map_channels<F: Fn(&[f32]) -> Vec<f32>>(&self, f: F) -> Self {
        Self {
            sample_rate: self.sample_rate,
            channels: self.channels.iter().map(|c| f(c)).collect(),
        }
    }

    /// Normalise to a target integrated loudness (LUFS) per ITU-R BS.1770-4 /
    /// EBU R128. A single broadband gain is applied to every channel, so the
    /// stereo image is preserved and loudness is measured jointly across
    /// channels (not per-channel).
    ///
    /// Operates in linear mode: the gain is reduced if needed so the true peak
    /// stays at or below `true_peak_ceiling_db` dBTP. When that guard engages,
    /// the output sits below the loudness target rather than clipping. Silent
    /// input is returned unchanged.
    pub fn normalize_r128(&self, target_lufs: f32, true_peak_ceiling_db: f32) -> Self {
        let measured = integrated_loudness(&self.channels, self.sample_rate);
        if !measured.is_finite() {
            return self.clone();
        }
        let mut gain_db = target_lufs - measured;
        let tp = true_peak_dbtp(&self.channels, self.sample_rate);
        if tp.is_finite() && tp + gain_db > true_peak_ceiling_db {
            gain_db = true_peak_ceiling_db - tp;
        }
        let gain = 10f32.powf(gain_db / 20.0);
        Self {
            sample_rate: self.sample_rate,
            channels: self.channels.iter().map(|c| c.iter().map(|s| s * gain).collect()).collect(),
        }
    }

    /// Resample every channel to `target_rate` with the shared Kaiser-windowed
    /// sinc resampler. This is the main-path resampler — any stage can call it
    /// to bring mixed-rate inputs to a common rate. Returns a clone when the
    /// rate already matches.
    pub fn resample(&self, target_rate: u32) -> Self {
        if target_rate == self.sample_rate {
            return self.clone();
        }
        Self {
            sample_rate: target_rate,
            channels: self
                .channels
                .iter()
                .map(|c| resample(c, self.sample_rate, target_rate))
                .collect(),
        }
    }
}

/// Encode a (non-negative) value as an 80-bit IEEE 754 extended-precision
/// float — the format AIFF's COMM chunk uses to store the sample rate.
pub(crate) fn ieee754_extended(value: f64) -> [u8; 10] {
    let mut bytes = [0u8; 10];
    if value <= 0.0 {
        return bytes;
    }
    let exp = value.log2().floor() as i32;
    let mantissa = (value / 2f64.powi(exp) * 2f64.powi(63)).round() as u64;
    let biased = (exp + 16383) as u16;
    bytes[0] = (biased >> 8) as u8;
    bytes[1] = biased as u8;
    bytes[2..10].copy_from_slice(&mantissa.to_be_bytes());
    bytes
}

/// Rewrite a mono float WAV's channel mask from `FRONT_LEFT` to `FRONT_CENTER`.
///
/// `hound` writes 32-bit float WAV as `WAVE_FORMAT_EXTENSIBLE` and assigns a
/// positional channel mask; for a single channel that mask is `FRONT_LEFT`,
/// which makes layout-aware players (e.g. CoreAudio / `afplay`) route the file
/// to the left speaker only. Patching the mask to `FRONT_CENTER` makes a mono
/// file play centred. Leaves the file untouched if the header isn't the
/// expected mono extensible layout.
fn fix_mono_wav_channel_mask(path: &str) -> Result<(), Error> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let mut hdr = [0u8; 24];
    if f.read_exact(&mut hdr).is_err() {
        return Ok(());
    }
    let fmt_tag = u16::from_le_bytes([hdr[20], hdr[21]]);
    let n_ch = u16::from_le_bytes([hdr[22], hdr[23]]);
    // RIFF/WAVE/fmt header, WAVE_FORMAT_EXTENSIBLE (0xFFFE), one channel.
    if &hdr[0..4] == b"RIFF"
        && &hdr[8..12] == b"WAVE"
        && &hdr[12..16] == b"fmt "
        && fmt_tag == 0xFFFE
        && n_ch == 1
    {
        // dwChannelMask sits 20 bytes into the fmt data, which starts at offset 20.
        f.seek(SeekFrom::Start(40))?;
        f.write_all(&0x0000_0004u32.to_le_bytes())?; // SPEAKER_FRONT_CENTER
    }
    Ok(())
}

fn probe_format(
    path: &std::path::Path,
) -> Result<Box<dyn symphonia::core::formats::FormatReader>, Error> {
    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| Error::Decode(format!("{e}")))
}

fn decoder_attempts(
    path: &std::path::Path,
) -> Result<Vec<(AudioCodecParameters, Option<u32>)>, Error> {
    let format = probe_format(path)?;
    let track = format.default_track(TrackType::Audio).ok_or(Error::NoAudioTrack)?;
    let Some(CodecParameters::Audio(params)) = &track.codec_params else {
        return Err(Error::NoAudioTrack);
    };
    let mut attempts = vec![(params.clone(), None)];
    if params.codec == CODEC_ID_AAC {
        for (extra, core, out) in he_aac_lc_candidates(params.extra_data.as_deref().unwrap_or(&[]))
        {
            let mut lc = params.clone();
            lc.with_extra_data(extra).with_sample_rate(core);
            attempts.push((lc, Some(out)));
        }
    }
    Ok(attempts)
}

fn decode_pcm(
    path: &std::path::Path,
    params: &AudioCodecParameters,
    upsample_to: Option<u32>,
) -> Result<AudioData, Error> {
    let mut format = probe_format(path)?;
    let track_id = format.default_track(TrackType::Audio).ok_or(Error::NoAudioTrack)?.id;
    let mut decoder = match symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
    {
        Ok(d) => d,
        Err(e) => {
            let hint = if e.to_string().contains("too complex") {
                " (HE-AAC/SBR and surround AAC need an AAC-LC transcode)"
            } else {
                ""
            };
            return Err(Error::Decode(format!("{e}{hint}")));
        }
    };
    let mut sample_rate = decoder.codec_params().sample_rate.ok_or(Error::UnsupportedFormat)?;
    let num_channels = decoder
        .codec_params()
        .channels
        .as_ref()
        .map(symphonia::core::audio::Channels::count)
        .filter(|&n| n > 0)
        .ok_or(Error::UnsupportedFormat)?;

    let mut channels = vec![Vec::new(); num_channels];
    let mut interleaved: Vec<f32> = Vec::new();
    loop {
        // Some demuxers (notably FLAC) signal end-of-stream with an
        // `UnexpectedEof` I/O error rather than `Ok(None)`; treat that as a
        // clean end rather than a decode failure.
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => return Err(Error::Decode(format!("{e}"))),
        };
        if packet.track_id != track_id {
            continue;
        }
        // DecodeError: this packet is junk — skip it. ResetRequired: seek/gap.
        let decoded = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(symphonia::core::errors::Error::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(Error::Decode(format!("{e}"))),
        };
        interleaved.clear();
        decoded.copy_to_vec_interleaved(&mut interleaved);
        if interleaved.is_empty() {
            continue;
        }
        for (i, sample) in interleaved.iter().enumerate() {
            channels[i % num_channels].push(*sample);
        }
    }
    if let Some(target) = upsample_to.filter(|&t| t != sample_rate && t > 0) {
        channels = channels.into_iter().map(|ch| resample(&ch, sample_rate, target)).collect();
        sample_rate = target;
    }
    Ok(AudioData { sample_rate, channels })
}

/// HE-AAC (AAC+ / SBR) is AAC-LC at half rate plus a high-band extension.
/// Symphonia's AAC decoder only implements LC and rejects SBR as "too complex".
/// Return LC extra_data candidates (core rate first, then swapped — extra_data
/// layouts disagree on which frequency is the core).
fn he_aac_lc_candidates(extra: &[u8]) -> Vec<(Box<[u8]>, u32, u32)> {
    let Some(asc) = AudioSpecificConfig::read(extra).ok() else {
        return Vec::new();
    };
    if !asc.sbr_present || asc.object_type != AudioObjectType::Lc {
        return Vec::new();
    }
    let Some(ch) = asc.channels.as_ref().map(symphonia::core::audio::Channels::count) else {
        return Vec::new();
    };
    if !(1..=2).contains(&ch) || asc.samples != 1024 {
        return Vec::new();
    }
    let mut pairs = Vec::new();
    match asc.sbr_ps_info {
        Some((ext, _)) if ext > 0 && asc.sample_rate > 0 && ext != asc.sample_rate => {
            pairs.push((ext, asc.sample_rate));
            pairs.push((asc.sample_rate, ext));
        }
        _ => {
            pairs.push((asc.sample_rate, asc.sample_rate.saturating_mul(2)));
            if asc.sample_rate >= 2 {
                pairs.push((asc.sample_rate / 2, asc.sample_rate));
            }
        }
    }
    let mut out = Vec::new();
    for (core, dest) in pairs {
        if core == 0 {
            continue;
        }
        if let Some(cfg) = aac_lc_specific_config(core, ch) {
            out.push((cfg, core, dest));
        }
    }
    out
}

const MPEG4_AUDIO_RATES: [u32; 13] =
    [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];

fn aac_lc_specific_config(sample_rate: u32, channels: usize) -> Option<Box<[u8]>> {
    let ch = u16::try_from(channels).ok().filter(|c| (1..=7).contains(c))?;
    let sr_idx = MPEG4_AUDIO_RATES.iter().position(|&r| r == sample_rate);
    // 5-bit AOT=LC (2), 4-bit rate index, 4-bit channel config, 3 flag bits = 0.
    match sr_idx {
        Some(idx) => {
            let bits = (2u16 << 11) | ((idx as u16) << 7) | (ch << 3);
            Some(Box::from(bits.to_be_bytes().as_slice()))
        }
        None => {
            // Escape index 15 + 24-bit explicit rate.
            let mut bits: u64 = 0;
            bits |= 2u64 << 35; // AOT LC
            bits |= 0xF << 31; // sample-rate escape
            bits |= u64::from(sample_rate) << 7;
            bits |= u64::from(ch) << 3;
            Some(Box::from(bits.to_be_bytes()[3..8].to_vec()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aac_lc_asc_round_trips() {
        let extra = aac_lc_specific_config(22050, 2).unwrap();
        let asc = AudioSpecificConfig::read(&extra).unwrap();
        assert_eq!(asc.object_type, AudioObjectType::Lc);
        assert!(!asc.sbr_present);
        assert_eq!(asc.sample_rate, 22050);
        assert_eq!(asc.samples, 1024);
        assert_eq!(asc.channels.unwrap().count(), 2);
    }

    #[test]
    fn he_aac_explicit_sbr_falls_back_to_lc_core() {
        // AOT=5 (SBR), 44.1 kHz out, stereo, 22.05 kHz core, nested AOT=LC, 1024-frame.
        let extra = [0x2A, 0x13, 0x88, 0x00];
        let cands = he_aac_lc_candidates(&extra);
        assert!(cands.iter().any(|&(_, core, out)| core == 22050 && out == 44100));
        assert!(cands.iter().any(|&(_, core, out)| core == 44100 && out == 22050));
        let (lc, core, out) = &cands[0];
        assert_eq!(*core, 22050);
        assert_eq!(*out, 44100);
        let asc = AudioSpecificConfig::read(lc).unwrap();
        assert_eq!(asc.object_type, AudioObjectType::Lc);
        assert!(!asc.sbr_present);
        assert_eq!(asc.sample_rate, 22050);
    }
}
