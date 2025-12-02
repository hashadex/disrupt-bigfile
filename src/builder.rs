use std::error;
use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::compression::CompressionScheme;
use crate::fat::{Entry, Fat, FatMetadata, FatSerializationError, FatVersion};

#[derive(Debug)]
pub enum PackError {
    Io(io::Error),
    FileTooLarge(u64),
    CantParseUnknownFileHash(PathBuf),
}

impl From<io::Error> for PackError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::FileTooLarge(size) => write!(
                f,
                "can't add file because its size is too large to fit into an Entry struct (expected {} max, got {size})",
                u32::MAX
            ),
            Self::CantParseUnknownFileHash(path) => write!(
                f,
                "can't parse name hash from unknown file path {}",
                path.display()
            ),
        }
    }
}

impl error::Error for PackError {}

pub struct ArchiveBuilder<W: Write + Seek> {
    fat: Fat,
    dat: W,
    dat_position: u64,
    last_add_failed: bool,
}

impl<W: Write + Seek> ArchiveBuilder<W> {
    pub fn new(fat_metadata: FatMetadata, mut dat: W) -> Result<Self, io::Error> {
        let dat_position = dat.stream_position()?;

        let fat = Fat::new(fat_metadata);

        Ok(ArchiveBuilder {
            fat,
            dat,
            dat_position,
            last_add_failed: false,
        })
    }

    pub fn into_inner(self) -> (Fat, W) {
        (self.fat, self.dat)
    }

    const FNV1_SEED: u64 = 0xCBF29CE484222325;

    fn compute_name_hash(&self, relative_entry_path: &impl AsRef<Path>) -> Result<u64, PackError> {
        let path = relative_entry_path.as_ref();

        let mut hash;

        if path.starts_with("__UNKNOWN") {
            hash = path
                .file_stem()
                .and_then(|stem| u64::from_str_radix(&stem.to_string_lossy(), 16).ok())
                .ok_or_else(|| PackError::CantParseUnknownFileHash(path.to_path_buf()))?
        } else if path.starts_with("__DUPLICATE") {
            todo!();
        } else {
            let windows_path = path.to_string_lossy().replace('/', "\\");

            hash = Self::FNV1_SEED;

            for byte in windows_path.bytes() {
                hash = hash.wrapping_mul(0x100000001B3);
                hash ^= u64::from(byte);
            }
        }

        if self.fat.metadata.fat_version == FatVersion::Fat3 {
            hash &= 0xFFFFFFFF;
        }

        Ok(hash)
    }

    pub fn add(
        &mut self,
        mut data: impl Read,
        relative_entry_path: &impl AsRef<Path>,
    ) -> Result<(), PackError> {
        // If the last add failed, dat_position might not accurately reflect dat's actual position.
        // Let's fix this by rewinding dat to the position after the last successful add() call.
        if self.last_add_failed {
            self.dat.seek(SeekFrom::Start(self.dat_position))?;
            self.last_add_failed = false;
        }

        let copied = io::copy(&mut data, &mut self.dat)
            .map_err(PackError::Io)
            .and_then(|copied| u32::try_from(copied).map_err(|_| PackError::FileTooLarge(copied)))
            .inspect_err(|_| self.last_add_failed = true)?;

        let name_hash = self.compute_name_hash(relative_entry_path)?;

        let entry = Entry {
            name_hash,
            offset: self.dat_position,
            compression_scheme: CompressionScheme::None,
            uncompressed_size: copied,
            compressed_size: copied,
        };
        self.fat.entries.push(entry);

        self.dat_position += u64::from(copied);

        Ok(())
    }

    pub fn add_file(
        &mut self,
        archive_root: &impl AsRef<Path>,
        relative_entry_path: &impl AsRef<Path>,
    ) -> Result<(), PackError> {
        let archive_root = archive_root.as_ref();
        let relative_entry_path = relative_entry_path.as_ref();

        let file_path: PathBuf = [archive_root, relative_entry_path].iter().collect();
        let file = File::open(file_path)?;

        self.add(file, &relative_entry_path)
    }

    pub fn write_fat(&self, out: impl Write) -> Result<(), FatSerializationError> {
        self.fat.serialize(out)
    }

    pub fn create_fat(&self, fat_path: &impl AsRef<Path>) -> Result<(), FatSerializationError> {
        let fat_file = BufWriter::new(File::create(fat_path)?);

        self.write_fat(fat_file)
    }
}

impl ArchiveBuilder<File> {
    pub fn create(
        fat_metadata: FatMetadata,
        dat_path: impl AsRef<Path>,
    ) -> Result<Self, io::Error> {
        let dat = File::create(dat_path)?;

        Self::new(fat_metadata, dat)
    }
}
