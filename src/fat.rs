use std::error;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use byteorder::{LE, ReadBytesExt, WriteBytesExt};

use crate::compression::{CompressionScheme, CompressionVersion};
use crate::filelists;

const FAT3_MAGIC: u32 = 0x46415433;

#[derive(Debug)]
pub enum FatDeserializationError {
    Io(io::Error),
    BadMagic(u32),
    UnknownEntryVersion(u32),
    UnknownPlatformId(u8),
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
                write!(f, "bad magic 0x{magic:X}, expected 0x{FAT3_MAGIC:X}")
            }
            Self::UnknownEntryVersion(version) => {
                write!(f, "unknown entry version {version}")
            }
            Self::UnknownPlatformId(id) => write!(f, "unknown platform id {id}"),
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

impl error::Error for FatDeserializationError {}

#[derive(Debug)]
pub enum FatSerializationError {
    Io(io::Error),
    UnsupportedCompressionScheme {
        scheme: CompressionScheme,
        version: CompressionVersion,
    },
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
            Self::UnsupportedCompressionScheme { scheme, version } => write!(
                f,
                "compression scheme {scheme} is not supported by compression version {version}"
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

impl error::Error for FatSerializationError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FatVersion {
    Fat3,
}

impl TryFrom<u32> for FatVersion {
    type Error = FatDeserializationError;

    fn try_from(magic: u32) -> Result<Self, Self::Error> {
        match magic {
            FAT3_MAGIC => Ok(Self::Fat3),
            _ => Err(Self::Error::BadMagic(magic)),
        }
    }
}

impl fmt::Display for FatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fat3 => write!(f, "FAT3"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryVersion {
    V8,
}

impl TryFrom<u32> for EntryVersion {
    type Error = FatDeserializationError;

    fn try_from(version: u32) -> Result<Self, Self::Error> {
        match version {
            8 => Ok(Self::V8),
            _ => Err(Self::Error::UnknownEntryVersion(version)),
        }
    }
}

impl From<EntryVersion> for u32 {
    fn from(version: EntryVersion) -> Self {
        match version {
            EntryVersion::V8 => 8,
        }
    }
}

impl fmt::Display for EntryVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V8 => write!(f, "V8"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    Any,
    Win32,
    Xenon,
    Ps3,
    Win64,
    WiiU,
}

impl TryFrom<u8> for Platform {
    type Error = FatDeserializationError;

    fn try_from(id: u8) -> Result<Self, Self::Error> {
        match id {
            0 => Ok(Self::Any),
            1 => Ok(Self::Win32),
            2 => Ok(Self::Xenon),
            3 => Ok(Self::Ps3),
            4 => Ok(Self::Win64),
            8 => Ok(Self::WiiU),
            _ => Err(Self::Error::UnknownPlatformId(id)),
        }
    }
}

impl From<Platform> for u8 {
    fn from(platform: Platform) -> Self {
        match platform {
            Platform::Any => 0,
            Platform::Win32 => 1,
            Platform::Xenon => 2,
            Platform::Ps3 => 3,
            Platform::Win64 => 4,
            Platform::WiiU => 8,
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => write!(f, "Any"),
            Self::Win32 => write!(f, "Win32"),
            Self::Xenon => write!(f, "Xenon"),
            Self::Ps3 => write!(f, "PS3"),
            Self::Win64 => write!(f, "Win64"),
            Self::WiiU => write!(f, "WiiU"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameHashVersion {
    V50,
}

impl TryFrom<u8> for NameHashVersion {
    type Error = FatDeserializationError;

    fn try_from(version: u8) -> Result<Self, Self::Error> {
        match version {
            50 => Ok(Self::V50),
            _ => Err(Self::Error::UnknownNameHashVersion(version)),
        }
    }
}

impl From<NameHashVersion> for u8 {
    fn from(version: NameHashVersion) -> Self {
        match version {
            NameHashVersion::V50 => 50,
        }
    }
}

impl fmt::Display for NameHashVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V50 => write!(f, "V50"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FatMetadata {
    pub fat_version: FatVersion,
    pub entry_version: EntryVersion,
    pub platform: Platform,
    pub compression_version: CompressionVersion,
    pub name_hash_version: NameHashVersion,
}

impl FatMetadata {
    pub const WD1_WIN64: FatMetadata = FatMetadata {
        fat_version: FatVersion::Fat3,
        entry_version: EntryVersion::V8,
        platform: Platform::Win64,
        compression_version: CompressionVersion::V5,
        name_hash_version: NameHashVersion::V50,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    pub name_hash: u64,
    pub offset: u64,
    pub compression_scheme: CompressionScheme,
    pub uncompressed_size: u32,
    pub compressed_size: u32,
}

impl Entry {
    // Entry V8 layout

    // hhhhhhhh hhhhhhhh hhhhhhhh hhhhhhhh
    // uuuuuuuu uuuuuuuu uuuuuuuu uuuuusss
    // oooccccc cccccccc cccccccc cccccccc
    // oooooooo oooooooo oooooooo oooooooo

    // [h] hash = 32 bits
    // [u] uncompressed size = 29 bits
    // [s] compression scheme = 3 bits
    // [o] offset = 35 bits
    // [c] compressed size = 29 bits

    fn deserialize_v8(
        mut data: impl Read,
        compression_version: CompressionVersion,
    ) -> Result<Self, FatDeserializationError> {
        let a = data.read_u32::<LE>()?;
        let b = data.read_u32::<LE>()?;
        let c = data.read_u32::<LE>()?;
        let d = data.read_u32::<LE>()?;

        let name_hash = u64::from(a);
        let mut uncompressed_size = b >> 3;
        let compression_scheme_id =
            u8::try_from(b & 0b111).expect("u32 & 0b111 should always fit into u8");
        let offset = (u64::from(d) << 3) | u64::from(c >> 29);
        let compressed_size = c & 0x1FFFFFFF;

        let compression_scheme =
            CompressionScheme::from_scheme_id(compression_scheme_id, compression_version).ok_or(
                FatDeserializationError::UnknownCompressionScheme {
                    scheme_id: compression_scheme_id,
                    compression_version,
                },
            )?;

        // For some reason, if the entry's compression scheme is None, uncompressed size is set to
        // 0 and compressed size is set to the size of the entry. Let's set both to the same value
        // for convinience.
        if compression_scheme == CompressionScheme::None {
            uncompressed_size = compressed_size;
        }

        Ok(Self {
            name_hash,
            offset,
            compression_scheme,
            uncompressed_size,
            compressed_size,
        })
    }

    // const V8_MAX_NAME_HASH: u64 = u32::MAX.into();
    // https://github.com/rust-lang/rust/issues/143874
    const V8_MAX_NAME_HASH: u64 = 2u64.pow(32);
    const V8_MAX_OFFSET: u64 = 2u64.pow(35);
    const V8_MAX_SIZE: u32 = 2u32.pow(29);

    fn serialize_v8(
        &self,
        mut out: impl Write,
        compression_version: CompressionVersion,
    ) -> Result<(), FatSerializationError> {
        if self.name_hash > Entry::V8_MAX_NAME_HASH {
            return Err(FatSerializationError::NameHashWontFit {
                name_hash: self.name_hash,
                max: Entry::V8_MAX_NAME_HASH,
            });
        }

        if self.offset > Entry::V8_MAX_OFFSET {
            return Err(FatSerializationError::OffsetWontFit {
                offset: self.offset,
                max: Entry::V8_MAX_OFFSET,
            });
        }

        if self.compressed_size > Entry::V8_MAX_SIZE {
            return Err(FatSerializationError::SizeWontFit {
                size: self.compressed_size,
                max: Entry::V8_MAX_SIZE,
            });
        }
        if self.uncompressed_size > Entry::V8_MAX_SIZE {
            return Err(FatSerializationError::SizeWontFit {
                size: self.uncompressed_size,
                max: Entry::V8_MAX_SIZE,
            });
        }

        let name_hash: u32 = self
            .name_hash
            .try_into()
            .expect("name_hash <= Entry::V8_MAX_NAME_HASH, so it should fit into u32");
        let compression_scheme_id: u32 = self
            .compression_scheme
            .as_scheme_id(compression_version)
            .ok_or(FatSerializationError::UnsupportedCompressionScheme {
                scheme: self.compression_scheme,
                version: compression_version,
            })?
            .into();
        let offset_lsb: u32 = (self.offset & 0b111)
            .try_into()
            .expect("u64 & 0b111 should always fit into u32");
        let offset_msb: u32 = (self.offset >> 3)
            .try_into()
            .expect("35 bit int >> 3 should always fit into u32");

        let a = name_hash;
        let b = (self.uncompressed_size << 3) | compression_scheme_id;
        let c = (offset_lsb << 29) | self.compressed_size;
        let d = offset_msb;

        out.write_u32::<LE>(a)?;
        out.write_u32::<LE>(b)?;
        out.write_u32::<LE>(c)?;
        out.write_u32::<LE>(d)?;

        Ok(())
    }

    pub fn deserialize(
        data: impl Read,
        entry_version: EntryVersion,
        compression_version: CompressionVersion,
    ) -> Result<Self, FatDeserializationError> {
        match entry_version {
            EntryVersion::V8 => Self::deserialize_v8(data, compression_version),
        }
    }

    pub fn serialize(
        &self,
        out: impl Write,
        entry_version: EntryVersion,
        compression_version: CompressionVersion,
    ) -> Result<(), FatSerializationError> {
        match entry_version {
            EntryVersion::V8 => self.serialize_v8(out, compression_version),
        }
    }

    pub fn path(&self) -> PathBuf {
        filelists::HASH_SOURCE_MAP.get(&self.name_hash).map_or_else(
            || format!("__UNKNOWN/{:X}", self.name_hash).into(),
            PathBuf::from,
        )
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}B {} @ 0x{:X}: {}",
            self.compressed_size,
            self.compression_scheme,
            self.offset,
            self.path().display()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Fat {
    pub metadata: FatMetadata,
    pub entries: Vec<Entry>,
}

impl Fat {
    pub fn deserialize(mut data: impl Read) -> Result<Self, FatDeserializationError> {
        let fat_version = FatVersion::try_from(data.read_u32::<LE>()?)?;
        let entry_version = EntryVersion::try_from(data.read_u32::<LE>()?)?;

        let platform = Platform::try_from(data.read_u8()?)?;
        let compression_version = CompressionVersion::try_from(data.read_u8()?)?;
        let name_hash_version = NameHashVersion::try_from(data.read_u8()?)?;
        let padding_byte = data.read_u8()?;
        if padding_byte != 0x00 {
            return Err(FatDeserializationError::UnexpectedPaddingByte(padding_byte));
        }

        let entry_count = data.read_u32::<LE>()?;
        let mut entries = Vec::with_capacity(
            entry_count
                .try_into()
                .expect("u32 should fit into usize on PCs"),
        );
        for _ in 0..entry_count {
            let entry = Entry::deserialize(&mut data, entry_version, compression_version)?;
            entries.push(entry);
        }

        Ok(Self {
            metadata: FatMetadata {
                fat_version,
                entry_version,
                platform,
                compression_version,
                name_hash_version,
            },
            entries,
        })
    }

    pub fn new(metadata: FatMetadata) -> Self {
        Self {
            metadata,
            entries: Vec::new(),
        }
    }

    pub fn open(path: &impl AsRef<Path>) -> Result<Self, FatDeserializationError> {
        let file = BufReader::new(File::open(path)?);
        Self::deserialize(file)
    }

    pub fn serialize(&self, mut out: impl Write) -> Result<(), FatSerializationError> {
        let metadata = self.metadata;
        let entries = &self.entries;

        let magic = match metadata.fat_version {
            FatVersion::Fat3 => FAT3_MAGIC,
        };
        out.write_u32::<LE>(magic)?;

        out.write_u32::<LE>(metadata.entry_version.into())?;

        // Flags
        out.write_u8(metadata.platform.into())?;
        out.write_u8(metadata.compression_version.into())?;
        out.write_u8(metadata.name_hash_version.into())?;
        out.write_u8(0x00)?;

        let entry_count = entries.len();
        let entry_count: u32 = entry_count
            .try_into()
            .map_err(|_| FatSerializationError::EntryCountWontFit(entry_count))?;
        out.write_u32::<LE>(entry_count)?;

        for entry in entries {
            entry.serialize(
                &mut out,
                metadata.entry_version,
                metadata.compression_version,
            )?;
        }

        // Localization count
        out.write_u32::<LE>(0)?;

        out.flush()?;

        Ok(())
    }
}
