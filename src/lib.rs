mod compression;
pub mod filelists;

use std::error;
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use compression::{CompressionScheme, CompressionVersion};

use byteorder::{LE, ReadBytesExt};

#[derive(Debug)]
pub enum Error {
    IoError(io::Error),
    BadMagic(u32),
    UnsupportedEntryVersion(u32),
    UnknownPlatformId(u32),
    UnsupportedCompressionVersion(u32),
    UnknownCompressionScheme {
        compression_scheme_id: u8,
        compression_version: CompressionVersion,
    },
}

impl From<io::Error> for Error {
    fn from(io_error: io::Error) -> Self {
        Error::IoError(io_error)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::IoError(error) => write!(f, "io error: {error}"),
            Error::BadMagic(magic) => {
                write!(f, "bad magic 0x{magic:X}, expected 0x{FAT3_SIGNATURE:X}")
            }
            Error::UnsupportedEntryVersion(version) => write!(f, "unsupported version {version}"),
            Error::UnknownPlatformId(id) => write!(f, "unknown platform id {id}"),
            Error::UnsupportedCompressionVersion(version) => {
                write!(f, "unsupported compression version {version} for FAT3")
            }
            Error::UnknownCompressionScheme {
                compression_scheme_id,
                compression_version,
            } => write!(
                f,
                "unknown compression scheme {compression_scheme_id} for compression_version {compression_version}"
            ),
        }
    }
}

impl error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Platform {
    Any,
    Xenon,
    PS3,
    Win64,
    WiiU,
}

impl TryFrom<u32> for Platform {
    type Error = Error;

    fn try_from(id: u32) -> Result<Self> {
        match id {
            0 => Ok(Platform::Any),
            2 => Ok(Platform::Xenon),
            3 => Ok(Platform::PS3),
            4 => Ok(Platform::Win64),
            8 => Ok(Platform::WiiU),
            _ => Err(Error::UnknownPlatformId(id)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum EntryVersion {
    V8,
}

impl TryFrom<u32> for EntryVersion {
    type Error = Error;

    fn try_from(version: u32) -> Result<Self> {
        match version {
            8 => Ok(Self::V8),
            _ => Err(Error::UnsupportedEntryVersion(version)),
        }
    }
}

#[derive(Debug)]
pub struct Entry {
    pub name_hash: u64,
    pub offset: u64,
    pub compression_scheme: CompressionScheme,
    pub uncompressed_size: u32,
    pub compressed_size: u32,
}

impl Entry {
    fn deserialize(
        entry_version: EntryVersion,
        bytes: [u8; 16],
        compression_version: CompressionVersion,
    ) -> Result<Self> {
        match entry_version {
            EntryVersion::V8 => Self::deserialize_v8(bytes, compression_version),
        }
    }

    fn deserialize_v8(bytes: [u8; 16], compression_version: CompressionVersion) -> Result<Self> {
        // hhhhhhhh hhhhhhhh hhhhhhhh hhhhhhhh
        // uuuuuuuu uuuuuuuu uuuuuuuu uuuuusss
        // oooccccc cccccccc cccccccc cccccccc
        // oooooooo oooooooo oooooooo oooooooo

        // [h] hash = 32 bits
        // [u] uncompressed size = 29 bits
        // [s] compression scheme = 3 bits
        // [o] offset = 35 bits
        // [c] compressed size = 29 bits

        let a = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let b = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let c = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let d = u32::from_le_bytes(bytes[12..16].try_into().unwrap());

        let name_hash = a as u64;
        let mut uncompressed_size = b >> 3;
        let compression_scheme_id = (b & 7) as u8;
        let compressed_size = c & 0x1FFFFFFF;
        let offset = (d << 3 | c >> 29) as u64;

        let compression_scheme =
            CompressionScheme::from_scheme_id(compression_scheme_id, compression_version)?;
        // For some reason, if the entry's compression scheme is None, the uncompressed size is set
        // to 0, and compressed size is set to the actual size of the file. Let's set both to the
        // same value for convinience.
        if let CompressionScheme::None = compression_scheme {
            uncompressed_size = compressed_size;
        }

        Ok(Entry {
            name_hash,
            offset,
            compression_scheme,
            uncompressed_size,
            compressed_size,
        })
    }

    pub fn path(&self) -> PathBuf {
        if let Some(source) = filelists::HASH_SOURCE_MAP.get(&self.name_hash) {
            source.replace("\\", "/").into()
        } else {
            format!("_UNKNOWN/{:X}", self.name_hash).into()
        }
    }
}

const FAT3_SIGNATURE: u32 = 0x46415433; // "FAT3"

#[derive(Debug)]
pub struct Fat3 {
    pub entry_version: EntryVersion,
    pub platform: Platform,
    pub compression_version: CompressionVersion,
    pub entries: Vec<Entry>,
}

impl Fat3 {
    pub fn deserialize(data: &mut impl Read) -> Result<Self> {
        let magic = data.read_u32::<LE>()?;
        if magic != FAT3_SIGNATURE {
            return Err(Error::BadMagic(magic));
        }

        let entry_version = EntryVersion::try_from(data.read_u32::<LE>()?)?;

        let flags = data.read_u32::<LE>()?;
        let platform = Platform::try_from(flags & 0xFF)?;
        let compression_version = CompressionVersion::try_from(flags >> 8 & 0xFF)?;

        let entry_count = data.read_u32::<LE>()?;
        let mut entries = Vec::with_capacity(entry_count as usize);
        for _ in 0..entry_count {
            let mut entry_buf: [u8; 16] = [0; 16];
            data.read_exact(&mut entry_buf)?;

            let entry = Entry::deserialize(entry_version, entry_buf, compression_version)?;
            entries.push(entry);
        }

        Ok(Fat3 {
            entry_version,
            platform,
            compression_version,
            entries,
        })
    }
}
