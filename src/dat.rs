use std::error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader, ErrorKind, Read, Seek, SeekFrom, Take, Write};
use std::path::{Path, PathBuf};

use crate::compression::{self, XMemCompressError};
use crate::fat::{CompressionScheme, Entry};

#[derive(Debug)]
pub enum UnpackError {
    Io(io::Error),
    XMemCompress(XMemCompressError),
}

impl From<io::Error> for UnpackError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<XMemCompressError> for UnpackError {
    fn from(err: XMemCompressError) -> Self {
        Self::XMemCompress(err)
    }
}

impl fmt::Display for UnpackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::XMemCompress(err) => write!(f, "XMemCompress error: {err}"),
        }
    }
}

impl error::Error for UnpackError {}

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
        self.inner.seek(SeekFrom::Start(entry.offset))?;
        Ok((&mut self.inner).take(entry.compressed_size.into()))
    }

    pub fn write_decompressed(
        &mut self,
        entry: Entry,
        mut out: impl Write,
    ) -> Result<(), UnpackError> {
        let mut raw_data = self.raw_entry_data(entry)?;

        match entry.compression_scheme {
            CompressionScheme::None => {
                let copied = io::copy(&mut raw_data, &mut out)?;

                if copied == u64::from(entry.uncompressed_size) {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        ErrorKind::UnexpectedEof,
                        format!(
                            "unexpected EOF: expected to copy {} bytes, but copied {copied}",
                            entry.uncompressed_size
                        ),
                    )
                    .into())
                }
            }
            CompressionScheme::LZO1x => todo!(),
            CompressionScheme::Zlib => todo!(),
            CompressionScheme::XMemCompress => {
                compression::decompress_xmemcompress(raw_data, &mut out)
                    .map_err(XMemCompressError::into)
            }
            CompressionScheme::LZMA => todo!(),
            CompressionScheme::LZ4LW => todo!(),
            CompressionScheme::Oodle => todo!(),
        }
    }

    pub fn unpack_to_file(
        &mut self,
        entry: Entry,
        dest: &impl AsRef<Path>,
    ) -> Result<(), UnpackError> {
        // BufWriter will not help here because decompression functions write in big chunks
        let outfile = File::create(dest)?;
        self.write_decompressed(entry, outfile)
    }

    pub fn unpack_to_dir(
        &mut self,
        entry: Entry,
        archive_root_dir: &impl AsRef<Path>,
    ) -> Result<(), UnpackError> {
        let dest: PathBuf = [archive_root_dir.as_ref(), &entry.path()].iter().collect();

        let dest_dir = dest.parent().expect("dest should always have a parent");
        fs::create_dir_all(dest_dir)?;

        self.unpack_to_file(entry, &dest)
    }
}

impl Dat<BufReader<File>> {
    pub fn open(path: &impl AsRef<Path>) -> Result<Self, io::Error> {
        let file = BufReader::new(File::open(path)?);
        Ok(Self { inner: file })
    }
}
