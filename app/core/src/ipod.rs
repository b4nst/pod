//! The iPod's firmware partition: partition table, image directory and the
//! bootloader install. Ported from Rockbox's `utils/ipodpatcher` (GPLv2+,
//! Dave Chapman), restricted to the iPod mini, the only target pod builds.
//!
//! The Apple bootloader loads the OSOS image. Installing appends our
//! bootloader to that image and points the image's entry at it, so the
//! Apple firmware stays in place behind it. Images after OSOS are moved
//! forward when the bootloader doesn't fit in the gap.

use std::fmt;
use std::io::{self, Read, Seek, SeekFrom, Write};

use crate::Error;

/// The firmware partition is addressed in 512 byte sectors on every mini.
pub const SECTOR: u64 = 512;
/// ipodpatcher's I/O buffer, the largest OSOS image plus bootloader it allows.
const BUFFER_SIZE: u64 = 8 * 1024 * 1024;
const DIR_ENTRY: usize = 40;

const APPLE_STOP_SIGN: &[u8; 256] = b"{{~~  /-----\\   \
{{~~ /       \\  \
{{~~|         | \
{{~~| S T O P | \
{{~~|         | \
{{~~ \\       /  \
{{~~  \\-----/   \
Copyright(C) 200\
1 Apple Computer\
, Inc.----------\
----------------\
----------------\
----------------\
----------------\
----------------\
---------------\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Model {
    Mini1g,
    Mini2g,
}

impl Model {
    pub const ALL: [Model; 2] = [Model::Mini1g, Model::Mini2g];

    /// The Rockbox target name, also used in release asset names.
    pub fn target(self) -> &'static str {
        match self {
            Model::Mini1g => "ipodmini1g",
            Model::Mini2g => "ipodmini2g",
        }
    }

    pub fn from_target(s: &str) -> Option<Model> {
        Model::ALL.into_iter().find(|m| m.target() == s)
    }

    /// Seeds the `.ipod` file checksum.
    fn modelnum(self) -> u32 {
        match self {
            Model::Mini1g => 9,
            Model::Mini2g => 11,
        }
    }

    /// The tag in the `.ipod` file header.
    fn tag(self) -> &'static [u8; 4] {
        match self {
            Model::Mini1g => b"mini",
            Model::Mini2g => b"mn2g",
        }
    }

    /// From the OSOS image version, as ipodpatcher's `getmodel`.
    fn from_firmware(version: u32) -> Result<Model, Error> {
        match version >> 8 {
            0x40 => Ok(Model::Mini1g),
            0x70 => Ok(Model::Mini2g),
            v => Err(Error::Unsupported(format!("this iPod (firmware id {v:#x}) is not an iPod mini"))),
        }
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Model::Mini1g => "iPod mini 1G",
            Model::Mini2g => "iPod mini 2G",
        })
    }
}

/// A `.ipod` bootloader file: 4 byte big endian checksum, 4 byte model tag,
/// then the image.
pub struct Bootloader {
    pub model: Model,
    data: Vec<u8>,
}

impl Bootloader {
    pub fn parse(file: &[u8]) -> Result<Bootloader, Error> {
        if file.len() < 8 {
            return Err(Error::BadImage("bootloader file is too short".into()));
        }
        let model = Model::ALL
            .into_iter()
            .find(|m| &file[4..8] == m.tag())
            .ok_or_else(|| Error::BadImage("not an iPod mini bootloader".into()))?;
        let data = file[8..].to_vec();
        let want = u32::from_be_bytes(file[0..4].try_into().unwrap());
        if checksum(model.modelnum(), &data) != want {
            return Err(Error::BadImage("bootloader checksum mismatch".into()));
        }
        Ok(Bootloader { model, data })
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// What sits in front of the Apple firmware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootState {
    /// The stock Apple bootloader only.
    Stock,
    /// The pod bootloader, with its version (`pod-v0.1.0`).
    Pod(String),
    /// Some other bootloader, most likely Rockbox's.
    Other,
}

#[derive(Debug, Clone, Copy)]
struct Image {
    kind: [u8; 4],
    /// Relative to `fwoffset`.
    dev_offset: u32,
    len: u32,
    entry_offset: u32,
    vers: u32,
}

/// The firmware partition of an opened iPod.
#[derive(Debug)]
pub struct Firmware {
    pub model: Model,
    /// Byte offset of the firmware partition.
    start: u64,
    /// Byte offset the images' `dev_offset` is relative to.
    fwoffset: u64,
    /// Directory offset relative to `start`, not sector aligned.
    diroffset: u64,
    images: Vec<Image>,
}

fn align(n: u64) -> u64 {
    n.div_ceil(SECTOR) * SECTOR
}

/// The `.ipod` checksum: a 32 bit sum of the bytes.
fn checksum(seed: u32, data: &[u8]) -> u32 {
    data.iter().fold(seed, |s, &b| s.wrapping_add(b as u32))
}

fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn put_le32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn read_at<D: Read + Seek>(d: &mut D, pos: u64, len: u64) -> io::Result<Vec<u8>> {
    d.seek(SeekFrom::Start(pos))?;
    let mut buf = vec![0; len as usize];
    d.read_exact(&mut buf)?;
    Ok(buf)
}

fn write_at<D: Write + Seek>(d: &mut D, pos: u64, buf: &[u8]) -> io::Result<()> {
    d.seek(SeekFrom::Start(pos))?;
    d.write_all(buf)
}

impl Firmware {
    /// Read the partition table and the firmware directory.
    pub fn open<D: Read + Seek>(d: &mut D) -> Result<Firmware, Error> {
        let mbr = read_at(d, 0, SECTOR)?;
        if &mbr[0..2] == b"ER" {
            return Err(Error::MacFormatted);
        }
        if mbr[510] != 0x55 || mbr[511] != 0xaa {
            return Err(Error::NotIpod("no partition table".into()));
        }
        let part = |i: usize| {
            let p = &mbr[0x1be + 16 * i..];
            (p[4], le32(p, 8), le32(p, 12))
        };
        let (t0, s0, n0) = part(0);
        let (t1, _, _) = part(1);
        // partition 1 is the firmware (type 0, not empty), 2 is FAT32
        if t0 != 0 || n0 == 0 || s0 == 0 || (t1 != 0x0b && t1 != 0x0c) {
            return Err(Error::NotIpod("partition layout is not an iPod".into()));
        }
        let start = s0 as u64 * SECTOR;

        let head = read_at(d, start, SECTOR)?;
        if head[..256] != APPLE_STOP_SIGN[..] {
            return Err(Error::NotIpod("no Apple firmware partition".into()));
        }
        if &head[0x100..0x104] != b"]ih[" {
            return Err(Error::NotIpod("bad firmware directory".into()));
        }
        let version = u16::from_le_bytes([head[0x10a], head[0x10b]]);
        let diroffset = le32(&head, 0x104) as u64 + 0x200;

        let (dir, x) = Self::read_dir(d, start, diroffset)?;
        let mut images = Vec::new();
        let mut p = x;
        while images.len() < 10 && p + DIR_ENTRY <= x + 400 && p + DIR_ENTRY <= dir.len() {
            if &dir[p..p + 4] != b"!ATA" {
                break;
            }
            images.push(Image {
                kind: dir[p + 4..p + 8].try_into().unwrap(),
                dev_offset: le32(&dir, p + 12),
                len: le32(&dir, p + 16),
                entry_offset: le32(&dir, p + 24),
                vers: le32(&dir, p + 32),
            });
            p += DIR_ENTRY;
        }
        // ipodpatcher patches image 0 and assumes it is OSOS
        let osos =
            images.first().filter(|i| &i.kind == b"soso").ok_or_else(|| Error::NotIpod("no OSOS image".into()))?;
        let model = Model::from_firmware(osos.vers)?;
        let fwoffset = if images.len() > 1 && version == 2 { start } else { start + SECTOR };
        Ok(Firmware { model, start, fwoffset, diroffset, images })
    }

    /// The directory sectors (two, in case it straddles a boundary) and the
    /// directory's offset in them.
    fn read_dir<D: Read + Seek>(d: &mut D, start: u64, diroffset: u64) -> io::Result<(Vec<u8>, usize)> {
        let x = diroffset % SECTOR;
        Ok((read_at(d, start + diroffset - x, 2 * SECTOR)?, x as usize))
    }

    fn osos(&self) -> &Image {
        &self.images[0]
    }

    pub fn boot_state<D: Read + Seek>(&self, d: &mut D) -> Result<BootState, Error> {
        let osos = self.osos();
        if osos.entry_offset == 0 {
            return Ok(BootState::Stock);
        }
        let len = osos.len.saturating_sub(osos.entry_offset) as u64;
        let at = self.fwoffset + osos.dev_offset as u64 + osos.entry_offset as u64;
        let image = read_at(d, at, align(len).min(BUFFER_SIZE))?;
        Ok(find_version(&image).map_or(BootState::Other, BootState::Pod))
    }

    /// Install or replace the bootloader.
    pub fn install<D: Read + Write + Seek>(&mut self, d: &mut D, boot: &Bootloader) -> Result<(), Error> {
        if boot.model != self.model {
            return Err(Error::WrongModel { ipod: self.model, file: boot.model });
        }
        let osos = *self.osos();
        let entry = if osos.entry_offset > 0 { osos.entry_offset as u64 } else { align(osos.len as u64) };
        let length = boot.data.len() as u64;
        let padded = align(length);
        if entry + padded > BUFFER_SIZE {
            return Err(Error::BadImage("bootloader too big".into()));
        }

        let mut delta = 0u64;
        if let Some(next) = self.images.get(1) {
            let end = osos.dev_offset as u64 + entry + padded;
            if end > next.dev_offset as u64 {
                delta = end - next.dev_offset as u64 + SECTOR;
                self.move_images(d, delta)?;
            }
        }

        // the Apple firmware up to the entry point, then the bootloader
        let at = self.fwoffset + osos.dev_offset as u64;
        let mut image = read_at(d, at, entry)?;
        image.extend_from_slice(&boot.data);
        let sum = checksum(0, &image);
        image.resize((entry + padded) as usize, 0);
        write_at(d, at, &image)?;

        let (mut dir, x) = Self::read_dir(d, self.start, self.diroffset)?;
        put_le32(&mut dir, x + 16, (entry + length) as u32);
        put_le32(&mut dir, x + 24, entry as u32);
        put_le32(&mut dir, x + 28, sum);
        put_le32(&mut dir, x + 36, 0xffff_ffff); // loadAddr
        if delta > 0 {
            for i in 1..self.images.len() {
                let o = x + i * DIR_ENTRY + 12;
                let v = le32(&dir, o) + delta as u32;
                put_le32(&mut dir, o, v);
            }
        }
        write_at(d, self.start + self.diroffset - x as u64, &dir)?;
        d.flush()?;

        let i = &mut self.images;
        i[0].len = (entry + length) as u32;
        i[0].entry_offset = entry as u32;
        for img in i.iter_mut().skip(1) {
            img.dev_offset += delta as u32;
        }
        Ok(())
    }

    /// Remove the bootloader. The Apple firmware boots directly again.
    pub fn uninstall<D: Read + Write + Seek>(&mut self, d: &mut D) -> Result<(), Error> {
        let osos = *self.osos();
        if osos.entry_offset == 0 {
            return Err(Error::NoBootloader);
        }
        let length = osos.entry_offset as u64;
        let image = read_at(d, self.fwoffset + osos.dev_offset as u64, align(length))?;
        let sum = checksum(0, &image[..length as usize]);

        let (mut dir, x) = Self::read_dir(d, self.start, self.diroffset)?;
        put_le32(&mut dir, x + 16, length as u32);
        put_le32(&mut dir, x + 24, 0);
        put_le32(&mut dir, x + 28, sum);
        write_at(d, self.start + self.diroffset - x as u64, &dir)?;
        d.flush()?;

        self.images[0].len = length as u32;
        self.images[0].entry_offset = 0;
        Ok(())
    }

    /// Move images 1.. forward by `delta` bytes, last chunk first.
    /// Offsets as in ipodpatcher's `diskmove`: relative to `start`, with the
    /// sector `fwoffset` adds folded into the end.
    fn move_images<D: Read + Write + Seek>(&self, d: &mut D, delta: u64) -> io::Result<()> {
        let last = self.images.last().unwrap();
        let src_start = self.images[1].dev_offset as u64;
        let mut src_end = align(last.dev_offset as u64 + SECTOR + last.len as u64);
        let mut left = src_end - src_start;
        while left > 0 {
            let chunk = left.min(BUFFER_SIZE);
            let buf = read_at(d, self.start + src_end - chunk, chunk)?;
            write_at(d, self.start + src_end - chunk + delta, &buf)?;
            src_end -= chunk;
            left -= chunk;
        }
        Ok(())
    }
}

/// The `pod-...` version string the pod bootloader embeds.
pub fn find_version(image: &[u8]) -> Option<String> {
    image.windows(4).enumerate().find_map(|(i, w)| {
        if w != b"pod-" {
            return None;
        }
        let rest = &image[i + 4..];
        let n = rest.iter().take_while(|&&b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_').count();
        (n > 0).then(|| String::from_utf8_lossy(&image[i..i + 4 + n]).into_owned())
    })
}

/// Fake disks and bootloaders, for tests here and the `flash-image` example.
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures {
    use super::*;

    pub const FW_START: u32 = 63;
    pub const DIROFF: u32 = 0x4000;
    pub const OSOS_LEN: u32 = 0x1_0123;
    pub const RSRC_LEN: u32 = 0x8000;
    pub const AUPD_LEN: u32 = 0x2000;

    fn noise(seed: u32, len: usize) -> Vec<u8> {
        let mut x = seed.wrapping_mul(2654435761) | 1;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect()
    }

    /// A disk image shaped like a factory iPod mini: MBR, firmware
    /// partition with OSOS, RSRC and AUPD packed back to back, a FAT32
    /// partition after it.
    pub fn fake_ipod(model: Model) -> Vec<u8> {
        let fw_sectors = 40_000u32;
        let mut disk = vec![0u8; ((FW_START + fw_sectors + 64) as u64 * SECTOR) as usize];
        let e = 0x1be;
        put_le32(&mut disk, e + 8, FW_START);
        put_le32(&mut disk, e + 12, fw_sectors);
        disk[e + 16 + 4] = 0x0b;
        put_le32(&mut disk, e + 16 + 8, FW_START + fw_sectors);
        put_le32(&mut disk, e + 16 + 12, 64);
        disk[510] = 0x55;
        disk[511] = 0xaa;

        let start = (FW_START as u64 * SECTOR) as usize;
        disk[start..start + 256].copy_from_slice(APPLE_STOP_SIGN);
        disk[start + 0x100..start + 0x104].copy_from_slice(b"]ih[");
        put_le32(&mut disk, start + 0x104, DIROFF - 0x200);
        disk[start + 0x10a] = 3;

        let vers = match model {
            Model::Mini1g => 0x4001,
            Model::Mini2g => 0x7001,
        };
        let fwoffset = start + SECTOR as usize;
        let mut off = 0x4200u32;
        let mut dir = start + DIROFF as usize;
        for (i, (kind, len)) in [(b"soso", OSOS_LEN), (b"crsr", RSRC_LEN), (b"dpua", AUPD_LEN)].into_iter().enumerate()
        {
            let data = noise(i as u32 + 1, len as usize);
            disk[fwoffset + off as usize..][..len as usize].copy_from_slice(&data);
            disk[dir..dir + 4].copy_from_slice(b"!ATA");
            disk[dir + 4..dir + 8].copy_from_slice(kind);
            put_le32(&mut disk, dir + 12, off);
            put_le32(&mut disk, dir + 16, len);
            put_le32(&mut disk, dir + 28, checksum(0, &data));
            put_le32(&mut disk, dir + 32, vers);
            dir += DIR_ENTRY;
            off = align((off + len) as u64) as u32;
        }
        disk
    }

    pub fn fake_bootloader(model: Model, version: &str) -> Vec<u8> {
        let mut data = noise(99, 0x3456);
        data[0x100..0x100 + version.len()].copy_from_slice(version.as_bytes());
        data[0x100 + version.len()] = 0;
        let mut file = checksum(model.modelnum(), &data).to_be_bytes().to_vec();
        file.extend_from_slice(model.tag());
        file.extend_from_slice(&data);
        file
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::fixtures::*;
    use super::*;
    use std::io::Cursor;

    fn image(disk: &[u8], fw: &Firmware, i: usize) -> Vec<u8> {
        let img = fw.images[i];
        let at = (fw.fwoffset + img.dev_offset as u64) as usize;
        disk[at..at + img.len as usize].to_vec()
    }

    #[test]
    fn reads_the_directory() {
        let mut d = Cursor::new(fake_ipod(Model::Mini2g));
        let fw = Firmware::open(&mut d).unwrap();
        assert_eq!(fw.model, Model::Mini2g);
        assert_eq!(fw.images.len(), 3);
        assert_eq!(fw.boot_state(&mut d).unwrap(), BootState::Stock);
    }

    #[test]
    fn install_moves_images_and_keeps_the_apple_firmware() {
        let disk = fake_ipod(Model::Mini1g);
        let before = Firmware::open(&mut Cursor::new(&disk)).unwrap();
        let (osos, rsrc, aupd) = (image(&disk, &before, 0), image(&disk, &before, 1), image(&disk, &before, 2));

        let mut d = Cursor::new(disk);
        let mut fw = Firmware::open(&mut d).unwrap();
        let boot = Bootloader::parse(&fake_bootloader(Model::Mini1g, "pod-v0.1.0")).unwrap();
        fw.install(&mut d, &boot).unwrap();

        let disk = d.into_inner();
        let after = Firmware::open(&mut Cursor::new(&disk)).unwrap();
        let entry = align(OSOS_LEN as u64) as usize;
        assert_eq!(after.images[0].entry_offset as usize, entry);
        assert_eq!(after.images[0].len as usize, entry + boot.len());
        let combined = image(&disk, &after, 0);
        assert_eq!(&combined[..OSOS_LEN as usize], &osos[..]);
        assert_eq!(&combined[entry..], &boot.data[..]);
        assert!(after.images[1].dev_offset > before.images[1].dev_offset);
        assert_eq!(image(&disk, &after, 1), rsrc);
        assert_eq!(image(&disk, &after, 2), aupd);
        assert_eq!(after.boot_state(&mut Cursor::new(&disk)).unwrap(), BootState::Pod("pod-v0.1.0".into()));
    }

    #[test]
    fn reinstall_and_uninstall() {
        let original = fake_ipod(Model::Mini2g);
        let mut d = Cursor::new(original.clone());
        let mut fw = Firmware::open(&mut d).unwrap();
        fw.install(&mut d, &Bootloader::parse(&fake_bootloader(Model::Mini2g, "pod-v0.1.0")).unwrap()).unwrap();
        let moved = fw.images[1].dev_offset;
        fw.install(&mut d, &Bootloader::parse(&fake_bootloader(Model::Mini2g, "pod-v0.2.0")).unwrap()).unwrap();
        let mut fw = Firmware::open(&mut d).unwrap();
        assert_eq!(fw.images[1].dev_offset, moved, "an update fits where the old one was");
        assert_eq!(fw.boot_state(&mut d).unwrap(), BootState::Pod("pod-v0.2.0".into()));

        fw.uninstall(&mut d).unwrap();
        let disk = d.into_inner();
        let fw = Firmware::open(&mut Cursor::new(&disk)).unwrap();
        assert_eq!(fw.boot_state(&mut Cursor::new(&disk)).unwrap(), BootState::Stock);
        // like ipodpatcher, the length comes back sector aligned; the
        // padding is zeros, so the checksum is the factory one
        assert_eq!(fw.images[0].len as u64, align(OSOS_LEN as u64));
        let dir = (FW_START as u64 * SECTOR + DIROFF as u64) as usize;
        assert_eq!(disk[dir..dir + 16], original[dir..dir + 16]);
        assert_eq!(disk[dir + 20..dir + 36], original[dir + 20..dir + 36]);
    }

    #[test]
    fn refuses_the_wrong_model() {
        let mut d = Cursor::new(fake_ipod(Model::Mini1g));
        let mut fw = Firmware::open(&mut d).unwrap();
        let boot = Bootloader::parse(&fake_bootloader(Model::Mini2g, "pod-x")).unwrap();
        assert!(matches!(fw.install(&mut d, &boot), Err(Error::WrongModel { .. })));
    }

    #[test]
    fn rejects_a_corrupt_bootloader() {
        let mut file = fake_bootloader(Model::Mini1g, "pod-x");
        file[100] ^= 1;
        assert!(Bootloader::parse(&file).is_err());
    }

    #[test]
    fn detects_a_mac_formatted_ipod() {
        let mut disk = fake_ipod(Model::Mini1g);
        disk[0..2].copy_from_slice(b"ER");
        assert!(matches!(Firmware::open(&mut Cursor::new(disk)), Err(Error::MacFormatted)));
    }

    /// Cross-check against the C ipodpatcher: POD_IPODPATCHER=dist/ipodpatcher
    #[test]
    fn matches_ipodpatcher() {
        let Ok(tool) = std::env::var("POD_IPODPATCHER") else { return };
        let dir = tempfile::tempdir().unwrap();
        let (img, boot) = (dir.path().join("disk.img"), dir.path().join("boot.ipod"));
        let disk = fake_ipod(Model::Mini1g);
        std::fs::write(&img, &disk).unwrap();
        let file = fake_bootloader(Model::Mini1g, "pod-v0.1.0");
        std::fs::write(&boot, &file).unwrap();
        let out = std::process::Command::new(tool).arg(&img).arg("-a").arg(&boot).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

        let mut d = Cursor::new(disk);
        Firmware::open(&mut d).unwrap().install(&mut d, &Bootloader::parse(&file).unwrap()).unwrap();
        assert!(std::fs::read(&img).unwrap() == d.into_inner(), "disk images differ");
    }
}
