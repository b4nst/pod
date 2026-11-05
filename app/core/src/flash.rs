//! Writing the bootloader needs raw disk access, so it runs in a helper: the
//! app re-runs itself as `podlink flash-helper ...` with root (macOS: the
//! system password prompt, Linux: pkexec, Windows: UAC). The helper writes
//! its [`Report`] as JSON to a file the app reads back.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::ipod::{BootState, Bootloader, Firmware};
use crate::{Error, Result};

pub const HELPER_ARG: &str = "flash-helper";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Install(PathBuf),
    Uninstall,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Report {
    pub model: String,
    pub before: String,
    pub after: String,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Outcome {
    Ok(Report),
    Err { error: String },
}

fn describe(s: &BootState) -> String {
    match s {
        BootState::Stock => "Apple bootloader".into(),
        BootState::Pod(v) => v.clone(),
        BootState::Other => "other bootloader (Rockbox?)".into(),
    }
}

/// Apply `action` to the raw disk. Runs as root.
pub fn apply(disk: &str, action: &Action) -> Result<Report> {
    // read the file before touching the disk
    let boot = match action {
        Action::Install(p) => Some(Bootloader::parse(&fs::read(p)?)?),
        Action::Uninstall => None,
    };
    let dev = raw_device(disk);
    let mut d = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&dev)
        .map_err(|e| Error::Other(format!("cannot open {dev}: {e}")))?;
    let mut fw = Firmware::open(&mut d)?;
    let before = fw.boot_state(&mut d)?;
    match &boot {
        Some(b) => fw.install(&mut d, b)?,
        None => fw.uninstall(&mut d)?,
    }
    // block devices don't take F_FULLFSYNC on macOS (ENOTTY); the raw
    // device is unbuffered anyway
    if let Err(e) = d.sync_all()
        && e.raw_os_error() != Some(25)
    {
        return Err(e.into());
    }
    let after = fw.boot_state(&mut d)?;
    Ok(Report { model: fw.model.to_string(), before: describe(&before), after: describe(&after) })
}

/// macOS: `/dev/disk5` -> `/dev/rdisk5`, the unbuffered character device.
fn raw_device(disk: &str) -> String {
    match disk.strip_prefix("/dev/disk") {
        Some(n) if cfg!(target_os = "macos") => format!("/dev/rdisk{n}"),
        _ => disk.to_string(),
    }
}

/// Entry point of `podlink flash-helper --disk D --out F (--install FILE | --uninstall)`.
/// Returns the process exit code.
pub fn helper_main(args: impl Iterator<Item = OsString>) -> i32 {
    let mut disk = None;
    let mut out = None;
    let mut action = None;
    let mut args = args.map(|a| a.to_string_lossy().into_owned());
    while let Some(a) = args.next() {
        match a.as_str() {
            "--disk" => disk = args.next(),
            "--out" => out = args.next().map(PathBuf::from),
            "--install" => action = args.next().map(|p| Action::Install(p.into())),
            "--uninstall" => action = Some(Action::Uninstall),
            _ => {}
        }
    }
    let (Some(disk), Some(action)) = (disk, action) else {
        eprintln!("usage: podlink {HELPER_ARG} --disk D --out F (--install FILE | --uninstall)");
        return 2;
    };
    let res = apply(&disk, &action);
    let outcome = match &res {
        Ok(r) => Outcome::Ok(r.clone()),
        Err(e) => Outcome::Err { error: e.to_string() },
    };
    let json = serde_json::to_string(&outcome).unwrap();
    match out {
        Some(p) => {
            let _ = fs::write(p, &json);
        }
        None => println!("{json}"),
    }
    if res.is_ok() { 0 } else { 1 }
}

/// Run the helper with root on `disk`, prompting for the password.
pub fn run_elevated(disk: &str, action: &Action) -> Result<Report> {
    let exe = std::env::current_exe()?;
    let dir = tempdir()?;
    let out = dir.join("report.json");
    let mut args: Vec<String> = vec![HELPER_ARG.into(), "--disk".into(), disk.into(), "--out".into()];
    args.push(out.to_string_lossy().into_owned());
    match action {
        Action::Install(p) => {
            args.push("--install".into());
            args.push(p.to_string_lossy().into_owned());
        }
        Action::Uninstall => args.push("--uninstall".into()),
    }
    let status = elevate(&exe, &args, disk)?;
    let report = fs::read_to_string(&out);
    let _ = fs::remove_dir_all(&dir);
    match report.ok().and_then(|t| serde_json::from_str::<Outcome>(&t).ok()) {
        Some(Outcome::Ok(r)) => Ok(r),
        Some(Outcome::Err { error }) => Err(Error::Other(error)),
        None if status.0 => Err(Error::Other("the flash helper wrote no report".into())),
        None => Err(Error::Other(status.1)),
    }
}

fn tempdir() -> Result<PathBuf> {
    let d = std::env::temp_dir().join(format!("podlink-{}", std::process::id()));
    fs::create_dir_all(&d)?;
    // the helper runs as another user on Linux and Windows
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&d, fs::Permissions::from_mode(0o777))?;
    }
    Ok(d)
}

/// POSIX single quoting.
#[allow(dead_code)]
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Ran, and the error text when it failed.
type Status = (bool, String);

fn status_of(out: std::process::Output) -> Status {
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    (out.status.success(), if err.is_empty() { format!("helper failed ({})", out.status) } else { err })
}

#[cfg(target_os = "macos")]
fn elevate(exe: &Path, args: &[String], disk: &str) -> Result<Status> {
    // the volume has to be unmounted for raw writes; mount it again after
    let cmd: Vec<String> =
        std::iter::once(exe.to_string_lossy().into_owned()).chain(args.iter().cloned()).map(|a| sh_quote(&a)).collect();
    let script = format!(
        "/usr/sbin/diskutil unmountDisk {d} >/dev/null && {cmd}; r=$?; /usr/sbin/diskutil mountDisk {d} >/dev/null; exit $r",
        d = sh_quote(disk),
        cmd = cmd.join(" ")
    );
    let applescript = format!(
        "do shell script \"{}\" with prompt \"pod link needs to write the bootloader to the iPod.\" \
         with administrator privileges",
        script.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let out = Command::new("/usr/bin/osascript").arg("-e").arg(applescript).output()?;
    let st = status_of(out);
    if st.1.contains("-128") {
        return Err(Error::Cancelled);
    }
    Ok(st)
}

#[cfg(target_os = "linux")]
fn elevate(exe: &Path, args: &[String], _disk: &str) -> Result<Status> {
    let root = fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("Uid:")).map(|l| l.split_whitespace().nth(2) == Some("0")))
        .unwrap_or(false);
    let out = if root {
        Command::new(exe).args(args).output()?
    } else {
        Command::new("pkexec").arg(exe).args(args).output().map_err(|e| {
            Error::Other(format!("pkexec is needed to flash ({e}). Or run: sudo {} {}", exe.display(), args.join(" ")))
        })?
    };
    // pkexec: 126 = the user dismissed the dialog
    if out.status.code() == Some(126) {
        return Err(Error::Cancelled);
    }
    Ok(status_of(out))
}

#[cfg(windows)]
fn elevate(exe: &Path, args: &[String], _disk: &str) -> Result<Status> {
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let list: Vec<String> = args.iter().map(|a| q(&format!("\"{a}\""))).collect();
    let script = format!(
        "try {{ $p = Start-Process -FilePath {} -ArgumentList {} -Verb RunAs -WindowStyle Hidden -Wait -PassThru; \
         exit $p.ExitCode }} catch {{ exit 1223 }}",
        q(&exe.to_string_lossy()),
        list.join(",")
    );
    let out = crate::device::windows::powershell(&script)?;
    if out.status.code() == Some(1223) {
        return Err(Error::Cancelled);
    }
    Ok(status_of(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model;
    use crate::ipod::fixtures::{fake_bootloader, fake_ipod};

    #[test]
    fn helper_flashes_a_disk_image() {
        let dir = tempfile::tempdir().unwrap();
        let (img, boot, out) = (dir.path().join("d.img"), dir.path().join("b.ipod"), dir.path().join("r.json"));
        fs::write(&img, fake_ipod(Model::Mini2g)).unwrap();
        fs::write(&boot, fake_bootloader(Model::Mini2g, "pod-v1.0.0")).unwrap();
        let args =
            ["--disk", img.to_str().unwrap(), "--out", out.to_str().unwrap(), "--install", boot.to_str().unwrap()];
        assert_eq!(helper_main(args.iter().map(OsString::from)), 0);
        let r: Report = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(r.after, "pod-v1.0.0");
        assert_eq!(r.before, "Apple bootloader");

        let args = ["--disk", img.to_str().unwrap(), "--out", out.to_str().unwrap(), "--uninstall"];
        assert_eq!(helper_main(args.iter().map(OsString::from)), 0);
        let r: Report = serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(r.after, "Apple bootloader");
    }

    #[test]
    fn helper_reports_errors() {
        let dir = tempfile::tempdir().unwrap();
        let (img, out) = (dir.path().join("d.img"), dir.path().join("r.json"));
        fs::write(&img, vec![0u8; 4096]).unwrap();
        let args = ["--disk", img.to_str().unwrap(), "--out", out.to_str().unwrap(), "--uninstall"];
        assert_eq!(helper_main(args.iter().map(OsString::from)), 1);
        assert!(fs::read_to_string(&out).unwrap().contains("error"));
    }
}
