use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::entry::{CompressionScheme, Entry, EntryError};
use crate::fat::Fat;
use crate::header::{FatHeader, FatVersion};

#[derive(Debug)]
pub enum PackError {
    Io(io::Error),
    Entry(EntryError),
    TableIsFull,
    CantParseUnknownFileHash(PathBuf),
}

impl From<io::Error> for PackError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<EntryError> for PackError {
    fn from(err: EntryError) -> Self {
        Self::Entry(err)
    }
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Entry(err) => write!(f, "entry error: {err}"),
            Self::TableIsFull => write!(f, "can't add more than {} entries", u32::MAX),
            Self::CantParseUnknownFileHash(path) => write!(
                f,
                "can't parse name hash from unknown file path {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PackError {}

pub struct ArchiveBuilder<W: Write + Seek> {
    fat_header: FatHeader,
    entries: Vec<Entry>,
    dat: W,
    dat_position: u64,
    last_add_failed: bool,
}

impl<W: Write + Seek> ArchiveBuilder<W> {
    pub fn new(fat_header: FatHeader, mut dat: W) -> Result<Self, io::Error> {
        let dat_position = dat.stream_position()?;

        Ok(ArchiveBuilder {
            fat_header,
            entries: vec![],
            dat,
            dat_position,
            last_add_failed: false,
        })
    }

    fn compute_name_hash(&self, relative_entry_path: impl AsRef<Path>) -> Result<u64, PackError> {
        let path = relative_entry_path.as_ref();

        let mut hash;

        if path.starts_with("__UNKNOWN") {
            hash = path
                .file_stem()
                .and_then(|stem| u64::from_str_radix(&stem.to_string_lossy(), 16).ok())
                .ok_or_else(|| PackError::CantParseUnknownFileHash(path.to_path_buf()))?;
        } else if path.starts_with("__DUPLICATE") {
            todo!();
        } else {
            let windows_path = path.to_string_lossy().to_lowercase().replace('/', "\\");

            hash = 0xCBF2_9CE4_8422_2325; // Set hash to default seed

            for byte in windows_path.bytes() {
                hash = hash.wrapping_mul(0x0100_0000_01B3);
                hash ^= u64::from(byte);
            }

            if self.fat_header.fat_version() == FatVersion::Fat5 {
                // The three highest bits in all FAT5 name hashes seem to be always set to 101.
                hash &= 0x1FFF_FFFF_FFFF_FFFF; // 0b0001_1111...
                hash |= 0xA000_0000_0000_0000; // 0b1010_0000...
            }
        }

        if self.fat_header.fat_version() == FatVersion::Fat3 {
            hash &= 0xFFFF_FFFF;
        }

        Ok(hash)
    }

    pub fn add(
        &mut self,
        mut data: impl Read,
        relative_entry_path: impl AsRef<Path>,
    ) -> Result<(), PackError> {
        let entry_count: u32 = self
            .entries
            .len()
            .try_into()
            .expect("add() should guarantee that entries.len() <= u32::MAX");
        if entry_count == u32::MAX {
            return Err(PackError::TableIsFull);
        }

        // If the last add failed, dat_position might not accurately reflect dat's actual position.
        // Let's fix this by rewinding dat to the position after the last successful add() call.
        if self.last_add_failed {
            self.dat.seek(SeekFrom::Start(self.dat_position))?;
            self.last_add_failed = false;
        }

        let copied = io::copy(&mut data, &mut self.dat)?;

        let name_hash = self.compute_name_hash(relative_entry_path)?;
        let entry = Entry {
            name_hash,
            offset: self.dat_position,
            compression_scheme: CompressionScheme::None,
            uncompressed_size: copied,
            compressed_size: copied,
        }
        .validate(
            self.fat_header.table_version(),
            self.fat_header.compression_version(),
        )?;

        self.entries.push(entry);

        self.dat_position += copied;

        Ok(())
    }

    pub fn add_file(
        &mut self,
        archive_root: impl AsRef<Path>,
        relative_entry_path: impl AsRef<Path>,
    ) -> Result<(), PackError> {
        let file_path = archive_root.as_ref().join(&relative_entry_path);
        let file = File::open(file_path)?;

        self.add(file, relative_entry_path)
    }

    pub fn finish(mut self) -> Result<Fat, io::Error> {
        self.dat.flush()?;

        Ok(Fat::new_unchecked(self.fat_header, self.entries))
    }
}

impl ArchiveBuilder<File> {
    pub fn create(fat_header: FatHeader, dat_path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let dat = File::create(dat_path)?;

        Self::new(fat_header, dat)
    }
}
