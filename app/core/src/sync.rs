//! Sync a music folder into `/Music`. pod builds its database from the files
//! at boot, so syncing is copying: no iTunes database to write.
//!
//! A file is up to date when size and modification time match; FAT keeps
//! times to 2 s. Copies get the source's mtime so the next sync agrees.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use filetime::FileTime;
use walkdir::WalkDir;

use crate::{Error, Progress, Result, human_bytes};

/// What pod plays, plus album art and playlists.
const MEDIA: &[&str] = &[
    "mp3", "m4a", "m4b", "aac", "mp4", "alac", "flac", "ogg", "oga", "opus", "wav", "aif", "aiff", "wv", "ape", "jpg",
    "jpeg", "png", "bmp", "m3u", "m3u8",
];

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

#[derive(Debug, Clone)]
pub struct CopyOp {
    pub src: PathBuf,
    /// Relative to `/Music`.
    pub dst: PathBuf,
    pub size: u64,
    /// Size of the file it replaces.
    pub replaces: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub copies: Vec<CopyOp>,
    /// Relative to `/Music`, with their sizes.
    pub deletes: Vec<(PathBuf, u64)>,
    pub unchanged: usize,
}

impl Plan {
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

/// Finder litter and other hidden files.
fn is_hidden(name: &str) -> bool {
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

fn same_time(a: SystemTime, b: SystemTime) -> bool {
    let d = match a.duration_since(b) {
        Ok(d) => d,
        Err(e) => e.duration(),
    };
    d.as_secs() <= 2
}

/// Compare `src` with `<mount>/Music`.
pub fn plan(src: &Path, mount: &Path, mode: Mode, cancel: &AtomicBool) -> Result<Plan> {
    let music = mount.join("Music");
    let mut plan = Plan::default();
    let mut taken: HashSet<String> = HashSet::new();

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
        let mut dst: PathBuf = rel.iter().map(|c| sanitize(&c.to_string_lossy())).collect();
        // FAT is case insensitive: two names that differ in case collide
        let mut n = 2;
        let base = dst.clone();
        while taken.contains(&key(&dst)) {
            dst = numbered(&base, n);
            n += 1;
        }
        taken.insert(key(&dst));

        let meta = e.metadata().map_err(|e| Error::Other(e.to_string()))?;
        match fs::metadata(music.join(&dst)) {
            Ok(d) if d.len() == meta.len() && same_time(mtime(&d), mtime(&meta)) => plan.unchanged += 1,
            Ok(d) => plan.copies.push(CopyOp { src: e.path().into(), dst, size: meta.len(), replaces: d.len() }),
            Err(_) => plan.copies.push(CopyOp { src: e.path().into(), dst, size: meta.len(), replaces: 0 }),
        }
    }

    if mode == Mode::Mirror && music.is_dir() {
        for e in WalkDir::new(&music).into_iter().filter_map(|e| e.ok()) {
            if !e.file_type().is_file() {
                continue;
            }
            let rel = e.path().strip_prefix(&music).unwrap();
            let name = e.file_name().to_string_lossy();
            // `._x.mp3` AppleDouble files go too: they confuse the scanner
            if (is_media(rel) || name.starts_with("._")) && !taken.contains(&key(rel)) {
                plan.deletes.push((rel.to_path_buf(), e.metadata().map(|m| m.len()).unwrap_or(0)));
            }
        }
    }
    Ok(plan)
}

/// Carry out `plan`: deletions first to make room, then the copies.
pub fn apply(plan: &Plan, mount: &Path, progress: &mut dyn FnMut(Progress), cancel: &AtomicBool) -> Result<()> {
    let music = mount.join("Music");
    for (rel, _) in &plan.deletes {
        fs::remove_file(music.join(rel))?;
    }
    if !plan.deletes.is_empty() {
        remove_empty_dirs(&music);
    }

    let total = plan.copy_bytes();
    let mut done = 0;
    let mut buf = vec![0; 1024 * 1024];
    let mut dirs: HashSet<PathBuf> = HashSet::new();
    for c in &plan.copies {
        let dst = music.join(&c.dst);
        let parent = dst.parent().unwrap().to_path_buf();
        if dirs.insert(parent.clone()) {
            fs::create_dir_all(&parent)?;
        }
        let tmp = dst.with_file_name(format!(".{}.podlink", dst.file_name().unwrap().to_string_lossy()));
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
                progress(Progress { done, total, item: item.clone() });
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
mod tests {
    use super::*;

    fn put(p: &Path, data: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }

    fn no() -> AtomicBool {
        AtomicBool::new(false)
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

        let p = plan(&src, &ipod, Mode::Update, &no()).unwrap();
        assert_eq!(p.copies.len(), 3);
        assert!(p.deletes.is_empty());
        apply(&p, &ipod, &mut |_| {}, &no()).unwrap();
        assert_eq!(fs::read(ipod.join("Music/Odd_ Name/a.flac")).unwrap(), b"flac");
        assert!(!ipod.join("Music/Artist/Album/notes.txt").exists());

        let p = plan(&src, &ipod, Mode::Update, &no()).unwrap();
        assert!(p.is_empty(), "{p:?}");
        assert_eq!(p.unchanged, 3);

        put(&src.join("Artist/Album/01 Song.mp3"), b"changed!");
        let p = plan(&src, &ipod, Mode::Mirror, &no()).unwrap();
        assert_eq!(p.copies.len(), 1);
        assert_eq!(p.copies[0].replaces, 4);
        assert_eq!(p.deletes.len(), 1);
        apply(&p, &ipod, &mut |_| {}, &no()).unwrap();
        assert!(!ipod.join("Music/Old").exists(), "empty folders are removed");
        assert_eq!(fs::read(ipod.join("Music/Artist/Album/01 Song.mp3")).unwrap(), b"changed!");
    }

    #[test]
    fn names_that_differ_in_case_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        put(&src.join("a/Song.mp3"), b"1");
        put(&src.join("a/song.MP3"), b"2");
        let p = plan(&src, &dir.path().join("ipod"), Mode::Update, &no()).unwrap();
        let mut names: Vec<_> = p.copies.iter().map(|c| key(&c.dst)).collect();
        names.sort();
        names.dedup();
        // on a case-insensitive host the folder holds one file only
        assert_eq!(names.len(), p.copies.len());
    }

    #[test]
    fn checks_space() {
        let p = Plan {
            copies: vec![CopyOp { src: "a".into(), dst: "a".into(), size: 50_000_000, replaces: 0 }],
            ..Default::default()
        };
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
        let p = plan(&src, &mount, Mode::Mirror, &no).unwrap();
        apply(&p, &mount, &mut |_| {}, &no).unwrap();
        let again = plan(&src, &mount, Mode::Mirror, &no).unwrap();
        assert!(again.is_empty(), "{again:?}");
        assert!(again.unchanged > 0);
    }
}
