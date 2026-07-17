use std::borrow::Cow;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use byteorder::{BE, ReadBytesExt, WriteBytesExt};

use crate::fat::FatDeserializationError;
use crate::header::{CompressionVersion, TableVersion};
use crate::name_hash_db;

#[derive(Debug, thiserror::Error)]
pub enum EntryError {
    #[error("io error")]
    Io(#[from] io::Error),

    #[error("name hash 0x{hash:X} is too large for current table version (expected 0x{max:X} max)")]
    NameHashWontFit { hash: u64, max: u64 },

    #[error("offset 0x{offset:X} is too large for current table version (expected 0x{max:X} max)")]
    OffsetWontFit { offset: u64, max: u64 },

    #[error(
        "compression scheme {scheme} is not supported by compression version {compression_version}"
    )]
    UnsupportedCompressionScheme {
        scheme: CompressionScheme,
        compression_version: CompressionVersion,
    },

    #[error("uncompressed size {size} is too large for current table version (expected {max} max)")]
    UncompressedSizeWontFit { size: u64, max: u64 },

    #[error("compressed size {size} is too large for current table version (expected {max} max)")]
    CompressedSizeWontFit { size: u64, max: u64 },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CompressionScheme {
    None,
    Lzo1x,
    Zlib,
    Xmemcompress,
    Lzma,
    Lz4lw,
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

    #[must_use]
    pub fn is_supported_for(self, compression_version: CompressionVersion) -> bool {
        self.try_to_scheme_id(compression_version).is_ok()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Entry {
    pub name_hash: u64,
    pub offset: u64,
    pub compression_scheme: CompressionScheme,
    pub uncompressed_size: u64,
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

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}B ", self.uncompressed_size)?;
        if self.compression_scheme != CompressionScheme::None {
            write!(
                f,
                "({}B {}) ",
                self.compressed_size, self.compression_scheme
            )?;
        }
        write!(f, "@ 0x{:X}: {}", self.offset, self.path().display())
    }
}
