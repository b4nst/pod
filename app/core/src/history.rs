//! Keep an iPod's listening history on the desktop: the play log pod writes
//! to `/.pod/plays.log`, and the per-track stats (play count, rating) from its
//! database. Both live in a store per iPod, so the iPod can be wiped, or its
//! library rescanned, without losing them.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::tagcache::{self, TrackStats};
use crate::{Error, Result};

const HEADER: &str = "#AUDIOSCROBBLER/1.1\n#TZ/UNKNOWN\n#CLIENT/pod link\n";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Imported {
    /// New lines of the play log.
    pub plays: usize,
    /// Tracks whose stats were read from the iPod's database.
    pub tracks: usize,
}

/// Names the iPod: `/.pod/podlink-id`, made on first use.
pub fn device_id(mount: &Path) -> Result<String> {
    let file = mount.join(".pod/podlink-id");
    if let Ok(id) = fs::read_to_string(&file) {
        let id = id.trim();
        if !id.is_empty() {
            return Ok(id.to_string());
        }
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let seed = format!("{} {} {}", mount.display(), now.as_nanos(), std::process::id());
    let id: String = Sha256::digest(seed.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
    fs::write(&file, &id)?;
    Ok(id)
}

/// Where the stores are kept, one directory per iPod id.
pub fn stores_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("podlink/devices")
}

/// Appends the new lines of the iPod's play log to `store/plays.log`, then
/// empties the iPod's. Lines are matched whole, so an import that was cut
/// short can be run again. The timestamps are the iPod's local time and stay
/// as they are.
pub fn import_plays(mount: &Path, store: &Path) -> Result<usize> {
    let device = mount.join(".pod/plays.log");
    let text = match fs::read(&device) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    fs::create_dir_all(store)?;
    let kept = store.join("plays.log");
    let known = fs::read_to_string(&kept).unwrap_or_default();
    let mut seen: HashSet<&str> = known.lines().collect();
    let new: Vec<&str> =
        text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).filter(|l| seen.insert(l)).collect();
    if !new.is_empty() {
        let fresh = !kept.exists();
        let mut f = OpenOptions::new().create(true).append(true).open(&kept)?;
        if fresh {
            f.write_all(HEADER.as_bytes())?;
        }
        for l in &new {
            writeln!(f, "{l}")?;
        }
        f.sync_all()?;
    }
    // only now that the store has them; pod writes its header back
    File::create(&device)?;
    Ok(new.len())
}

/// Merges the stats in the iPod's database into `store/stats.json`, by path.
/// The record with the higher play count wins (a tie goes to the later play),
/// and none is ever dropped, so a wiped database can't erase the backup.
/// Returns how many tracks the iPod had stats for.
pub fn backup_stats(mount: &Path, store: &Path) -> Result<usize> {
    let read = tagcache::read(&mount.join(".pod"))?;
    let file = store.join("stats.json");
    let mut all: BTreeMap<String, TrackStats> = match fs::read(&file) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| Error::Other(format!("{}: {e}", file.display())))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => return Err(e.into()),
    };
    let tracks = read.len();
    for s in read.into_iter().filter(|s| !s.path.is_empty()) {
        match all.get(&s.path) {
            Some(old) if (old.playcount, old.lastplayed) > (s.playcount, s.lastplayed) => {}
            _ => {
                all.insert(s.path.clone(), s);
            }
        }
    }
    fs::create_dir_all(store)?;
    let tmp = store.join("stats.json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&all).map_err(|e| Error::Other(e.to_string()))?)?;
    fs::rename(&tmp, &file)?;
    Ok(tracks)
}

/// Imports the play log and backs up the stats of the iPod at `mount` into
/// its store under `base`. An iPod without `/.pod` has none.
pub fn import(mount: &Path, base: &Path) -> Result<Imported> {
    if !mount.join(".pod").is_dir() {
        return Ok(Imported::default());
    }
    let store = base.join(device_id(mount)?);
    let plays = import_plays(mount, &store)?;
    let tracks = backup_stats(mount, &store)?;
    Ok(Imported { plays, tracks })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An iPod with the fixture's play log and database.
    fn ipod() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let pod = dir.path().join(".pod");
        fs::create_dir(&pod).unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/history");
        for f in fs::read_dir(fixtures).unwrap() {
            let f = f.unwrap().path();
            fs::copy(&f, pod.join(f.file_name().unwrap())).unwrap();
        }
        dir
    }

    fn lines(file: &Path) -> Vec<String> {
        fs::read_to_string(file).unwrap().lines().filter(|l| !l.starts_with('#')).map(String::from).collect()
    }

    #[test]
    fn imports_the_plays_once() {
        let (dev, store) = (ipod(), tempfile::tempdir().unwrap());
        assert_eq!(import_plays(dev.path(), store.path()).unwrap(), 3);
        assert_eq!(lines(&store.path().join("plays.log")).len(), 3);
        assert_eq!(fs::metadata(dev.path().join(".pod/plays.log")).unwrap().len(), 0);
        assert_eq!(import_plays(dev.path(), store.path()).unwrap(), 0);
        assert_eq!(lines(&store.path().join("plays.log")).len(), 3);
    }

    #[test]
    fn a_replayed_import_adds_no_duplicates() {
        let (dev, store) = (ipod(), tempfile::tempdir().unwrap());
        let log = dev.path().join(".pod/plays.log");
        let saved = fs::read(&log).unwrap();
        import_plays(dev.path(), store.path()).unwrap();
        // cut short before the iPod's log was emptied
        fs::write(&log, saved).unwrap();
        assert_eq!(import_plays(dev.path(), store.path()).unwrap(), 0);
        assert_eq!(lines(&store.path().join("plays.log")).len(), 3);
    }

    #[test]
    fn the_backup_keeps_what_the_database_lost() {
        let (dev, store) = (ipod(), tempfile::tempdir().unwrap());
        let n = backup_stats(dev.path(), store.path()).unwrap();
        assert!(n >= 1);
        let before = fs::read(store.path().join("stats.json")).unwrap();
        for f in fs::read_dir(dev.path().join(".pod")).unwrap() {
            let f = f.unwrap().path();
            if f.extension().is_some_and(|e| e == "tcd") {
                fs::remove_file(f).unwrap();
            }
        }
        assert_eq!(backup_stats(dev.path(), store.path()).unwrap(), 0);
        assert_eq!(fs::read(store.path().join("stats.json")).unwrap(), before);
    }

    #[test]
    fn the_backup_keeps_the_higher_play_count() {
        let (dev, store) = (ipod(), tempfile::tempdir().unwrap());
        backup_stats(dev.path(), store.path()).unwrap();
        let file = store.path().join("stats.json");
        let mut all: BTreeMap<String, TrackStats> = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        all.values_mut().for_each(|s| s.playcount += 5);
        fs::write(&file, serde_json::to_vec(&all).unwrap()).unwrap();
        backup_stats(dev.path(), store.path()).unwrap();
        let now: BTreeMap<String, TrackStats> = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        assert_eq!(now, all);
    }

    #[test]
    fn the_id_is_kept() {
        let dev = ipod();
        let id = device_id(dev.path()).unwrap();
        assert_eq!(id.len(), 16);
        assert_eq!(device_id(dev.path()).unwrap(), id);
    }

    #[test]
    fn no_pod_dir_is_nothing_to_import() {
        let (dev, base) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        assert_eq!(import(dev.path(), base.path()).unwrap(), Imported::default());
    }

    #[test]
    fn imports_both() {
        let (dev, base) = (ipod(), tempfile::tempdir().unwrap());
        let got = import(dev.path(), base.path()).unwrap();
        assert_eq!(got.plays, 3);
        assert!(got.tracks >= 1);
    }
}
