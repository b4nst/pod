//! Convert lossless music to Ogg Vorbis while syncing, to fit more of it on
//! the iPod. Tags, ReplayGain and cover art are carried over. Vorbis only:
//! pod plays it gapless, and an MP3 would need the LAME tag written by hand.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::num::{NonZeroU8, NonZeroU32};
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoder, VorbisEncoderBuilder};

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
}

impl Profile {
    pub const ALL: [Profile; 4] = [Profile::Off, Profile::Vorbis(3), Profile::Vorbis(5), Profile::Vorbis(7)];

    /// Average bitrate in kbit/s, to estimate sizes.
    pub fn kbps(self) -> u64 {
        match self {
            Profile::Off => 0,
            Profile::Vorbis(q) if q <= 3 => 112,
            Profile::Vorbis(q) if q <= 5 => 160,
            Profile::Vorbis(_) => 224,
        }
    }

    /// Estimated size of `len` of music.
    pub fn estimate(self, len: Duration) -> u64 {
        len.as_secs_f64().ceil() as u64 * self.kbps() * 125
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Profile::Off => f.write_str("off"),
            Profile::Vorbis(q) => write!(f, "ogg-q{q}"),
        }
    }
}

impl FromStr for Profile {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "off" => Ok(Profile::Off),
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

/// How long `src` plays when it is lossless, so worth converting, and mono or
/// stereo. `None` for lossy files and for anything the encoder can't take:
/// those are copied as they are.
pub fn needs(src: &Path) -> Option<Duration> {
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
    (props.channels()? <= 2).then(|| props.duration())
}

/// Encode the first `max` frames of `block` but for the `skip` first ones
/// still to drop. Returns how many were encoded.
fn write(
    encoder: &mut VorbisEncoder<File>,
    block: &[Vec<f32>],
    skip: &mut usize,
    max: u64,
) -> std::result::Result<u64, vorbis_rs::VorbisError> {
    let drop = (*skip).min(block[0].len());
    *skip -= drop;
    let n = ((block[0].len() - drop) as u64).min(max) as usize;
    if n > 0 {
        let cut: Vec<&[f32]> = block.iter().map(|c| &c[drop..drop + n]).collect();
        encoder.encode_audio_block(&cut)?;
    }
    Ok(n as u64)
}

/// Encode `src` to the Ogg Vorbis file `tmp` at quality `q`/10.
pub fn encode(src: &Path, tmp: &Path, q: u8, cancel: &AtomicBool) -> Result<()> {
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
    let mut encoder = VorbisEncoderBuilder::new(
        NonZeroU32::new(out_rate).unwrap(),
        NonZeroU8::new(channels as u8).unwrap(),
        File::create(tmp)?,
    )
    .map_err(|e| other(&e))?
    .bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr { target_quality: q as f32 / 10.0 })
    .build()
    .map_err(|e| other(&e))?;

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
                encoder.encode_audio_block(&pending).map_err(|e| other(&e))?;
                pending.iter_mut().for_each(Vec::clear);
            }
            Some(r) => {
                while pending[0].len() >= r.input_frames_next() {
                    let n = r.input_frames_next();
                    let chunk: Vec<&[f32]> = pending.iter().map(|c| &c[..n]).collect();
                    let out = r.process(&chunk, None).map_err(|e| other(&e))?;
                    out_frames += write(&mut encoder, &out, &mut skip, u64::MAX).map_err(|e| other(&e))?;
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
            out_frames += write(&mut encoder, &out, &mut skip, total - out_frames).map_err(|e| other(&e))?;
        }
    }
    encoder.finish().map_err(|e| other(&e))?.sync_all()?;

    // the tags go last, into the finished file; pod reads pictures from
    // METADATA_BLOCK_PICTURE comments
    let tagged = lofty::read_from_path(src).map_err(|e| other(&e))?;
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        let mut f = OpenOptions::new().read(true).write(true).open(tmp)?;
        VorbisComments::from(tag.clone()).save_to(&mut f, WriteOptions::default()).map_err(|e| other(&e))?;
        f.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
        assert!(needs(Path::new(FLAC)).is_some());
        let mp3 = dir.path().join("a.mp3");
        std::fs::write(&mp3, b"x").unwrap();
        assert!(needs(&mp3).is_none());
    }

    #[test]
    fn keeps_tags_replaygain_and_art() {
        let dir = tempfile::tempdir().unwrap();
        let ogg = dir.path().join("tone.ogg");
        encode(Path::new(FLAC), &ogg, 5, &AtomicBool::new(false)).unwrap();

        let file = Probe::open(&ogg).unwrap().read().unwrap();
        assert_eq!(file.file_type(), lofty::file::FileType::Vorbis);
        let tag = file.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("Tone"));
        assert_eq!(tag.get_string(&ItemKey::ReplayGainTrackGain), Some("-3.50 dB"));
        assert_eq!(tag.picture_count(), 1);
        assert!(file.properties().duration().as_millis().abs_diff(250) < 20);
    }

    #[test]
    fn resamples_high_rates() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("hi.wav");
        crate::sync::tests::write_wav(&wav, 96_000, 0.5);
        let ogg = dir.path().join("hi.ogg");
        encode(&wav, &ogg, 3, &AtomicBool::new(false)).unwrap();
        let file = Probe::open(&ogg).unwrap().read().unwrap();
        assert_eq!(file.properties().sample_rate(), Some(RESAMPLED));
        assert!(file.properties().duration().as_millis().abs_diff(500) < 5);
    }
}
