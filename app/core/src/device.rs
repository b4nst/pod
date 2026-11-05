//! Find mounted iPods and what is on them, without root.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Error, Model, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub mount: PathBuf,
    pub label: String,
    /// The whole disk: `/dev/disk5`, `/dev/sdb`, `\\.\PhysicalDrive2`.
    pub disk: Option<String>,
    /// As the OS names it: `msdos`, `vfat`, `FAT32`, `hfs`...
    pub filesystem: String,
    /// Best guess from the files on the disk. Flashing reads the firmware
    /// partition, which is authoritative.
    pub model: Option<Model>,
    /// `Version:` of the installed pod, from `.pod/rockbox-info.txt`.
    pub firmware: Option<String>,
    /// A stock Rockbox install is present in `.rockbox`.
    pub rockbox: bool,
    pub free: u64,
    pub total: u64,
}

impl Device {
    /// pod (and Rockbox) can only run from a FAT32 data partition.
    pub fn is_fat(&self) -> bool {
        let f = self.filesystem.to_ascii_lowercase();
        f.contains("fat") || f == "msdos"
    }

    pub fn music_dir(&self) -> PathBuf {
        self.mount.join("Music")
    }

    /// Re-read what is on the disk, keeping the OS level details.
    pub fn refresh(&self) -> Device {
        let mut d = inspect(&self.mount).unwrap_or_else(|| self.clone());
        d.disk.clone_from(&self.disk);
        d.filesystem.clone_from(&self.filesystem);
        d
    }

    /// Unmount and, where the OS can, power the iPod down for unplugging.
    pub fn eject(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        let st = Command::new("diskutil")
            .arg("eject")
            .arg(self.disk.as_deref().unwrap_or(&self.mount.to_string_lossy()))
            .output()?;
        #[cfg(target_os = "linux")]
        let st = {
            let src = linux::mounts().into_iter().find(|m| m.mount == self.mount).map(|m| m.source);
            match src {
                Some(src) if which("udisksctl") => {
                    let o = Command::new("udisksctl").args(["unmount", "-b", &src]).output()?;
                    if o.status.success() {
                        if let Some(disk) = &self.disk {
                            let _ = Command::new("udisksctl").args(["power-off", "-b", disk]).output();
                        }
                    }
                    o
                }
                _ => Command::new("umount").arg(&self.mount).output()?,
            }
        };
        #[cfg(windows)]
        let st = windows::powershell(&format!(
            "(New-Object -ComObject Shell.Application).Namespace(17).ParseName('{}').InvokeVerb('Eject')",
            self.mount.to_string_lossy().trim_end_matches('\\')
        ))?;
        if st.status.success() {
            Ok(())
        } else {
            Err(Error::Other(format!("eject failed: {}", String::from_utf8_lossy(&st.stderr).trim())))
        }
    }
}

/// Every mounted volume that looks like an iPod.
pub fn scan() -> Vec<Device> {
    let mut found = Vec::new();
    for v in volumes() {
        if let Some(mut d) = inspect(&v.mount) {
            d.disk = v.disk;
            d.filesystem = v.filesystem;
            if !v.label.is_empty() {
                d.label = v.label;
            }
            found.push(d);
        }
    }
    found
}

/// What is on the volume at `mount`, if it is an iPod.
pub fn inspect(mount: &Path) -> Option<Device> {
    let control = mount.join("iPod_Control");
    let pod = mount.join(".pod");
    if !control.is_dir() && !pod.is_dir() {
        return None;
    }
    let info = |dir: &str| fs::read_to_string(mount.join(dir).join("rockbox-info.txt")).ok();
    let field = |text: &str, key: &str| text.lines().find_map(|l| l.strip_prefix(key)).map(|v| v.trim().to_string());
    let pod_info = info(".pod");
    let rb_info = info(".rockbox");
    let sysinfo = fs::read_to_string(control.join("Device").join("SysInfo")).ok();
    let model = pod_info
        .as_deref()
        .or(rb_info.as_deref())
        .and_then(|t| field(t, "Target:"))
        .and_then(|t| Model::from_target(&t))
        .or_else(|| sysinfo.as_deref().and_then(model_from_sysinfo));
    Some(Device {
        mount: mount.to_path_buf(),
        label: mount.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        disk: None,
        filesystem: String::new(),
        model,
        firmware: pod_info.as_deref().and_then(|t| field(t, "Version:")),
        rockbox: mount.join(".rockbox").is_dir(),
        free: fs4::available_space(mount).unwrap_or(0),
        total: fs4::total_space(mount).unwrap_or(0),
    })
}

/// `ModelNumStr: xM9160` in the SysInfo file the Apple firmware writes.
/// Model numbers as in libgpod's table.
fn model_from_sysinfo(text: &str) -> Option<Model> {
    let line = text.lines().find_map(|l| l.strip_prefix("ModelNumStr:"))?;
    let num: String = line.chars().filter(char::is_ascii_digit).collect();
    match num.get(..4)? {
        "9160" | "9434" | "9435" | "9436" | "9437" => Some(Model::Mini1g),
        "9800" | "9801" | "9802" | "9803" | "9804" | "9805" | "9806" | "9807" => Some(Model::Mini2g),
        _ => None,
    }
}

struct Volume {
    mount: PathBuf,
    disk: Option<String>,
    filesystem: String,
    label: String,
}

#[allow(dead_code)]
fn which(cmd: &str) -> bool {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file())).unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn volumes() -> Vec<Volume> {
    let Ok(dir) = fs::read_dir("/Volumes") else { return Vec::new() };
    dir.flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| p.join("iPod_Control").is_dir() || p.join(".pod").is_dir())
        .map(|mount| {
            let info = Command::new("diskutil")
                .arg("info")
                .arg("-plist")
                .arg(&mount)
                .output()
                .ok()
                .and_then(|o| plist::Value::from_reader_xml(&o.stdout[..]).ok());
            let get = |k: &str| {
                info.as_ref()
                    .and_then(|v| v.as_dictionary())
                    .and_then(|d| d.get(k))
                    .and_then(|v| v.as_string())
                    .map(str::to_string)
            };
            Volume {
                disk: get("ParentWholeDisk").map(|d| format!("/dev/{d}")),
                filesystem: get("FilesystemType").unwrap_or_default(),
                label: get("VolumeName").unwrap_or_default(),
                mount,
            }
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn volumes() -> Vec<Volume> {
    linux::mounts()
        .into_iter()
        .filter(|m| m.source.starts_with("/dev/"))
        .map(|m| Volume {
            disk: linux::whole_disk(&m.source),
            filesystem: m.fstype,
            label: String::new(),
            mount: m.mount,
        })
        .collect()
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::PathBuf;

    pub struct Mount {
        pub mount: PathBuf,
        pub fstype: String,
        pub source: String,
    }

    /// Mount points escape space, tab, newline and backslash as `\ooo`.
    fn unescape(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
                let v = (b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0');
                out.push(v);
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    pub fn mounts() -> Vec<Mount> {
        let text = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
        text.lines()
            .filter_map(|l| {
                let (pre, post) = l.split_once(" - ")?;
                let mount = pre.split(' ').nth(4)?;
                let mut post = post.split(' ');
                Some(Mount {
                    mount: PathBuf::from(unescape(mount)),
                    fstype: post.next()?.to_string(),
                    source: unescape(post.next()?),
                })
            })
            .collect()
    }

    /// `/dev/sdb2` -> `/dev/sdb`, through sysfs.
    pub fn whole_disk(part: &str) -> Option<String> {
        let name = std::path::Path::new(part).file_name()?.to_str()?;
        let sys = std::fs::canonicalize(format!("/sys/class/block/{name}")).ok()?;
        if sys.join("partition").exists() {
            let parent = sys.parent()?.file_name()?.to_str()?;
            Some(format!("/dev/{parent}"))
        } else {
            Some(part.to_string())
        }
    }

    #[test]
    fn unescapes_mount_points() {
        assert_eq!(unescape(r"/media/me/IPOD\040MINI"), "/media/me/IPOD MINI");
    }
}

#[cfg(windows)]
fn volumes() -> Vec<Volume> {
    (b'D'..=b'Z')
        .map(|c| PathBuf::from(format!("{}:\\", c as char)))
        .filter(|p| p.join("iPod_Control").is_dir() || p.join(".pod").is_dir())
        .map(|mount| {
            let letter = mount.to_string_lossy().chars().next().unwrap();
            let out = windows::powershell(&format!(
                "$p = Get-Partition -DriveLetter {letter}; $v = Get-Volume -DriveLetter {letter}; \
                 \"$($p.DiskNumber)|$($v.FileSystem)|$($v.FileSystemLabel)\""
            ))
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
            let mut f = out.splitn(3, '|');
            let disk = f.next().filter(|n| !n.is_empty()).map(|n| format!(r"\\.\PhysicalDrive{n}"));
            Volume {
                disk,
                filesystem: f.next().unwrap_or_default().to_string(),
                label: f.next().unwrap_or_default().to_string(),
                mount,
            }
        })
        .collect()
}

#[cfg(windows)]
pub(crate) mod windows {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Output};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn powershell(script: &str) -> std::io::Result<Output> {
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_installed_pod() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("iPod_Control/Device")).unwrap();
        fs::create_dir_all(dir.path().join(".pod")).unwrap();
        fs::write(dir.path().join(".pod/rockbox-info.txt"), "Target: ipodmini2g\nVersion: pod-v0.1.0\n").unwrap();
        let d = inspect(dir.path()).unwrap();
        assert_eq!(d.model, Some(Model::Mini2g));
        assert_eq!(d.firmware.as_deref(), Some("pod-v0.1.0"));
        assert!(!d.rockbox);
    }

    #[test]
    fn guesses_the_model_from_sysinfo() {
        assert_eq!(model_from_sysinfo("BoardHwName: iPod Q21\nModelNumStr: xM9436\n"), Some(Model::Mini1g));
        assert_eq!(model_from_sysinfo("ModelNumStr: M9805\n"), Some(Model::Mini2g));
        assert_eq!(model_from_sysinfo("ModelNumStr: MA350\n"), None);
    }

    #[test]
    fn ignores_other_volumes() {
        let dir = tempfile::tempdir().unwrap();
        assert!(inspect(dir.path()).is_none());
    }
}
