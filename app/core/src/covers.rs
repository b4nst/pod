//! The covers pod link writes to `/.pod/covers.bin` on every sync: a large and
//! a small square cover of the art of each album on the iPod, ready for the
//! 2 bpp LCD, keyed by the hash the deck groups its tracks by. An album without art gets
//! no record and the deck draws its generated card.
//!
//! File (little endian): `PODC`, `u16` version, `u16` large w/h and small w/h,
//! `u32` count, `count` sorted `u32` hashes, then large and small of each
//! record, in index order. `/.pod/covers.json` remembers where each album's
//! art came from, so only the albums whose source changed are decoded again.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use filetime::FileTime;
use image::DynamicImage;
use image::imageops::{self, FilterType};
use lofty::file::{TaggedFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::prelude::ItemKey;
use lofty::tag::Tag;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::sync::{is_audio, is_hidden};
use crate::{Error, Progress, Result};

const LARGE: (usize, usize) = (44, 44);
const SMALL: (usize, usize) = (28, 28);
const VERSION: u16 = 2;
/// Bytes of a large and a small cover: rows of 4 pixels per byte.
const LARGE_BYTES: usize = LARGE.0.div_ceil(4) * LARGE.1;
const RECORD: usize = LARGE_BYTES + SMALL.0.div_ceil(4) * SMALL.1;
const HEADER: usize = 18;
/// The STN panel crushes darks: lift the mid-tones before dithering.
const GAMMA: f32 = 1.8;
const UNTAGGED: &str = "<Untagged>";
const FOLDER_ART: [&str; 3] = ["cover.jpg", "folder.jpg", "front.jpg"];

/// The album key of the deck, a port of `hash_tags` in the deck: FNV-1a over
/// the album, a zero separator and the artist, each byte through ASCII `tolower`.
pub fn album_hash(album: &str, artist: &str) -> u32 {
    let mut h = 2166136261u32;
    for b in album.bytes() {
        h = (h ^ b.to_ascii_lowercase() as u32).wrapping_mul(16777619);
    }
    h = h.wrapping_mul(16777619);
    for b in artist.bytes() {
        h = (h ^ b.to_ascii_lowercase() as u32).wrapping_mul(16777619);
    }
    h
}

/// The album and artist the database has for a track: `canonicalartist` is
/// the track artist, or the album artist when there is none.
fn album_key(tag: Option<&Tag>) -> (String, String) {
    let text = |keys: &[ItemKey]| {
        let tag = tag?;
        keys.iter().find_map(|k| tag.get_string(k).filter(|s| !s.is_empty())).map(str::to_string)
    };
    (
        text(&[ItemKey::AlbumTitle]).unwrap_or_else(|| UNTAGGED.into()),
        text(&[ItemKey::TrackArtist, ItemKey::AlbumArtist]).unwrap_or_else(|| UNTAGGED.into()),
    )
}

fn tag_of(file: &TaggedFile) -> Option<&Tag> {
    file.primary_tag().or_else(|| file.first_tag())
}

/// The front cover of a tag, else its first picture.
fn picture(tag: &Tag) -> Option<&[u8]> {
    let pics = tag.pictures();
    pics.iter().find(|p| p.pic_type() == PictureType::CoverFront).or(pics.first()).map(|p| p.data())
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// `ink` (one 0-3 value per pixel, 3 = `POD_INK`) as the LCD has it: 4 pixels
/// to a byte, the leftmost in the high bits, rows padded to a byte.
fn pack(ink: &[u8], w: usize, h: usize) -> Vec<u8> {
    let stride = w.div_ceil(4);
    let mut out = vec![0; stride * h];
    for y in 0..h {
        for x in 0..w {
            out[y * stride + x / 4] |= (ink[y * w + x] & 3) << (6 - 2 * (x & 3));
        }
    }
    out
}

/// `img` cropped to a square around its centre, resized to `side` and dithered
/// to the 4 inks, packed.
fn render(img: &DynamicImage, side: usize) -> Vec<u8> {
    let (w, h) = (side, side);
    let (iw, ih) = (img.width() as usize, img.height() as usize);
    let c = iw.min(ih).max(1);
    let crop = img.crop_imm(((iw - c) / 2) as u32, ((ih - c) / 2) as u32, c as u32, c as u32);
    let small = imageops::resize(&crop.to_luma32f(), w as u32, h as u32, FilterType::Lanczos3);
    let mut v: Vec<f32> = small.pixels().map(|p| p.0[0].clamp(0.0, 1.0).powf(1.0 / GAMMA) * 3.0).collect();

    // Atkinson: an eighth of the error to each of 6 neighbours
    let mut ink = vec![0; w * h];
    for y in 0..h {
        for x in 0..w {
            let q = v[y * w + x].round().clamp(0.0, 3.0);
            ink[y * w + x] = 3 - q as u8;
            let e = (v[y * w + x] - q) / 8.0;
            for (dx, dy) in [(1, 0), (2, 0), (-1, 1), (0, 1), (1, 1), (0, 2)] {
                let (nx, ny) = (x as i32 + dx, y + dy);
                if nx >= 0 && (nx as usize) < w && ny < h {
                    v[ny * w + nx as usize] += e;
                }
            }
        }
    }
    pack(&ink, w, h)
}

#[derive(Serialize, Deserialize, Clone)]
struct Track {
    size: u64,
    mtime: i64,
    album: String,
    artist: String,
    /// sha256 of the picture the tags carry
    picture: Option<String>,
}

/// Where the art of an album came from.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
enum Source {
    Picture(String),
    File { path: String, size: u64, mtime: i64 },
}

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    version: u16,
    /// by path under the mount
    tracks: BTreeMap<String, Track>,
    albums: BTreeMap<u32, Source>,
}

impl Cache {
    fn load(path: &Path) -> Cache {
        let cache: Cache = fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if cache.version == VERSION { cache } else { Cache::default() }
    }
}

/// The records of a `covers.bin` by hash; none when it isn't a file of this
/// version and size.
fn read_records(path: &Path) -> BTreeMap<u32, Vec<u8>> {
    let Ok(b) = fs::read(path) else { return BTreeMap::new() };
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let sizes = [VERSION, LARGE.0 as u16, LARGE.1 as u16, SMALL.0 as u16, SMALL.1 as u16];
    if b.len() < HEADER || &b[..4] != b"PODC" || (0..5).any(|i| u16_at(4 + 2 * i) != sizes[i]) {
        return BTreeMap::new();
    }
    let n = u32::from_le_bytes([b[14], b[15], b[16], b[17]]) as usize;
    if b.len() != HEADER + n * (4 + RECORD) {
        return BTreeMap::new();
    }
    (0..n)
        .map(|i| {
            let at = HEADER + 4 * i;
            let hash = u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
            let rec = HEADER + 4 * n + i * RECORD;
            (hash, b[rec..rec + RECORD].to_vec())
        })
        .collect()
}

/// The image of an album folder: `cover.jpg`, `folder.jpg` or `front.jpg`, else
/// the only picture there is.
fn folder_art(dir: &Path) -> Option<PathBuf> {
    let files: Vec<(String, PathBuf)> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| (e.file_name().to_string_lossy().to_ascii_lowercase(), e.path()))
        .collect();
    let named = FOLDER_ART.iter().find_map(|n| files.iter().find(|(f, _)| f == n));
    let images: Vec<_> = files
        .iter()
        .filter(|(f, _)| matches!(f.rsplit_once('.'), Some((_, "jpg" | "jpeg" | "png"))) && !f.starts_with('.'))
        .collect();
    named.or(if let [only] = images[..] { Some(only) } else { None }).map(|(_, p)| p.clone())
}

fn stamp(path: &Path) -> Option<(u64, i64)> {
    let md = fs::metadata(path).ok()?;
    Some((md.len(), FileTime::from_last_modification_time(&md).unix_seconds()))
}

/// What a `write` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// albums found on the iPod
    pub albums: usize,
    /// albums with a record in `covers.bin`
    pub covers: usize,
    /// images decoded: the rest came from the old `covers.bin`
    pub decoded: usize,
}

/// An album being collected: the tracks of one key, in path order.
struct Album {
    key: (String, String),
    /// path under the mount and picture hash of each track
    tracks: Vec<(String, Option<String>)>,
}

/// Writes `/.pod/covers.bin` for the albums under `/Music` of the iPod at
/// `mount`. A cancel or an error leaves the old files in place.
pub fn write(mount: &Path, progress: &mut dyn FnMut(Progress), cancel: &AtomicBool) -> Result<Report> {
    let pod = mount.join(".pod");
    let (bin_path, cache_path) = (pod.join("covers.bin"), pod.join("covers.json"));
    let old = Cache::load(&cache_path);
    let old_records = read_records(&bin_path);

    // the tags of every track, read again only when the file changed
    let mut tracks = BTreeMap::new();
    let files: Vec<_> = WalkDir::new(mount.join("Music"))
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(&e.file_name().to_string_lossy()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_audio(e.path()))
        .collect();
    for (done, e) in files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let rel = format!("/{}", e.path().strip_prefix(mount).unwrap_or(e.path()).to_string_lossy().replace('\\', "/"));
        progress(Progress { done: done as u64, total: files.len() as u64, item: rel.clone(), step: "COVERS" });
        let Some((size, mtime)) = stamp(e.path()) else { continue };
        let track = match old.tracks.get(&rel) {
            Some(t) if t.size == size && t.mtime == mtime => t.clone(),
            _ => {
                let file = lofty::read_from_path(e.path()).ok();
                let tag = file.as_ref().and_then(tag_of);
                let (album, artist) = album_key(tag);
                let picture = tag.and_then(picture).map(sha256_hex);
                Track { size, mtime, album, artist, picture }
            }
        };
        tracks.insert(rel, track);
    }

    // the first key seen owns a hash, a track with another key is ignored
    let mut albums: BTreeMap<u32, Album> = BTreeMap::new();
    for (path, t) in &tracks {
        let key = (t.album.clone(), t.artist.clone());
        let a = albums.entry(album_hash(&key.0, &key.1)).or_insert_with(|| Album { key: key.clone(), tracks: vec![] });
        if a.key == key {
            a.tracks.push((path.clone(), t.picture.clone()));
        }
    }

    let total = albums.len() as u64;
    let mut report = Report { albums: albums.len(), ..Report::default() };
    let mut records: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    let mut sources = BTreeMap::new();
    for (done, (hash, album)) in albums.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        progress(Progress { done: done as u64, total, item: album.key.0.clone(), step: "COVERS" });
        let abs = |rel: &str| mount.join(rel.trim_start_matches('/'));
        // the art: a picture of the first track that has one, else a file of its folder
        let (source, track) = match album.tracks.iter().find_map(|(p, s)| Some((p, Source::Picture(s.clone()?)))) {
            Some((p, source)) => (source, Some(p)),
            None => {
                let Some(file) = folder_art(abs(&album.tracks[0].0).parent().unwrap_or(mount)) else { continue };
                let Some((size, mtime)) = stamp(&file) else { continue };
                let path = file.strip_prefix(mount).unwrap_or(&file).to_string_lossy().replace('\\', "/");
                (Source::File { path: format!("/{path}"), size, mtime }, None)
            }
        };
        let reused = old_records.get(hash).filter(|_| old.albums.get(hash) == Some(&source));
        let record = match reused {
            Some(r) => r.clone(),
            None => {
                let bytes = match (&source, track) {
                    (Source::File { path, .. }, _) => fs::read(abs(path)).ok(),
                    (_, Some(p)) => lofty::read_from_path(abs(p))
                        .ok()
                        .and_then(|f| tag_of(&f).and_then(picture).map(<[u8]>::to_vec)),
                    _ => None,
                };
                let Some(img) = bytes.and_then(|b| image::load_from_memory(&b).ok()) else { continue };
                report.decoded += 1;
                [render(&img, LARGE.0), render(&img, SMALL.0)].concat()
            }
        };
        records.insert(*hash, record);
        sources.insert(*hash, source);
    }
    report.covers = records.len();
    progress(Progress { done: total, total, item: String::new(), step: "COVERS" });

    let mut bin = b"PODC".to_vec();
    for v in [VERSION, LARGE.0 as u16, LARGE.1 as u16, SMALL.0 as u16, SMALL.1 as u16] {
        bin.extend(v.to_le_bytes());
    }
    bin.extend((records.len() as u32).to_le_bytes());
    bin.extend(records.keys().flat_map(|h| h.to_le_bytes()));
    bin.extend(records.values().flatten());
    replace(&bin_path, &bin)?;
    // after the bin: a stale cache only makes the next sync redo some albums
    let cache = Cache { version: VERSION, tracks, albums: sources };
    replace(&cache_path, &serde_json::to_vec(&cache).map_err(|e| Error::Other(format!("covers.json: {e}")))?)?;
    Ok(report)
}

/// Writes `data` to `path` through a temp file and a rename.
fn replace(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let r = fs::write(&tmp, data).and_then(|_| fs::rename(&tmp, path));
    if r.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(r?)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{ImageFormat, Rgb, RgbImage};
    use lofty::config::WriteOptions;
    use lofty::picture::{MimeType, Picture};
    use lofty::tag::{Accessor, TagExt, TagType};
    use tempfile::tempdir;

    use super::*;
    use crate::sync::tests::write_wav;

    fn no() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn run(mount: &Path) -> Report {
        write(mount, &mut |_| {}, &no()).unwrap()
    }

    /// A 20×20 image, a gradient of `shade`.
    fn art(format: ImageFormat, shade: u8) -> Vec<u8> {
        let img = RgbImage::from_fn(20, 20, |x, _| Rgb([x as u8 * 6 + shade; 3]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    /// A WAV in `dir` tagged `album` by `artist`, with `picture` embedded.
    fn track(dir: &Path, name: &str, album: &str, artist: &str, picture: Option<Vec<u8>>) {
        let p = dir.join(name);
        fs::create_dir_all(dir).unwrap();
        write_wav(&p, 8000, 0.1);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_album(album.into());
        tag.set_artist(artist.into());
        if let Some(data) = picture {
            tag.push_picture(Picture::new_unchecked(PictureType::CoverFront, Some(MimeType::Png), None, data));
        }
        tag.save_to_path(&p, WriteOptions::default()).unwrap();
    }

    fn pod(mount: &Path) {
        fs::create_dir_all(mount.join(".pod")).unwrap();
    }

    fn index(bin: &[u8]) -> Vec<u32> {
        let n = u32::from_le_bytes(bin[14..18].try_into().unwrap()) as usize;
        (0..n).map(|i| u32::from_le_bytes(bin[18 + 4 * i..22 + 4 * i].try_into().unwrap())).collect()
    }

    #[test]
    fn hashes_like_the_deck() {
        // from hash_tags in the deck, compiled on its own
        assert_eq!(album_hash("Abbey Road", "The Beatles"), 0xf87adaab);
        assert_eq!(album_hash("<Untagged>", "<Untagged>"), 0x5b6e5b89);
        assert_eq!(album_hash("ABBEY ROAD", "the beatles"), album_hash("Abbey Road", "The Beatles"));
    }

    #[test]
    fn key_follows_canonical_artist() {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_album("A".into());
        tag.insert_text(ItemKey::AlbumArtist, "Band".into());
        assert_eq!(album_key(Some(&tag)), ("A".into(), "Band".into()));
        tag.set_artist("Guest".into());
        assert_eq!(album_key(Some(&tag)), ("A".into(), "Guest".into()));
        assert_eq!(album_key(None), (UNTAGGED.into(), UNTAGGED.into()));
    }

    #[test]
    fn packs_the_leftmost_pixel_high() {
        assert_eq!(pack(&[0, 1, 2, 3], 4, 1), [0x1B]);
        assert_eq!(pack(&[3; 5], 5, 1), [0xFF, 0xC0]);
    }

    #[test]
    fn crops_to_the_centre() {
        // black left half, white right half
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(100, 100, |x, _| Rgb([if x < 50 { 0 } else { 255 }; 3])));
        let bytes = render(&img, LARGE.0);
        assert_eq!(bytes.len(), LARGE_BYTES);
        let ink = |x: usize, y: usize| bytes[y * LARGE.0.div_ceil(4) + x / 4] >> (6 - 2 * (x & 3)) & 3;
        assert!((0..LARGE.1).all(|y| ink(2, y) == 3 && ink(41, y) == 0));
    }

    #[test]
    fn crops_a_wide_image_to_a_square() {
        // black sides, white in the middle square: the sides are gone
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(200, 100, |x, _| {
            Rgb([if (50..150).contains(&x) { 255 } else { 0 }; 3])
        }));
        assert!(render(&img, LARGE.0).iter().all(|&b| b == 0));
        assert!(render(&img, SMALL.0).iter().all(|&b| b == 0));
    }

    #[test]
    fn crops_a_tall_image_to_a_square() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(100, 200, |_, y| {
            Rgb([if (50..150).contains(&y) { 255 } else { 0 }; 3])
        }));
        assert!(render(&img, LARGE.0).iter().all(|&b| b == 0));
        assert!(render(&img, SMALL.0).iter().all(|&b| b == 0));
    }

    #[test]
    fn writes_the_format() {
        let dir = tempdir().unwrap();
        let m = dir.path();
        pod(m);
        track(&m.join("Music/A"), "1.wav", "One", "Band", Some(art(ImageFormat::Png, 0)));
        track(&m.join("Music/B"), "1.wav", "Two", "Band", Some(art(ImageFormat::Png, 40)));
        let r = run(m);
        assert_eq!(r, Report { albums: 2, covers: 2, decoded: 2 });
        let bin = fs::read(m.join(".pod/covers.bin")).unwrap();
        assert_eq!(RECORD, 680);
        assert_eq!(bin.len(), 18 + 4 * 2 + RECORD * 2);
        assert_eq!(&bin[..18], b"PODC\x02\x00\x2c\x00\x2c\x00\x1c\x00\x1c\x00\x02\x00\x00\x00");
        let idx = index(&bin);
        assert!(idx[0] < idx[1]);
        // the records follow the index
        let one = idx.iter().position(|&h| h == album_hash("One", "Band")).unwrap();
        let img = image::load_from_memory(&art(ImageFormat::Png, 0)).unwrap();
        let want = [render(&img, LARGE.0), render(&img, SMALL.0)].concat();
        assert_eq!(bin[18 + 8 + one * RECORD..][..RECORD], want[..]);
    }

    #[test]
    fn finds_folder_art_and_skips_bare_albums() {
        let dir = tempdir().unwrap();
        let m = dir.path();
        pod(m);
        track(&m.join("Music/A"), "1.wav", "Folder", "Band", None);
        fs::write(m.join("Music/A/Cover.JPG"), art(ImageFormat::Jpeg, 0)).unwrap();
        // two pictures and none named for it
        track(&m.join("Music/B"), "1.wav", "Many", "Band", None);
        fs::write(m.join("Music/B/a.png"), art(ImageFormat::Png, 0)).unwrap();
        fs::write(m.join("Music/B/b.png"), art(ImageFormat::Png, 0)).unwrap();
        track(&m.join("Music/C"), "1.wav", "Only", "Band", None);
        fs::write(m.join("Music/C/scan.png"), art(ImageFormat::Png, 0)).unwrap();
        track(&m.join("Music/D"), "1.wav", "Bare", "Band", None);
        let r = run(m);
        assert_eq!(r, Report { albums: 4, covers: 2, decoded: 2 });
        let idx = index(&fs::read(m.join(".pod/covers.bin")).unwrap());
        assert!(idx.contains(&album_hash("Folder", "Band")) && idx.contains(&album_hash("Only", "Band")));
    }

    #[test]
    fn a_collision_keeps_the_first_album() {
        let dir = tempdir().unwrap();
        let m = dir.path();
        pod(m);
        // the same hash, as it folds case, but another key
        track(&m.join("Music/A"), "1.wav", "X", "Y", Some(art(ImageFormat::Png, 0)));
        track(&m.join("Music/B"), "1.wav", "x", "y", Some(art(ImageFormat::Png, 90)));
        assert_eq!(run(m), Report { albums: 1, covers: 1, decoded: 1 });
    }

    #[test]
    fn a_resync_decodes_nothing() {
        let dir = tempdir().unwrap();
        let m = dir.path();
        pod(m);
        track(&m.join("Music/A"), "1.wav", "Emb", "Band", Some(art(ImageFormat::Png, 0)));
        track(&m.join("Music/B"), "1.wav", "Fol", "Band", None);
        let cover = m.join("Music/B/cover.jpg");
        fs::write(&cover, art(ImageFormat::Jpeg, 0)).unwrap();
        assert_eq!(run(m).decoded, 2);
        let bin = fs::read(m.join(".pod/covers.bin")).unwrap();
        assert_eq!(run(m), Report { albums: 2, covers: 2, decoded: 0 });
        assert_eq!(fs::read(m.join(".pod/covers.bin")).unwrap(), bin);

        filetime::set_file_mtime(&cover, FileTime::from_unix_time(1_700_000_000, 0)).unwrap();
        assert_eq!(run(m).decoded, 1);
    }

    #[test]
    fn a_resync_rewrites_a_v1_file() {
        let dir = tempdir().unwrap();
        let m = dir.path();
        pod(m);
        track(&m.join("Music/A"), "1.wav", "Emb", "Band", Some(art(ImageFormat::Png, 0)));
        run(m);
        // what the last version left: its header and cache
        let mut v1 = b"PODC\x01\x00\x26\x00\x3b\x00\x0c\x00\x13\x00\x01\x00\x00\x00".to_vec();
        v1.extend(album_hash("Emb", "Band").to_le_bytes());
        v1.extend([0; 647]);
        fs::write(m.join(".pod/covers.bin"), v1).unwrap();
        let json = fs::read(m.join(".pod/covers.json")).unwrap();
        let mut cache: serde_json::Value = serde_json::from_slice(&json).unwrap();
        cache["version"] = 1.into();
        fs::write(m.join(".pod/covers.json"), serde_json::to_vec(&cache).unwrap()).unwrap();

        assert_eq!(run(m).decoded, 1);
        let bin = fs::read(m.join(".pod/covers.bin")).unwrap();
        assert_eq!(&bin[4..6], b"\x02\x00");
        assert_eq!(bin.len(), 18 + 4 + RECORD);
    }

    #[test]
    fn a_cancel_keeps_the_old_file() {
        let dir = tempdir().unwrap();
        let m = dir.path();
        pod(m);
        track(&m.join("Music/A"), "1.wav", "One", "Band", Some(art(ImageFormat::Png, 0)));
        run(m);
        let bin = fs::read(m.join(".pod/covers.bin")).unwrap();
        track(&m.join("Music/B"), "1.wav", "Two", "Band", Some(art(ImageFormat::Png, 40)));
        let cancel = AtomicBool::new(false);
        let r = write(m, &mut |_| cancel.store(true, Ordering::Relaxed), &cancel);
        assert!(matches!(r, Err(Error::Cancelled)));
        assert_eq!(fs::read(m.join(".pod/covers.bin")).unwrap(), bin);
        assert!(!m.join(".pod/covers.tmp").exists());
    }
}
