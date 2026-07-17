use std::borrow::Cow;
use std::fs::File;
use std::hash::Hasher;
use std::hint;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use fnv1::Fnv1Hasher;

use crate::entry::{CompressionScheme, Entry, EntryError};
use crate::fat::Fat;
use crate::header::{FatHeader, FatVersion};

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("io error")]
    Io(#[from] io::Error),

    #[error("failed to create an entry")]
    Entry(#[from] EntryError),

    #[error("can't add more than {} entries", u32::MAX)]
    TableIsFull,

    #[error("can't compute name hash for invalid special path '{0}'")]
    InvalidSpecialPath(PathBuf),
}

pub struct ArchiveBuilder<W: Write + Seek> {
    fat_header: FatHeader,
    entries: Vec<Entry>,
    dat: W,
    dat_position: u64,
    last_write_failed: bool,
}

impl<W: Write + Seek> ArchiveBuilder<W> {
    pub fn new(fat_header: FatHeader, mut dat: W) -> Result<Self, io::Error> {
        let dat_position = dat.stream_position()?;

        Ok(ArchiveBuilder {
            fat_header,
            entries: Vec::new(),
            dat,
            dat_position,
            last_write_failed: false,
        })
    }

    fn compute_name_hash(&self, path: &Path) -> Option<u64> {
        if path.starts_with("__UNKNOWN") {
            let stem = path.file_stem()?.to_str()?;
            u64::from_str_radix(stem, 16).ok()
        } else {
            // By avoiding calling functions like to_str when hashing regular paths and using
            // cold_path hints, we get a 50% performance boost.

            let mut hasher = Fnv1Hasher::new();

            let clean_path = if path.starts_with("__DUPLICATE") {
                hint::cold_path();

                let (clean_stem, _) = path.file_stem()?.to_str()?.split_once("__DUPLICATE_")?;
                let extension = path.extension();

                let mut clean_path = path
                    .strip_prefix("__DUPLICATE")
                    .expect("just checked that path starts with __DUPLICATE")
                    .with_file_name(clean_stem);
                if let Some(extension) = extension {
                    clean_path.set_extension(extension);
                }

                Cow::Owned(clean_path)
            } else {
                Cow::Borrowed(path)
            };

            for &byte in clean_path.as_os_str().as_encoded_bytes() {
                let windows_byte = if byte == b'/' {
                    hint::cold_path();

                    b'\\'
                } else {
                    byte
                };

                hasher.write_u8(windows_byte);
            }

            let mut hash = hasher.finish();
            match self.fat_header.fat_version() {
                FatVersion::Fat3 => hash &= 0xFFFF_FFFF,
                FatVersion::Fat5 => {
                    // The three highest bits in all FAT5 name hashes seem to be always set to 101.
                    hash &= 0x1FFF_FFFF_FFFF_FFFF; // 0b0001_1111...
                    hash |= 0xA000_0000_0000_0000; // 0b1010_0000...
                }
            }

            Some(hash)
        }
    }

    pub fn add(
        &mut self,
        mut input: impl Read,
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

        // If the last write failed, dat_position might not accurately reflect dat's actual
        // position. Let's fix this by rewinding dat to the position after the last successful
        // add() call.
        if self.last_write_failed {
            self.dat.seek(SeekFrom::Start(self.dat_position))?;
            self.last_write_failed = false;
        }

        let relative_entry_path = relative_entry_path.as_ref();
        let name_hash = self
            .compute_name_hash(relative_entry_path)
            .ok_or_else(|| PackError::InvalidSpecialPath(relative_entry_path.to_path_buf()))?;
        let offset = self.dat_position;

        let copied =
            io::copy(&mut input, &mut self.dat).inspect_err(|_| self.last_write_failed = true)?;
        self.dat_position += copied;

        let entry = Entry {
            name_hash,
            offset,
            compression_scheme: CompressionScheme::None,
            uncompressed_size: copied,
            compressed_size: copied,
        }
        .validate(
            self.fat_header.table_version(),
            self.fat_header.compression_version(),
        )?;

        self.entries.push(entry);

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

impl ArchiveBuilder<BufWriter<File>> {
    pub fn create(fat_header: FatHeader, dat_path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let dat = BufWriter::new(File::create(dat_path)?);

        Self::new(fat_header, dat)
    }
}
