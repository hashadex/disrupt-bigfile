use std::io::{self, Read, Seek, Write};
use std::{cmp, fmt};

use byteorder::{LE, ReadBytesExt};

use crate::entry::Entry;

#[derive(Debug)]
pub enum LZ4LWError {
    Io(io::Error),
    InvalidEntry {
        uncompressed_size: u64,
        compressed_size: u64,
    },
    OffsetTooLarge {
        offset: usize,
        max: usize,
    },
}

impl From<io::Error> for LZ4LWError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<io::ErrorKind> for LZ4LWError {
    fn from(kind: io::ErrorKind) -> Self {
        Self::Io(kind.into())
    }
}

impl fmt::Display for LZ4LWError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::InvalidEntry {
                uncompressed_size,
                compressed_size,
            } => write!(
                f,
                "can't perform decompression if entry's compressed size ({compressed_size}) is larger than the uncompressed size ({uncompressed_size})"
            ),
            Self::OffsetTooLarge { offset, max } => write!(
                f,
                "offset ({offset}) cannot be larger than the amount of currently decompressed bytes ({max})"
            ),
        }
    }
}

impl std::error::Error for LZ4LWError {}

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

pub fn decompress_lz4lw(
    mut input: impl Read + Seek,
    mut out: impl Write,
    entry: Entry,
) -> Result<u64, LZ4LWError> {
    if entry.compressed_size > entry.uncompressed_size {
        return Err(LZ4LWError::InvalidEntry {
            uncompressed_size: entry.uncompressed_size,
            compressed_size: entry.compressed_size,
        });
    }

    let mut out_buf = vec![];
    if let Ok(uncompressed_size) = entry.uncompressed_size.try_into() {
        out_buf.reserve(uncompressed_size);
    }

    // The first few bytes in a LZ4LW compressed file are a header. The header is 1-4 bytes long.
    // If the highest bit of a header byte is 1, this means that the next byte is also part of the
    // header.
    // The information inside the header does not seem to affect decompression and therefore can be
    // skipped.
    for _ in 0..4 {
        let header_byte = input.read_u8()?;

        if header_byte >> 7 == 0 {
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
            let extra: usize = input.read_u8()?.into();
            offset += extra * 8192;
        }
        if offset > out_buf.len() {
            return Err(LZ4LWError::OffsetTooLarge {
                offset,
                max: out_buf.len(),
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

    let overwritten: i64 = (out_buf.len() as u64 - (in_place_margin + input.stream_position()?))
        .try_into()
        .expect("it's extremely unlikely we have overwritten > 8 EiB");
    input.seek_relative(overwritten)?;
    io::copy(&mut input, &mut out_buf)?;

    let copied = io::copy(&mut out_buf.as_slice(), &mut out)?;

    Ok(copied)
}
