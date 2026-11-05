//! Install a firmware zip (`pod-<target>.zip`) into `/.pod`.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use zip::ZipArchive;

use crate::{Error, Model, Progress, Result};

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Leave the device's `.pod/config.cfg` alone.
    pub keep_settings: bool,
    /// Delete a stock Rockbox install (`.rockbox/`).
    pub remove_rockbox: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { keep_settings: true, remove_rockbox: true }
    }
}

/// What a firmware zip says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub model: Option<Model>,
    pub version: String,
}

pub fn inspect(zip: &Path) -> Result<Info> {
    let mut a = ZipArchive::new(File::open(zip)?)?;
    read_info(&mut a)
}

fn read_info<R: Read + io::Seek>(a: &mut ZipArchive<R>) -> Result<Info> {
    let mut text = String::new();
    a.by_name(".pod/rockbox-info.txt")
        .map_err(|_| Error::BadImage("not a pod firmware zip (no .pod/rockbox-info.txt)".into()))?
        .read_to_string(&mut text)?;
    let field = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)).map(|v| v.trim().to_string());
    Ok(Info {
        model: field("Target:").and_then(|t| Model::from_target(&t)),
        version: field("Version:").unwrap_or_else(|| "unknown".into()),
    })
}

/// Unpack `zip` at the root of the iPod mounted at `mount`. Files are written
/// next to their final name and renamed in place, and `pod.ipod` goes last,
/// so an interrupted install leaves a firmware that boots.
pub fn install(
    zip: &Path,
    mount: &Path,
    model: Option<Model>,
    opts: Options,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Info> {
    let mut a = ZipArchive::new(File::open(zip)?)?;
    let info = read_info(&mut a)?;
    if let (Some(want), Some(got)) = (model, info.model)
        && want != got
    {
        return Err(Error::WrongModel { ipod: want, file: got });
    }

    let keep_cfg = opts.keep_settings && mount.join(".pod/config.cfg").exists();
    let mut files: Vec<(usize, PathBuf, u64)> = Vec::new();
    for i in 0..a.len() {
        let f = a.by_index(i)?;
        let Some(name) = f.enclosed_name() else { continue };
        if !matches!(name.components().next(), Some(Component::Normal(c)) if c == ".pod") {
            continue;
        }
        if f.is_dir() || (keep_cfg && name == Path::new(".pod/config.cfg")) {
            continue;
        }
        files.push((i, name, f.size()));
    }
    files.sort_by_key(|(_, n, _)| n.file_name().is_some_and(|f| f == "pod.ipod"));
    let total = files.iter().map(|f| f.2).sum();

    let mut done = 0;
    let mut buf = vec![0; 256 * 1024];
    for (i, name, _) in files {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let dst = mount.join(&name);
        fs::create_dir_all(dst.parent().unwrap())?;
        let tmp = dst.with_file_name(format!(".{}.podlink", dst.file_name().unwrap().to_string_lossy()));
        let mut f = a.by_index(i)?;
        let mut out = File::create(&tmp)?;
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            done += n as u64;
            progress(Progress { done, total, item: name.to_string_lossy().into_owned() });
        }
        out.sync_all()?;
        drop(out);
        fs::rename(&tmp, &dst)?;
    }

    if opts.remove_rockbox && mount.join(".rockbox").is_dir() {
        fs::remove_dir_all(mount.join(".rockbox"))?;
    }
    Ok(info)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    pub(crate) fn fake_zip(path: &Path, target: &str, version: &str) {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        let o = SimpleFileOptions::default();
        z.add_directory(".pod/", o).unwrap();
        z.start_file(".pod/rockbox-info.txt", o).unwrap();
        write!(z, "Target: {target}\nVersion: {version}\n").unwrap();
        z.start_file(".pod/pod.ipod", o).unwrap();
        z.write_all(b"firmware").unwrap();
        z.start_file(".pod/config.cfg", o).unwrap();
        z.write_all(b"volume: -20\n").unwrap();
        z.start_file(".pod/codecs/mpa.codec", o).unwrap();
        z.write_all(&[7; 1000]).unwrap();
        z.start_file("../evil", o).unwrap();
        z.write_all(b"x").unwrap();
        z.finish().unwrap();
    }

    #[test]
    fn installs_and_keeps_settings() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("pod.zip");
        fake_zip(&zip, "ipodmini1g", "pod-v0.2.0");
        let mount = dir.path().join("ipod");
        fs::create_dir_all(mount.join(".pod")).unwrap();
        fs::create_dir_all(mount.join(".rockbox")).unwrap();
        fs::write(mount.join(".pod/config.cfg"), "volume: -5\n").unwrap();

        let mut last = Progress::default();
        let info =
            install(&zip, &mount, Some(Model::Mini1g), Options::default(), &mut |p| last = p, &AtomicBool::new(false))
                .unwrap();
        assert_eq!(info.version, "pod-v0.2.0");
        assert_eq!(fs::read(mount.join(".pod/pod.ipod")).unwrap(), b"firmware");
        assert_eq!(fs::read_to_string(mount.join(".pod/config.cfg")).unwrap(), "volume: -5\n");
        assert_eq!(fs::read(mount.join(".pod/codecs/mpa.codec")).unwrap().len(), 1000);
        assert!(!mount.join(".rockbox").exists());
        assert!(!dir.path().join("evil").exists());
        assert_eq!(last.item, ".pod/pod.ipod", "pod.ipod is written last");
        assert_eq!(last.done, last.total);
    }

    #[test]
    fn refuses_the_wrong_model() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("pod.zip");
        fake_zip(&zip, "ipodmini2g", "pod-v0.2.0");
        let r =
            install(&zip, dir.path(), Some(Model::Mini1g), Options::default(), &mut |_| {}, &AtomicBool::new(false));
        assert!(matches!(r, Err(Error::WrongModel { .. })));
    }
}

/// Against a real FAT volume: POD_FAT_MOUNT=/Volumes/X POD_FAT_ZIP=dist/pod-ipodmini2g.zip
#[cfg(test)]
mod fat {
    use super::*;

    #[test]
    fn installs_onto_fat() {
        let (Ok(mount), Ok(zip)) = (std::env::var("POD_FAT_MOUNT"), std::env::var("POD_FAT_ZIP")) else { return };
        let info =
            install(Path::new(&zip), Path::new(&mount), None, Options::default(), &mut |_| {}, &AtomicBool::new(false))
                .unwrap();
        let on_disk = fs::read_to_string(Path::new(&mount).join(".pod/rockbox-info.txt")).unwrap();
        assert!(on_disk.contains(&info.version));
    }
}
