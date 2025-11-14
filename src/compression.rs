use std::fmt;
use std::io::{self, Read, Seek, Write};

use byteorder::{BE, ReadBytesExt};
use lzxd::{self, Lzxd, WindowSize};

use crate::{FatError, FatResult};

#[derive(Clone, Copy, Debug)]
pub enum CompressionVersion {
    V0,
    V4,
    V5,
}

impl TryFrom<u32> for CompressionVersion {
    type Error = FatError;

    fn try_from(value: u32) -> FatResult<Self> {
        match value {
            0 => Ok(Self::V0),
            4 => Ok(Self::V4),
            5 => Ok(Self::V5),
            _ => Err(FatError::UnsupportedCompressionVersion(value)),
        }
    }
}

impl fmt::Display for CompressionVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V0 => write!(f, "V0"),
            Self::V4 => write!(f, "V4"),
            Self::V5 => write!(f, "V5"),
        }
    }
}

#[derive(Debug)]
pub enum CompressionScheme {
    None,
    LZO1x,
    Zlib,
    XMemCompress,
}

impl fmt::Display for CompressionScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "no compression"),
            Self::LZO1x => write!(f, "LZO1x"),
            Self::Zlib => write!(f, "Zlib"),
            Self::XMemCompress => write!(f, "XMemCompress"),
        }
    }
}

impl CompressionScheme {
    pub fn from_scheme_id(
        compression_scheme_id: u8,
        compression_version: CompressionVersion,
    ) -> FatResult<Self> {
        match (compression_version, compression_scheme_id) {
            (_, 0) => Ok(Self::None),
            (CompressionVersion::V4 | CompressionVersion::V5, 1) => Ok(Self::LZO1x),
            (CompressionVersion::V4 | CompressionVersion::V5, 2) => Ok(Self::Zlib),
            (CompressionVersion::V5, 3) => Ok(Self::XMemCompress),
            _ => Err(FatError::UnsupportedCompressionScheme {
                compression_scheme_id,
                compression_version,
            }),
        }
    }
}

#[derive(Debug)]
pub enum XMemCompressError {
    IoError(io::Error),
    BadMagic(u32),
    UnknownVersion(u16),
    UnexpectedReserved(u16),
    UnknownContextFlags(u32),
    UnknownFlags(u32),
    UnexpectedWindowSize(u32),
    UnexpectedCompressionPartitionSize(u32),
    LzxdError(lzxd::DecompressError),
}

impl From<io::Error> for XMemCompressError {
    fn from(io_error: io::Error) -> Self {
        Self::IoError(io_error)
    }
}

impl From<lzxd::DecompressError> for XMemCompressError {
    fn from(lzxd_error: lzxd::DecompressError) -> Self {
        Self::LzxdError(lzxd_error)
    }
}

impl fmt::Display for XMemCompressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoError(error) => write!(f, "io error: {error}"),
            Self::BadMagic(magic) => write!(
                f,
                "bad magic 0x{magic:X} in compressed file's header, expected 0x{XMEMCOMPRESS_LZXNATIVE_SIGNATURE:X}"
            ),
            Self::UnknownVersion(version) => write!(
                f,
                "unknown version 0x{version:X} in compressed file's header, expected 0x{XMEMCOMPRESS_VERSION:X}"
            ),
            Self::UnexpectedReserved(reserved) => write!(
                f,
                "unexpected data 0x{reserved:X} in reserved, expected 0x{XMEMCOMPRESS_RESERVED:X}"
            ),
            Self::UnknownContextFlags(cflags) => write!(
                f,
                "unknown context flags 0x{cflags:X}, expected 0x{XMEMCOMPRESS_CONTEXT_FLAGS:X}"
            ),
            Self::UnknownFlags(flags) => write!(
                f,
                "unknown flags 0x{flags:X}, expected 0x{XMEMCOMPRESS_FLAGS:X}"
            ),
            Self::UnexpectedWindowSize(size) => write!(
                f,
                "unexpected window size {size}, expected {XMEMCOMPRESS_EXPECTED_WINDOW_SIZE}"
            ),
            Self::UnexpectedCompressionPartitionSize(part_size) => write!(
                f,
                "unexpected compression partition size {part_size}, expected {XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE}"
            ),
            Self::LzxdError(error) => write!(f, "lzxd error: {error}"),
        }
    }
}

impl std::error::Error for XMemCompressError {}

const XMEMCOMPRESS_LZXNATIVE_SIGNATURE: u32 = 0x0FF512EE;
const XMEMCOMPRESS_VERSION: u16 = 0x0103;
const XMEMCOMPRESS_RESERVED: u16 = 0x0;
const XMEMCOMPRESS_CONTEXT_FLAGS: u32 = 0x0;
const XMEMCOMPRESS_FLAGS: u32 = 0x0;
const XMEMCOMPRESS_EXPECTED_WINDOW_SIZE: u32 = 32768; // 32 KiB
const XMEMCOMPRESS_LZXD_WINDOW_SIZE: WindowSize = WindowSize::KB32;
const XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE: u32 = 32768;

pub fn decompress_xmemcompress(
    compressed_data: &mut (impl Read + Seek),
    out_buf: &mut impl Write,
) -> Result<(), XMemCompressError> {
    let magic = compressed_data.read_u32::<BE>()?;
    if magic != XMEMCOMPRESS_LZXNATIVE_SIGNATURE {
        return Err(XMemCompressError::BadMagic(magic));
    }

    let version = compressed_data.read_u16::<BE>()?;
    if version != XMEMCOMPRESS_VERSION {
        return Err(XMemCompressError::UnknownVersion(version));
    }

    let reserved = compressed_data.read_u16::<BE>()?;
    if reserved != XMEMCOMPRESS_RESERVED {
        return Err(XMemCompressError::UnexpectedReserved(reserved));
    }

    let context_flags = compressed_data.read_u32::<BE>()?;
    if context_flags != XMEMCOMPRESS_CONTEXT_FLAGS {
        return Err(XMemCompressError::UnknownContextFlags(context_flags));
    }

    let flags = compressed_data.read_u32::<BE>()?;
    if flags != XMEMCOMPRESS_FLAGS {
        return Err(XMemCompressError::UnknownFlags(flags));
    }

    let window_size = compressed_data.read_u32::<BE>()?;
    if window_size != XMEMCOMPRESS_EXPECTED_WINDOW_SIZE {
        return Err(XMemCompressError::UnexpectedWindowSize(window_size));
    }

    let compression_partition_size = compressed_data.read_u32::<BE>()?;
    if compression_partition_size != XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE {
        return Err(XMemCompressError::UnexpectedCompressionPartitionSize(
            compression_partition_size,
        ));
    }

    compressed_data.seek_relative(4)?; // Skip uncompressed_size_high

    let uncompressed_size_low = compressed_data.read_u32::<BE>()?;

    // Skip compressed_size_high, compressed_size_low, uncompressed_block_size,
    // compressed_block_size
    compressed_data.seek_relative(16)?;

    let mut lzxd_context = Lzxd::new(XMEMCOMPRESS_LZXD_WINDOW_SIZE);

    let expected_block_count =
        uncompressed_size_low.div_ceil(XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE);
    for block_num in 0..expected_block_count {
        let compressed_block_size = compressed_data.read_u32::<BE>()?;
        let mut compressed_block_buf = vec![0; compressed_block_size.try_into().unwrap()];

        compressed_data.read_exact(&mut compressed_block_buf)?;

        let uncompressed_block_size = if block_num != expected_block_count - 1 {
            // This is not the last block, so it's size is
            // XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE (32 KiB)
            XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE
        } else {
            // This is the last block, which means it may be less than
            // XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE (32 KiB)
            uncompressed_size_low - block_num * XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE
        };

        let decompressed_block = lzxd_context.decompress_next(
            &compressed_block_buf,
            uncompressed_block_size.try_into().unwrap(),
        )?;

        out_buf.write_all(decompressed_block)?;
    }

    out_buf.flush()?;

    Ok(())
}

#[derive(Debug)]
pub enum DecompressionError {
    IoError(io::Error),
    XMemCompressError(XMemCompressError),
}

impl From<io::Error> for DecompressionError {
    fn from(io_error: io::Error) -> Self {
        Self::IoError(io_error)
    }
}

impl fmt::Display for DecompressionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoError(error) => write!(f, "io error: {error}"),
            Self::XMemCompressError(error) => write!(f, "XMemCompress error: {error}"),
        }
    }
}

impl std::error::Error for DecompressionError {}

pub type DecompressionResult<T> = Result<T, DecompressionError>;
