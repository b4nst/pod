//! Everything pod link does to an iPod, without the UI: find it, flash the
//! bootloader, install firmware releases and sync music.

pub mod device;
pub mod flash;
pub mod history;
pub mod ipod;
pub mod mixes;
pub mod release;
pub mod sync;
pub mod tagcache;
pub mod transcode;
pub mod upgrade;

use std::io;

pub use ipod::Model;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("not an iPod: {0}")]
    NotIpod(String),
    #[error(
        "this iPod is Mac formatted (HFS+). pod needs a Windows formatted (FAT32) iPod: restore it \
         from a Windows PC, or convert it first"
    )]
    MacFormatted,
    #[error("{0}")]
    Unsupported(String),
    #[error("bad image: {0}")]
    BadImage(String),
    #[error("the bootloader is for the {file}, this is an {ipod}")]
    WrongModel { ipod: Model, file: Model },
    #[error("no bootloader is installed")]
    NoBootloader,
    #[error("network: {0}")]
    Http(String),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("checksum mismatch for {0}")]
    Checksum(String),
    #[error("not enough space on the iPod: {need} needed, {free} free")]
    NoSpace { need: String, free: String },
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Self {
        Error::Http(e.to_string())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Progress of a long task: bytes done of bytes total, and what it is on.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    pub item: String,
}

/// `1.2 MB`, base 10 like the Finder and Explorer.
pub fn human_bytes(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}
