use std::fmt;
use std::io::{self, Read, Write};

use byteorder::{LE, ReadBytesExt, WriteBytesExt};
use clap::ValueEnum;

use crate::fat::{FatConstructionError, FatDeserializationError};
use crate::vec;

pub const FAT3_MAGIC: u32 = 0x4641_5433;
pub const FAT5_MAGIC: u32 = 0x4641_5435;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ValueEnum)]
pub enum FatVersion {
    /// Used in Watch Dogs 1.
    #[value(name = "v3")]
    Fat3,

    /// Used in Watch Dogs 2 and Legion.
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

    #[must_use]
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
pub enum TableVersion {
    V7,

    /// Used in Watch Dogs 1.
    V8,

    /// Used in Watch Dogs 2.
    V11,

    /// Used in Watch Dogs Legion.
    V13,
}

impl TryFrom<u32> for TableVersion {
    type Error = FatDeserializationError;

    fn try_from(version: u32) -> Result<Self, Self::Error> {
        match version {
            7 => Ok(Self::V7),
            8 => Ok(Self::V8),
            11 => Ok(Self::V11),
            13 => Ok(Self::V13),
            _ => Err(Self::Error::UnknownTableVersion(version)),
        }
    }
}

impl From<TableVersion> for u32 {
    fn from(version: TableVersion) -> Self {
        match version {
            TableVersion::V7 => 7,
            TableVersion::V8 => 8,
            TableVersion::V11 => 11,
            TableVersion::V13 => 13,
        }
    }
}

impl fmt::Display for TableVersion {
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
    pub fn try_from_platform_id(
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

    pub fn try_to_platform_id(self, fat_version: FatVersion) -> Result<u8, FatConstructionError> {
        match (fat_version, self) {
            (_, Self::Any) => Ok(0),
            (FatVersion::Fat3, Self::Win32) | (FatVersion::Fat5, Self::Win64) => Ok(1),
            (FatVersion::Fat3, Self::Xenon) => Ok(2),
            (FatVersion::Fat3, Self::Ps3) | (FatVersion::Fat5, Self::Orbis) => Ok(3),
            (FatVersion::Fat3, Self::Win64) => Ok(4),
            (FatVersion::Fat3, Self::WiiU) => Ok(8),
            _ => Err(FatConstructionError::UnsupportedPlatform {
                platform: self,
                fat_version,
            }),
        }
    }

    #[must_use]
    pub fn is_supported_for(self, fat_version: FatVersion) -> bool {
        self.try_to_platform_id(fat_version).is_ok()
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
    /// Used in WD1 and WD2 for the uncompressed sound archives.
    V0,

    V4,

    /// Used for most archives in WD1
    V5,

    /// Used in most archives of the Windows version of WD2
    V6,

    /// Used by all archives of WDL
    V8,

    /// Used in most archives of the PS4 release of WD2
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
    pub fn deserialize(mut input: impl Read) -> Result<Dependency, io::Error> {
        let archive_hash = input.read_u64::<LE>()?;
        let name_hash = input.read_u64::<LE>()?;

        Ok(Dependency {
            archive_hash,
            name_hash,
        })
    }

    pub fn serialize(self, mut out: impl Write) -> Result<(), io::Error> {
        out.write_u64::<LE>(self.archive_hash)?;
        out.write_u64::<LE>(self.name_hash)?;

        Ok(())
    }
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Archive hash {:#X}, Name hash {:#X}",
            self.archive_hash, self.name_hash
        )
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FatHeader {
    fat_version: FatVersion,
    table_version: TableVersion,
    platform: Platform,
    compression_version: CompressionVersion,
    name_hash_version: NameHashVersion,
    archive_hash: Option<u64>,
    dependencies: Option<Vec<Dependency>>,
}

impl FatHeader {
    fn validate(self) -> Result<Self, FatConstructionError> {
        if !self.platform.is_supported_for(self.fat_version) {
            Err(FatConstructionError::UnsupportedPlatform {
                platform: self.platform,
                fat_version: self.fat_version,
            })
        } else if let Some(ref dependencies) = self.dependencies
            && u32::try_from(dependencies.len()).is_err()
        {
            Err(FatConstructionError::DependencyCountWontFit(
                dependencies.len(),
            ))
        } else {
            Ok(self)
        }
    }

    #[must_use]
    pub fn new_fat3(
        table_version: TableVersion,
        platform: Platform,
        compression_version: CompressionVersion,
        name_hash_version: NameHashVersion,
    ) -> Result<Self, FatConstructionError> {
        Self {
            fat_version: FatVersion::Fat3,
            table_version,
            platform,
            compression_version,
            name_hash_version,
            archive_hash: None,
            dependencies: None,
        }
        .validate()
    }

    pub fn new_fat5(
        table_version: TableVersion,
        platform: Platform,
        compression_version: CompressionVersion,
        name_hash_version: NameHashVersion,
        archive_hash: u64,
        dependencies: Vec<Dependency>,
    ) -> Result<Self, FatConstructionError> {
        Self {
            fat_version: FatVersion::Fat5,
            table_version,
            platform,
            compression_version,
            name_hash_version,
            archive_hash: Some(archive_hash),
            dependencies: Some(dependencies),
        }
        .validate()
    }

    pub fn deserialize(mut input: impl Read) -> Result<Self, FatDeserializationError> {
        let fat_version = FatVersion::try_from_magic(input.read_u32::<LE>()?)?;
        let table_version = TableVersion::try_from(input.read_u32::<LE>()?)?;

        let platform = Platform::try_from_platform_id(input.read_u8()?, fat_version)?;
        let compression_version = CompressionVersion::try_from(input.read_u8()?)?;
        let name_hash_version = NameHashVersion::try_from(input.read_u8()?)?;
        let padding_byte = input.read_u8()?;
        if padding_byte != 0x00 {
            return Err(FatDeserializationError::UnexpectedPaddingByte(padding_byte));
        }

        let (archive_hash, dependencies) = match fat_version {
            FatVersion::Fat3 => (None, None),
            FatVersion::Fat5 => {
                let archive_hash = input.read_u64::<LE>()?;

                let dependency_count = input.read_u32::<LE>()?;
                let mut dependencies = vec::try_with_capacity(dependency_count).ok_or(
                    FatDeserializationError::DependencyAllocationFailed(dependency_count),
                )?;

                for _ in 0..dependency_count {
                    let dependency = Dependency::deserialize(&mut input)?;
                    dependencies.push(dependency);
                }

                (Some(archive_hash), Some(dependencies))
            }
        };

        Ok(Self {
            fat_version,
            table_version,
            platform,
            compression_version,
            name_hash_version,
            archive_hash,
            dependencies,
        })
    }

    pub fn serialize(&self, mut out: impl Write) -> Result<(), io::Error> {
        out.write_u32::<LE>(self.fat_version.to_magic())?;
        out.write_u32::<LE>(self.table_version.into())?;

        // Flags
        out.write_u8(self.platform.try_to_platform_id(self.fat_version).expect(
            "constructor should guarantee that platform is supported by current fat version",
        ))?;
        out.write_u8(self.compression_version.into())?;
        out.write_u8(self.name_hash_version.into())?;
        out.write_u8(0x00)?;

        if self.fat_version == FatVersion::Fat5 {
            out.write_u64::<LE>(
                self.archive_hash
                    .expect("constructor should guarantee that archive hash is present on FAT5"),
            )?;

            let dependencies = self
                .dependencies()
                .expect("constructor should guarantee that dependencies are present on FAT5");
            let dependency_count: u32 = dependencies
                .len()
                .try_into()
                .expect("constructor should guarantee that dependency count fits into u32");

            out.write_u32::<LE>(dependency_count)?;
            for dependency in dependencies {
                dependency.serialize(&mut out)?;
            }
        }

        Ok(())
    }

    #[must_use]
    pub fn fat_version(&self) -> FatVersion {
        self.fat_version
    }

    #[must_use]
    pub fn table_version(&self) -> TableVersion {
        self.table_version
    }

    #[must_use]
    pub fn platform(&self) -> Platform {
        self.platform
    }

    #[must_use]
    pub fn compression_version(&self) -> CompressionVersion {
        self.compression_version
    }

    #[must_use]
    pub fn name_hash_version(&self) -> NameHashVersion {
        self.name_hash_version
    }

    #[must_use]
    pub fn archive_hash(&self) -> Option<u64> {
        self.archive_hash
    }

    #[must_use]
    pub fn dependencies(&self) -> Option<&[Dependency]> {
        self.dependencies.as_deref()
    }

    #[must_use]
    pub const fn new_wd1_win64() -> Self {
        Self {
            fat_version: FatVersion::Fat3,
            table_version: TableVersion::V8,
            platform: Platform::Win64,
            compression_version: CompressionVersion::V5,
            name_hash_version: NameHashVersion::V50,
            archive_hash: None,
            dependencies: None,
        }
    }

    #[must_use]
    pub const fn new_wd1_win64_sound() -> Self {
        Self {
            fat_version: FatVersion::Fat3,
            table_version: TableVersion::V8,
            platform: Platform::Any,
            compression_version: CompressionVersion::V0,
            name_hash_version: NameHashVersion::V50,
            archive_hash: None,
            dependencies: None,
        }
    }

    #[must_use]
    pub const fn new_wd1_wiiu() -> Self {
        Self {
            fat_version: FatVersion::Fat3,
            table_version: TableVersion::V8,
            platform: Platform::WiiU,
            compression_version: CompressionVersion::V5,
            name_hash_version: NameHashVersion::V56,
            archive_hash: None,
            dependencies: None,
        }
    }

    #[must_use]
    pub const fn new_wd1_wiiu_sound() -> Self {
        Self {
            fat_version: FatVersion::Fat3,
            table_version: TableVersion::V8,
            platform: Platform::Any,
            compression_version: CompressionVersion::V0,
            name_hash_version: NameHashVersion::V56,
            archive_hash: None,
            dependencies: None,
        }
    }

    #[must_use]
    pub const fn new_wd2_win64() -> Self {
        Self {
            fat_version: FatVersion::Fat5,
            table_version: TableVersion::V11,
            platform: Platform::Win64,
            compression_version: CompressionVersion::V6,
            name_hash_version: NameHashVersion::V70,
            archive_hash: Some(0xFFFF_FFFF_FFFF_FFFF),
            dependencies: Some(Vec::new()),
        }
    }

    #[must_use]
    pub const fn new_wd2_ps4() -> Self {
        Self {
            fat_version: FatVersion::Fat5,
            table_version: TableVersion::V11,
            platform: Platform::Orbis,
            compression_version: CompressionVersion::V9,
            name_hash_version: NameHashVersion::V70,
            archive_hash: Some(0xFFFF_FFFF_FFFF_FFFF),
            dependencies: Some(Vec::new()),
        }
    }

    #[must_use]
    pub const fn new_wd2_sound() -> Self {
        Self {
            fat_version: FatVersion::Fat5,
            table_version: TableVersion::V11,
            platform: Platform::Any,
            compression_version: CompressionVersion::V0,
            name_hash_version: NameHashVersion::V70,
            archive_hash: Some(0xFFFF_FFFF_FFFF_FFFF),
            dependencies: Some(Vec::new()),
        }
    }

    #[must_use]
    pub const fn new_wdl_win64() -> Self {
        Self {
            fat_version: FatVersion::Fat5,
            table_version: TableVersion::V13,
            platform: Platform::Win64,
            compression_version: CompressionVersion::V8,
            name_hash_version: NameHashVersion::V70,
            archive_hash: Some(0xFFFF_FFFF_FFFF_FFFF),
            dependencies: Some(Vec::new()),
        }
    }

    #[must_use]
    pub fn new_wdl_win64_london() -> Self {
        Self {
            fat_version: FatVersion::Fat5,
            table_version: TableVersion::V13,
            platform: Platform::Win64,
            compression_version: CompressionVersion::V8,
            name_hash_version: NameHashVersion::V70,
            archive_hash: Some(0xA7E2_977F_3F32_B98E),
            dependencies: Some(vec![Dependency {
                archive_hash: 0xB782_28C0_B350_CC14,
                name_hash: 0xBE38_E2B5_954E_5FA4,
            }]),
        }
    }

    #[must_use]
    pub fn new_wdl_win64_london_cache() -> Self {
        Self {
            fat_version: FatVersion::Fat5,
            table_version: TableVersion::V13,
            platform: Platform::Win64,
            compression_version: CompressionVersion::V8,
            name_hash_version: NameHashVersion::V70,
            archive_hash: Some(0xB782_28C0_B350_CC14),
            dependencies: Some(vec![Dependency {
                archive_hash: 0xA7E2_977F_3F32_B98E,
                name_hash: 0xB058_64FD_230C_EA67,
            }]),
        }
    }
}

impl fmt::Display for FatHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            writeln!(f, "FAT version:         {}", self.fat_version)?;
            writeln!(f, "Table version:       {}", self.table_version)?;
            writeln!(f, "Platform:            {}", self.platform)?;
            writeln!(f, "Compression version: {}", self.compression_version)?;
            writeln!(f, "Name hash version:   {}", self.name_hash_version)?;
        } else {
            write!(
                f,
                "{}, Table {}, Platform {}, Compression {}, Name hash {}",
                self.fat_version,
                self.table_version,
                self.platform,
                self.compression_version,
                self.name_hash_version,
            )?;
        }

        if self.fat_version == FatVersion::Fat5 {
            let archive_hash = self
                .archive_hash
                .expect("constructor should guarantee that archive hash is present on FAT5");
            let dependencies = self
                .dependencies()
                .expect("constructor should guarantee that dependencies are present on FAT5")
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<String>>()
                .join(", ");

            if f.alternate() {
                writeln!(f, "Archive hash:        {archive_hash:#X}")?;
                writeln!(f, "Dependencies:        [{dependencies}]")?;
            } else {
                write!(
                    f,
                    ", Archive hash {archive_hash:#X}, Dependencies [{dependencies}]"
                )?;
            }
        }

        Ok(())
    }
}
