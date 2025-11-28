use std::fmt;
use std::io::{self, Read, Seek, Write};

use byteorder::{BE, ReadBytesExt};
use lzxd::{self, Lzxd, WindowSize};

use crate::FatDeserializationError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompressionVersion {
    V0,
    V4,
    V5,
}

impl TryFrom<u8> for CompressionVersion {
    type Error = FatDeserializationError;

    fn try_from(value: u8) -> Result<Self, FatDeserializationError> {
        match value {
            0 => Ok(Self::V0),
            4 => Ok(Self::V4),
            5 => Ok(Self::V5),
            _ => Err(Self::Error::UnknownCompressionVersion(value)),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    pub fn from_scheme_id(scheme_id: u8, compression_version: CompressionVersion) -> Option<Self> {
        match (compression_version, scheme_id) {
            (_, 0) => Some(Self::None),
            (CompressionVersion::V4 | CompressionVersion::V5, 1) => Some(Self::LZO1x),
            (CompressionVersion::V4 | CompressionVersion::V5, 2) => Some(Self::Zlib),
            (CompressionVersion::V5, 3) => Some(Self::XMemCompress),
            _ => None,
        }
    }
}

const XMEMCOMPRESS_LZXNATIVE_SIGNATURE: u32 = 0x0FF512EE;
const XMEMCOMPRESS_VERSION: u16 = 0x0103;
const XMEMCOMPRESS_RESERVED: u16 = 0x0;
const XMEMCOMPRESS_CONTEXT_FLAGS: u32 = 0x0;
const XMEMCOMPRESS_FLAGS: u32 = 0x0;
const XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE: u32 = 32768;

#[derive(Debug)]
pub enum XMemCompressError {
    IoError(io::Error),
    BadMagic(u32),
    UnknownVersion(u16),
    UnexpectedReserved(u16),
    UnknownContextFlags(u32),
    UnknownFlags(u32),
    UnsupportedWindowSize(u32),
    UnexpectedCompressionPartitionSize(u32),
    LzxdError {
        chunk_num: u64,
        err: lzxd::DecompressError,
    },
}

impl From<io::Error> for XMemCompressError {
    fn from(io_error: io::Error) -> Self {
        Self::IoError(io_error)
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
            Self::UnsupportedWindowSize(size) => {
                write!(f, "window size {size} is unsupported by lzxd")
            }
            Self::UnexpectedCompressionPartitionSize(part_size) => write!(
                f,
                "unexpected compression partition size {part_size}, expected {XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE}"
            ),
            Self::LzxdError { chunk_num, err } => {
                write!(f, "lzxd error on chunk #{chunk_num}: {err}")
            }
        }
    }
}

impl std::error::Error for XMemCompressError {}

pub fn decompress_xmemcompress(
    mut compressed_data: impl Read + Seek,
    mut out_buf: impl Write,
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
    let window_size = match window_size {
        32_768 => WindowSize::KB32,
        65_536 => WindowSize::KB64,
        137_072 => WindowSize::KB128,
        262_144 => WindowSize::KB256,
        1_048_576 => WindowSize::MB1,
        2_097_152 => WindowSize::MB2,
        4_194_304 => WindowSize::MB4,
        8_388_608 => WindowSize::MB8,
        16_777_216 => WindowSize::MB16,
        33_554_432 => WindowSize::MB32,
        _ => return Err(XMemCompressError::UnsupportedWindowSize(window_size)),
    };

    let compression_partition_size = compressed_data.read_u32::<BE>()?;
    if compression_partition_size != XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE {
        return Err(XMemCompressError::UnexpectedCompressionPartitionSize(
            compression_partition_size,
        ));
    }

    let uncompressed_file_size = compressed_data.read_u64::<BE>()?;

    // Skip u64 compressed_file_size, u32 largest_uncompressed_chunk_size,
    // u32 largest_compressed_chunk_size
    compressed_data.seek_relative(16)?;

    let expected_chunk_count = uncompressed_file_size.div_ceil(compression_partition_size.into());
    for chunk_num in 0..expected_chunk_count {
        // Each chunk in the 0F F5 12 EE format has two headers: external and internal.
        //
        // 1. External header:
        //    * Contains the total size (including the internal header) of the chunk that follows
        //      as a u32.
        // 2. Internal header:
        //    * Begins with an optional 0xFF prefix.
        //    * If 0xFF is present, the next 2 bytes store the chunk’s uncompressed size
        //      (usually 32 KiB). If 0xFF is absent, the uncompressed size implicitly defaults to
        //      32 KiB.
        //    * After the optional uncompressed-size field, the internal header *also* stores the
        //      compressed length of the chunk, for some odd reason.
        //
        // The compressed length in the internal header is always 10 bytes smaller than the
        // length from the external header. This is because the internal length value excludes:
        //    * the 5-byte internal header itself, and
        //    * the 5 trailing 0x00 bytes after each chunk.
        //
        // We will ignore the size from the internal header here to keep the alignment to chunks.

        let chunk_size = compressed_data.read_u32::<BE>()?;

        let uncompressed_chunk_size;
        let internal_header_size;
        if compressed_data.read_u8()? == 0xFF {
            uncompressed_chunk_size = compressed_data.read_u16::<BE>()?;
            internal_header_size = 5;

            compressed_data.seek_relative(2)?;
        } else {
            uncompressed_chunk_size = 32_768;
            internal_header_size = 2;

            compressed_data.seek_relative(1)?;
        }

        let chunk_buf_size: usize = (chunk_size - internal_header_size)
            .try_into()
            .expect("u32 should fit into usize on PCs");
        let mut compressed_chunk_buf = vec![0; chunk_buf_size];

        compressed_data.read_exact(&mut compressed_chunk_buf)?;

        // Strangely enough, we have to use a different context for each chunk, or else the
        // decompression will fail on the second chunk.
        let mut lzxd_context = Lzxd::new(window_size);
        let decompressed_chunk_buf = lzxd_context
            .decompress_next(&compressed_chunk_buf, uncompressed_chunk_size.into())
            .map_err(|err| XMemCompressError::LzxdError { chunk_num, err })?;

        out_buf.write_all(decompressed_chunk_buf)?;
    }

    out_buf.flush()?;

    Ok(())
}
