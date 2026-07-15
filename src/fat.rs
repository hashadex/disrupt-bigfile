use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use byteorder::{LE, ReadBytesExt, WriteBytesExt};

use crate::entry::{Entry, EntryError};
use crate::header::{
    CompressionVersion, FAT3_MAGIC, FAT5_MAGIC, FatHeader, FatVersion, Platform, TableVersion,
};
use crate::vec;

#[derive(Debug, thiserror::Error)]
pub enum FatDeserializationError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("bad magic 0x{0:X}, expected 0x{FAT3_MAGIC:X} or 0x{FAT5_MAGIC:X}")]
    BadMagic(u32),

    #[error("unknown table version {0}")]
    UnknownTableVersion(u32),

    #[error("platform id {platform_id} is not supported for {fat_version}")]
    UnsupportedPlatformId {
        platform_id: u8,
        fat_version: FatVersion,
    },

    #[error("unknown compression version {0}")]
    UnknownCompressionVersion(u8),

    #[error("unknown name hash version {0}")]
    UnknownNameHashVersion(u8),

    #[error("unexpected padding byte 0x{0:X}, expected 0x00")]
    UnexpectedPaddingByte(u8),

    #[error("failed to allocate memory for {0} dependencies")]
    DependencyAllocationFailed(u32),

    #[error("failed to allocate memory for {0} entries")]
    EntryAllocationFailed(u32),

    #[error(
        "unknown compression scheme id {scheme_id} for compression version {compression_version}"
    )]
    UnknownCompressionScheme {
        scheme_id: u8,
        compression_version: CompressionVersion,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum FatConstructionError {
    #[error("platform {platform} is not supported for {fat_version}")]
    UnsupportedPlatform {
        platform: Platform,
        fat_version: FatVersion,
    },

    #[error(
        "dependency count is too large to fit into header, expected {max} dependencies max, got {0}",
        max = u32::MAX
    )]
    DependencyCountWontFit(usize),

    #[error(
        "entry count is too large to fit into header, expected {max} entries max, got {0}",
        max = u32::MAX
    )]
    EntryCountWontFit(usize),

    #[error("error on entry with name hash 0x{:X}: {error}", .entry.name_hash)]
    Entry { entry: Entry, error: EntryError },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Fat {
    header: FatHeader,
    entries: Vec<Entry>,
}

impl Fat {
    #[must_use]
    pub fn new_unchecked(header: FatHeader, mut entries: Vec<Entry>) -> Self {
        let extract_name_hash = |entry: &Entry| entry.name_hash;
        if !entries.is_sorted_by_key(extract_name_hash) {
            entries.sort_by_key(extract_name_hash);
        }

        Self { header, entries }
    }

    pub fn new(header: FatHeader, entries: Vec<Entry>) -> Result<Self, FatConstructionError> {
        let entry_count = entries.len();
        if u32::try_from(entry_count).is_err() {
            return Err(FatConstructionError::EntryCountWontFit(entry_count));
        }

        for &entry in &entries {
            entry
                .validate(header.table_version(), header.compression_version())
                .map_err(|error| FatConstructionError::Entry { entry, error })?;
        }

        Ok(Self::new_unchecked(header, entries))
    }

    pub fn deserialize(mut data: impl Read) -> Result<Self, FatDeserializationError> {
        let header = FatHeader::deserialize(&mut data)?;

        let entry_count = data.read_u32::<LE>()?;
        let mut entries = vec::try_with_capacity(entry_count)
            .ok_or(FatDeserializationError::EntryAllocationFailed(entry_count))?;
        for _ in 0..entry_count {
            let entry = Entry::deserialize(
                &mut data,
                header.table_version(),
                header.compression_version(),
            )?;
            entries.push(entry);
        }

        Ok(Self::new_unchecked(header, entries))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, FatDeserializationError> {
        let file = BufReader::new(File::open(path)?);
        Self::deserialize(file)
    }

    pub fn serialize(&self, mut out: impl Write) -> Result<(), io::Error> {
        let header = &self.header;
        let entries = &self.entries;

        header.serialize(&mut out)?;

        let entry_count: u32 = entries
            .len()
            .try_into()
            .expect("constructor should guarantee that entry count fits into u32");
        out.write_u32::<LE>(entry_count)?;

        for entry in entries {
            entry.serialize_unchecked(
                &mut out,
                header.table_version(),
                header.compression_version(),
            )?;
        }

        // Duplicate count
        if header.table_version() == TableVersion::V13 {
            out.write_u32::<LE>(0)?;
        }

        // Localization count
        out.write_u32::<LE>(0)?;

        out.flush()?;

        Ok(())
    }

    pub fn create(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        let mut file = BufWriter::new(File::create(path)?);
        self.serialize(&mut file).and_then(|()| file.flush())
    }

    #[must_use]
    pub fn into_inner(self) -> (FatHeader, Vec<Entry>) {
        (self.header, self.entries)
    }

    #[must_use]
    pub fn header(&self) -> &FatHeader {
        &self.header
    }

    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}
