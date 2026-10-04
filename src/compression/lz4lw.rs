//! [`Lz4lw`] decompression errors.
//!
//! [`Lz4lw`]: crate::entry::CompressionScheme::Lz4lw

use std::cmp;
use std::io::{self, Read, Seek, Write};

use byteorder::{LE, ReadBytesExt};

use crate::entry::Entry;
use crate::vec;

/// Errors that might happen when decompressing a file that uses [`Lz4lw`].
///
/// [`Lz4lw`]: crate::entry::CompressionScheme::Lz4lw
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Failed to read or write enough bytes due to an I/O error.
    #[error("io error")]
    Io(#[from] io::Error),

    /// Can't perform decompression if the [`Entry`]'s [`compressed_size`] is larger than the
    /// expected [`uncompressed_size`].
    ///
    /// [`compressed_size`]: Entry::compressed_size
    /// [`uncompressed_size`]: Entry::uncompressed_size
    #[error(
        "can't perform decompression if entry's compressed size ({compressed_size}) is larger than the uncompressed size ({uncompressed_size})"
    )]
    InvalidEntrySizes {
        uncompressed_size: u64,
        compressed_size: u64,
    },

    /// Failed to allocate enough memory for the output buffer, due to the system not having enough
    /// available memory, pointer width, or due to the expected [`Entry::uncompressed_size`] being
    /// ridiculous (like [`u64::MAX`]).
    #[error("failed to allocate {0} bytes for the output buffer")]
    OutputBufferAllocationFailed(u64),

    /// The `extra` offset byte was so large it caused an overflow.
    #[error("offset extra byte ({0}) was so large it caused an overflow")]
    OffsetExtraOverflow(u8),

    /// The `offset` of a block was larger than the amount of currently `decompressed` bytes.
    #[error(
        "offset ({offset}) was larger than the amount of currently decompressed bytes ({decompressed})"
    )]
    OffsetTooLarge { offset: usize, decompressed: usize },
}

impl From<io::ErrorKind> for Error {
    fn from(kind: io::ErrorKind) -> Self {
        Self::Io(kind.into())
    }
}

fn read_lsic(mut input: impl Read) -> Result<usize, io::Error> {
    let mut result = 0;

    loop {
        let byte = input.read_u8()?;

        result += usize::from(byte);

        if byte != 0xFF {
            break;
        }
    }

    Ok(result)
}

pub(crate) fn decompress_lz4lw(
    mut input: impl Read + Seek,
    mut out: impl Write,
    entry: Entry,
) -> Result<u64, Error> {
    // Decompressor implementation based on the reverse engineered code from @ahmet-celik
    // https://github.com/gibbed/Gibbed.Disrupt/issues/2#issuecomment-722033656

    if entry.compressed_size > entry.uncompressed_size {
        return Err(Error::InvalidEntrySizes {
            uncompressed_size: entry.uncompressed_size,
            compressed_size: entry.compressed_size,
        });
    }

    let mut out_buf = vec::try_with_capacity(entry.uncompressed_size)
        .ok_or(Error::OutputBufferAllocationFailed(entry.uncompressed_size))?;

    // The first few bytes in a LZ4LW compressed file are a header. The header is 1-4 bytes long.
    // If the highest bit of a header byte is 1, this means that the next byte is also part of the
    // header.
    // The information inside the header does not seem to affect decompression and therefore can be
    // skipped.
    for _ in 0..4 {
        if input.read_u8()? >> 7 == 0 {
            break;
        }
    }

    // The original game's decoder uses in-place decompression. It uses the same buffer for
    // compressed and decompressed data. The compressed bytes are written at the end of the buffer,
    // and the decompressed bytes are written at the start of the buffer. During decompression,
    // decompressed bytes overwrite compressed bytes which are not needed anymore.
    // When the decompressed bytes overwrite the position at which the next compressed bytes are
    // supposed to be read from, the decompressor stops. The final few bytes are already in the
    // place in the buffer.
    // Since we don't use in-place decompression here, we have to emulate this behaviour by copying
    // the final bytes after the decompression loop.

    // The position at which the compressed bytes would be written to if we used in-place
    // decompression.
    let in_place_margin = entry.uncompressed_size - entry.compressed_size;

    while (out_buf.len() as u64) < in_place_margin + input.stream_position()? {
        // The rest of the decompression process is identical to regular LZ4 with the exception
        // that the offset may have an additional "extra" byte.

        let token = input.read_u8()?;

        let mut literal_length: u64 = (token >> 4).into();
        if literal_length == 0b1111 {
            literal_length += read_lsic(&mut input)? as u64;
        }

        let mut literals = (&mut input).take(literal_length);
        let literals_copied = io::copy(&mut literals, &mut out_buf)?;
        if literals_copied != literal_length {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        let mut offset: usize = input.read_u16::<LE>()?.into();
        if offset >> 13 == 0b111 {
            let extra = input.read_u8()?;

            offset = usize::from(extra)
                .checked_mul(8192)
                .and_then(|extra_bytes| offset.checked_add(extra_bytes))
                .ok_or(Error::OffsetExtraOverflow(extra))?;
        }
        if offset > out_buf.len() {
            return Err(Error::OffsetTooLarge {
                offset,
                decompressed: out_buf.len(),
            });
        }

        let mut copy_length: usize = (token & 0x0F).into();
        if copy_length == 0b1111 {
            copy_length += read_lsic(&mut input)?;
        }
        copy_length += 4;

        while copy_length > 0 {
            let start = out_buf.len() - offset;
            let end = cmp::min(out_buf.len(), start + copy_length);

            out_buf.extend_from_within(start..end);

            copy_length -= end - start;
        }
    }

    let overwritten = (out_buf.len() as u64)
        .checked_signed_diff(in_place_margin + input.stream_position()?)
        .expect("it's extremely unlikely we have overwritten > 8 EiB");
    input.seek_relative(overwritten)?;
    io::copy(&mut input, &mut out_buf)?;

    let decompressed = io::copy(&mut out_buf.as_slice(), &mut out)?;

    Ok(decompressed)
}
