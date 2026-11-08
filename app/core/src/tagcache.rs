//! Read the per-track play stats out of pod's tagcache files
//! (`/.pod/database_*.tcd`), which is where Rockbox keeps play counts,
//! ratings and last played times.
//!
//! The layout is `struct index_entry` and `struct tagfile_entry` in
//! `apps/tagcache.c`. `database_idx.tcd` is a 24 byte `master_header` and
//! then one 96 byte entry per track: 23 `i32` `tag_seek` and an `i32` flag.
//! For a string tag, `tag_seek` is the offset of the value in
//! `database_<tag>.tcd`, a `{i32 len, i32 idx_id, len bytes}` record (the
//! length counts the NUL). For a number it is the number itself. All values
//! are little endian, as written by the iPod's ARM.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// `TAGCACHE_MAGIC`, the header version of the files.
const MAGIC: i32 = 0x5443_4810;
const HEADER: usize = 24;
const TAG_COUNT: usize = 23;
const ENTRY: usize = (TAG_COUNT + 1) * 4;

const FLAG_DELETED: i32 = 0x1;
const FLAG_DIRTYNUM: i32 = 0x4;

// `enum tag_type`
const ARTIST: usize = 0;
const ALBUM: usize = 1;
const TITLE: usize = 3;
const FILENAME: usize = 4;
const LENGTH: usize = 14;
const PLAYCOUNT: usize = 15;
const RATING: usize = 16;
const PLAYTIME: usize = 17;
const LASTPLAYED: usize = 18;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackStats {
    pub path: String,
    pub artist: String,
    pub album: String,
    pub title: String,
    /// Milliseconds.
    pub length: i32,
    pub playcount: i32,
    pub rating: i32,
    /// Milliseconds played in total.
    pub playtime: i32,
    /// Seconds since the Unix epoch, on the iPod's clock.
    pub lastplayed: i32,
}

fn le32(b: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The string at `seek` in a tag file.
fn string(file: &[u8], seek: i32) -> String {
    let at = usize::try_from(seek).unwrap_or(usize::MAX);
    let Some(len) = le32(file, at).and_then(|n| usize::try_from(n).ok()) else { return String::new() };
    let data = file.get(at + 8..).and_then(|d| d.get(..len)).unwrap_or_default();
    String::from_utf8_lossy(data.split(|&b| b == 0).next().unwrap_or_default()).into_owned()
}

/// The stats of every track that has some. A missing database gives none.
pub fn read(pod: &Path) -> Result<Vec<TrackStats>> {
    let idx = match fs::read(pod.join("database_idx.tcd")) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    if le32(&idx, 0) != Some(MAGIC) {
        return Err(Error::Unsupported("unknown database version".into()));
    }
    let count = le32(&idx, 8).and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
    let tag = |n: usize| fs::read(pod.join(format!("database_{n}.tcd"))).map_err(Error::from);
    let (artists, albums, titles, files) = (tag(ARTIST)?, tag(ALBUM)?, tag(TITLE)?, tag(FILENAME)?);

    let mut out = Vec::new();
    let (entries, _) = idx.get(HEADER..).unwrap_or_default().as_chunks::<ENTRY>();
    for e in entries.iter().take(count) {
        let seek = |n: usize| le32(e, n * 4).unwrap_or(0);
        let flag = seek(TAG_COUNT);
        if flag & FLAG_DELETED != 0 || flag & FLAG_DIRTYNUM == 0 {
            continue;
        }
        out.push(TrackStats {
            path: string(&files, seek(FILENAME)),
            artist: string(&artists, seek(ARTIST)),
            album: string(&albums, seek(ALBUM)),
            title: string(&titles, seek(TITLE)),
            length: seek(LENGTH),
            playcount: seek(PLAYCOUNT),
            rating: seek(RATING),
            playtime: seek(PLAYTIME),
            lastplayed: seek(LASTPLAYED),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/history")
    }

    #[test]
    fn reads_the_played_tracks() {
        let stats = read(&fixture()).unwrap();
        let carrier = stats.iter().find(|s| s.title == "Carrier").expect("Carrier has stats");
        assert!(carrier.playcount >= 1);
        assert!(carrier.path.starts_with("/Music/"), "{}", carrier.path);
        assert_eq!(carrier.artist, "Unit Δelta");
        assert_eq!(carrier.album, "Signal / Noise");
        assert!(carrier.length > 0);
    }

    #[test]
    fn no_database_is_no_stats() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn unknown_version_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("database_idx.tcd"), [0u8; HEADER]).unwrap();
        assert!(matches!(read(dir.path()), Err(Error::Unsupported(_))));
    }
}
