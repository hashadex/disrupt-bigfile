use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use byteorder::{LE, ReadBytesExt, WriteBytesExt};

use crate::entry::{Entry, EntryError};
use crate::header::{self, CompressionVersion, FatHeader, FatVersion, Platform, TableVersion};

#[derive(Debug)]
pub enum FatDeserializationError {
    Io(io::Error),
    BadMagic(u32),
    UnknownTableVersion(u32),
    UnsupportedPlatformId {
        platform_id: u8,
        fat_version: FatVersion,
    },
    UnknownCompressionVersion(u8),
    UnknownNameHashVersion(u8),
    UnexpectedPaddingByte(u8),
    UnknownCompressionScheme {
        scheme_id: u8,
        compression_version: CompressionVersion,
    },
}

impl From<io::Error> for FatDeserializationError {
    fn from(err: io::Error) -> Self {
        FatDeserializationError::Io(err)
    }
}

impl fmt::Display for FatDeserializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::BadMagic(magic) => {
                write!(
                    f,
                    "bad magic 0x{magic:X}, expected 0x{:X} or 0x{:X}",
                    header::FAT3_MAGIC,
                    header::FAT5_MAGIC,
                )
            }
            Self::UnknownTableVersion(version) => {
                write!(f, "unknown table version {version}")
            }
            Self::UnsupportedPlatformId {
                platform_id,
                fat_version,
            } => write!(
                f,
                "platform id {platform_id} is not supported for {fat_version}"
            ),
            Self::UnknownCompressionVersion(version) => {
                write!(f, "unknown compression version {version}")
            }
            Self::UnknownNameHashVersion(version) => {
                write!(f, "unknown name hash version {version}")
            }
            Self::UnexpectedPaddingByte(byte) => {
                write!(f, "unexpected padding byte 0x{byte:X}, expected 0x00")
            }
            Self::UnknownCompressionScheme {
                scheme_id,
                compression_version,
            } => write!(
                f,
                "unknown compression scheme id {scheme_id} for compression version {compression_version}"
            ),
        }
    }
}

impl std::error::Error for FatDeserializationError {}

#[derive(Debug)]
pub enum FatConstructionError {
    UnsupportedPlatform {
        platform: Platform,
        fat_version: FatVersion,
    },
    DependencyCountWontFit(usize),
    EntryCountWontFit(usize),
    Entry {
        entry: Entry,
        error: EntryError,
    },
}

impl fmt::Display for FatConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform {
                platform,
                fat_version,
            } => write!(f, "platform {platform} is not supported for {fat_version}"),
            Self::DependencyCountWontFit(count) => write!(
                f,
                "dependency count is too large to fit into header, expected {} dependencies max, got {count}",
                u32::MAX
            ),
            Self::EntryCountWontFit(count) => write!(
                f,
                "entry count is too large to fit into header, expected {} entries max, got {count}",
                u32::MAX
            ),
            Self::Entry { entry, error } => write!(
                f,
                "error on entry with name hash 0x{:X}: {error}",
                entry.name_hash
            ),
        }
    }
}

impl std::error::Error for FatConstructionError {}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Fat {
    header: FatHeader,
    entries: Vec<Entry>,
}

impl Fat {
    pub fn new_unchecked(header: FatHeader, mut entries: Vec<Entry>) -> Self {
        let extract_name_hash = |entry: &Entry| entry.name_hash;
        if !entries.is_sorted_by_key(extract_name_hash) {
            entries.sort_unstable_by_key(extract_name_hash);
        }

        Self { header, entries }
    }

    pub fn new(header: FatHeader, entries: Vec<Entry>) -> Result<Self, FatConstructionError> {
        let entry_count = entries.len();
        u32::try_from(entry_count)
            .map_err(|_| FatConstructionError::EntryCountWontFit(entry_count))?;

        for &entry in &entries {
            entry
                .validate(header.table_version(), header.compression_version())
                .map_err(|error| FatConstructionError::Entry { entry, error })?;
        }

        Ok(Self::new_unchecked(header, entries))
    }

    pub fn deserialize(mut data: impl Read) -> Result<Self, FatDeserializationError> {
        let header = FatHeader::deserialize(&mut data)?;

        let entry_count: usize = data
            .read_u32::<LE>()?
            .try_into()
            .expect("u32 should fit into usize on PCs");
        let mut entries = Vec::with_capacity(entry_count);
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

    pub fn into_inner(self) -> (FatHeader, Vec<Entry>) {
        (self.header, self.entries)
    }

    pub fn header(&self) -> &FatHeader {
        &self.header
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}
