//! [`Xmemcompress`] decompression errors.
//!
//! [`Xmemcompress`]: crate::entry::CompressionScheme::Xmemcompress

use std::io::{self, Read, Seek, Write};

use byteorder::{BE, ReadBytesExt};
use lzxd::{self, Lzxd, WindowSize};

use crate::vec;

const XMEMCOMPRESS_LZXNATIVE_SIGNATURE: u32 = 0x0FF5_12EE;
const XMEMCOMPRESS_VERSION: u16 = 0x0103;
const XMEMCOMPRESS_RESERVED: u16 = 0x0;
const XMEMCOMPRESS_CONTEXT_FLAGS: u32 = 0x0;
const XMEMCOMPRESS_FLAGS: u32 = 0x0;
const XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE: u32 = 32768;

/// Error that might happen when decompressing a chunk in a file that uses [`Xmemcompress`].
///
/// [`Xmemcompress`]: crate::entry::CompressionScheme::Xmemcompress
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ChunkDecompressionError(lzxd::DecompressError);

/// Errors that might happen when decompressing a file that uses [`Xmemcompress`].
///
/// [`Xmemcompress`]: crate::entry::CompressionScheme::Xmemcompress
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Failed to read or write enough bytes due to an I/O error
    #[error("io error")]
    Io(#[from] io::Error),

    /// The signature of the compressed file was invalid or unknown.
    #[error("bad magic {0:#X} in header, expected {XMEMCOMPRESS_LZXNATIVE_SIGNATURE:#X}")]
    BadMagic(u32),

    /// The value of the version field in the header of the compressed file was unknown.
    #[error("unknown version {0:#X} in header, expected {XMEMCOMPRESS_VERSION:#X}")]
    UnknownVersion(u16),

    /// The reserved field in the header contained non-zero bytes.
    #[error("unexpected data {0:#X} in reserved, expected {XMEMCOMPRESS_RESERVED:#X}")]
    UnexpectedReserved(u16),

    /// The value of the context flags field in the header of the compressed file was unknown.
    #[error("unknown context flags {0:#X}, expected {XMEMCOMPRESS_CONTEXT_FLAGS:#X}")]
    UnknownContextFlags(u32),

    /// The value of the flags field in the header of the compressed file was unknown.
    #[error("unknown flags {0:#X}, expected {XMEMCOMPRESS_FLAGS:#X}")]
    UnknownFlags(u32),

    /// The value of the window size fields in the header of the compressed file was unsupported.
    #[error("window size {0} is not supported")]
    UnsupportedWindowSize(u32),

    /// The value of the compression partition size field in the header of the compressed file was
    /// unknown.
    #[error(
        "unexpected compression partition size {0}, expected {XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE}"
    )]
    UnexpectedCompressionPartitionSize(u32),

    /// The size of the chunk specified in the external header of the chunk was smaller than the
    /// size of the internal header of the chunk.
    #[error("chunk has an invalid size of {0}")]
    InvalidChunkSize(u32),

    /// Failed to allocate enough memory for the compressed chunk buffer, due to the system not
    /// having enough available memory, pointer width, or due to the size of the buffer being
    /// ridiculous (like [`u32::MAX`]).
    #[error("failed to allocate compressed chunk buffer of {0} bytes")]
    CompressedChunkBufferAllocationFailed(u32),

    /// Failed to decompress a chunk of the file.
    #[error("failed to decompress chunk")]
    ChunkDecompression(#[from] ChunkDecompressionError),
}

pub(crate) fn decompress_xmemcompress(
    mut input: impl Read + Seek,
    mut out: impl Write,
) -> Result<u64, Error> {
    let magic = input.read_u32::<BE>()?;
    if magic != XMEMCOMPRESS_LZXNATIVE_SIGNATURE {
        return Err(Error::BadMagic(magic));
    }

    let version = input.read_u16::<BE>()?;
    if version != XMEMCOMPRESS_VERSION {
        return Err(Error::UnknownVersion(version));
    }

    let reserved = input.read_u16::<BE>()?;
    if reserved != XMEMCOMPRESS_RESERVED {
        return Err(Error::UnexpectedReserved(reserved));
    }

    let context_flags = input.read_u32::<BE>()?;
    if context_flags != XMEMCOMPRESS_CONTEXT_FLAGS {
        return Err(Error::UnknownContextFlags(context_flags));
    }

    let flags = input.read_u32::<BE>()?;
    if flags != XMEMCOMPRESS_FLAGS {
        return Err(Error::UnknownFlags(flags));
    }

    let window_size = input.read_u32::<BE>()?;
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
        _ => return Err(Error::UnsupportedWindowSize(window_size)),
    };

    let compression_partition_size = input.read_u32::<BE>()?;
    if compression_partition_size != XMEMCOMPRESS_COMPRESSION_PARTITION_SIZE {
        return Err(Error::UnexpectedCompressionPartitionSize(
            compression_partition_size,
        ));
    }

    let uncompressed_file_size = input.read_u64::<BE>()?;

    // Skip u64 compressed_file_size, u32 largest_uncompressed_chunk_size,
    // u32 largest_compressed_chunk_size
    input.seek_relative(16)?;

    let mut decompressed = 0;
    let expected_chunk_count = uncompressed_file_size.div_ceil(compression_partition_size.into());
    for _ in 0..expected_chunk_count {
        // Each chunk in the 0F F5 12 EE format has two headers: external and internal.
        //
        // 1. External header:
        //     * Contains the total size (including the internal header) of the chunk that follows
        //       as a u32.
        // 2. Internal header:
        //     * Begins with an optional 0xFF prefix.
        //     * If 0xFF is present, the next 2 bytes store the chunk's uncompressed size
        //       (usually 32 KiB). If 0xFF is absent, the uncompressed size implicitly defaults to
        //       32 KiB.
        //     * After the optional uncompressed size field, the internal header *also* stores the
        //       compressed length of the chunk, for some odd reason.
        //
        // The conpressed length in the internal header seems to be always 10 bytes smaller than
        // the length from the external header. This is because the internal length value excludes:
        //     * the 5 byte internal header itself, and
        //     * the 5 trailing 0x00 bytes after each chunk.
        //
        // We will ignore the size from the internal header to keep the reader aligned to "external"
        // chunks.

        let mut compressed_chunk_size = input.read_u32::<BE>()?;

        let uncompressed_chunk_size;
        let internal_header_size;
        if input.read_u8()? == 0xFF {
            uncompressed_chunk_size = input.read_u16::<BE>()?;
            internal_header_size = 5;

            input.seek_relative(2)?;
        } else {
            uncompressed_chunk_size = 32_768;
            internal_header_size = 2;

            input.seek_relative(1)?;
        }

        compressed_chunk_size = compressed_chunk_size
            .checked_sub(internal_header_size)
            .ok_or(Error::InvalidChunkSize(compressed_chunk_size))?;
        let mut compressed_chunk_buf = vec::try_with_elements(compressed_chunk_size).ok_or(
            Error::CompressedChunkBufferAllocationFailed(compressed_chunk_size),
        )?;

        input.read_exact(&mut compressed_chunk_buf)?;

        // Strangely enough, we have to use a different context for each chunk, or else the
        // decompression will fail on the second chunk.
        let mut lzxd_context = Lzxd::new(window_size);
        let decompressed_chunk_buf = lzxd_context
            .decompress_next(&compressed_chunk_buf, uncompressed_chunk_size.into())
            .map_err(ChunkDecompressionError)?;

        out.write_all(decompressed_chunk_buf)?;
        decompressed += decompressed_chunk_buf.len() as u64;
    }

    Ok(decompressed)
}
