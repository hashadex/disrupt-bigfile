use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Take, Write};
use std::path::Path;

use crate::compression::lz4lw;
use crate::compression::xmemcompress;
use crate::entry::{CompressionScheme, Entry};

#[derive(Debug, thiserror::Error)]
pub enum UnpackError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("{0} decompression is not supported")]
    DecompressionUnsupported(CompressionScheme),

    #[error("LZ4LW error: {0}")]
    Lz4lw(#[from] lz4lw::Error),

    #[error("XMemCompress error: {0}")]
    Xmemcompress(#[from] xmemcompress::Error),

    #[error("expected to unpack {expected} bytes, but unpacked {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
}

pub struct Dat<R: Read + Seek> {
    inner: R,
}

impl<R: Read + Seek> Dat<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> R {
        self.inner
    }

    pub fn raw_entry_data(&mut self, entry: Entry) -> Result<Take<&mut R>, io::Error> {
        if let Some(relative_seek_position) = entry
            .offset
            .checked_signed_diff(self.inner.stream_position()?)
        {
            self.inner.seek_relative(relative_seek_position)?;
        } else {
            self.inner.seek(SeekFrom::Start(entry.offset))?;
        }

        Ok((&mut self.inner).take(entry.compressed_size))
    }

    pub fn write_decompressed(
        &mut self,
        entry: Entry,
        mut out: impl Write,
    ) -> Result<(), UnpackError> {
        let mut raw_data = self.raw_entry_data(entry)?;

        let decompressed = match entry.compression_scheme {
            CompressionScheme::None => io::copy(&mut raw_data, &mut out).map_err(UnpackError::Io),
            CompressionScheme::Xmemcompress => {
                xmemcompress::decompress_xmemcompress(raw_data, &mut out)
                    .map_err(UnpackError::Xmemcompress)
            }
            CompressionScheme::Lz4lw => {
                lz4lw::decompress_lz4lw(raw_data, &mut out, entry).map_err(UnpackError::Lz4lw)
            }
            other => Err(UnpackError::DecompressionUnsupported(other)),
        }?;

        out.flush()?;

        if decompressed == entry.uncompressed_size {
            Ok(())
        } else {
            Err(UnpackError::SizeMismatch {
                expected: entry.uncompressed_size,
                actual: decompressed,
            })
        }
    }

    pub fn unpack_to_file(
        &mut self,
        entry: Entry,
        dest: impl AsRef<Path>,
    ) -> Result<(), UnpackError> {
        // BufWriter will not help here because decompression functions write in big chunks
        let outfile = File::create(dest)?;
        self.write_decompressed(entry, outfile)
    }

    pub fn unpack_to_dir(
        &mut self,
        entry: Entry,
        archive_root_dir: impl AsRef<Path>,
    ) -> Result<(), UnpackError> {
        let dest = archive_root_dir.as_ref().join(entry.path());

        let dest_dir = dest.parent().expect("dest should always have a parent");
        fs::create_dir_all(dest_dir)?;

        self.unpack_to_file(entry, dest)
    }

    pub fn bulk_unpack_to_dir(
        &mut self,
        mut entries: Vec<Entry>,
        archive_root_dir: impl AsRef<Path>,
    ) -> impl ExactSizeIterator<Item = (Entry, Result<(), UnpackError>)> {
        // Unpacking entries in offset order minimizes the amount of seek operations and can give a
        // performance increase as large as 20% in archives with a huge amount of entries.
        entries.sort_unstable_by_key(|entry| entry.offset);

        entries
            .into_iter()
            .map(move |entry| (entry, self.unpack_to_dir(entry, &archive_root_dir)))
    }
}

impl Dat<BufReader<File>> {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let file = BufReader::new(File::open(path)?);
        Ok(Self { inner: file })
    }
}
