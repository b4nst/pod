//! pod releases on GitHub. Asset names carry no version (see the CI
//! workflow), so the same names work for every tag.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{Error, Model, Progress, Result};

pub const REPO: &str = "b4nst/pod";
const UA: &str = concat!("podlink/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub published_at: Option<String>,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

pub fn firmware_asset(m: Model) -> String {
    format!("pod-{}.zip", m.target())
}

pub fn bootloader_asset(m: Model) -> String {
    format!("pod-bootloader-{}.ipod", m.target())
}

impl Release {
    /// `pod-v0.1.0`, as the firmware reports itself.
    pub fn version(&self) -> String {
        format!("pod-{}", self.tag_name)
    }

    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// The newest release, prereleases included. `None` before the first one.
pub fn latest() -> Result<Option<Release>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=10");
    let releases: Vec<Release> = ureq::get(&url)
        .header("User-Agent", UA)
        .header("Accept", "application/vnd.github+json")
        .call()?
        .into_body()
        .read_json()?;
    Ok(releases.into_iter().next())
}

fn cache_dir(tag: &str) -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("podlink").join(tag)
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// The checksum `SHA256SUMS` lists for `name`.
fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (sum, file) = l.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| sum.to_ascii_lowercase())
    })
}

fn get(url: &str, progress: &mut dyn FnMut(Progress), name: &str, cancel: &AtomicBool) -> Result<Vec<u8>> {
    let resp = ureq::get(url).header("User-Agent", UA).call()?;
    let total = resp.body().content_length().unwrap_or(0);
    let mut body = resp.into_body().into_reader();
    let mut data = Vec::with_capacity(total as usize);
    let mut buf = vec![0; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let n = body.read(&mut buf)?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        progress(Progress { done: data.len() as u64, total, item: name.into() });
    }
    Ok(data)
}

/// Download `name` from `release` into the cache, checked against the
/// release's `SHA256SUMS`. A cached copy that checks out is reused.
pub fn fetch(
    release: &Release,
    name: &str,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    let asset = release.asset(name).ok_or_else(|| Error::Other(format!("{} has no {name}", release.tag_name)))?;
    let dir = cache_dir(&release.tag_name);
    fs::create_dir_all(&dir)?;
    let sums_path = dir.join("SHA256SUMS");
    let sums = match fs::read_to_string(&sums_path) {
        Ok(s) => Some(s),
        Err(_) => match release.asset("SHA256SUMS") {
            Some(a) => {
                let s = String::from_utf8_lossy(&get(&a.browser_download_url, &mut |_| {}, "SHA256SUMS", cancel)?)
                    .into_owned();
                fs::write(&sums_path, &s)?;
                Some(s)
            }
            None => None,
        },
    };
    let want = sums.as_deref().and_then(|s| expected_sum(s, name));
    let path = dir.join(name);
    if let (Ok(data), Some(want)) = (fs::read(&path), &want)
        && &sha256_hex(&data) == want
    {
        return Ok(path);
    }
    let data = get(&asset.browser_download_url, progress, name, cancel)?;
    if let Some(want) = want
        && sha256_hex(&data) != want
    {
        return Err(Error::Checksum(name.into()));
    }
    let tmp = path.with_extension("part");
    fs::File::create(&tmp)?.write_all(&data)?;
    fs::rename(&tmp, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checksums() {
        let sums = "abc123  pod-ipodmini1g.zip\nDEF456 *pod-ipodmini2g.zip\n";
        assert_eq!(expected_sum(sums, "pod-ipodmini1g.zip").as_deref(), Some("abc123"));
        assert_eq!(expected_sum(sums, "pod-ipodmini2g.zip").as_deref(), Some("def456"));
        assert_eq!(expected_sum(sums, "nope"), None);
    }

    #[test]
    fn parses_the_api() {
        let json = r#"[{"tag_name":"v0.1.0","prerelease":false,"published_at":"2026-11-05T12:00:00Z",
            "assets":[{"name":"pod-ipodmini1g.zip","browser_download_url":"https://x/y","size":12,"extra":1}]}]"#;
        let r: Vec<Release> = serde_json::from_str(json).unwrap();
        assert_eq!(r[0].version(), "pod-v0.1.0");
        assert!(r[0].asset(&firmware_asset(Model::Mini1g)).is_some());
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
