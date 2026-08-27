//! FAT entry serialization and deserialization.

use std::borrow::Cow;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use byteorder::{BE, ReadBytesExt, WriteBytesExt};

use crate::fat::FatDeserializationError;
use crate::header::{CompressionVersion, TableVersion};
use crate::name_hash_db;

/// Errors that might happen when validating or serializing an [`Entry`].
#[derive(Debug, thiserror::Error)]
pub enum EntryError {
    /// Failed to write the entry's serialized bytes to an output due to an I/O error.
    #[error("io error")]
    Io(#[from] io::Error),

    /// The selected entry binary layout version does not have enough bits to store the name hash.
    #[error("name hash {hash:#X} is too large for current table version (expected {max:#X} max)")]
    NameHashWontFit { hash: u64, max: u64 },

    /// The selected entry binary layout version does not have enough bits to store the offset.
    #[error("offset {offset:#X} is too large for current table version (expected {max:#X} max)")]
    OffsetWontFit { offset: u64, max: u64 },

    /// The entry's [`CompressionScheme`] is not supported by the selected [`CompressionVersion`].
    #[error(
        "compression scheme {scheme} is not supported by compression version {compression_version}"
    )]
    UnsupportedCompressionScheme {
        scheme: CompressionScheme,
        compression_version: CompressionVersion,
    },

    /// The selected entry binary layout version does not have enough bits to store the
    /// uncompressed size.
    #[error("uncompressed size {size} is too large for current table version (expected {max} max)")]
    UncompressedSizeWontFit { size: u64, max: u64 },

    /// The selected entry binary layout version does not have enough bits to store the compressed
    /// size.
    #[error("compressed size {size} is too large for current table version (expected {max} max)")]
    CompressedSizeWontFit { size: u64, max: u64 },
}

/// The compression scheme that was used on the file described by an [`Entry`].
///
/// # Scheme IDs
///
/// The compression scheme is stored in the form of a binary number ID in the serialized FAT entry.
/// Each compression scheme's ID is dictated by the archive's [`CompressionVersion`]. This means
/// that depending on the compression version, some schemes may have different IDs and some
/// schemes may not have an ID assigned to them at all. In that case, it means that the compression
/// scheme is not supported by the current compression version.
///
/// The table below shows the meaning of each scheme ID on every compression version. Empty cell
/// means that no compression scheme is assigned to that ID on that compression version.
///
/// | ID | V0     | V4      | V5             | V6      | V8      | V9      |
/// |----|--------|---------|----------------|---------|---------|---------|
/// | 0  | `None` | `None`  | `None`         | `None`  | `None`  | `None`  |
/// | 1  |        | `Lzo1x` | `Lzo1x`        | `Lzma`  | `Oodle` | `Oodle` |
/// | 2  |        | `Zlib`  | `Zlib`         | `Lz4lw` | `Lzma`  | `Lzma`  |
/// | 3  |        |         | `Xmemcompress` |         | `Lz4lw` | `Lz4lw` |
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CompressionScheme {
    /// No compression.
    ///
    /// Used in Watch Dogs 1, 2 and Legion.
    None,

    /// [LZO1x](https://en.wikipedia.org/wiki/Lempel%E2%80%93Ziv%E2%80%93Oberhumer) compression.
    ///
    /// Used in the Wii U version of Watch Dogs 1.
    Lzo1x,

    /// [Zlib](https://en.wikipedia.org/wiki/Zlib) compression.
    Zlib,

    /// XMemCompress compression.
    ///
    /// Used in the Windows version of Watch Dogs 1.
    Xmemcompress,

    /// [LZMA](https://en.wikipedia.org/wiki/LZMA) compression.
    ///
    /// Used in the PS4 version of Watch Dogs 2.
    Lzma,

    /// LZ4LW compression, which is a slightly modified version of
    /// [LZ4](https://en.wikipedia.org/wiki/LZ4_(compression_algorithm)).
    ///
    /// Used in the Windows version of Watch Dogs 2 and Legion.
    Lz4lw,

    /// [Oodle](https://www.radgametools.com/oodle.htm) compression.
    Oodle,
}

impl fmt::Display for CompressionScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "no compression"),
            Self::Lzo1x => write!(f, "LZO1x"),
            Self::Zlib => write!(f, "Zlib"),
            Self::Xmemcompress => write!(f, "XMemCompress"),
            Self::Lzma => write!(f, "LZMA"),
            Self::Lz4lw => write!(f, "LZ4LW"),
            Self::Oodle => write!(f, "Oodle"),
        }
    }
}

impl CompressionScheme {
    /// Creates a compression scheme from its `scheme_id`, which is based on the archive's
    /// `compression_version`.
    ///
    /// See the [Scheme IDs] section of the enum documentation for details.
    ///
    /// # Errors
    ///
    /// This function will return [`UnknownCompressionScheme`] if the given `scheme_id` is unknown
    /// or unsupported for the given `compression_version`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::assert_matches;
    ///
    /// use disrupt_bigfile::entry::CompressionScheme;
    /// use disrupt_bigfile::header::CompressionVersion;
    ///
    /// let xmemcompress = CompressionScheme::try_from_scheme_id(3, CompressionVersion::V5);
    /// assert_matches!(xmemcompress, Ok(CompressionScheme::Xmemcompress));
    ///
    /// let unsupported = CompressionScheme::try_from_scheme_id(3, CompressionVersion::V0);
    /// assert!(unsupported.is_err());
    ///
    /// let unknown = CompressionScheme::try_from_scheme_id(123, CompressionVersion::V9);
    /// assert!(unsupported.is_err());
    /// ```
    ///
    /// [Scheme IDs]: Self#scheme-ids
    /// [`UnknownCompressionScheme`]: FatDeserializationError::UnknownCompressionScheme
    pub fn try_from_scheme_id(
        scheme_id: u8,
        compression_version: CompressionVersion,
    ) -> Result<Self, FatDeserializationError> {
        match (scheme_id, compression_version) {
            (0, _) => Ok(Self::None),
            (1, CompressionVersion::V4 | CompressionVersion::V5) => Ok(Self::Lzo1x),
            (2, CompressionVersion::V4 | CompressionVersion::V5) => Ok(Self::Zlib),
            (3, CompressionVersion::V5) => Ok(Self::Xmemcompress),
            (1, CompressionVersion::V6) | (2, CompressionVersion::V8 | CompressionVersion::V9) => {
                Ok(Self::Lzma)
            }
            (2, CompressionVersion::V6) | (3, CompressionVersion::V8 | CompressionVersion::V9) => {
                Ok(Self::Lz4lw)
            }
            (1, CompressionVersion::V8 | CompressionVersion::V9) => Ok(Self::Oodle),
            _ => Err(FatDeserializationError::UnknownCompressionScheme {
                scheme_id,
                compression_version,
            }),
        }
    }

    /// Converts this compression scheme to its scheme ID, based on the archive's
    /// `compression_version`.
    ///
    /// See the [Scheme IDs] section of the enum documentation for details.
    ///
    /// # Errors
    ///
    /// This function will return [`UnsupportedCompressionScheme`] if the given
    /// `compression_version` does not have an ID for this compression scheme.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::assert_matches;
    ///
    /// use disrupt_bigfile::entry::CompressionScheme;
    /// use disrupt_bigfile::header::CompressionVersion;
    ///
    /// let lz4lw = CompressionScheme::Lz4lw;
    ///
    /// assert_matches!(lz4lw.try_to_scheme_id(CompressionVersion::V6), Ok(2));
    /// assert_matches!(lz4lw.try_to_scheme_id(CompressionVersion::V8), Ok(3));
    /// assert!(lz4lw.try_to_scheme_id(CompressionVersion::V0).is_err());
    /// ```
    ///
    /// [Scheme IDs]: Self#scheme-ids
    /// [`UnsupportedCompressionScheme`]: EntryError::UnsupportedCompressionScheme
    pub fn try_to_scheme_id(
        self,
        compression_version: CompressionVersion,
    ) -> Result<u8, EntryError> {
        match (self, compression_version) {
            (Self::None, _) => Ok(0),

            (Self::Lzo1x, CompressionVersion::V4 | CompressionVersion::V5)
            | (Self::Lzma, CompressionVersion::V6)
            | (Self::Oodle, CompressionVersion::V8 | CompressionVersion::V9) => Ok(1),

            (Self::Zlib, CompressionVersion::V4 | CompressionVersion::V5)
            | (Self::Lz4lw, CompressionVersion::V6)
            | (Self::Lzma, CompressionVersion::V8 | CompressionVersion::V9) => Ok(2),

            (Self::Xmemcompress, CompressionVersion::V5)
            | (Self::Lz4lw, CompressionVersion::V8 | CompressionVersion::V9) => Ok(3),

            _ => Err(EntryError::UnsupportedCompressionScheme {
                scheme: self,
                compression_version,
            }),
        }
    }

    /// Checks if `compression_version` has an ID for this compression scheme.
    ///
    /// See the [Scheme IDs] section of the enum documentation for details.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::entry::CompressionScheme;
    /// use disrupt_bigfile::header::CompressionVersion;
    ///
    /// let lzma = CompressionScheme::Lzma;
    ///
    /// assert!(lzma.is_supported_for(CompressionVersion::V6));
    /// assert!(!lzma.is_supported_for(CompressionVersion::V0));
    /// ```
    ///
    /// [Scheme IDs]: Self#scheme-ids
    #[must_use]
    pub fn is_supported_for(self, compression_version: CompressionVersion) -> bool {
        self.try_to_scheme_id(compression_version).is_ok()
    }
}

/// Information about an archived file.
///
/// This struct represents an entry in the index of a FAT file. It contains the information needed
/// to locate and extract an archived file from a [`Dat`].
///
/// `Entry` instances can be constructed manually, deserialized from binary data using
/// [`Self::deserialize`], or created when adding files to a new archive using [`ArchiveBuilder`].
///
/// # Compression and binary format versions TODO TODO TODO
///
/// `Entry` instances can be converted to and from a binary format that has multiple versions.
/// The entry format version used in a FAT file is dictated by the [`TableVersion`] header field.
///
/// Every version has a different amount of bits allocated to each field. This means that the same
/// instance of `Entry` that can be serialized to one version without any problems, may have fields
/// that are too large for some other version.
///
/// Also, not all [`CompressionScheme`]s are supported in an archive. The availability of each
/// scheme is dictated by the [`CompressionVersion`] header field, see the
/// [Scheme IDs section of the documentation for `CompressionScheme`](CompressionScheme#scheme-ids).
///
/// Use [`Self::validate`] to check if an `Entry` can be serialized to a given format and
/// compression versions. Use [`Self::serialize`] to validate and serialize at the same time, or
/// [`Self::serialize_unchecked`] to skip validation if you guarantee that your `Entry` is valid
/// for the given format version.
///
/// [`Dat`]: crate::dat::Dat
/// [`ArchiveBuilder`]: crate::builder::ArchiveBuilder
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Entry {
    /// [FNV-1 hash](https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function#FNV-1_hash)
    /// of the entry's filename.
    ///
    /// The hash is 32 or 64 bit, depending on the [`TableVersion`].
    ///
    /// Use [`Self::path`] to try to look up the source of this hash.
    pub name_hash: u64,

    /// Offset at which the file's start is located in the DAT.
    pub offset: u64,

    /// Compression scheme that should be used when extracting this file.
    pub compression_scheme: CompressionScheme,

    /// Expected size (in bytes) of the file after decompression.
    ///
    /// In the original binary format, if the compression scheme is set to
    /// [`CompressionScheme::None`], then this field will always be set to 0 and the compressed
    /// size will be set to the actual size of the file.
    ///
    /// For convinience, during deserialization both the compressed and uncompressed size fields
    /// will be set to the file's size. This behavior will be handled during serialization as well.
    pub uncompressed_size: u64,

    /// Size (in bytes) of the raw, compressed file in the DAT.
    pub compressed_size: u64,
}

impl Entry {
    // V7 layout

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
    const V7_MAX_SIZE: u64 = 2u64.pow(30);

    fn deserialize_v7(
        mut entry_bytes: &[u8],
        compression_version: CompressionVersion,
    ) -> Result<Entry, FatDeserializationError> {
        let a = entry_bytes.read_u64::<BE>()?;
        let b = entry_bytes.read_u32::<BE>()?;
        let c = entry_bytes.read_u32::<BE>()?;

        let offset = a >> 30;
        let compressed_size = a & 0x3FFF_FFFF;
        let uncompressed_size = (b >> 2).into();
        let compression_scheme = CompressionScheme::try_from_scheme_id(
            (b & 0b11).try_into().expect("2 bit int should fit into u8"),
            compression_version,
        )?;
        let name_hash = c.into();

        Ok(Entry {
            name_hash,
            offset,
            compression_scheme,
            uncompressed_size,
            compressed_size,
        })
    }

    fn serialize_v7(
        self,
        buf: &mut Vec<u8>,
        uncompressed_size: u64,
        compression_scheme_id: u8,
    ) -> Result<(), io::Error> {
        buf.reserve(16);

        let a = (self.offset << 30) | self.compressed_size;
        let b =
            (uncompressed_size << 34) | (u64::from(compression_scheme_id) << 32) | self.name_hash;

        buf.write_u64::<BE>(a)?;
        buf.write_u64::<BE>(b)?;

        Ok(())
    }

    // V8 layout

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
    const V8_MAX_SIZE: u64 = 2u64.pow(29);

    fn deserialize_v8(
        mut entry_bytes: &[u8],
        compression_version: CompressionVersion,
    ) -> Result<Entry, FatDeserializationError> {
        let a = entry_bytes.read_u64::<BE>()?;
        let b = entry_bytes.read_u32::<BE>()?;
        let c = entry_bytes.read_u32::<BE>()?;

        let offset = a >> 29;
        let compressed_size = a & 0x1FFF_FFFF;
        let uncompressed_size = (b >> 3).into();
        let compression_scheme = CompressionScheme::try_from_scheme_id(
            (b & 0b111)
                .try_into()
                .expect("3 bit int should fit into u8"),
            compression_version,
        )?;
        let name_hash = c.into();

        Ok(Entry {
            name_hash,
            offset,
            compression_scheme,
            uncompressed_size,
            compressed_size,
        })
    }

    fn serialize_v8(
        self,
        buf: &mut Vec<u8>,
        uncompressed_size: u64,
        compression_scheme_id: u8,
    ) -> Result<(), io::Error> {
        buf.reserve(16);

        let a = (self.offset << 29) | self.compressed_size;
        let b =
            (uncompressed_size << 35) | (u64::from(compression_scheme_id) << 3) | self.name_hash;

        buf.write_u64::<BE>(a)?;
        buf.write_u64::<BE>(b)?;

        Ok(())
    }

    // V11/V13 layout

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
    const V11_V13_MAX_SIZE: u64 = 2u64.pow(30);

    fn deserialize_v11_v13(
        mut entry_bytes: &[u8],
        compression_version: CompressionVersion,
    ) -> Result<Entry, FatDeserializationError> {
        let a = entry_bytes.read_u32::<BE>()?;
        let b = entry_bytes.read_u64::<BE>()?;
        let c = entry_bytes.read_u64::<BE>()?;

        let uncompressed_size = (a >> 2).into();
        let compression_scheme = CompressionScheme::try_from_scheme_id(
            (a & 0b11).try_into().expect("2 bit int should fit into u8"),
            compression_version,
        )?;
        let offset = b >> 30;
        let compressed_size = b & 0x3FFF_FFFF;
        let name_hash = c;

        Ok(Entry {
            name_hash,
            offset,
            compression_scheme,
            uncompressed_size,
            compressed_size,
        })
    }

    fn serialize_v11_v13(
        self,
        buf: &mut Vec<u8>,
        uncompressed_size: u64,
        compression_scheme_id: u8,
    ) -> Result<(), io::Error> {
        buf.reserve(20);

        let uncompressed_size: u32 = uncompressed_size
            .try_into()
            .expect("caller should guarantee uncompressed_size fits into u32");

        let a = (uncompressed_size << 2) | u32::from(compression_scheme_id);
        let b = (self.offset << 30) | self.compressed_size;
        let c = self.name_hash;

        buf.write_u32::<BE>(a)?;
        buf.write_u64::<BE>(b)?;
        buf.write_u64::<BE>(c)?;

        Ok(())
    }

    /// Reads some bytes from `input` and constructs a new entry from the binary format according
    /// to the FAT file's `table_version` and `compression_version`.
    ///
    /// # Errors
    ///
    /// This function will return an error in the following cases:
    ///
    /// - [`Io`]: failed to read enough bytes due to an I/O error, such as an unexpected EOF, etc.
    /// - [`UnknownCompressionScheme`]: the compression scheme ID in the binary representation was
    ///   unknown or not supported by the given `compression_version`.
    ///
    /// [`Io`]: FatDeserializationError::Io
    /// [`UnknownCompressionScheme`]: FatDeserializationError::UnknownCompressionScheme
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Cursor;
    ///
    /// use disrupt_bigfile::entry::Entry;
    /// use disrupt_bigfile::header::{CompressionVersion, TableVersion};
    ///
    /// let mut entry_bytes = Cursor::new([
    ///     // Entry A
    ///     0xA0, 0x76, 0x94, 0x00, 0x33, 0x5B, 0x22, 0x00, 0xE4, 0xF5, 0x00, 0xE0, 0x89, 0xD6,
    ///     0x08, 0x00,
    ///     // Entry B
    ///     0xA3, 0x1B, 0xC2, 0x00, 0x23, 0x4A, 0x00, 0x00, 0xA2, 0x02, 0x00, 0x20, 0xF4, 0x79,
    ///     0x06, 0x00,
    /// ]);
    ///
    /// let table_version = TableVersion::V8;
    /// let compression_version = CompressionVersion::V5;
    ///
    /// // You can pass a mutable reference to your reader in order to avoid consuming it and use
    /// // it multiple times:
    /// let entry_a = Entry::deserialize(&mut entry_bytes, table_version, compression_version)?;
    /// let entry_b = Entry::deserialize(&mut entry_bytes, table_version, compression_version)?;
    ///
    /// assert_eq!(entry_a.name_hash, 0x9476A0);
    /// assert_eq!(entry_b.name_hash, 0xC21BA3);
    /// # Ok::<(), disrupt_bigfile::fat::FatDeserializationError>(())
    /// ```
    pub fn deserialize(
        mut input: impl Read,
        table_version: TableVersion,
        compression_version: CompressionVersion,
    ) -> Result<Self, FatDeserializationError> {
        let entry_length = match table_version {
            TableVersion::V7 | TableVersion::V8 => 16,
            TableVersion::V11 | TableVersion::V13 => 20,
        };
        let mut buf = vec![0; entry_length];

        input.read_exact(&mut buf)?;

        buf.reverse();

        let deserializer = match table_version {
            TableVersion::V7 => Self::deserialize_v7,
            TableVersion::V8 => Self::deserialize_v8,
            TableVersion::V11 | TableVersion::V13 => Self::deserialize_v11_v13,
        };
        let mut entry = deserializer(&buf, compression_version)?;

        // For some reason, if the entry's compression scheme is None, uncompressed size is set to
        // 0 and compressed size is set to the size of the entry. Let's set both to the same value
        // for convinience.
        if entry.compression_scheme == CompressionScheme::None {
            entry.uncompressed_size = entry.compressed_size;
        }

        Ok(entry)
    }

    /// Checks if this entry can be correctly serialized to a given `table_version` and
    /// `compression_version`, returning an [`Ok`] if it is invalid or an [`Err`] containing the
    /// reason otherwise.
    ///
    /// In most cases, you should use [`Self::serialize`] if just you need to validate and
    /// serialize an entry at the same time.
    ///
    /// # Errors
    ///
    /// If this entry is invalid, this function will return an [`EntryError`] describing exactly
    /// what's wrong with it.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    /// use disrupt_bigfile::header::{CompressionVersion, TableVersion};
    ///
    /// let valid_entry = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 0xCAFE_BABE,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 4096,
    ///     compressed_size: 2048,
    /// };
    /// let invalid_entry = Entry {
    ///     name_hash: u64::MAX,
    ///     offset: u64::MAX,
    ///     compression_scheme: CompressionScheme::Lzma,
    ///     uncompressed_size: u64::MAX,
    ///     compressed_size: u64::MAX,
    /// };
    ///
    /// let table_version = TableVersion::V8;
    /// let compression_version = CompressionVersion::V5;
    ///
    /// assert!(valid_entry.validate(table_version, compression_version).is_ok());
    /// assert!(invalid_entry.validate(table_version, compression_version).is_err());
    /// ```
    pub fn validate(
        self,
        table_version: TableVersion,
        compression_version: CompressionVersion,
    ) -> Result<Self, EntryError> {
        let (max_name_hash, max_offset, max_size) = match table_version {
            TableVersion::V7 => (
                Self::V7_MAX_NAME_HASH,
                Self::V7_MAX_OFFSET,
                Self::V7_MAX_SIZE,
            ),
            TableVersion::V8 => (
                Self::V8_MAX_NAME_HASH,
                Self::V8_MAX_OFFSET,
                Self::V8_MAX_SIZE,
            ),
            TableVersion::V11 | TableVersion::V13 => (
                Self::V11_V13_MAX_NAME_HASH,
                Self::V11_V13_MAX_OFFSET,
                Self::V11_V13_MAX_SIZE,
            ),
        };

        if self.name_hash > max_name_hash {
            return Err(EntryError::NameHashWontFit {
                hash: self.name_hash,
                max: max_name_hash,
            });
        }

        if self.offset > max_offset {
            return Err(EntryError::OffsetWontFit {
                offset: self.offset,
                max: max_offset,
            });
        }

        if !self
            .compression_scheme
            .is_supported_for(compression_version)
        {
            return Err(EntryError::UnsupportedCompressionScheme {
                scheme: self.compression_scheme,
                compression_version,
            });
        }

        if self.uncompressed_size > max_size {
            return Err(EntryError::UncompressedSizeWontFit {
                size: self.uncompressed_size,
                max: max_size,
            });
        }

        if self.compressed_size > max_size {
            return Err(EntryError::CompressedSizeWontFit {
                size: self.compressed_size,
                max: max_size,
            });
        }

        Ok(self)
    }

    /// Converts this entry to a binary format according to the FAT file's `table_version` and
    /// `compression_version` and writes the bytes to `out`, **without validating the entry**.
    ///
    /// The caller should guarantee that this entry is valid for the given `table_version` and
    /// `compression_version`, either manually or using [`Self::validate`]. Violating this
    /// guarantee is considered a logic error and may lead to invalid outputs or panics. However,
    /// this function is completely memory safe and will not lead to [undefined behavior] under any
    /// circumstances.
    ///
    /// Use [`Self::serialize`] if you want to validate and serialize the entry at the same time.
    ///
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    ///
    /// # Panics
    ///
    /// This function will panic if this entry's compression scheme is not supported by the given
    /// `compression_version` or if any of the fields are too large for the given `table_version`.
    ///
    /// # Errors
    ///
    /// This function will return an I/O error if writing the serialized bytes to `out` fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    /// use disrupt_bigfile::header::{CompressionVersion, TableVersion};
    ///
    /// let entry = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 0xCAFE_BABE,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 4096,
    ///     compressed_size: 2048,
    /// };
    /// let table_version = TableVersion::V8;
    /// let compression_version = CompressionVersion::V5;
    ///
    /// let mut out = Vec::new();
    ///
    /// entry.validate(table_version, compression_version)?
    ///     .serialize_unchecked(&mut out, table_version, compression_version)?;
    ///
    /// assert!(!out.is_empty());
    /// # Ok::<(), disrupt_bigfile::entry::EntryError>(())
    /// ```
    pub fn serialize_unchecked(
        self,
        mut out: impl Write,
        table_version: TableVersion,
        compression_version: CompressionVersion,
    ) -> Result<(), io::Error> {
        // See comment in deserialize()
        let uncompressed_size = if self.compression_scheme == CompressionScheme::None {
            0
        } else {
            self.uncompressed_size
        };
        let compression_scheme_id = self
            .compression_scheme
            .try_to_scheme_id(compression_version)
            .expect("caller should guarantee that compression scheme is supported by current compression version");

        let mut buf = Vec::new();

        let serializer = match table_version {
            TableVersion::V7 => Self::serialize_v7,
            TableVersion::V8 => Self::serialize_v8,
            TableVersion::V11 | TableVersion::V13 => Self::serialize_v11_v13,
        };
        serializer(self, &mut buf, uncompressed_size, compression_scheme_id)?;

        buf.reverse();

        out.write_all(&buf)?;

        Ok(())
    }

    /// Validates this entry, converts it to a binary format according to the FAT file's
    /// `table_version` and `compression_version` and writes the bytes to `out`.
    ///
    /// If you are absolutely sure that this entry is valid and want to skip validation performed
    /// by this function, use the [`Self::serialize_unchecked`] function.
    ///
    /// # Errors
    ///
    /// This function will return an [`EntryError::Io`] if it fails to write the serialized bytes
    /// to `out` due to an I/O error, or other [`EntryError`] variants if this entry could not be
    /// serialized due to it being invalid for the given `table_version` and `compression_version`.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    /// use disrupt_bigfile::header::{CompressionVersion, TableVersion};
    ///
    /// let entry_a = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 0xCAFE_BABE,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 4096,
    ///     compressed_size: 2048,
    /// };
    /// let entry_b = Entry {
    ///     name_hash: 0xFEED_FACE,
    ///     offset: 0xCAFE_FEED,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 16384,
    ///     compressed_size: 8192,
    /// };
    /// let table_version = TableVersion::V8;
    /// let compression_version = CompressionVersion::V5;
    ///
    /// let mut out = Vec::new();
    ///
    /// // You can pass a mutable reference to your reader in order to avoid consuming it and use
    /// // it multiple times:
    /// entry_a.serialize(&mut out, table_version, compression_version)?;
    /// entry_b.serialize(&mut out, table_version, compression_version)?;
    ///
    /// assert!(!out.is_empty());
    /// # Ok::<(), disrupt_bigfile::entry::EntryError>(())
    /// ```
    pub fn serialize(
        self,
        out: impl Write,
        table_version: TableVersion,
        compression_version: CompressionVersion,
    ) -> Result<(), EntryError> {
        self.validate(table_version, compression_version)?
            .serialize_unchecked(out, table_version, compression_version)
            .map_err(EntryError::Io)
    }

    /// Tries to find the name of this entry by looking up its [name hash](Self::name_hash).
    ///
    /// This function will return a [`Cow::Borrowed`] containing a `'static` reference to the
    /// entry's file name if it was found, otherwise it will return a [`Cow::Owned`] with a
    /// placeholder name in the form of `__UNKNOWN/<HEX_NAME_HASH>`
    /// (for example, `__UNKNOWN/9DFB477E`).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    ///
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    /// use disrupt_bigfile::header::{CompressionVersion, TableVersion};
    ///
    /// let known_entry = Entry {
    ///     name_hash: 0x2672_724B,
    ///     offset: 0x5A75607,
    ///     compression_scheme: CompressionScheme::Xmemcompress,
    ///     uncompressed_size: 288_650,
    ///     compressed_size: 58_070,
    /// };
    /// let unknown_entry = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 0xCAFE_BABE,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 4096,
    ///     compressed_size: 2048,
    /// };
    ///
    /// assert_eq!(known_entry.path(), Path::new("credits/pc/credits.xml"));
    /// assert_eq!(unknown_entry.path(), Path::new("__UNKNOWN/DEADBEEF"));
    /// ```
    #[must_use]
    pub fn path(self) -> Cow<'static, Path> {
        if let Some(path) = name_hash_db::get(self.name_hash) {
            Cow::Borrowed(path.as_ref())
        } else {
            let unknown_path = PathBuf::from(format!("__UNKNOWN/{:X}", self.name_hash));

            Cow::Owned(unknown_path)
        }
    }
}

/// Displays the entry's name via [`Self::path`]. If the alternate flag (`#`) is specified, the
/// uncompressed and uncompressed size, compression scheme and offset will also be displayed.
///
/// # Examples
///
/// ```
/// use disrupt_bigfile::entry::{CompressionScheme, Entry};
/// use disrupt_bigfile::header::{CompressionVersion, TableVersion};
///
/// let entry = Entry {
///     name_hash: 0x2672_724B,
///     offset: 0x5A75607,
///     compression_scheme: CompressionScheme::Xmemcompress,
///     uncompressed_size: 288_650,
///     compressed_size: 58_070,
/// };
///
/// assert_eq!(format!("{entry}"), "credits/pc/credits.xml");
/// assert_eq!(
///     format!("{entry:#}"),
///     "288650B (58070B XMemCompress) @ 0x5A75607: credits/pc/credits.xml"
/// );
/// ```
impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "{}B ", self.uncompressed_size)?;
            if self.compression_scheme != CompressionScheme::None {
                write!(
                    f,
                    "({}B {}) ",
                    self.compressed_size, self.compression_scheme
                )?;
            }
            write!(f, "@ {:#X}: ", self.offset)?;
        }
        write!(f, "{}", self.path().display())
    }
}
