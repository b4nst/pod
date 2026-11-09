//! The mixes pod link writes to `/Mixes` on every sync, built from the
//! listening history of the iPod (see `history`): Heavy Rotation, Forgotten
//! Gems, Never Played, Mornings and Nights. Each is a `.m3u8` of absolute
//! `/Music/...` paths, which is what the library's Mixes row plays.
//!
//! Loved (the rated tracks) follows the ratings in the history, #11.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::Result;
use crate::tagcache::{self, TrackStats};

/// Tracks per mix.
const CAP: usize = 100;
const DAY: i64 = 86_400;
const HEAVY_DAYS: i64 = 30;
const FORGOTTEN_DAYS: i64 = 180;
/// Plays a track needs to be forgotten, and in a part of the day.
const FORGOTTEN_PLAYS: usize = 5;
const DAYTIME_PLAYS: usize = 2;
/// Plays before a track can count as often skipped.
const SKIP_PLAYS: usize = 3;

pub const NAMES: [&str; 5] = ["Heavy Rotation", "Forgotten Gems", "Never Played", "Mornings", "Nights"];

/// A line of the play log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Play {
    pub artist: String,
    pub album: String,
    pub title: String,
    pub skipped: bool,
    /// Seconds since the Unix epoch, on the iPod's local time.
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mix {
    pub name: &'static str,
    pub paths: Vec<String>,
}

/// The plays in an Audioscrobbler 1.1 log: tab separated artist, album,
/// title, track number, length, `L` or `S` and the time. Other lines are
/// skipped.
pub fn parse_plays(text: &str) -> Vec<Play> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let skipped = match *f.get(5)? {
                "L" => false,
                "S" => true,
                _ => return None,
            };
            Some(Play {
                artist: f[0].into(),
                album: f[1].into(),
                title: f[2].into(),
                skipped,
                at: f.get(6)?.trim().parse().ok()?,
            })
        })
        .collect()
}

/// What the log says about one track.
#[derive(Default)]
struct Log {
    listened: Vec<i64>,
    skips: usize,
}

impl Log {
    fn often_skipped(&self) -> bool {
        let plays = self.listened.len() + self.skips;
        plays >= SKIP_PLAYS && self.skips * 2 > plays
    }

    fn last(&self) -> Option<i64> {
        self.listened.iter().copied().max()
    }
}

/// The firmware logs an empty album when there is none, the database has
/// `<Untagged>`.
fn key(artist: &str, album: &str, title: &str) -> (String, String, String) {
    let album = if album.is_empty() { "<untagged>" } else { album };
    (artist.to_lowercase(), album.to_lowercase(), title.to_lowercase())
}

/// Tracks by count, most first, then by path.
fn top(mut ranked: Vec<(usize, &str)>) -> Vec<String> {
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    ranked.into_iter().take(CAP).map(|(_, p)| p.to_string()).collect()
}

/// The mixes for `tracks` (every track of the iPod), the stats backup by path
/// and the plays, at time `now`. Plays that match no track are dropped.
pub fn build(tracks: &[TrackStats], backup: &BTreeMap<String, TrackStats>, plays: &[Play], now: i64) -> Vec<Mix> {
    // the log has the album artist when a track has no artist
    let mut by_name: HashMap<_, _> = HashMap::new();
    for t in tracks {
        by_name.entry(key(&t.albumartist, &t.album, &t.title)).or_insert(t.path.as_str());
    }
    for t in tracks {
        by_name.insert(key(&t.artist, &t.album, &t.title), t.path.as_str());
    }
    let mut logs: HashMap<&str, Log> = HashMap::new();
    for p in plays {
        let Some(&path) = by_name.get(&key(&p.artist, &p.album, &p.title)) else { continue };
        let log = logs.entry(path).or_default();
        if p.skipped {
            log.skips += 1;
        } else {
            log.listened.push(p.at);
        }
    }

    let none = Log::default();
    let mut heavy = Vec::new();
    let mut forgotten = Vec::new();
    let mut never = Vec::new();
    let mut mornings = Vec::new();
    let mut nights = Vec::new();
    for t in tracks {
        let log = logs.get(t.path.as_str()).unwrap_or(&none);
        if log.often_skipped() {
            continue;
        }
        let path = t.path.as_str();
        let old = backup.get(path);
        let recent = log.listened.iter().filter(|&&at| at >= now.saturating_sub(HEAVY_DAYS * DAY)).count();
        if recent > 0 {
            heavy.push((recent, path));
        }
        let count = log.listened.len().max(old.map_or(0, |o| o.playcount.max(0) as usize));
        // the database's lastplayed is a serial, so only the log has times
        if count >= FORGOTTEN_PLAYS && log.last().is_some_and(|at| at < now.saturating_sub(FORGOTTEN_DAYS * DAY)) {
            forgotten.push((count, path));
        }
        if log.listened.is_empty() && t.playcount <= 0 && old.is_none_or(|o| o.playcount <= 0) {
            never.push(path);
        }
        let hours = |keep: fn(i64) -> bool| log.listened.iter().filter(|&&at| keep(at.rem_euclid(DAY) / 3600)).count();
        let (morning, night) = (hours(|h| (5..=10).contains(&h)), hours(|h| h >= 21 || h <= 3));
        if morning >= DAYTIME_PLAYS {
            mornings.push((morning, path));
        }
        if night >= DAYTIME_PLAYS {
            nights.push((night, path));
        }
    }

    // a different pick every day, instead of always starting with artist A
    let day = now.div_euclid(DAY);
    let mut never: Vec<(Vec<u8>, &str)> =
        never.into_iter().map(|p| (Sha256::digest(format!("{p}{day}")).to_vec(), p)).collect();
    never.sort();
    let never = never.into_iter().take(CAP).map(|(_, p)| p.to_string()).collect();

    let paths = [top(heavy), top(forgotten), never, top(mornings), top(nights)];
    NAMES.into_iter().zip(paths).map(|(name, paths)| Mix { name, paths }).collect()
}

/// Writes the mixes of the iPod at `mount` to `/Mixes`, from its store, and
/// returns how many. A mix with no tracks is deleted; other files in `/Mixes`
/// stay. An iPod without `/.pod` or a store has none. `renames` are the files
/// sync just replaced: the database still knows them by their old path.
pub fn write(mount: &Path, store: &Path, now: i64, renames: &[(String, String)]) -> Result<usize> {
    let pod = mount.join(".pod");
    if !pod.is_dir() || !store.is_dir() {
        return Ok(0);
    }
    let plays = parse_plays(&fs::read_to_string(store.join("plays.log")).unwrap_or_default());
    let backup = match fs::read(store.join("stats.json")) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| crate::Error::Other(format!("stats.json: {e}")))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => return Err(e.into()),
    };
    // Mirror may have just deleted some, and new copies aren't in the database yet
    let mut tracks = tagcache::tracks(&pod)?;
    for t in &mut tracks {
        if let Some((_, new)) = renames.iter().find(|(old, _)| *old == t.path) {
            t.path = new.clone();
        }
    }
    tracks.retain(|t| mount.join(t.path.trim_start_matches('/')).is_file());

    let dir = mount.join("Mixes");
    let mut written = 0;
    for mix in build(&tracks, &backup, &plays, now) {
        let file = dir.join(format!("{}.m3u8", mix.name));
        if mix.paths.is_empty() {
            match fs::remove_file(&file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => continue,
            }
        }
        fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!("{}.m3u8.tmp", mix.name));
        fs::write(&tmp, mix.paths.iter().map(|p| format!("{p}\n")).collect::<String>())?;
        fs::rename(&tmp, &file)?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000 / DAY * DAY + 12 * 3600;

    fn track(n: u32) -> TrackStats {
        TrackStats {
            path: format!("/Music/t{n:03}.mp3"),
            artist: "A".into(),
            albumartist: String::new(),
            album: "B".into(),
            title: format!("T{n:03}"),
            length: 0,
            playcount: 0,
            rating: 0,
            playtime: 0,
            lastplayed: 0,
        }
    }

    fn play(n: u32, skipped: bool, at: i64) -> Play {
        Play { artist: "a".into(), album: "B".into(), title: format!("t{n:03}"), skipped, at }
    }

    fn mix<'a>(mixes: &'a [Mix], name: &str) -> &'a [String] {
        &mixes.iter().find(|m| m.name == name).unwrap().paths
    }

    fn path(n: u32) -> String {
        track(n).path
    }

    #[test]
    fn parses_the_log() {
        let text = "#AUDIOSCROBBLER/1.1\nA\tB\tT\t1\t20\tL\t100\t\nA\tB\tU\t2\t20\tS\t200\t\nbad\n";
        let plays = parse_plays(text);
        assert_eq!(plays.len(), 2);
        assert_eq!((plays[0].title.as_str(), plays[0].skipped, plays[0].at), ("T", false, 100));
        assert!(plays[1].skipped);
    }

    #[test]
    fn heavy_rotation_is_the_last_30_days() {
        let tracks = [track(1), track(2), track(3)];
        let plays = [
            play(1, false, NOW - DAY),
            play(2, false, NOW - DAY),
            play(2, false, NOW - 2 * DAY),
            play(3, false, NOW - 31 * DAY),
        ];
        let mixes = build(&tracks, &BTreeMap::new(), &plays, NOW);
        assert_eq!(mix(&mixes, "Heavy Rotation"), [path(2), path(1)]);
    }

    #[test]
    fn forgotten_gems_were_played_a_lot_long_ago() {
        let tracks = [track(1), track(2), track(3), track(4), track(5)];
        let long_ago = NOW - 200 * DAY;
        let mut backup = BTreeMap::new();
        backup.insert(path(1), TrackStats { playcount: 8, ..track(1) });
        backup.insert(path(2), TrackStats { playcount: 4, ..track(2) });
        backup.insert(path(3), TrackStats { playcount: 9, ..track(3) });
        backup.insert(path(5), TrackStats { playcount: 9, ..track(5) });
        let mut plays: Vec<Play> = (0..5).map(|_| play(4, false, long_ago)).collect();
        plays.extend([play(1, false, long_ago), play(2, false, long_ago), play(3, false, NOW - 10 * DAY)]);
        let mixes = build(&tracks, &backup, &plays, NOW);
        // 5 has no play in the log: the database has no time to tell
        assert_eq!(mix(&mixes, "Forgotten Gems"), [path(1), path(4)]);
    }

    #[test]
    fn plays_of_untagged_tracks_match() {
        let tracks =
            [TrackStats { artist: String::new(), albumartist: "Z".into(), album: "<Untagged>".into(), ..track(1) }];
        let plays = [Play { artist: "Z".into(), album: String::new(), title: "T001".into(), skipped: false, at: NOW }];
        let mixes = build(&tracks, &BTreeMap::new(), &plays, NOW);
        assert_eq!(mix(&mixes, "Heavy Rotation"), [path(1)]);
    }

    #[test]
    fn never_played_leaves_out_what_the_backup_knows() {
        let tracks = [track(1), track(2), track(3)];
        let mut backup = BTreeMap::new();
        backup.insert(path(2), TrackStats { playcount: 1, ..track(2) });
        let plays = [play(3, false, NOW)];
        let mixes = build(&tracks, &backup, &plays, NOW);
        assert_eq!(mix(&mixes, "Never Played"), [path(1)]);
    }

    #[test]
    fn never_played_changes_every_day() {
        let tracks: Vec<_> = (0..20).map(track).collect();
        let (a, b) = (build(&tracks, &BTreeMap::new(), &[], NOW), build(&tracks, &BTreeMap::new(), &[], NOW + DAY));
        assert_eq!(a, build(&tracks, &BTreeMap::new(), &[], NOW));
        assert_ne!(mix(&a, "Never Played"), mix(&b, "Never Played"));
    }

    #[test]
    fn mornings_and_nights() {
        let tracks = [track(1), track(2)];
        let day = NOW - 12 * 3600;
        let plays = [
            play(1, false, day + 6 * 3600),
            play(1, false, day - DAY + 9 * 3600),
            play(2, false, day + 23 * 3600),
            play(2, false, day + 2 * 3600),
        ];
        let mixes = build(&tracks, &BTreeMap::new(), &plays, NOW);
        assert_eq!(mix(&mixes, "Mornings"), [path(1)]);
        assert_eq!(mix(&mixes, "Nights"), [path(2)]);
    }

    #[test]
    fn an_often_skipped_track_is_in_no_mix() {
        let tracks = [track(1), track(2)];
        let plays = [
            play(1, true, NOW),
            play(1, true, NOW),
            play(1, false, NOW),
            play(2, true, NOW),
            play(2, false, NOW),
            play(2, false, NOW),
        ];
        let mixes = build(&tracks, &BTreeMap::new(), &plays, NOW);
        assert_eq!(mix(&mixes, "Heavy Rotation"), [path(2)]);
        assert!(!mixes.iter().any(|m| m.paths.contains(&path(1))));
    }

    #[test]
    fn a_mix_is_capped() {
        let tracks: Vec<_> = (0..150).map(track).collect();
        let mixes = build(&tracks, &BTreeMap::new(), &[], NOW);
        assert_eq!(mix(&mixes, "Never Played").len(), CAP);
    }

    #[test]
    fn odd_times_dont_panic() {
        let tracks = [track(1)];
        let plays = [play(1, false, i64::MIN), play(1, false, i64::MAX), play(1, false, -5)];
        build(&tracks, &BTreeMap::new(), &plays, i64::MAX);
        build(&tracks, &BTreeMap::new(), &plays, i64::MIN);
    }

    /// An iPod with the fixture's database, and `files` of its tracks on disk.
    fn ipod(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let pod = dir.path().join(".pod");
        fs::create_dir(&pod).unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/history");
        for f in fs::read_dir(fixtures).unwrap() {
            let f = f.unwrap().path();
            fs::copy(&f, pod.join(f.file_name().unwrap())).unwrap();
        }
        for f in files {
            let f = dir.path().join(f.trim_start_matches('/'));
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, b"x").unwrap();
        }
        dir
    }

    fn lines(file: &Path) -> Vec<String> {
        fs::read_to_string(file).unwrap().lines().map(String::from).collect()
    }

    #[test]
    fn a_replaced_track_stays_in_its_mix() {
        let all = tagcache::tracks(&ipod(&[]).path().join(".pod")).unwrap();
        let paths: Vec<&str> = all.iter().map(|t| t.path.as_str()).collect();
        // every track is on the iPod, but the first as an .ogg now
        let (dev, store) = (ipod(&paths[1..]), tempfile::tempdir().unwrap());
        let new = "/Music/replaced.ogg";
        fs::write(dev.path().join(new.trim_start_matches('/')), b"x").unwrap();
        let renames = [(paths[0].to_string(), new.to_string())];
        write(dev.path(), store.path(), NOW, &renames).unwrap();
        let never = lines(&dev.path().join("Mixes/Never Played.m3u8"));
        assert!(never.contains(&new.to_string()) && !never.contains(&paths[0].to_string()));
    }

    #[test]
    fn writes_the_mixes() {
        let all = tagcache::tracks(&ipod(&[]).path().join(".pod")).unwrap();
        let paths: Vec<&str> = all.iter().map(|t| t.path.as_str()).collect();
        let (dev, store) = (ipod(&paths[1..]), tempfile::tempdir().unwrap());
        fs::create_dir(dev.path().join("Mixes")).unwrap();
        fs::write(dev.path().join("Mixes/Mine.m3u8"), "/Music/mine.mp3\n").unwrap();
        fs::write(dev.path().join("Mixes/Nights.m3u8"), "/Music/old.mp3\n").unwrap();

        assert_eq!(write(dev.path(), store.path(), NOW, &[]).unwrap(), 1);
        let never = lines(&dev.path().join("Mixes/Never Played.m3u8"));
        assert!(!never.is_empty() && never.iter().all(|p| p.starts_with("/Music/")));
        assert!(!never.contains(&paths[0].to_string()), "a missing file is skipped");
        assert!(!dev.path().join("Mixes/Nights.m3u8").exists(), "an empty mix is deleted");
        assert!(!dev.path().join("Mixes/Heavy Rotation.m3u8").exists());
        assert_eq!(lines(&dev.path().join("Mixes/Mine.m3u8")), ["/Music/mine.mp3"]);
        assert!(!dev.path().join("Mixes/Never Played.m3u8.tmp").exists());
    }

    #[test]
    fn no_store_is_no_mixes() {
        let dev = ipod(&[]);
        assert_eq!(write(dev.path(), &dev.path().join("nope"), NOW, &[]).unwrap(), 0);
        assert!(!dev.path().join("Mixes").exists());
        let bare = tempfile::tempdir().unwrap();
        assert_eq!(write(bare.path(), bare.path(), NOW, &[]).unwrap(), 0);
    }
}
