use std::fmt;
use std::io::{self, Read, Write};

use byteorder::{LE, ReadBytesExt, WriteBytesExt};
use clap::ValueEnum;

use crate::fat::{FatDeserializationError, FatSerializationError};

pub const FAT3_MAGIC: u32 = 0x4641_5433;
pub const FAT5_MAGIC: u32 = 0x4641_5435;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ValueEnum)]
pub enum FatVersion {
    #[value(name = "v3")]
    Fat3,

    #[value(name = "v5")]
    Fat5,
}

impl FatVersion {
    pub fn try_from_magic(magic: u32) -> Result<FatVersion, FatDeserializationError> {
        match magic {
            FAT3_MAGIC => Ok(Self::Fat3),
            FAT5_MAGIC => Ok(Self::Fat5),
            _ => Err(FatDeserializationError::BadMagic(magic)),
        }
    }

    pub fn to_magic(self) -> u32 {
        match self {
            Self::Fat3 => FAT3_MAGIC,
            Self::Fat5 => FAT5_MAGIC,
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
