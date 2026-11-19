//! Convert lossless music while syncing, to fit more of it on the iPod: to
//! Ogg Vorbis, or to 16-bit FLAC for the hi-res files. Tags, ReplayGain and
//! cover art are carried over. Vorbis, not MP3: pod plays it gapless, and an
//! MP3 would need the LAME tag written by hand.
//!
//! pod's output is 16-bit at 44.1 or 48 kHz, so a hi-res file plays no better
//! than its 16-bit reduction and only costs space and battery to decode.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::num::{NonZeroU8, NonZeroU32};
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

use flacenc::component::BitRepr;
use flacenc::error::{SourceError, Verify};
use flacenc::source::{Fill, Source};
use lofty::config::WriteOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::ogg::VorbisComments;
use lofty::tag::TagExt;
use rubato::{FftFixedInOut, Resampler};
use serde::{Deserialize, Serialize};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_ALAC, DecoderOptions};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

use crate::{Error, Result};

/// The PP5020 can't decode Vorbis above this rate in real time.
const MAX_RATE: u32 = 48_000;
const RESAMPLED: u32 = 44_100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub enum Profile {
    /// Copy everything as it is.
    #[default]
    Off,
    /// Ogg Vorbis at quality 3, 5 or 7.
    Vorbis(u8),
    /// FLAC at 16 bits and at most 48 kHz, for the files above that; the
    /// others are copied.
    Flac16,
}

impl Profile {
    pub const ALL: [Profile; 5] =
        [Profile::Off, Profile::Flac16, Profile::Vorbis(3), Profile::Vorbis(5), Profile::Vorbis(7)];

    /// Extension of the converted files.
    pub fn ext(self) -> &'static str {
        match self {
            Profile::Flac16 => "flac",
            _ => "ogg",
        }
    }

    /// Average Vorbis bitrate in kbit/s, to estimate sizes.
    fn kbps(q: u8) -> u64 {
        match q {
            0..=3 => 112,
            4..=5 => 160,
            _ => 224,
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Profile::Off => f.write_str("off"),
            Profile::Vorbis(q) => write!(f, "ogg-q{q}"),
            Profile::Flac16 => f.write_str("flac-16"),
        }
    }
}

impl FromStr for Profile {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "off" => Ok(Profile::Off),
            "flac-16" => Ok(Profile::Flac16),
            _ => match s.strip_prefix("ogg-q").and_then(|q| q.parse().ok()) {
                Some(q @ 0..=10) => Ok(Profile::Vorbis(q)),
                _ => Err(format!("unknown profile {s}")),
            },
        }
    }
}

impl From<Profile> for String {
    fn from(p: Profile) -> String {
        p.to_string()
    }
}

impl TryFrom<String> for Profile {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        s.parse()
    }
}

fn ext(src: &Path) -> String {
    src.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn open(src: &Path) -> Result<symphonia::core::probe::ProbeResult> {
    let mss = MediaSourceStream::new(Box::new(File::open(src)?), Default::default());
    let mut hint = Hint::new();
    hint.with_extension(&ext(src));
    symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| Error::Other(format!("{}: {e}", src.display())))
}

/// The estimated size of `src` converted to `profile`. `None` when it stays
/// as it is: lossy files, anything the encoder can't take, and with
/// [`Profile::Flac16`] the files that are 16-bit at 48 kHz or less already.
pub fn needs(src: &Path, profile: Profile) -> Option<u64> {
    if profile == Profile::Off {
        return None;
    }
    match ext(src).as_str() {
        "flac" | "wav" | "aif" | "aiff" => {}
        "m4a" | "mp4" => {
            let track = open(src).ok()?.format.default_track()?.codec_params.clone();
            if track.codec != CODEC_TYPE_ALAC {
                return None;
            }
        }
        _ => return None,
    }
    let props = lofty::read_from_path(src).ok()?.properties().clone();
    if props.channels()? > 2 {
        return None;
    }
    match profile {
        Profile::Vorbis(q) => Some(props.duration().as_secs_f64().ceil() as u64 * Profile::kbps(q) * 125),
        _ => {
            let (bits, rate) = (props.bit_depth()? as u64, props.sample_rate()? as u64);
            if bits <= 16 && rate <= MAX_RATE as u64 {
                return None;
            }
            let out_rate = if rate > MAX_RATE as u64 { RESAMPLED as u64 } else { rate };
            // high: the bits dropped are the noise FLAC can't compress
            let size = fs::metadata(src).ok()?.len();
            Some(size * 16.min(bits) / bits * out_rate / rate)
        }
    }
}

/// Decode `src`, resampled to at most [`MAX_RATE`]. `start` makes the
/// encoder from the rate and channels, `block` feeds it the planar audio,
/// block by block.
fn decode<E>(
    src: &Path,
    cancel: &AtomicBool,
    start: impl FnOnce(u32, usize) -> Result<E>,
    mut block: impl FnMut(&mut E, &[&[f32]]) -> Result<()>,
) -> Result<E> {
    let other = |e: &dyn fmt::Display| Error::Other(format!("{}: {e}", src.display()));
    let mut format = open(src)?.format;
    let track = format.default_track().ok_or_else(|| other(&"no audio"))?;
    let (id, params) = (track.id, track.codec_params.clone());
    let rate = params.sample_rate.ok_or_else(|| other(&"unknown sample rate"))?;
    let channels = params.channels.ok_or_else(|| other(&"unknown channels"))?.count();
    let mut decoder =
        symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).map_err(|e| other(&e))?;

    let out_rate = if rate > MAX_RATE { RESAMPLED } else { rate };
    let mut resampler = (out_rate != rate)
        .then(|| FftFixedInOut::<f32>::new(rate as usize, out_rate as usize, 1024, channels))
        .transpose()
        .map_err(|e| other(&e))?;
    let mut encoder = start(out_rate, channels)?;

    // the first `max` frames of `out` but for the `skip` first ones still to
    // drop; returns how many went to `block`
    let mut write = |encoder: &mut E, out: &[Vec<f32>], skip: &mut usize, max: u64| -> Result<u64> {
        let drop = (*skip).min(out[0].len());
        *skip -= drop;
        let n = ((out[0].len() - drop) as u64).min(max) as usize;
        if n > 0 {
            let cut: Vec<&[f32]> = out.iter().map(|c| &c[drop..drop + n]).collect();
            block(encoder, &cut)?;
        }
        Ok(n as u64)
    };

    // decoded frames not yet resampled, planar
    let mut pending: Vec<Vec<f32>> = vec![Vec::new(); channels];
    let mut samples: Option<SampleBuffer<f32>> = None;
    // the resampler's output starts with its filter delay, which is dropped
    let mut skip = resampler.as_ref().map_or(0, |r| r.output_delay());
    let mut in_frames = 0u64;
    let mut out_frames = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(other(&e)),
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(|e| other(&e))?;
        let buf = samples.get_or_insert_with(|| SampleBuffer::new(decoded.capacity() as u64, *decoded.spec()));
        buf.copy_interleaved_ref(decoded);
        for frame in buf.samples().chunks_exact(channels) {
            in_frames += 1;
            for (c, s) in pending.iter_mut().zip(frame) {
                c.push(*s);
            }
        }
        match resampler.as_mut() {
            None => {
                write(&mut encoder, &pending, &mut 0, u64::MAX)?;
                pending.iter_mut().for_each(Vec::clear);
            }
            Some(r) => {
                while pending[0].len() >= r.input_frames_next() {
                    let n = r.input_frames_next();
                    let chunk: Vec<&[f32]> = pending.iter().map(|c| &c[..n]).collect();
                    let out = r.process(&chunk, None).map_err(|e| other(&e))?;
                    out_frames += write(&mut encoder, &out, &mut skip, u64::MAX)?;
                    pending.iter_mut().for_each(|c| {
                        c.drain(..n);
                    });
                }
            }
        }
    }
    if let Some(r) = resampler.as_mut() {
        // the length the converted track must have to stay gapless
        let total = (in_frames * out_rate as u64).div_ceil(rate as u64);
        let mut tail: Option<&[Vec<f32>]> = (!pending[0].is_empty()).then_some(&pending);
        while out_frames < total {
            let out = r.process_partial(tail.take(), None).map_err(|e| other(&e))?;
            out_frames += write(&mut encoder, &out, &mut skip, total - out_frames)?;
        }
    }
    Ok(encoder)
}

/// Encode `src` to the file `tmp` in `profile`.
pub fn encode(src: &Path, tmp: &Path, profile: Profile, cancel: &AtomicBool) -> Result<()> {
    let other = |e: &dyn fmt::Display| Error::Other(format!("{}: {e}", src.display()));
    match profile {
        Profile::Off => unreachable!("not a conversion"),
        Profile::Vorbis(q) => {
            let encoder = decode(
                src,
                cancel,
                |rate, channels| {
                    VorbisEncoderBuilder::new(
                        NonZeroU32::new(rate).unwrap(),
                        NonZeroU8::new(channels as u8).unwrap(),
                        File::create(tmp)?,
                    )
                    .map_err(|e| other(&e))?
                    .bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
                        target_quality: q as f32 / 10.0,
                    })
                    .build()
                    .map_err(|e| other(&e))
                },
                |e, b| e.encode_audio_block(b).map_err(|e| other(&e)),
            )?;
            encoder.finish().map_err(|e| other(&e))?.sync_all()?;
        }
        Profile::Flac16 => {
            let mut dither = Dither(0x9e37_79b9_7f4a_7c15);
            // flacenc takes the whole track at once
            let pcm = decode(
                src,
                cancel,
                |rate, channels| Ok(Pcm16 { bytes: Vec::new(), channels, rate, at: 0 }),
                |pcm, b| {
                    pcm.bytes.reserve(2 * b.len() * b[0].len());
                    for i in 0..b[0].len() {
                        for c in b {
                            let s = (c[i] * 32768.0 + dither.tpdf()).round().clamp(-32768.0, 32767.0) as i16;
                            pcm.bytes.extend(s.to_le_bytes());
                        }
                    }
                    Ok(())
                },
            )?;
            let flac = flac(pcm).map_err(|e| other(&e))?;
            let mut f = File::create(tmp)?;
            f.write_all(&flac)?;
            f.sync_all()?;
        }
    }

    // the tags go last, into the finished file; pod reads pictures from
    // METADATA_BLOCK_PICTURE comments in Vorbis, PICTURE blocks in FLAC
    let tagged = lofty::read_from_path(src).map_err(|e| other(&e))?;
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        let mut f = OpenOptions::new().read(true).write(true).open(tmp)?;
        VorbisComments::from(tag.clone()).save_to(&mut f, WriteOptions::default()).map_err(|e| other(&e))?;
        f.sync_all()?;
    }
    Ok(())
}

/// A track of interleaved 16-bit samples, as little-endian bytes: a quarter
/// of the memory of flacenc's `MemSource`, which copies `i32`s.
struct Pcm16 {
    bytes: Vec<u8>,
    channels: usize,
    rate: u32,
    /// Bytes read.
    at: usize,
}

impl Source for Pcm16 {
    fn channels(&self) -> usize {
        self.channels
    }

    fn bits_per_sample(&self) -> usize {
        16
    }

    fn sample_rate(&self) -> usize {
        self.rate as usize
    }

    fn read_samples<F: Fill>(&mut self, block_size: usize, dest: &mut F) -> std::result::Result<usize, SourceError> {
        let end = (self.at + 2 * self.channels * block_size).min(self.bytes.len());
        dest.fill_le_bytes(&self.bytes[self.at..end], 2)?;
        let frames = (end - self.at) / (2 * self.channels);
        self.at = end;
        Ok(frames)
    }

    fn len_hint(&self) -> Option<usize> {
        Some(self.bytes.len() / (2 * self.channels))
    }
}

/// Encode `source` to a FLAC stream.
fn flac(source: impl Source) -> std::result::Result<Vec<u8>, String> {
    let config = flacenc::config::Encoder::default().into_verified().map_err(|(_, e)| e.to_string())?;
    let mut stream =
        flacenc::encode_with_fixed_block_size(&config, source, config.block_size).map_err(|e| e.to_string())?;
    // lofty adds the tags after the last block but leaves STREAMINFO marked
    // last when it is alone
    stream.add_metadata_block(flacenc::component::MetadataBlockData::new_unknown(1, &[]).map_err(|e| e.to_string())?);
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|e| e.to_string())?;
    let mut bytes = sink.into_inner();
    // STREAMINFO's minimum block size leaves out the shorter last block, or
    // symphonia takes the stream for a variable-size one and loses sync
    bytes.copy_within(10..12, 8);
    Ok(bytes)
}

/// Triangular dither of one 16-bit step, to reduce to 16 bits without
/// distortion: a xorshift, no need for a better random.
struct Dither(u64);

impl Dither {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn tpdf(&mut self) -> f32 {
        self.next() - self.next()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lofty::prelude::*;
    use lofty::probe::Probe;

    const FLAC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sync/tone.flac");

    #[test]
    fn profile_round_trips() {
        for p in Profile::ALL {
            assert_eq!(p.to_string().parse(), Ok(p));
        }
        assert!("ogg-q5x".parse::<Profile>().is_err());
        assert_eq!(serde_json::to_string(&Profile::Vorbis(5)).unwrap(), "\"ogg-q5\"");
    }

    #[test]
    fn only_lossless_is_converted() {
        let dir = tempfile::tempdir().unwrap();
        assert!(needs(Path::new(FLAC), Profile::Vorbis(5)).is_some());
        assert!(needs(Path::new(FLAC), Profile::Off).is_none());
        // 16-bit at 44.1 kHz already
        assert!(needs(Path::new(FLAC), Profile::Flac16).is_none());
        let mp3 = dir.path().join("a.mp3");
        std::fs::write(&mp3, b"x").unwrap();
        assert!(needs(&mp3, Profile::Vorbis(5)).is_none());
    }

    /// Encode the 24-bit `wav` to FLAC as it is.
    pub(crate) fn flac24(wav: &Path, dst: &Path) {
        let (rate, samples) = decode(
            wav,
            &AtomicBool::new(false),
            |r, _| Ok((r, Vec::new())),
            |(_, samples), b| {
                for i in 0..b[0].len() {
                    samples.extend(b.iter().map(|c| (c[i] * 8_388_608.0) as i32));
                }
                Ok(())
            },
        )
        .unwrap();
        let source = flacenc::source::MemSource::from_samples(&samples, 2, 24, rate as usize);
        std::fs::write(dst, flac(source).unwrap()).unwrap();
    }

    #[test]
    fn reduces_hi_res_to_16_bits() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("hi.wav");
        crate::sync::tests::write_wav_bits(&wav, 96_000, 24, 0.5);
        let est = needs(&wav, Profile::Flac16).unwrap();
        assert!(est < fs::metadata(&wav).unwrap().len() / 3);
        let flac = dir.path().join("hi.flac");
        encode(&wav, &flac, Profile::Flac16, &AtomicBool::new(false)).unwrap();
        let file = Probe::open(&flac).unwrap().read().unwrap();
        assert_eq!(file.file_type(), lofty::file::FileType::Flac);
        let props = file.properties();
        assert_eq!((props.bit_depth(), props.sample_rate()), (Some(16), Some(RESAMPLED)));
        assert!(props.duration().as_millis().abs_diff(500) < 5);
        assert!(fs::metadata(&flac).unwrap().len() <= est);
        // and decodes back, to the frame
        let frames = decode(
            &flac,
            &AtomicBool::new(false),
            |_, _| Ok(0),
            |n, b| {
                *n += b[0].len();
                Ok(())
            },
        );
        assert_eq!(frames.unwrap(), 22_050);
    }

    #[test]
    fn keeps_tags_replaygain_and_art() {
        let dir = tempfile::tempdir().unwrap();
        for (p, kind) in
            [(Profile::Vorbis(5), lofty::file::FileType::Vorbis), (Profile::Flac16, lofty::file::FileType::Flac)]
        {
            let out = dir.path().join(format!("tone.{}", p.ext()));
            encode(Path::new(FLAC), &out, p, &AtomicBool::new(false)).unwrap();

            let file = Probe::open(&out).unwrap().read().unwrap();
            assert_eq!(file.file_type(), kind);
            let tag = file.primary_tag().unwrap();
            assert_eq!(tag.title().as_deref(), Some("Tone"));
            assert_eq!(tag.get_string(&ItemKey::ReplayGainTrackGain), Some("-3.50 dB"));
            assert_eq!(tag.picture_count(), 1, "{p}");
            assert!(file.properties().duration().as_millis().abs_diff(250) < 20);
        }
    }

    #[test]
    fn resamples_high_rates() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("hi.wav");
        crate::sync::tests::write_wav(&wav, 96_000, 0.5);
        let ogg = dir.path().join("hi.ogg");
        encode(&wav, &ogg, Profile::Vorbis(3), &AtomicBool::new(false)).unwrap();
        let file = Probe::open(&ogg).unwrap().read().unwrap();
        assert_eq!(file.properties().sample_rate(), Some(RESAMPLED));
        assert!(file.properties().duration().as_millis().abs_diff(500) < 5);
    }
}
