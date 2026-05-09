use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;

use byteorder::{LE, ReadBytesExt, WriteBytesExt};

use crate::entry::{CompressionScheme, Entry};
use crate::header::{
    self, CompressionVersion, Dependency, FatHeader, FatVersion, NameHashVersion, Platform,
    TableVersion,
};

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
pub enum FatSerializationError {
    Io(io::Error),
    UnsupportedPlatform {
        platform: Platform,
        fat_version: FatVersion,
    },
    UnsupportedCompressionScheme {
        scheme: CompressionScheme,
        compression_version: CompressionVersion,
    },
    MissingArchiveHash,
    DependencyCountWontFit(usize),
    EntryCountWontFit(usize),
    NameHashWontFit {
        name_hash: u64,
        max: u64,
    },
    OffsetWontFit {
        offset: u64,
        max: u64,
    },
    SizeWontFit {
        size: u32,
        max: u32,
    },
}

impl From<io::Error> for FatSerializationError {
    fn from(err: io::Error) -> Self {
        FatSerializationError::Io(err)
    }
}

impl fmt::Display for FatSerializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::UnsupportedPlatform {
                platform,
                fat_version,
            } => write!(f, "platform {platform} is not supported by {fat_version}"),
            Self::UnsupportedCompressionScheme {
                scheme,
                compression_version,
            } => write!(
                f,
                "compression scheme {scheme} is not supported by compression version {compression_version}"
            ),
            Self::MissingArchiveHash => write!(f, "archive hash cannot be None in FAT5"),
            Self::DependencyCountWontFit(count) => write!(
                f,
                "dependency count is too large to fit into header, expected {} entries max, got {count}",
                u32::MAX
            ),
            Self::EntryCountWontFit(count) => write!(
                f,
                "entry count is too large to fit into header, expected {} entries max, got {count}",
                u32::MAX
            ),
            Self::NameHashWontFit { name_hash, max } => write!(
                f,
                "name hash 0x{name_hash:X} is too large to fit into entry, expected 0x{max:X} max"
            ),
            Self::OffsetWontFit { offset, max } => write!(
                f,
                "offset 0x{offset:X} is too large to fit into entry, expected 0x{max:X} max"
            ),
            Self::SizeWontFit { size, max } => write!(
                f,
                "compressed/uncompressed size {size} is too large to fit into entry, expected {max} max"
            ),
        }
    }
}

impl std::error::Error for FatSerializationError {}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Fat {
    pub header: FatHeader,
    pub entries: Vec<Entry>,
}

impl Fat {
    pub fn deserialize(mut data: impl Read) -> Result<Self, FatDeserializationError> {
        let fat_version = FatVersion::try_from_magic(data.read_u32::<LE>()?)?;
        let table_version = TableVersion::try_from(data.read_u32::<LE>()?)?;

        let platform = Platform::try_from_platform_id(data.read_u8()?, fat_version)?;
        let compression_version = CompressionVersion::try_from(data.read_u8()?)?;
        let name_hash_version = NameHashVersion::try_from(data.read_u8()?)?;
        let padding_byte = data.read_u8()?;
        if padding_byte != 0x00 {
            return Err(FatDeserializationError::UnexpectedPaddingByte(padding_byte));
        }

        let mut archive_hash = None;
        let mut dependencies = Vec::new();
        if fat_version == FatVersion::Fat5 {
            archive_hash = Some(data.read_u64::<LE>()?);

            let dependency_count = data.read_u32::<LE>()?;
            dependencies.reserve(
                dependency_count
                    .try_into()
                    .expect("u32 should fit into usize on PCs"),
            );
            for _ in 0..dependency_count {
                let dependency = Dependency::deserialize(&mut data)?;
                dependencies.push(dependency);
            }
        }

        let entry_count = data.read_u32::<LE>()?;
        let mut entries = Vec::with_capacity(
            entry_count
                .try_into()
                .expect("u32 should fit into usize on PCs"),
        );
        for _ in 0..entry_count {
            let entry = Entry::deserialize(&mut data, table_version, compression_version)?;
            entries.push(entry);
        }

        Ok(Self {
            header: FatHeader {
                fat_version,
                table_version,
                platform,
                compression_version,
                name_hash_version,
                archive_hash,
                dependencies,
            },
            entries,
        })
    }

    pub fn new(header: FatHeader) -> Self {
        Self {
            header,
            entries: Vec::new(),
        }
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, FatDeserializationError> {
        let file = BufReader::new(File::open(path)?);
        Self::deserialize(file)
    }

    pub fn serialize(mut self, mut out: impl Write) -> Result<(), FatSerializationError> {
        let header = &self.header;

        let magic: u32 = header.fat_version.to_magic();
        out.write_u32::<LE>(magic)?;

        out.write_u32::<LE>(header.table_version.into())?;

        // Flags
        out.write_u8(header.platform.try_to_platform_id(header.fat_version)?)?;
        out.write_u8(header.compression_version.into())?;
        out.write_u8(header.name_hash_version.into())?;
        out.write_u8(0x00)?;

        if header.fat_version == FatVersion::Fat5 {
            out.write_u64::<LE>(
                header
                    .archive_hash
                    .ok_or(FatSerializationError::MissingArchiveHash)?,
            )?;

            let dependency_count = header.dependencies.len();
            let dependency_count: u32 = dependency_count
                .try_into()
                .map_err(|_| FatSerializationError::DependencyCountWontFit(dependency_count))?;
            out.write_u32::<LE>(dependency_count)?;

            for dependency in &header.dependencies {
                dependency.serialize(&mut out)?;
            }
        }

        let entries = &mut self.entries;

        let entry_count = entries.len();
        let entry_count: u32 = entry_count
            .try_into()
            .map_err(|_| FatSerializationError::EntryCountWontFit(entry_count))?;
        out.write_u32::<LE>(entry_count)?;

        entries.sort_unstable_by_key(|entry| entry.name_hash);
        for entry in entries {
            entry.serialize(&mut out, header.table_version, header.compression_version)?;
        }

        // Duplicate count
        if header.table_version == TableVersion::V13 {
            out.write_u32::<LE>(0)?;
        }

        // Localization count
        out.write_u32::<LE>(0)?;

        out.flush()?;

        Ok(())
    }
}
