//! Sync a music folder into `/Music`. pod builds its database from the files
//! at boot, so syncing is copying: no iTunes database to write.
//!
//! A file is up to date when size and modification time match; FAT keeps
//! times to 2 s. Copies get the source's mtime so the next sync agrees.
//!
//! With a transcode profile, lossless files are converted to Ogg Vorbis
//! instead. They can't be compared by size, so `/.pod/sync.json` records
//! where each one came from.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use filetime::FileTime;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::transcode::{self, Profile};
use crate::{Error, Progress, Result, human_bytes};

/// What pod plays, plus album art and playlists.
const MEDIA: &[&str] = &[
    "mp3", "m4a", "m4b", "aac", "mp4", "alac", "flac", "ogg", "oga", "opus", "wav", "aif", "aiff", "wv", "ape", "jpg",
    "jpeg", "png", "bmp", "m3u", "m3u8",
];

const IMAGES_AND_LISTS: &[&str] = &["jpg", "jpeg", "png", "bmp", "m3u", "m3u8"];

/// Room left free for pod's database and settings.
const RESERVE: u64 = 16 * 1000 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Copy new and changed files, delete nothing.
    #[default]
    Update,
    /// Also delete music on the iPod that is not in the folder.
    Mirror,
}

/// A converted file on the iPod, and the source it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Relative to the synced folder.
    pub src: String,
    pub size: u64,
    /// Seconds since the epoch.
    pub mtime: i64,
    pub profile: Profile,
}

/// `/.pod/sync.json`: what was converted, by path relative to `/Music`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    /// The profile of the last sync.
    pub profile: Profile,
    pub files: BTreeMap<String, Entry>,
}

impl Manifest {
    fn file(mount: &Path) -> PathBuf {
        mount.join(".pod/sync.json")
    }

    /// The manifest of the iPod at `mount`; empty when there is none, an error
    /// when it can't be read: starting over would convert everything again.
    pub fn load(mount: &Path) -> Result<Manifest> {
        match fs::read(Self::file(mount)) {
            Ok(b) => {
                serde_json::from_slice(&b).map_err(|e| Error::Other(format!("{}: {e}", Self::file(mount).display())))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn save(&self, mount: &Path) -> Result<()> {
        if self.profile == Profile::Off && self.files.is_empty() && !Self::file(mount).exists() {
            return Ok(());
        }
        let file = Self::file(mount);
        fs::create_dir_all(file.parent().unwrap())?;
        let tmp = file.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(|e| Error::Other(e.to_string()))?)?;
        fs::rename(&tmp, &file)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct CopyOp {
    pub src: PathBuf,
    /// Relative to the synced folder, with `/`.
    pub rel: String,
    /// Relative to `/Music`.
    pub dst: PathBuf,
    /// Bytes to copy, or the estimate of the converted file.
    pub size: u64,
    /// Size of the files it replaces.
    pub replaces: u64,
    /// Converts the file instead of copying it.
    pub transcode: Option<Profile>,
    /// The other form of the file (converted or not), deleted once `dst` is
    /// written, so changing the profile leaves no duplicate.
    pub old: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub copies: Vec<CopyOp>,
    /// Relative to `/Music`, with their sizes.
    pub deletes: Vec<(PathBuf, u64)>,
    pub unchanged: usize,
    pub profile: Profile,
    manifest: Manifest,
}

impl Plan {
    /// Files that will be converted.
    pub fn converted(&self) -> usize {
        self.copies.iter().filter(|c| c.transcode.is_some()).count()
    }

    pub fn copy_bytes(&self) -> u64 {
        self.copies.iter().map(|c| c.size).sum()
    }

    /// Bytes the sync adds to the disk (negative when it frees space).
    pub fn growth(&self) -> i64 {
        let freed: u64 =
            self.copies.iter().map(|c| c.replaces).sum::<u64>() + self.deletes.iter().map(|d| d.1).sum::<u64>();
        self.copy_bytes() as i64 - freed as i64
    }

    pub fn is_empty(&self) -> bool {
        self.copies.is_empty() && self.deletes.is_empty()
    }

    pub fn check_space(&self, free: u64) -> Result<()> {
        let need = self.growth().max(0) as u64;
        if need + RESERVE > free {
            return Err(Error::NoSpace { need: human_bytes(need), free: human_bytes(free) });
        }
        Ok(())
    }
}

fn is_media(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| MEDIA.contains(&e.to_ascii_lowercase().as_str()))
}

/// A media file that pod plays: not a picture or a playlist.
pub(crate) fn is_audio(p: &Path) -> bool {
    is_media(p)
        && !p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| IMAGES_AND_LISTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Finder litter and other hidden files.
pub(crate) fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// A path component FAT32 accepts: no `<>:"/\|?*` or control characters, no
/// trailing dots or spaces, no DOS device names, at most 255 UTF-16 units.
pub fn sanitize(name: &str) -> String {
    let mut s: String =
        name.chars().map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).collect();
    while s.ends_with(['.', ' ']) {
        s.pop();
    }
    if s.is_empty() {
        s.push('_');
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        s.insert(0, '_');
    }
    while s.encode_utf16().count() > 255 {
        // keep the extension
        let (stem, ext) = match s.rfind('.') {
            Some(i) if s.len() - i <= 6 => (s[..i].to_string(), s[i..].to_string()),
            _ => (s.clone(), String::new()),
        };
        let mut stem = stem;
        stem.pop();
        s = stem + &ext;
    }
    s
}

fn key(p: &Path) -> String {
    p.to_string_lossy().to_lowercase().replace('\\', "/")
}

/// `Song.mp3` -> `Song (2).mp3`
fn numbered(p: &Path, n: usize) -> PathBuf {
    let stem = p.file_stem().unwrap_or_default().to_string_lossy();
    let name = match p.extension() {
        Some(e) => format!("{stem} ({n}).{}", e.to_string_lossy()),
        None => format!("{stem} ({n})"),
    };
    p.with_file_name(name)
}

fn mtime(m: &fs::Metadata) -> SystemTime {
    m.modified().unwrap_or(SystemTime::UNIX_EPOCH)
}

fn secs(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// A path with `/`, whatever the host uses.
fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// `Artist/Song.mp3` as `/Music/Artist/Song.mp3`, the way pod stores paths.
fn device_path(rel: &Path) -> String {
    format!("/Music/{}", slash(rel))
}

fn same_time(a: SystemTime, b: SystemTime) -> bool {
    let d = match a.duration_since(b) {
        Ok(d) => d,
        Err(e) => e.duration(),
    };
    d.as_secs() <= 2
}

/// Compare `src` with `<mount>/Music`.
pub fn plan(src: &Path, mount: &Path, mode: Mode, profile: Profile, cancel: &AtomicBool) -> Result<Plan> {
    let music = mount.join("Music");
    let mut plan = Plan { profile, ..Default::default() };
    let mut taken: HashSet<String> = HashSet::new();
    // the other form of a file that is on the iPod, kept out of MIRROR's deletes
    let mut olds: HashSet<String> = HashSet::new();

    let mut manifest = Manifest::load(mount)?;
    manifest.files.retain(|d, _| music.join(d).is_file());
    let converted: HashMap<&str, &str> = manifest.files.iter().map(|(d, e)| (e.src.as_str(), d.as_str())).collect();

    let mut entries: Vec<_> = WalkDir::new(src)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(&e.file_name().to_string_lossy()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && is_media(e.path()))
        .collect();
    entries.sort_by(|a, b| a.path().cmp(b.path()));

    for e in entries {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let rel = e.path().strip_prefix(src).unwrap();
        let rel_s = slash(rel);
        let plain: PathBuf = rel.iter().map(|c| sanitize(&c.to_string_lossy())).collect();
        let length = if profile == Profile::Off { None } else { transcode::needs(e.path()) };
        let mut dst = if length.is_some() { plain.with_extension("ogg") } else { plain.clone() };
        // FAT is case insensitive: two names that differ in case collide
        let mut n = 2;
        let base = dst.clone();
        while taken.contains(&key(&dst)) {
            dst = numbered(&base, n);
            n += 1;
        }
        taken.insert(key(&dst));

        let meta = e.metadata().map_err(|e| Error::Other(e.to_string()))?;
        let dst_s = slash(&dst);
        let old = match (length, plain.extension()) {
            (Some(_), Some(ext)) => Some(dst.with_extension(ext)),
            (Some(_), None) => None,
            (None, _) => converted.get(rel_s.as_str()).filter(|d| **d != dst_s).map(PathBuf::from),
        };
        let old = old.and_then(|o| Some((music.join(&o).metadata().ok()?.len(), o)));
        if let Some((_, o)) = &old {
            olds.insert(key(o));
        }

        let current = fs::metadata(music.join(&dst)).ok();
        let up_to_date = current.as_ref().is_some_and(|d| match length {
            Some(_) => {
                manifest.files.get(&dst_s)
                    == Some(&Entry { src: rel_s.clone(), size: meta.len(), mtime: secs(mtime(&meta)), profile })
            }
            None => d.len() == meta.len() && same_time(mtime(d), mtime(&meta)),
        });
        if up_to_date {
            plan.unchanged += 1;
            if let Some((len, o)) = old {
                plan.deletes.push((o, len));
            }
            continue;
        }
        plan.copies.push(CopyOp {
            src: e.path().into(),
            rel: rel_s,
            dst,
            size: length.map_or(meta.len(), |l| profile.estimate(l)),
            replaces: current.map_or(0, |d| d.len()) + old.as_ref().map_or(0, |o| o.0),
            transcode: length.map(|_| profile),
            old: old.map(|o| o.1),
        });
    }

    if mode == Mode::Mirror && music.is_dir() {
        for e in WalkDir::new(&music).into_iter().filter_map(|e| e.ok()) {
            if !e.file_type().is_file() {
                continue;
            }
            let rel = e.path().strip_prefix(&music).unwrap();
            let name = e.file_name().to_string_lossy();
            // `._x.mp3` AppleDouble files go too: they confuse the scanner
            if (is_media(rel) || name.starts_with("._")) && !taken.contains(&key(rel)) && !olds.contains(&key(rel)) {
                plan.deletes.push((rel.to_path_buf(), e.metadata().map(|m| m.len()).unwrap_or(0)));
            }
        }
    }
    plan.manifest = manifest;
    Ok(plan)
}

/// Carry out `plan`: deletions first to make room, then the copies, then the
/// conversions. Returns the files that were replaced by another form of
/// themselves, as `/Music/...` paths from old to new, along with the outcome:
/// the renames are those done so far when it fails or is cancelled. The
/// manifest is saved in any case.
pub fn apply(
    plan: &Plan,
    mount: &Path,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> (Vec<(String, String)>, Result<()>) {
    let mut manifest = plan.manifest.clone();
    manifest.profile = plan.profile;
    let mut renames = Vec::new();
    let res = run(plan, mount, &mut manifest, &mut renames, progress, cancel);
    let saved = manifest.save(mount);
    (renames, res.and(saved))
}

fn run(
    plan: &Plan,
    mount: &Path,
    manifest: &mut Manifest,
    renames: &mut Vec<(String, String)>,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<()> {
    let music = mount.join("Music");
    for (rel, _) in &plan.deletes {
        fs::remove_file(music.join(rel))?;
        manifest.files.remove(&slash(rel));
    }
    if !plan.deletes.is_empty() {
        remove_empty_dirs(&music);
    }

    let total = plan.copy_bytes();
    let mut done = 0;
    let mut buf = vec![0; 1024 * 1024];
    let mut dirs: HashSet<PathBuf> = HashSet::new();
    for c in plan.copies.iter().filter(|c| c.transcode.is_none()) {
        let dst = music.join(&c.dst);
        let parent = dst.parent().unwrap().to_path_buf();
        if dirs.insert(parent.clone()) {
            fs::create_dir_all(&parent)?;
        }
        let tmp = tmp_name(&dst);
        let item = c.dst.to_string_lossy().into_owned();
        // plain read/write: fs::copy on macOS carries xattrs, which FAT
        // stores as `._` files
        let res = (|| -> Result<()> {
            let mut from = File::open(&c.src)?;
            let mut to = File::create(&tmp)?;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                let n = from.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                to.write_all(&buf[..n])?;
                done += n as u64;
                progress(Progress { done, total, item: item.clone(), ..Default::default() });
            }
            to.sync_all()?;
            Ok(())
        })();
        if let Err(e) = res {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        fs::rename(&tmp, &dst)?;
        let m = fs::metadata(&c.src)?;
        filetime::set_file_mtime(&dst, FileTime::from_last_modification_time(&m))?;
        manifest.files.remove(&slash(&c.dst));
        replaced(c, &music, manifest, renames)?;
    }

    let jobs: Vec<&CopyOp> = plan.copies.iter().filter(|c| c.transcode.is_some()).collect();
    for c in &jobs {
        let parent = music.join(&c.dst).parent().unwrap().to_path_buf();
        if dirs.insert(parent.clone()) {
            fs::create_dir_all(&parent)?;
        }
    }
    // the workers watch `stop`: it is set by a cancel, or by the first failure
    let stop = AtomicBool::new(cancel.load(Ordering::Relaxed));
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism().map_or(2, |n| n.get()).min(jobs.len());
    let mut failed = (!jobs.is_empty() && stop.load(Ordering::Relaxed)).then_some(Error::Cancelled);
    thread::scope(|s| {
        let (tx, rx) = mpsc::channel();
        for _ in 0..workers {
            let (tx, stop, next, jobs, music) = (tx.clone(), &stop, &next, &jobs, &music);
            s.spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(c) = jobs.get(i) else { break };
                    let res = convert(c, music, stop);
                    if tx.send((i, res)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok((i, Ok(()))) => {
                    let c = jobs[i];
                    done += c.size;
                    progress(Progress {
                        done,
                        total,
                        item: c.dst.to_string_lossy().into_owned(),
                        ..Default::default()
                    });
                    let m = fs::metadata(&c.src);
                    let entry = m.map(|m| Entry {
                        src: c.rel.clone(),
                        size: m.len(),
                        mtime: secs(mtime(&m)),
                        profile: c.transcode.unwrap_or_default(),
                    });
                    match entry {
                        Ok(entry) => {
                            manifest.files.insert(slash(&c.dst), entry);
                            if let Err(e) = replaced(c, &music, manifest, renames) {
                                failed.get_or_insert(e);
                            }
                        }
                        Err(e) => {
                            failed.get_or_insert(e.into());
                        }
                    }
                }
                Ok((_, Err(e))) => {
                    stop.store(true, Ordering::Relaxed);
                    // a worker that stops because of the first error says "cancelled"
                    if failed.is_none() || !matches!(e, Error::Cancelled) {
                        failed = Some(e);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if cancel.load(Ordering::Relaxed) {
                        stop.store(true, Ordering::Relaxed);
                        failed.get_or_insert(Error::Cancelled);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    failed.map_or(Ok(()), Err)
}

fn tmp_name(dst: &Path) -> PathBuf {
    dst.with_file_name(format!(".{}.podlink", dst.file_name().unwrap().to_string_lossy()))
}

/// Convert one file, through a temporary file that is renamed when done.
fn convert(c: &CopyOp, music: &Path, stop: &AtomicBool) -> Result<()> {
    let dst = music.join(&c.dst);
    let tmp = tmp_name(&dst);
    let Some(Profile::Vorbis(q)) = c.transcode else { unreachable!("not a conversion") };
    if let Err(e) = transcode::encode(&c.src, &tmp, q, stop) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    fs::rename(&tmp, &dst)?;
    let m = fs::metadata(&c.src)?;
    filetime::set_file_mtime(&dst, FileTime::from_last_modification_time(&m))?;
    Ok(())
}

/// `c` is written: delete the other form of the file.
fn replaced(c: &CopyOp, music: &Path, manifest: &mut Manifest, renames: &mut Vec<(String, String)>) -> Result<()> {
    if let Some(old) = &c.old {
        fs::remove_file(music.join(old))?;
        manifest.files.remove(&slash(old));
        renames.push((device_path(old), device_path(&c.dst)));
    }
    Ok(())
}

fn remove_empty_dirs(root: &Path) {
    for e in WalkDir::new(root).contents_first(true).min_depth(1).into_iter().filter_map(|e| e.ok()) {
        if e.file_type().is_dir() {
            let _ = fs::remove_dir(e.path()); // fails unless empty
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn put(p: &Path, data: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }

    fn no() -> AtomicBool {
        AtomicBool::new(false)
    }

    /// A silent stereo 16 bit WAV.
    pub(crate) fn write_wav(p: &Path, rate: u32, secs: f32) {
        let data = (rate as f32 * secs) as u32 * 4;
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend((36 + data).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend([1, 0, 2, 0]);
        b.extend(rate.to_le_bytes());
        b.extend((rate * 4).to_le_bytes());
        b.extend([4, 0, 16, 0]);
        b.extend(b"data");
        b.extend(data.to_le_bytes());
        b.extend((0..data / 4).flat_map(|i| {
            let s = ((i as f32 * 0.05).sin() * 8000.0) as i16;
            [s, s / 2].into_iter().flat_map(i16::to_le_bytes)
        }));
        put(p, &b);
    }

    #[test]
    fn sanitizes_for_fat() {
        assert_eq!(sanitize("AC/DC: Live?"), "AC_DC_ Live_");
        assert_eq!(sanitize("Vol. 1..."), "Vol. 1");
        assert_eq!(sanitize("con.mp3"), "_con.mp3");
        assert_eq!(sanitize("COM1"), "_COM1");
        assert_eq!(sanitize("Combo.mp3"), "Combo.mp3");
        let long = format!("{}.flac", "é".repeat(300));
        let s = sanitize(&long);
        assert!(s.encode_utf16().count() <= 255 && s.ends_with(".flac"));
    }

    #[test]
    fn copies_then_skips_then_mirrors() {
        let dir = tempfile::tempdir().unwrap();
        let (src, ipod) = (dir.path().join("src"), dir.path().join("ipod"));
        put(&src.join("Artist/Album/01 Song.mp3"), b"aaaa");
        put(&src.join("Artist/Album/cover.jpg"), b"jpg");
        put(&src.join("Artist/Album/notes.txt"), b"skip me");
        put(&src.join(".hidden/x.mp3"), b"skip me");
        put(&src.join("Odd: Name/a.flac"), b"flac");
        put(&ipod.join("Music/Old/gone.mp3"), b"old");

        let p = plan(&src, &ipod, Mode::Update, Profile::Off, &no()).unwrap();
        assert_eq!(p.copies.len(), 3);
        assert!(p.deletes.is_empty());
        apply(&p, &ipod, &mut |_| {}, &no()).1.unwrap();
        assert_eq!(fs::read(ipod.join("Music/Odd_ Name/a.flac")).unwrap(), b"flac");
        assert!(!ipod.join("Music/Artist/Album/notes.txt").exists());

        let p = plan(&src, &ipod, Mode::Update, Profile::Off, &no()).unwrap();
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(p.unchanged, 3);

        put(&src.join("Artist/Album/01 Song.mp3"), b"changed!");
        let p = plan(&src, &ipod, Mode::Mirror, Profile::Off, &no()).unwrap();
        assert_eq!(p.copies.len(), 1);
        assert_eq!(p.copies[0].replaces, 4);
        assert_eq!(p.deletes.len(), 1);
        apply(&p, &ipod, &mut |_| {}, &no()).1.unwrap();
        assert!(!ipod.join("Music/Old").exists(), "empty folders are removed");
        assert_eq!(fs::read(ipod.join("Music/Artist/Album/01 Song.mp3")).unwrap(), b"changed!");
    }

    #[test]
    fn names_that_differ_in_case_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        put(&src.join("a/Song.mp3"), b"1");
        put(&src.join("a/song.MP3"), b"2");
        let p = plan(&src, &dir.path().join("ipod"), Mode::Update, Profile::Off, &no()).unwrap();
        let mut names: Vec<_> = p.copies.iter().map(|c| key(&c.dst)).collect();
        names.sort();
        names.dedup();
        // on a case-insensitive host the folder holds one file only
        assert_eq!(names.len(), p.copies.len());
    }

    #[test]
    fn converts_then_follows_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let (src, ipod) = (dir.path().join("src"), dir.path().join("ipod"));
        write_wav(&src.join("A/one.wav"), 48_000, 1.0);
        put(&src.join("A/two.mp3"), b"mp3");
        let q5 = Profile::Vorbis(5);

        let p = plan(&src, &ipod, Mode::Update, q5, &no()).unwrap();
        assert_eq!(p.copies.len(), 2);
        let c = p.copies.iter().find(|c| c.rel == "A/one.wav").unwrap();
        assert_eq!(c.dst, Path::new("A/one.ogg"));
        assert!(c.size > 0 && c.transcode == Some(q5));
        assert_eq!(p.converted(), 1);
        let (renames, res) = apply(&p, &ipod, &mut |_| {}, &no());
        res.unwrap();
        assert_eq!(renames, []);
        assert_eq!(fs::read(ipod.join("Music/A/two.mp3")).unwrap(), b"mp3");
        let ogg = fs::read(ipod.join("Music/A/one.ogg")).unwrap();
        assert_eq!(&ogg[..4], b"OggS");
        assert!(
            fs::read_dir(ipod.join("Music/A")).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".podlink"))
        );
        assert!(Manifest::load(&ipod).unwrap().files.contains_key("A/one.ogg"));

        let p = plan(&src, &ipod, Mode::Update, q5, &no()).unwrap();
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(p.unchanged, 2);

        // back to AS IS: the .wav comes back and the .ogg goes
        let p = plan(&src, &ipod, Mode::Update, Profile::Off, &no()).unwrap();
        assert_eq!(p.copies.len(), 1);
        assert_eq!(p.copies[0].old.as_deref(), Some(Path::new("A/one.ogg")));
        assert_eq!(p.copies[0].replaces, ogg.len() as u64);
        let (renames, res) = apply(&p, &ipod, &mut |_| {}, &no());
        res.unwrap();
        assert_eq!(renames, [("/Music/A/one.ogg".to_string(), "/Music/A/one.wav".to_string())]);
        assert!(!ipod.join("Music/A/one.ogg").exists() && ipod.join("Music/A/one.wav").exists());
        assert!(Manifest::load(&ipod).unwrap().files.is_empty());

        // and to Vorbis again: the .wav goes
        let p = plan(&src, &ipod, Mode::Update, q5, &no()).unwrap();
        assert_eq!(p.copies[0].old.as_deref(), Some(Path::new("A/one.wav")));
        apply(&p, &ipod, &mut |_| {}, &no()).1.unwrap();
        assert!(ipod.join("Music/A/one.ogg").exists() && !ipod.join("Music/A/one.wav").exists());

        // a new quality re-encodes in place
        let p = plan(&src, &ipod, Mode::Update, Profile::Vorbis(7), &no()).unwrap();
        assert_eq!(p.copies.len(), 1);
        assert!(p.copies[0].old.is_none());
    }

    #[test]
    fn cancel_leaves_no_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let (src, ipod) = (dir.path().join("src"), dir.path().join("ipod"));
        write_wav(&src.join("one.wav"), 44_100, 1.0);
        let p = plan(&src, &ipod, Mode::Update, Profile::Vorbis(5), &no()).unwrap();
        assert!(matches!(apply(&p, &ipod, &mut |_| {}, &AtomicBool::new(true)).1, Err(Error::Cancelled)));
        let left: Vec<_> = fs::read_dir(ipod.join("Music")).unwrap().collect();
        assert!(left.is_empty(), "{left:?}");
        assert_eq!(Manifest::load(&ipod).unwrap().profile, Profile::Vorbis(5));
    }

    #[test]
    fn checks_space() {
        let p = Plan { copies: vec![CopyOp { size: 50_000_000, ..Default::default() }], ..Default::default() };
        assert!(p.check_space(10_000_000).is_err());
        assert!(p.check_space(100_000_000).is_ok());
    }
}

/// Against a real FAT volume: POD_FAT_MOUNT=/Volumes/X POD_FAT_SRC=dir
#[cfg(test)]
mod fat {
    use super::*;

    #[test]
    fn syncs_to_fat_and_then_is_up_to_date() {
        let (Ok(mount), Ok(src)) = (std::env::var("POD_FAT_MOUNT"), std::env::var("POD_FAT_SRC")) else { return };
        let (mount, src) = (PathBuf::from(mount), PathBuf::from(src));
        let no = AtomicBool::new(false);
        let p = plan(&src, &mount, Mode::Mirror, Profile::Off, &no).unwrap();
        apply(&p, &mount, &mut |_| {}, &no).1.unwrap();
        let again = plan(&src, &mount, Mode::Mirror, Profile::Off, &no).unwrap();
        assert!(again.is_empty(), "{again:?}");
        assert!(again.unchanged > 0);
    }
}
