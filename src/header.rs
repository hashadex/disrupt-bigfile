//! FAT header serialization/deserialization and field enums.

use std::fmt;
use std::io::{self, Read, Write};

use byteorder::{LE, ReadBytesExt, WriteBytesExt};
use clap::ValueEnum;

use crate::fat::{FatConstructionError, FatDeserializationError};
use crate::vec;

/// File signature of [`Fat3`] files.
///
/// Means `FAT3` in ASCII.
///
/// [`Fat3`]: FatVersion::Fat3
pub const FAT3_MAGIC: u32 = 0x4641_5433;

/// File signature of [`Fat5`] files.
///
/// Means `FAT5` in ASCII.
///
/// [`Fat5`]: FatVersion::Fat5
pub const FAT5_MAGIC: u32 = 0x4641_5435;

/// Version/type of the FAT file.
///
/// Affects which target [`Platform`]s are supported in the archive, as well the [archive hash] and
/// [`Dependencies`], which are only present on [`Fat5`].
///
/// The FAT version is determined by the file's signature (the first 4 bytes). [`33 54 41 46`]
/// (`3TAF`) is for FAT3 and [`35 54 41 46`] (`5TAF`) is for FAT5.
///
/// [archive hash]: FatHeader::archive_hash
/// [`Dependencies`]: Dependency
/// [`Fat5`]: Self::Fat5
/// [`33 54 41 46`]: FAT3_MAGIC
/// [`35 54 41 46`]: FAT5_MAGIC
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
    /// Determines the FAT version used in a FAT file by its `magic` signature.
    ///
    /// Returns [`Self::Fat3`] if `magic` is equal to [`FAT3_MAGIC`] or [`Self::Fat5`] if `magic`
    /// is equal to [`FAT5_MAGIC`]
    ///
    /// # Errors
    ///
    /// This function will return [`FatDeserializationError::BadMagic`] if `magic` is not equal to
    /// `FAT3_MAGIC` or `FAT5_MAGIC`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::assert_matches;
    ///
    /// use disrupt_bigfile::header::{FAT3_MAGIC, FAT5_MAGIC, FatVersion};
    ///
    /// assert_matches!(FatVersion::try_from_magic(FAT3_MAGIC), Ok(FatVersion::Fat3));
    /// assert_matches!(FatVersion::try_from_magic(FAT5_MAGIC), Ok(FatVersion::Fat5));
    /// assert_matches!(FatVersion::try_from_magic(0xDEAD_BEEF), Err(_));
    /// ```
    pub fn try_from_magic(magic: u32) -> Result<FatVersion, FatDeserializationError> {
        match magic {
            FAT3_MAGIC => Ok(Self::Fat3),
            FAT5_MAGIC => Ok(Self::Fat5),
            _ => Err(FatDeserializationError::BadMagic(magic)),
        }
    }

    /// Returns the file signature for this FAT version.
    ///
    /// Will return [`FAT3_MAGIC`] if this `FatVersion` is [`Fat3`] or [`FAT5_MAGIC`] for [`Fat5`].
    ///
    /// [`Fat3`]: Self::Fat3
    /// [`Fat5`]: Self::Fat5
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FAT3_MAGIC, FAT5_MAGIC, FatVersion};
    ///
    /// assert_eq!(FatVersion::Fat3.to_magic(), FAT3_MAGIC);
    /// assert_eq!(FatVersion::Fat5.to_magic(), FAT5_MAGIC);
    /// ```
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

/// Version of the binary format of the FAT file's body.
///
/// Affects the binary layout of serialized [`Entries`], as well a 4-byte long "duplicate count"
/// value at the very end of a FAT file, which is only present on [`V13`].
///
/// In [Gibbed.Disrupt], this is known as the `Version` field. In this library it was renamed to
/// `TableVersion` to avoid confusion with [`FatVersion`].
///
/// [`Entries`]: crate::entry::Entry
/// [`V13`]: Self::V13
/// [Gibbed.Disrupt]: https://github.com/gibbed/Gibbed.Disrupt
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

/// Target platform of the archive.
///
/// Does not seem to affect anything.
///
/// # Platform IDs
///
/// Similarly to the [`CompressionScheme`] of an [`Entry`], the `Platform` is stored as an ID
/// number in the serialized FAT header. The [`FatVersion`] dictates which ID numbers are assigned
/// to each platform. Some platforms have different ID numbers depending on the version, and some
/// platforms don't even have an ID assigned to them on some versions. In that case, it means that
/// the platform is not supported for that FAT version.
///
/// The table below what ID is assigned to each platform to each version. Empty cell means that the
/// platform is not supported on that version.
///
/// | Platform | [`Fat3`] | [`Fat5`] |
/// |----------|----------|----------|
/// | `Any`    | 0        | 0        |
/// | `Win32`  | 1        |          |
/// | `Xenon`  | 2        |          |
/// | `Ps3`    | 3        |          |
/// | `Win64`  | 4        | 1        |
/// | `WiiU`   | 8        |          |
/// | `Orbis`  |          | 3        |
///
/// [`CompressionScheme`]: crate::entry::CompressionScheme
/// [`Entry`]: crate::entry::Entry
/// [`Fat3`]: FatVersion::Fat3
/// [`Fat5`]: FatVersion::Fat5
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ValueEnum)]
#[non_exhaustive]
pub enum Platform {
    /// Any platform. Used for the uncompressed sound archives in Watch Dogs 1 and 2.
    Any,

    /// Windows platform. Does not seem to be used anywhere.
    Win32,

    /// Xbox 360 platform.
    Xenon,

    /// PlayStation 3 platform.
    Ps3,

    /// Windows platform.
    Win64,

    /// Wii U platform.
    WiiU,

    /// PlayStation 4 platform.
    Orbis,
}

impl Platform {
    /// Creates a `Platform` from a `platform_id` according to the FAT file's `fat_version`.
    ///
    /// See the [Platform IDs] section of the enum documentation for details.
    ///
    /// [Platform IDs]: Self#platform-ids
    ///
    /// # Errors
    ///
    /// This function will return [`FatDeserializationError::UnsupportedPlatformId`] if the given
    /// `fat_version` does not have a `Platform` assigned to the given `platform_id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::assert_matches;
    ///
    /// use disrupt_bigfile::header::{FatVersion, Platform};
    ///
    /// assert_matches!(Platform::try_from_platform_id(3, FatVersion::Fat3), Ok(Platform::Ps3));
    /// assert_matches!(Platform::try_from_platform_id(3, FatVersion::Fat5), Ok(Platform::Orbis));
    /// assert_matches!(Platform::try_from_platform_id(255, FatVersion::Fat3), Err(_));
    /// ```
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

    /// Returns the platform ID assigned to this `Platform` on the given `fat_version`.
    ///
    /// See the [Platform IDs] section of the enum documentation for details.
    ///
    /// [Platform IDs]: Self#platform-ids
    ///
    /// # Errors
    ///
    /// This function will return [`FatConstructionError::UnsupportedPlatform`] if the given
    /// `fat_version` does not have an ID assigned to this `Platform`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::assert_matches;
    ///
    /// use disrupt_bigfile::header::{FatVersion, Platform};
    ///
    /// assert_matches!(Platform::Win64.try_to_platform_id(FatVersion::Fat3), Ok(4));
    /// assert_matches!(Platform::Win64.try_to_platform_id(FatVersion::Fat5), Ok(1));
    /// assert_matches!(Platform::Orbis.try_to_platform_id(FatVersion::Fat3), Err(_));
    /// ```
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

    /// Checks if the given `fat_version` has an ID assigned to this `Platform`.
    ///
    /// See the [Platform IDs] section of the enum documentation for details.
    ///
    /// [Platform IDs]: Self#platform-ids
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatVersion, Platform};
    ///
    /// assert!(Platform::WiiU.is_supported_for(FatVersion::Fat3));
    /// assert!(!Platform::WiiU.is_supported_for(FatVersion::Fat5));
    /// ```
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

/// Compression version of the archive.
///
/// Affects which [`CompressionSchemes`] can be used in the [`Entries`] of this archive and their
/// scheme IDs. See the [scheme IDs section of `CompressionScheme` documentation] for details.
///
/// [`CompressionSchemes`]: crate::entry::CompressionScheme
/// [scheme IDs section of `CompressionScheme` documentation]: crate::entry::CompressionScheme#scheme-ids
/// [`Entries`]: crate::entry::Entry
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ValueEnum)]
pub enum CompressionVersion {
    /// Used in Watch Dogs 1 and 2 for the uncompressed sound archives.
    V0,

    /// Used in the PS3 version of Watch Dogs 1.
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

/// [`Entry` name hash] version of the archive.
///
/// Does not seem to affect anything.
///
/// [`Entry` name hash]: crate::entry::Entry::name_hash
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ValueEnum)]
pub enum NameHashVersion {
    /// Used in the Windows release of Watch Dogs 1.
    V50,

    /// Used in the Xbox 360 and PS3 releases of Watch Dogs 1.
    V55,

    /// Used in the Wii U release of Watch Dogs 1.
    V56,

    /// Used in the Wii U release of Watch Dogs 1.
    V58,

    /// Used in the Windows and PS4 releases of Watch Dogs 2 and in the Windows release of Legion.
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

/// Dependency of an archive.
///
/// Dependencies are only present on [`Fat5`] archives. Not sure what their purpose is.
///
/// In the Windows release of Watch Dogs: Legion, dependencies are used in the
/// `worlds/london/london` and `worlds/london/london_cache` archives. These two archives seem to
/// depend on each other, as the [dependency archive hash] in one archive is equal to the
/// [archive hash header field] of the other:
///
/// ```text
/// worlds/london/london.fat
/// Archive hash: 0xA7E2977F3F32B98E
/// Dependencies: [Archive hash 0xB78228C0B350CC14, Name hash 0xBE38E2B5954E5FA4]
///
/// worlds/london/london_cache.fat
/// Archive hash: 0xB78228C0B350CC14
/// Dependencies: [Archive hash 0xA7E2977F3F32B98E, Name hash 0xB05864FD230CEA67]
/// ```
///
/// [`Fat5`]: FatVersion::Fat5
/// [dependency archive hash]: Self::archive_hash
/// [archive hash header field]: FatHeader::archive_hash
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Dependency {
    pub archive_hash: u64,
    pub name_hash: u64,
}

impl Dependency {
    /// Reads some bytes from `input` and converts them into a new `Dependency`.
    ///
    /// # Errors
    ///
    /// This function will return an I/O error if it fails to read enough bytes from `input`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Cursor;
    ///
    /// use disrupt_bigfile::header::Dependency;
    ///
    /// let mut dependency_bytes = Cursor::new([
    ///     // Dependency A
    ///     0x14, 0xCC, 0x50, 0xB3, 0xC0, 0x28, 0x82, 0xB7, 0xA4, 0x5F, 0x4E, 0x95, 0xB5, 0xE2,
    ///     0x38, 0xBE,
    ///     // Dependency B
    ///     0x8E, 0xB9, 0x32, 0x3F, 0x7F, 0x97, 0xE2, 0xA7, 0x67, 0xEA, 0x0C, 0x23, 0xFD, 0x64,
    ///     0x58, 0xB0,
    /// ]);
    ///
    /// // You can pass a mutable reference to your reader in order to avoid consuming it and use
    /// // it multiple times:
    /// let dependency_a = Dependency::deserialize(&mut dependency_bytes)?;
    /// let dependency_b = Dependency::deserialize(&mut dependency_bytes)?;
    ///
    /// assert_eq!(dependency_a.archive_hash, 0xB782_28C0_B350_CC14);
    /// assert_eq!(dependency_b.archive_hash, 0xA7E2_977F_3F32_B98E);
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn deserialize(mut input: impl Read) -> Result<Dependency, io::Error> {
        let archive_hash = input.read_u64::<LE>()?;
        let name_hash = input.read_u64::<LE>()?;

        Ok(Dependency {
            archive_hash,
            name_hash,
        })
    }

    /// Converts this `Dependency` into a binary format and writes the bytes to `out`.
    ///
    /// # Errors
    ///
    /// This function will return an I/O error if it fails to write all serialized bytes to `out`.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::Dependency;
    ///
    /// let dependency_a = Dependency {
    ///     archive_hash: 0xB782_28C0_B350_CC14,
    ///     name_hash: 0xBE38_E2B5_954E_5FA4,
    /// };
    /// let dependency_b = Dependency {
    ///     archive_hash: 0xA7E2_977F_3F32_B98E,
    ///     name_hash: 0xB058_64FD_230C_EA67,
    /// };
    ///
    /// let mut out = Vec::new();
    ///
    /// // You can pass a mutable reference to your writer in order to avoid consuming it and use
    /// // it multiple times:
    /// dependency_a.serialize(&mut out)?;
    /// dependency_b.serialize(&mut out)?;
    ///
    /// assert!(!out.is_empty());
    /// # Ok::<(), std::io::Error>(())
    /// ```
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

/// BigFile archive metadata.
///
/// This struct represents the header of a FAT file. It stores various information needed to read
/// and decompress the contents of the archive properly, such as the binary layout version of the
/// file [`Entries`], the supported [`CompressionScheme`]s, etc. See the the [field access] methods
/// for the full list of fields contained in a `FatHeader`.
///
/// Instances of `FatHeader` can be [converted to and from binary data], [constructed manually] by
/// specifying the value of each field or [constructed from several available presets] used in the
/// original games.
///
/// It is not possible to construct a `FatHeader` with an invalid combination of fields that makes
/// it impossible to be serialized.
///
/// [`Entries`]: crate::entry::Entry
/// [`CompressionScheme`]: crate::entry::CompressionScheme
/// [field access]: #field-access
/// [converted to and from binary data]: #serialization-and-deserialization
/// [constructed manually]: #construction
/// [constructed from several available presets]: #presets
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

/// # Construction
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

    /// Constructs a new header with the [`fat_version`] set to [`Fat3`] and the given
    /// `table_version`, `platform`, `compression_version` and `name_hash_version`.
    ///
    /// [`fat_version`]: Self::fat_version
    /// [`Fat3`]: FatVersion::Fat3
    ///
    /// # Errors
    ///
    /// This function will return a [`FatConstructionError::UnsupportedPlatform`] if the given
    /// `platform` does not have an [ID] assigned to it on `Fat3`.
    ///
    /// [ID]: Platform#platform-ids
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{
    ///     CompressionVersion, FatHeader, FatVersion, NameHashVersion, Platform, TableVersion,
    /// };
    ///
    /// let header = FatHeader::new_fat3(
    ///     TableVersion::V8,
    ///     Platform::Win64,
    ///     CompressionVersion::V5,
    ///     NameHashVersion::V50,
    /// )?;
    /// assert_eq!(header.fat_version(), FatVersion::Fat3);
    /// # Ok::<(), disrupt_bigfile::fat::FatConstructionError>(())
    /// ```
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

    /// Constructs a new header with the [`fat_version`] set to [`Fat5`] and the given
    /// `table_version`, `platform`, `compression_version`, `name_hash_version`, `archive_hash` and
    /// zero or more `dependencies`.
    ///
    /// [`fat_version`]: Self::fat_version
    /// [`Fat5`]: FatVersion::Fat5
    ///
    /// # Errors
    ///
    /// This function will return the following error variants:
    ///
    /// * [`UnsupportedPlatform`]: the given `platform` does not have an [ID] assigned to it on
    ///   `Fat5`.
    /// * [`DependencyCountWontFit`]: the length of `dependencies` exceeds [`u32::MAX`].
    ///
    /// [`UnsupportedPlatform`]: FatConstructionError::UnsupportedPlatform
    /// [`DependencyCountWontFit`]: FatConstructionError::DependencyCountWontFit
    /// [ID]: Platform#platform-ids
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{
    ///     CompressionVersion, Dependency, FatHeader, FatVersion, NameHashVersion, Platform,
    ///     TableVersion,
    /// };
    ///
    /// let header = FatHeader::new_fat5(
    ///     TableVersion::V13,
    ///     Platform::Win64,
    ///     CompressionVersion::V8,
    ///     NameHashVersion::V70,
    ///     0xA7E2_977F_3F32_B98E,
    ///     vec![Dependency {
    ///         archive_hash: 0xB782_28C0_B350_CC14,
    ///         name_hash: 0xBE38_E2B5_954E_5FA4,
    ///     }],
    /// )?;
    /// assert_eq!(header.fat_version(), FatVersion::Fat5);
    /// # Ok::<(), disrupt_bigfile::fat::FatConstructionError>(())
    /// ```
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
}

/// # Serialization and deserialization
impl FatHeader {
    /// Reads some bytes from `input` and converts them into a new `FatHeader`.
    ///
    /// # Errors
    ///
    /// This function will return the following error variants:
    ///
    /// * [`Io`]: failed to read enough bytes due to an I/O error.
    /// * [`BadMagic`]: the first four bytes are not equal to [`FAT3_MAGIC`] or [`FAT5_MAGIC`].
    /// * [`UnknownTableVersion`]: the table version field is not equal to any of the
    ///   [known table versions].
    /// * [`UnsupportedPlatformId`]: the platform ID field is not equal to any of the
    ///   [supported platform IDs] for the FAT version of this header.
    /// * [`UnknownCompressionVersion`]: the compression version field is not equal to any of the
    ///   [known compression versions].
    /// * [`UnknownNameHashVersion`]: the name hash version field is not equal to any of the
    ///   [known name hash versions].
    /// * [`UnexpectedPaddingByte`]: the `0x0B` byte (after the name hash version field) is not
    ///   equal to `00`.
    /// * [`DependencyAllocationFailed`]: failed to allocate enough memory to store the
    ///   dependencies, due to the system not having enough available memory or pointer width.
    ///
    /// [`Io`]: FatDeserializationError::Io
    /// [`BadMagic`]: FatDeserializationError::BadMagic
    /// [`UnknownTableVersion`]: FatDeserializationError::UnknownTableVersion
    /// [`UnsupportedPlatformId`]: FatDeserializationError::UnsupportedPlatformId
    /// [`UnknownCompressionVersion`]: FatDeserializationError::UnknownCompressionVersion
    /// [`UnknownNameHashVersion`]: FatDeserializationError::UnknownNameHashVersion
    /// [`UnexpectedPaddingByte`]: FatDeserializationError::UnexpectedPaddingByte
    /// [`DependencyAllocationFailed`]: FatDeserializationError::DependencyAllocationFailed
    ///
    /// [known table versions]: TableVersion
    /// [supported platform IDs]: Platform#platform-ids
    /// [known compression versions]: CompressionVersion
    /// [known name hash versions]: NameHashVersion
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{
    ///     CompressionVersion, FatHeader, FatVersion, Platform, TableVersion,
    /// };
    ///
    /// let data = [
    ///     0x35, 0x54, 0x41, 0x46, // Magic
    ///     0x0D, 0x00, 0x00, 0x00, // Table version
    ///     0x01, // Platform ID
    ///     0x08, // Compression version
    ///     0x46, // Name hash version
    ///     0x00, // Padding byte
    ///     0x8E, 0xB9, 0x32, 0x3F, 0x7F, 0x97, 0xE2, 0xA7, // Archive hash
    ///     0x01, 0x00, 0x00, 0x00, // Dependency count
    ///     0x14, 0xCC, 0x50, 0xB3, 0xC0, 0x28, 0x82, 0xB7, // Dependency archive hash
    ///     0xA4, 0x5F, 0x4E, 0x95, 0xB5, 0xE2, 0x38, 0xBE, // Dependency name hash
    /// ];
    ///
    /// let header = FatHeader::deserialize(data.as_slice())?;
    ///
    /// assert_eq!(header.fat_version(), FatVersion::Fat5);
    /// assert_eq!(header.table_version(), TableVersion::V13);
    /// assert_eq!(header.platform(), Platform::Win64);
    /// assert_eq!(header.compression_version(), CompressionVersion::V8);
    /// assert!(header.archive_hash().is_some_and(|hash| hash == 0xA7E2_977F_3F32_B98E));
    /// assert!(header.dependencies().is_some_and(|deps| deps.len() == 1));
    /// # Ok::<(), disrupt_bigfile::fat::FatDeserializationError>(())
    /// ```
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

    /// Converts this header into a binary format and writes the bytes to `out`.
    ///
    /// # Errors
    ///
    /// This function will return an I/O error if it fails to write all serialized bytes to `out`.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wd1_win64();
    /// let mut out = Vec::new();
    ///
    /// // You can pass a mutable reference to your writer in order to avoid consuming it and use
    /// // it afterwards:
    /// header.serialize(&mut out)?;
    ///
    /// assert!(!out.is_empty());
    /// # Ok::<(), std::io::Error>(())
    /// ```
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
}

/// # Field access
impl FatHeader {
    /// Returns the [`FatVersion`] of this header.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, FatVersion};
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// assert_eq!(header.fat_version(), FatVersion::Fat3);
    /// ```
    #[must_use]
    pub fn fat_version(&self) -> FatVersion {
        self.fat_version
    }

    /// Returns the [`TableVersion`] of this header.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, TableVersion};
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// assert_eq!(header.table_version(), TableVersion::V8);
    /// ```
    #[must_use]
    pub fn table_version(&self) -> TableVersion {
        self.table_version
    }

    /// Returns the [`Platform`] of this header.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// assert_eq!(header.platform(), Platform::Win64);
    /// ```
    #[must_use]
    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Returns the [`CompressionVersion`] of this header.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{CompressionVersion, FatHeader};
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// assert_eq!(header.compression_version(), CompressionVersion::V5);
    /// ```
    #[must_use]
    pub fn compression_version(&self) -> CompressionVersion {
        self.compression_version
    }

    /// Returns the [`NameHashVersion`] of this header.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, NameHashVersion};
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// assert_eq!(header.name_hash_version(), NameHashVersion::V50);
    /// ```
    #[must_use]
    pub fn name_hash_version(&self) -> NameHashVersion {
        self.name_hash_version
    }

    /// Returns the [`Fat5`]-only archive hash of this header.
    ///
    /// The purpose of this field is unclear. In most archives it is set to
    /// `0xFFFF_FFFF_FFFF_FFFF`, except for the `worlds/london/london` and
    /// `worlds/london/london_cache` archives in Watch Dogs: Legion, where this field is set to
    /// `0xA7E2_977F_3F32_B98E` and `0xB782_28C0_B350_CC14` respectively.
    ///
    /// This function will always return [`Some`] on [`FatVersion::Fat5`] headers and [`None`] on
    /// [`FatVersion::Fat3`] headers.
    ///
    /// [`Fat5`]: FatVersion::Fat5
    ///
    /// # Examples
    ///
    /// ```
    /// use std::assert_matches;
    ///
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wdl_win64_london();
    ///
    /// assert_matches!(header.archive_hash(), Some(0xA7E2_977F_3F32_B98E));
    /// ```
    #[must_use]
    pub fn archive_hash(&self) -> Option<u64> {
        self.archive_hash
    }

    /// Returns the [`Fat5`]-only [`Dependencies`] of this header.
    ///
    /// This function will always return [`Some`] on [`FatVersion::Fat5`] headers and [`None`] on
    /// [`FatVersion::Fat3`] headers.
    ///
    /// [`Fat5`]: FatVersion::Fat5
    /// [`Dependencies`]: Dependency
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wdl_win64_london();
    ///
    /// assert!(header.dependencies().is_some_and(|deps| deps.len() == 1));
    /// ```
    #[must_use]
    pub fn dependencies(&self) -> Option<&[Dependency]> {
        self.dependencies.as_deref()
    }
}

/// # Presets
///
/// These functions allow you to quickly construct a header with the same configuration as in the
/// original games.
///
/// ## Values
///
/// | Preset                   | [FAT]    | [Table]  | [Platform] | [Compression] | [Name hash] | [Archive hash]          | [Dependencies]                                                  |
/// |--------------------------|----------|----------|------------|---------------|-------------|-------------------------|-----------------------------------------------------------------|
/// | [`new_wd1_win64`]        | [`Fat3`] | [`TV8`]  | [`Win64`]  | [`CV5`]       | [`V50`]     |                         |                                                                 |
/// | [`new_wd1_win64_sound`]  | [`Fat3`] | [`TV8`]  | [`Any`]    | [`CV0`]       | [`V50`]     |                         |                                                                 |
/// | [`new_wd1_wiiu`]         | [`Fat3`] | [`TV8`]  | [`WiiU`]   | [`CV5`]       | [`V56`]     |                         |                                                                 |
/// | [`new_wd1_wiiu_sound`]   | [`Fat3`] | [`TV8`]  | [`Any`]    | [`CV0`]       | [`V56`]     |                         |                                                                 |
/// | [`new_wd2_win64`]        | [`Fat5`] | [`TV11`] | [`Win64`]  | [`CV6`]       | [`V70`]     | `0xFFFF_FFFF_FFFF_FFFF` | `[]`                                                            |
/// | [`new_wd2_ps4`]          | [`Fat5`] | [`TV11`] | [`Orbis`]  | [`CV9`]       | [`V70`]     | `0xFFFF_FFFF_FFFF_FFFF` | `[]`                                                            |
/// | [`new_wd2_sound`]        | [`Fat5`] | [`TV11`] | [`Any`]    | [`CV0`]       | [`V70`]     | `0xFFFF_FFFF_FFFF_FFFF` | `[]`                                                            |
/// | [`new_wdl_win64`]        | [`Fat5`] | [`TV13`] | [`Win64`]  | [`CV8`]       | [`V70`]     | `0xFFFF_FFFF_FFFF_FFFF` | `[]`                                                            |
/// | [`new_wdl_win64_london`] | [`Fat5`] | [`TV13`] | [`Win64`]  | [`CV8`]       | [`V70`]     | `0xA7E2_977F_3F32_B98E` | `[(archive 0xB782_28C0_B350_CC14; name 0xBE38_E2B5_954E_5FA4)]` |
/// | [`new_wdl_win64_london`] | [`Fat5`] | [`TV13`] | [`Win64`]  | [`CV8`]       | [`V70`]     | `0xB782_28C0_B350_CC14` | `[(archive 0xA7E2_977F_3F32_B98E; name 0xB058_64FD_230C_EA67)]` |
///
/// [`new_wd1_win64`]: Self::new_wd1_win64
/// [`new_wd1_win64_sound`]: Self::new_wd1_win64_sound
/// [`new_wd1_wiiu`]: Self::new_wd1_wiiu
/// [`new_wd1_wiiu_sound`]: Self::new_wd1_wiiu_sound
/// [`new_wd2_win64`]: Self::new_wd2_win64
/// [`new_wd2_ps4`]: Self::new_wd2_ps4
/// [`new_wd2_sound`]: Self::new_wd2_sound
/// [`new_wdl_win64`]: Self::new_wdl_win64
/// [`new_wdl_win64_london`]: Self::new_wdl_win64_london
/// [`new_wdl_win64_london_cache`]: Self::new_wdl_win64_london_cache
///
/// [FAT]: Self::fat_version
/// [Table]: Self::table_version
/// [Platform]: Self::platform
/// [Compression]: Self::compression_version
/// [Name hash]: Self::name_hash_version
/// [Archive hash]: Self::archive_hash
/// [Dependencies]: Self::dependencies
///
/// [`Fat3`]: FatVersion::Fat3
/// [`Fat5`]: FatVersion::Fat5
///
/// [`TV7`]: TableVersion::V7
/// [`TV8`]: TableVersion::V8
/// [`TV11`]: TableVersion::V11
/// [`TV13`]: TableVersion::V13
///
/// [`Any`]: Platform::Any
/// [`Win32`]: Platform::Win32
/// [`Xenon`]: Platform::Xenon
/// [`Ps3`]: Platform::Ps3
/// [`Win64`]: Platform::Win64
/// [`WiiU`]: Platform::WiiU
/// [`Orbis`]: Platform::Orbis
///
/// [`CV0`]: CompressionVersion::V0
/// [`CV4`]: CompressionVersion::V4
/// [`CV5`]: CompressionVersion::V5
/// [`CV6`]: CompressionVersion::V6
/// [`CV8`]: CompressionVersion::V8
/// [`CV9`]: CompressionVersion::V9
///
/// [`V50`]: NameHashVersion::V50
/// [`V55`]: NameHashVersion::V55
/// [`V56`]: NameHashVersion::V56
/// [`V58`]: NameHashVersion::V58
/// [`V70`]: NameHashVersion::V70
impl FatHeader {
    /// Creates a new header with the configuration used in most archives in the Windows version of
    /// Watch Dogs 1.
    ///
    /// See the [table] for the values of this preset.
    ///
    /// [table]: Self#values
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// assert_eq!(header.platform(), Platform::Win64);
    /// ```
    #[must_use]
    pub fn new_wd1_win64() -> Self {
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

    /// Creates a new header with the configuration used in the `sound*` archives in the Windows
    /// version of Watch Dogs 1.
    ///
    /// See the [table] for the values of this preset.
    ///
    /// [table]: Self#values
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let header = FatHeader::new_wd1_win64_sound();
    ///
    /// assert_eq!(header.platform(), Platform::Any);
    /// ```
    #[must_use]
    pub fn new_wd1_win64_sound() -> Self {
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

    /// Creates a new header with the configuration used in most archives in the Wii U version of
    /// Watch Dogs 1.
    ///
    /// See the [table] for the values of this preset.
    ///
    /// [table]: Self#values
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let header = FatHeader::new_wd1_wiiu();
    ///
    /// assert_eq!(header.platform(), Platform::WiiU);
    /// ```
    #[must_use]
    pub fn new_wd1_wiiu() -> Self {
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

    /// Creates a new header with the configuration used in the `sound*` archives in the Wii U
    /// version of Watch Dogs 1.
    ///
    /// See the [table] for the values of this preset.
    ///
    /// [table]: Self#values
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let header = FatHeader::new_wd1_wiiu_sound();
    ///
    /// assert_eq!(header.platform(), Platform::Any);
    /// ```
    #[must_use]
    pub fn new_wd1_wiiu_sound() -> Self {
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

    /// Creates a new header with the configuration used in most archives in the Windows version of
    /// Watch Dogs 2.
    ///
    /// See the [table] for the values of this preset.
    ///
    /// [table]: Self#values
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let header = FatHeader::new_wd2_win64();
    ///
    /// assert_eq!(header.platform(), Platform::Win64);
    /// ```
    #[must_use]
    pub fn new_wd2_win64() -> Self {
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

    /// Creates a new header with the configuration used in most archives in the PlayStation 4
    /// version of Watch Dogs 2.
    ///
    /// See the [table] for the values of this preset.
    ///
    /// [table]: Self#values
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let header = FatHeader::new_wd2_ps4();
    ///
    /// assert_eq!(header.platform(), Platform::Orbis);
    /// ```
    #[must_use]
    pub fn new_wd2_ps4() -> Self {
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

    /// Creates a new header with the configuration used in the `sound*` archives in Watch Dogs 2.
    ///
    /// See the [table] for the values of this preset.
    ///
    /// [table]: Self#values
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let header = FatHeader::new_wd2_sound();
    ///
    /// assert_eq!(header.platform(), Platform::Any);
    /// ```
    #[must_use]
    pub fn new_wd2_sound() -> Self {
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

    /// Creates a new header with the configuration used in most archives in the Windows version of
    /// Watch Dogs: Legion.
    ///
    /// See the [table] for the values of this preset.
    ///
    /// [table]: Self#values
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let header = FatHeader::new_wdl_win64();
    ///
    /// assert_eq!(header.platform(), Platform::Win64);
    /// ```
    #[must_use]
    pub fn new_wdl_win64() -> Self {
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

    /// Creates a new header with the configuration used in the `worlds/london/london` archive in
    /// the Windows version of Watch Dogs: Legion.
    ///
    /// See the [table] for the values of this preset.
    ///
    /// [table]: Self#values
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let header = FatHeader::new_wdl_win64_london();
    ///
    /// assert!(header.dependencies().is_some_and(|deps| deps.len() == 1));
    /// ```
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

    /// Creates a new header with the configuration used in the `worlds/london/london_cache`
    /// archive in the Windows version of Watch Dogs: Legion.
    ///
    /// See the [table] for the values of this preset.
    ///
    /// [table]: Self#values
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let header = FatHeader::new_wdl_win64_london_cache();
    ///
    /// assert!(header.dependencies().is_some_and(|deps| deps.len() == 1));
    /// ```
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

/// Displays [all fields] of this header. If the alternate flag (`#`) is specified, the information
/// will be printed in a more verbose, multiline format.
///
/// [all fields]: Self#field-access
///
/// # Examples
///
/// ```
/// use disrupt_bigfile::header::FatHeader;
///
/// let header = FatHeader::new_wd1_win64();
///
/// assert_eq!(
///     format!("{header}"), "FAT3, Table V8, Platform Win64, Compression V5, Name hash V50"
/// );
/// ```
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
