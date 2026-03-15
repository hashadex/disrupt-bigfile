use std::error;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use byteorder::BE;
use byteorder::{LE, ReadBytesExt, WriteBytesExt};
use clap::ValueEnum;

use crate::filelists;

const FAT3_MAGIC: u32 = 0x4641_5433;
const FAT5_MAGIC: u32 = 0x4641_5435;

#[derive(Debug)]
pub enum FatDeserializationError {
    Io(io::Error),
    BadMagic(u32),
    UnknownEntryVersion(u32),
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
                    "bad magic 0x{magic:X}, expected 0x{FAT3_MAGIC:X} or 0x{FAT5_MAGIC:X}"
                )
            }
            Self::UnknownEntryVersion(version) => {
                write!(f, "unknown entry version {version}")
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

impl error::Error for FatDeserializationError {}

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

impl error::Error for FatSerializationError {}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ValueEnum)]
pub enum FatVersion {
    #[value(name = "v3")]
    Fat3,

    #[value(name = "v5")]
    Fat5,
}

impl TryFrom<u32> for FatVersion {
    type Error = FatDeserializationError;

    fn try_from(magic: u32) -> Result<Self, Self::Error> {
        match magic {
            FAT3_MAGIC => Ok(Self::Fat3),
            FAT5_MAGIC => Ok(Self::Fat5),
            _ => Err(Self::Error::BadMagic(magic)),
        }
    }
}

impl fmt::Display for FatVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fat3 => write!(f, "FAT3"),
            Self::Fat5 => write!(f, "FAT5"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ValueEnum)]
pub enum EntryVersion {
    V7,
    V8,
    V11,
    V13,
}

impl TryFrom<u32> for EntryVersion {
    type Error = FatDeserializationError;

    fn try_from(version: u32) -> Result<Self, Self::Error> {
        match version {
            7 => Ok(Self::V7),
            8 => Ok(Self::V8),
            11 => Ok(Self::V11),
            13 => Ok(Self::V13),
            _ => Err(Self::Error::UnknownEntryVersion(version)),
        }
    }
}

impl From<EntryVersion> for u32 {
    fn from(version: EntryVersion) -> Self {
        match version {
            EntryVersion::V7 => 7,
            EntryVersion::V8 => 8,
            EntryVersion::V11 => 11,
            EntryVersion::V13 => 13,
        }
    }
}

impl fmt::Display for EntryVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V7 => write!(f, "V7"),
            Self::V8 => write!(f, "V8"),
            Self::V11 => write!(f, "V11"),
            Self::V13 => write!(f, "V13"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ValueEnum)]
pub enum Platform {
    Any,
    Win32,
    Xenon,
    Ps3,
    Win64,
    WiiU,
    Orbis,
}

impl Platform {
    pub fn from_platform_id(
        platform_id: u8,
        fat_version: FatVersion,
    ) -> Result<Self, FatDeserializationError> {
        match (fat_version, platform_id) {
            (_, 0) => Ok(Self::Any),
            (FatVersion::Fat3, 1) => Ok(Self::Win32),
            (FatVersion::Fat3, 2) => Ok(Self::Xenon),
            (FatVersion::Fat3, 3) => Ok(Self::Ps3),
            (FatVersion::Fat3, 4) | (FatVersion::Fat5, 1) => Ok(Self::Win64),
            (FatVersion::Fat3, 8) => Ok(Self::WiiU),
            (FatVersion::Fat5, 3) => Ok(Self::Orbis),
            _ => Err(FatDeserializationError::UnsupportedPlatformId {
                platform_id,
                fat_version,
            }),
        }
    }

    pub fn as_platform_id(self, fat_version: FatVersion) -> Result<u8, FatSerializationError> {
        match (fat_version, self) {
            (_, Self::Any) => Ok(0),
            (FatVersion::Fat3, Self::Win32) | (FatVersion::Fat5, Self::Win64) => Ok(1),
            (FatVersion::Fat3, Self::Xenon) => Ok(2),
            (FatVersion::Fat3, Self::Ps3) | (FatVersion::Fat5, Self::Orbis) => Ok(3),
            (FatVersion::Fat3, Self::Win64) => Ok(4),
            (FatVersion::Fat3, Self::WiiU) => Ok(8),
            _ => Err(FatSerializationError::UnsupportedPlatform {
                platform: self,
                fat_version,
            }),
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
            Self::Orbis => write!(f, "Orbis"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ValueEnum)]
pub enum CompressionVersion {
    V0,
    V4,
    V5,
    V6,
    V8,
    V9,
}

impl TryFrom<u8> for CompressionVersion {
    type Error = FatDeserializationError;

    fn try_from(value: u8) -> Result<Self, FatDeserializationError> {
        match value {
            0 => Ok(Self::V0),
            4 => Ok(Self::V4),
            5 => Ok(Self::V5),
            6 => Ok(Self::V6),
            8 => Ok(Self::V8),
            9 => Ok(Self::V9),
            _ => Err(Self::Error::UnknownCompressionVersion(value)),
        }
    }
}

impl From<CompressionVersion> for u8 {
    fn from(version: CompressionVersion) -> Self {
        match version {
            CompressionVersion::V0 => 0,
            CompressionVersion::V4 => 4,
            CompressionVersion::V5 => 5,
            CompressionVersion::V6 => 6,
            CompressionVersion::V8 => 8,
            CompressionVersion::V9 => 9,
        }
    }
}

impl fmt::Display for CompressionVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V0 => write!(f, "V0"),
            Self::V4 => write!(f, "V4"),
            Self::V5 => write!(f, "V5"),
            Self::V6 => write!(f, "V6"),
            Self::V8 => write!(f, "V8"),
            Self::V9 => write!(f, "V9"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ValueEnum)]
pub enum NameHashVersion {
    V50,
    V55,
    V56,
    V58,
    V70,
}

impl TryFrom<u8> for NameHashVersion {
    type Error = FatDeserializationError;

    fn try_from(version: u8) -> Result<Self, Self::Error> {
        match version {
            50 => Ok(Self::V50),
            55 => Ok(Self::V55),
            56 => Ok(Self::V56),
            58 => Ok(Self::V58),
            70 => Ok(Self::V70),
            _ => Err(Self::Error::UnknownNameHashVersion(version)),
        }
    }
}

impl From<NameHashVersion> for u8 {
    fn from(version: NameHashVersion) -> Self {
        match version {
            NameHashVersion::V50 => 50,
            NameHashVersion::V55 => 55,
            NameHashVersion::V56 => 56,
            NameHashVersion::V58 => 58,
            NameHashVersion::V70 => 70,
        }
    }
}

impl fmt::Display for NameHashVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V50 => write!(f, "V50"),
            Self::V55 => write!(f, "V55"),
            Self::V56 => write!(f, "V56"),
            Self::V58 => write!(f, "V58"),
            Self::V70 => write!(f, "V70"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Dependency {
    pub archive_hash: u64,
    pub name_hash: u64,
}

impl Dependency {
    pub fn deserialize(mut data: impl Read) -> Result<Dependency, io::Error> {
        let archive_hash = data.read_u64::<LE>()?;
        let name_hash = data.read_u64::<LE>()?;

        Ok(Dependency {
            archive_hash,
            name_hash,
        })
    }

    pub fn serialize(&self, mut out: impl Write) -> Result<(), io::Error> {
        out.write_u64::<LE>(self.archive_hash)?;
        out.write_u64::<LE>(self.name_hash)?;

        Ok(())
    }
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Archive hash 0x{:X}, Name hash 0x{:X}",
            self.archive_hash, self.name_hash
        )
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FatMetadata {
    pub fat_version: FatVersion,
    pub entry_version: EntryVersion,
    pub platform: Platform,
    pub compression_version: CompressionVersion,
    pub name_hash_version: NameHashVersion,
    pub archive_hash: Option<u64>,
    pub dependencies: Vec<Dependency>,
}

impl FatMetadata {
    pub const WD1_WIN64: FatMetadata = FatMetadata {
        fat_version: FatVersion::Fat3,
        entry_version: EntryVersion::V8,
        platform: Platform::Win64,
        compression_version: CompressionVersion::V5,
        name_hash_version: NameHashVersion::V50,
        archive_hash: None,
        dependencies: Vec::new(),
    };

    pub const WD1_SOUND: FatMetadata = FatMetadata {
        fat_version: FatVersion::Fat3,
        entry_version: EntryVersion::V8,
        platform: Platform::Any,
        compression_version: CompressionVersion::V0,
        name_hash_version: NameHashVersion::V50,
        archive_hash: None,
        dependencies: Vec::new(),
    };
}

impl fmt::Display for FatMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}, Entry {}, Platform {}, Compression {}, Name hash {}",
            self.fat_version,
            self.entry_version,
            self.platform,
            self.compression_version,
            self.name_hash_version,
        )?;

        if let Some(archive_hash) = self.archive_hash {
            write!(f, ", Archive hash 0x{archive_hash:X}")?;
        }

        if !self.dependencies.is_empty() {
            write!(f, ", Dependencies [")?;

            for (index, dependency) in self.dependencies.iter().enumerate() {
                if index != 0 {
                    write!(f, ", ")?;
                }

                write!(f, "({dependency})")?;
            }

            write!(f, "]")?;
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CompressionScheme {
    None,
    LZO1x,
    Zlib,
    XMemCompress,
    LZMA,
    LZ4LW,
    Oodle,
}

impl fmt::Display for CompressionScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "no compression"),
            Self::LZO1x => write!(f, "LZO1x"),
            Self::Zlib => write!(f, "Zlib"),
            Self::XMemCompress => write!(f, "XMemCompress"),
            Self::LZMA => write!(f, "LZMA"),
            Self::LZ4LW => write!(f, "LZ4LW"),
            Self::Oodle => write!(f, "Oodle"),
        }
    }
}

impl CompressionScheme {
    pub fn from_scheme_id(
        scheme_id: u8,
        compression_version: CompressionVersion,
    ) -> Result<Self, FatDeserializationError> {
        match (scheme_id, compression_version) {
            (0, _) => Ok(Self::None),
            (1, CompressionVersion::V4 | CompressionVersion::V5) => Ok(Self::LZO1x),
            (2, CompressionVersion::V4 | CompressionVersion::V5) => Ok(Self::Zlib),
            (3, CompressionVersion::V5) => Ok(Self::XMemCompress),
            (1, CompressionVersion::V6) | (2, CompressionVersion::V8 | CompressionVersion::V9) => {
                Ok(Self::LZMA)
            }
            (2, CompressionVersion::V6) | (3, CompressionVersion::V8 | CompressionVersion::V9) => {
                Ok(Self::LZ4LW)
            }
            _ => Err(FatDeserializationError::UnknownCompressionScheme {
                scheme_id,
                compression_version,
            }),
        }
    }

    pub fn as_scheme_id(
        self,
        compression_version: CompressionVersion,
    ) -> Result<u8, FatSerializationError> {
        match (self, compression_version) {
            (Self::None, _) => Ok(0),

            (Self::LZO1x, CompressionVersion::V4 | CompressionVersion::V5)
            | (Self::LZMA, CompressionVersion::V6)
            | (Self::Oodle, CompressionVersion::V8 | CompressionVersion::V9) => Ok(1),

            (Self::Zlib, CompressionVersion::V4 | CompressionVersion::V5)
            | (Self::LZ4LW, CompressionVersion::V6)
            | (Self::LZMA, CompressionVersion::V8 | CompressionVersion::V9) => Ok(2),

            (Self::XMemCompress, CompressionVersion::V5)
            | (Self::LZ4LW, CompressionVersion::V8 | CompressionVersion::V9) => Ok(3),

            _ => Err(FatSerializationError::UnsupportedCompressionScheme {
                scheme: self,
                compression_version,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Entry {
    pub name_hash: u64,
    pub offset: u64,
    pub compression_scheme: CompressionScheme,
    pub uncompressed_size: u32,
    pub compressed_size: u32,
}

impl Entry {
    // Entry V7 layout

    // oooooooo oooooooo oooooooo oooooooo
    // oocccccc cccccccc cccccccc cccccccc
    // uuuuuuuu uuuuuuuu uuuuuuuu uuuuuuss
    // hhhhhhhh hhhhhhhh hhhhhhhh hhhhhhhh

    // [o] offset = 34 bits
    // [c] compressed size = 30 bits
    // [u] uncompressed size = 30 bits
    // [s] compression scheme = 2 bits
    // [h] hash = 32 bits

    const V7_MAX_NAME_HASH: u64 = u32::MAX as u64;
    const V7_MAX_OFFSET: u64 = 2u64.pow(34);
    const V7_MAX_SIZE: u32 = 2u32.pow(30);

    fn deserialize_v7(mut entry_bytes: &[u8]) -> Result<(u64, u64, u8, u32, u32), io::Error> {
        let a = entry_bytes.read_u64::<BE>()?;
        let b = entry_bytes.read_u32::<BE>()?;
        let c = entry_bytes.read_u32::<BE>()?;

        let offset = a >> 30;
        let compressed_size = (a & 0x3FFF_FFFF)
            .try_into()
            .expect("30 bit int should fit into u32");
        let uncompressed_size = b >> 2;
        let compression_scheme_id = (b & 0b11).try_into().expect("2 bit int should fit into u8");
        let name_hash = c.into();

        Ok((
            name_hash,
            offset,
            compression_scheme_id,
            uncompressed_size,
            compressed_size,
        ))
    }

    fn serialize_v7(
        &self,
        buf: &mut Vec<u8>,
        uncompressed_size: u32,
        compression_scheme_id: u8,
    ) -> Result<(), io::Error> {
        buf.reserve(16);

        let a = (self.offset << 30) | u64::from(self.compressed_size);
        let b = (uncompressed_size << 2) | u32::from(compression_scheme_id);
        let c = self
            .name_hash
            .try_into()
            .expect("serialize() should guarantee that name_hash fits into u32");

        buf.write_u64::<BE>(a)?;
        buf.write_u32::<BE>(b)?;
        buf.write_u32::<BE>(c)?;

        Ok(())
    }

    // Entry V8 layout

    // oooooooo oooooooo oooooooo oooooooo
    // oooccccc cccccccc cccccccc cccccccc
    // uuuuuuuu uuuuuuuu uuuuuuuu uuuuusss
    // hhhhhhhh hhhhhhhh hhhhhhhh hhhhhhhh

    // [o] offset = 35 bits
    // [c] compressed size = 29 bits
    // [u] uncompressed size = 29 bits
    // [s] compression scheme = 3 bits
    // [h] hash = 32 bits

    const V8_MAX_NAME_HASH: u64 = u32::MAX as u64;
    const V8_MAX_OFFSET: u64 = 2u64.pow(35);
    const V8_MAX_SIZE: u32 = 2u32.pow(29);

    fn deserialize_v8(mut entry_bytes: &[u8]) -> Result<(u64, u64, u8, u32, u32), io::Error> {
        let a = entry_bytes.read_u64::<BE>()?;
        let b = entry_bytes.read_u32::<BE>()?;
        let c = entry_bytes.read_u32::<BE>()?;

        let offset = a >> 29;
        let compressed_size = (a & 0x1FFF_FFFF)
            .try_into()
            .expect("29 bit int should fit into u32");
        let uncompressed_size = b >> 3;
        let compression_scheme_id = (b & 0b111)
            .try_into()
            .expect("3 bit int should fit into u8");
        let name_hash: u64 = c.into();

        Ok((
            name_hash,
            offset,
            compression_scheme_id,
            uncompressed_size,
            compressed_size,
        ))
    }

    fn serialize_v8(
        &self,
        buf: &mut Vec<u8>,
        uncompressed_size: u32,
        compression_scheme_id: u8,
    ) -> Result<(), io::Error> {
        buf.reserve(16);

        let a = (self.offset << 29) | u64::from(self.compressed_size);
        let b = (uncompressed_size << 3) | u32::from(compression_scheme_id);
        let c: u32 = self
            .name_hash
            .try_into()
            .expect("serialize() should guarantee that name_hash fits into u32");

        buf.write_u64::<BE>(a)?;
        buf.write_u32::<BE>(b)?;
        buf.write_u32::<BE>(c)?;

        Ok(())
    }

    // Entry V11/V13 layout

    // uuuuuuuu uuuuuuuu uuuuuuuu uuuuuuss
    // oooooooo oooooooo oooooooo oooooooo
    // oocccccc cccccccc cccccccc cccccccc
    // hhhhhhhh hhhhhhhh hhhhhhhh hhhhhhhh
    // hhhhhhhh hhhhhhhh hhhhhhhh hhhhhhhh

    // [u] uncompressed size = 30 bits
    // [s] compression scheme = 2 bits
    // [o] offset = 34 bits
    // [c] compressed size = 30 bits
    // [h] hash = 64 bits

    const V11_V13_MAX_NAME_HASH: u64 = u64::MAX;
    const V11_V13_MAX_OFFSET: u64 = 2u64.pow(34);
    const V11_V13_MAX_SIZE: u32 = 2u32.pow(30);

    fn deserialize_v11_v13(mut entry_bytes: &[u8]) -> Result<(u64, u64, u8, u32, u32), io::Error> {
        let a = entry_bytes.read_u32::<BE>()?;
        let b = entry_bytes.read_u64::<BE>()?;
        let c = entry_bytes.read_u64::<BE>()?;

        let uncompressed_size = a >> 2;
        let compression_scheme_id = (a & 0b11).try_into().expect("2 bit int should fit into u8");
        let offset = b >> 30;
        let compressed_size = (b & 0x3FFF_FFFF)
            .try_into()
            .expect("30 bit int should fit into u32");
        let name_hash = c;

        Ok((
            name_hash,
            offset,
            compression_scheme_id,
            uncompressed_size,
            compressed_size,
        ))
    }

    fn serialize_v11_v13(
        &self,
        buf: &mut Vec<u8>,
        uncompressed_size: u32,
        compression_scheme_id: u8,
    ) -> Result<(), io::Error> {
        buf.reserve(20);

        let a = (uncompressed_size << 2) | u32::from(compression_scheme_id);
        let b = (self.offset << 30) | u64::from(self.compressed_size);
        let c = self.name_hash;

        buf.write_u32::<BE>(a)?;
        buf.write_u64::<BE>(b)?;
        buf.write_u64::<BE>(c)?;

        Ok(())
    }

    pub fn deserialize(
        mut data: impl Read,
        entry_version: EntryVersion,
        compression_version: CompressionVersion,
    ) -> Result<Self, FatDeserializationError> {
        let entry_length = match entry_version {
            EntryVersion::V7 | EntryVersion::V8 => 16,
            EntryVersion::V11 | EntryVersion::V13 => 20,
        };
        let mut buf = vec![0; entry_length];

        data.read_exact(&mut buf)?;

        buf.reverse();

        let deserializer = match entry_version {
            EntryVersion::V7 => Self::deserialize_v7,
            EntryVersion::V8 => Self::deserialize_v8,
            EntryVersion::V11 | EntryVersion::V13 => Self::deserialize_v11_v13,
        };
        let (name_hash, offset, compression_scheme_id, mut uncompressed_size, compressed_size) =
            deserializer(&buf)?;

        let compression_scheme =
            CompressionScheme::from_scheme_id(compression_scheme_id, compression_version)?;

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

    pub fn serialize(
        &self,
        mut out: impl Write,
        entry_version: EntryVersion,
        compression_version: CompressionVersion,
    ) -> Result<(), FatSerializationError> {
        let (max_name_hash, max_offset, max_size) = match entry_version {
            EntryVersion::V7 => (
                Self::V7_MAX_NAME_HASH,
                Self::V7_MAX_OFFSET,
                Self::V7_MAX_SIZE,
            ),
            EntryVersion::V8 => (
                Self::V8_MAX_NAME_HASH,
                Self::V8_MAX_OFFSET,
                Self::V8_MAX_SIZE,
            ),
            EntryVersion::V11 | EntryVersion::V13 => (
                Self::V11_V13_MAX_NAME_HASH,
                Self::V11_V13_MAX_OFFSET,
                Self::V11_V13_MAX_SIZE,
            ),
        };

        if self.name_hash > max_name_hash {
            return Err(FatSerializationError::NameHashWontFit {
                name_hash: self.name_hash,
                max: max_name_hash,
            });
        }

        if self.offset > max_offset {
            return Err(FatSerializationError::OffsetWontFit {
                offset: self.offset,
                max: max_offset,
            });
        }

        if self.uncompressed_size > max_size {
            return Err(FatSerializationError::SizeWontFit {
                size: self.uncompressed_size,
                max: max_size,
            });
        }
        if self.compressed_size > max_size {
            return Err(FatSerializationError::SizeWontFit {
                size: self.compressed_size,
                max: max_size,
            });
        }

        // See comment in deserialize()
        let uncompressed_size = if self.compression_scheme == CompressionScheme::None {
            0
        } else {
            self.uncompressed_size
        };
        let compression_scheme_id = self.compression_scheme.as_scheme_id(compression_version)?;

        let mut buf = Vec::new();

        let serializer = match entry_version {
            EntryVersion::V7 => Self::serialize_v7,
            EntryVersion::V8 => Self::serialize_v8,
            EntryVersion::V11 | EntryVersion::V13 => Self::serialize_v11_v13,
        };
        serializer(self, &mut buf, uncompressed_size, compression_scheme_id)?;

        buf.reverse();

        out.write_all(&buf)?;

        Ok(())
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Fat {
    pub metadata: FatMetadata,
    pub entries: Vec<Entry>,
}

impl Fat {
    pub fn deserialize(mut data: impl Read) -> Result<Self, FatDeserializationError> {
        let fat_version = FatVersion::try_from(data.read_u32::<LE>()?)?;
        let entry_version = EntryVersion::try_from(data.read_u32::<LE>()?)?;

        let platform = Platform::from_platform_id(data.read_u8()?, fat_version)?;
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
                archive_hash,
                dependencies,
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
        let metadata = &self.metadata;
        let entries = &self.entries;

        let magic = match metadata.fat_version {
            FatVersion::Fat3 => FAT3_MAGIC,
            FatVersion::Fat5 => FAT5_MAGIC,
        };
        out.write_u32::<LE>(magic)?;

        out.write_u32::<LE>(metadata.entry_version.into())?;

        // Flags
        out.write_u8(metadata.platform.as_platform_id(metadata.fat_version)?)?;
        out.write_u8(metadata.compression_version.into())?;
        out.write_u8(metadata.name_hash_version.into())?;
        out.write_u8(0x00)?;

        if metadata.fat_version == FatVersion::Fat5 {
            out.write_u64::<LE>(
                metadata
                    .archive_hash
                    .ok_or(FatSerializationError::MissingArchiveHash)?,
            )?;

            let dependency_count = metadata.dependencies.len();
            let dependency_count: u32 = dependency_count
                .try_into()
                .map_err(|_| FatSerializationError::DependencyCountWontFit(dependency_count))?;
            out.write_u32::<LE>(dependency_count)?;

            for dependency in &metadata.dependencies {
                dependency.serialize(&mut out)?;
            }
        }

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

        // Duplicate count
        if metadata.entry_version == EntryVersion::V13 {
            out.write_u32::<LE>(0)?;
        }

        // Localization count
        out.write_u32::<LE>(0)?;

        out.flush()?;

        Ok(())
    }
}
