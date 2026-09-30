//! Archived file extraction.

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Take, Write};
use std::path::Path;

use crate::compression::lz4lw;
use crate::compression::xmemcompress;
use crate::entry::{CompressionScheme, Entry};

/// Errors that might happen when extracting an archived file from a [`Dat`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UnpackError {
    /// Failed to read the data from the DAT or write to a destination due to an I/O error.
    #[error("io error")]
    Io(#[from] io::Error),

    /// Decompression of this scheme is not supported.
    ///
    /// # Currently supported [`CompressionScheme`]s
    ///
    /// * [`None`](CompressionScheme::None)
    /// * [`Lz4lw`](CompressionScheme::Lz4lw)
    /// * [`Xmemcompress`](CompressionScheme::Xmemcompress)
    #[error("{0} decompression is not supported")]
    DecompressionUnsupported(CompressionScheme),

    /// [`Lz4lw`] decompression failed.
    ///
    /// [`Lz4lw`]: CompressionScheme::Lz4lw
    #[error("lz4lw error")]
    Lz4lw(#[from] lz4lw::Error),

    /// [`Xmemcompress`] decompression failed.
    ///
    /// [`Xmemcompress`]: CompressionScheme::Xmemcompress
    #[error("xmemcompress error")]
    Xmemcompress(#[from] xmemcompress::Error),

    /// The [`Entry::uncompressed_size`] field does not match the actual size of the decompressed
    /// file.
    #[error("expected to unpack {expected} bytes, but unpacked {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
}

/// Interface for extracting archived files from a DAT.
///
/// This struct wraps a seekable reader for a DAT and allows you to use [`Fat`] [`Entries`] in
/// order to locate, [read] and [extract] the archived files from it.
///
/// [`Fat`]: crate::fat::Fat
/// [`Entries`]: Entry
/// [read]: Self::raw_entry_data
/// [extract]: Self::unpack_to_dir
pub struct Dat<R: Read + Seek> {
    inner: R,
}

impl<R: Read + Seek> Dat<R> {
    /// Creates a `Dat` instance from a reader.
    ///
    /// If you need to create a `Dat` instance from a file, use [`Self::open`] instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Cursor;
    ///
    /// use disrupt_bigfile::dat::Dat;
    ///
    /// let mut data = Cursor::new([
    ///     0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE,
    /// ]);
    ///
    /// let dat = Dat::new(data);
    /// ```
    #[must_use]
    pub fn new(inner: R) -> Self {
        Self { inner }
    }

    /// Consumes this `Dat` and returns ownership of the wrapped reader.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Cursor;
    ///
    /// use disrupt_bigfile::dat::Dat;
    ///
    /// let dat = Dat::new(Cursor::new(vec![]));
    ///
    /// let reader = dat.into_inner();
    /// ```
    #[must_use]
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Returns the raw, possibly compressed contents of an archived file described by an `entry`.
    ///
    /// This function returns a [`Take`] that will yield only the portion of the DAT that
    /// corresponds to the file described by the `entry`. Keep in mind that the returned `Take`
    /// holds a mutable reference to the wrapped reader, so calling this function a second time
    /// will only be possible after the first `Take` goes out of scope.
    ///
    /// # Errors
    ///
    /// This function will return an error if it fails to seek the wrapped reader.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::{Cursor, Read};
    ///
    /// use byteorder::{BigEndian, ReadBytesExt};
    /// use disrupt_bigfile::dat::Dat;
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    ///
    /// let mut data = Cursor::new("Hello World");
    /// let world_entry = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 6,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 5,
    ///     compressed_size: 5,
    /// };
    ///
    /// let mut dat = Dat::new(data);
    /// let mut entry_bytes = dat.raw_entry_data(world_entry)?;
    ///
    /// let mut out = String::new();
    /// entry_bytes.read_to_string(&mut out)?;
    ///
    /// assert_eq!(out, "World");
    /// # Ok::<(), std::io::Error>(())
    /// ```
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

    /// Reads the file described by an `entry`, decompresses it if needed, and copies the result to
    /// `out`.
    ///
    /// If you need to extract an [`Entry`] to a file or directory, use [`Self::unpack_to_file`] or
    /// [`Self::unpack_to_dir`] instead.
    ///
    /// # Errors
    ///
    /// This function will return an error under a number of different circumstances. See the
    /// documentation for [`UnpackError`] for details.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use disrupt_bigfile::fat::Fat;
    /// use disrupt_bigfile::dat::Dat;
    ///
    /// let fat = Fat::open("path/to/file.fat")?;
    /// let mut dat = Dat::open("path/to/file.dat")?;
    ///
    /// let entry = fat.entries()[0];
    /// let mut decompressed = Vec::new();
    ///
    /// // You can pass a mutable reference to your writer in order to avoid consuming it and use
    /// // it afterwards:
    /// dat.write_decompressed(entry, &mut decompressed)?;
    ///
    /// assert!(!decompressed.is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
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

    /// Reads the archived file described by an `entry`, decompresses it if needed, and copies the
    /// result to a file created at `dest`.
    ///
    /// If the file at `dest` already exists, it will be overwritten.
    ///
    /// Use [`Self::unpack_to_dir`] if you want to unpack the file to its [`Entry::path`] instead
    /// of specifying the destination manually.
    ///
    /// # Errors
    ///
    /// This function will return an error under a number of different circumstances. See the
    /// documentation for [`UnpackError`] for details.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use disrupt_bigfile::fat::Fat;
    /// use disrupt_bigfile::dat::Dat;
    ///
    /// let fat = Fat::open("path/to/file.fat")?;
    /// let mut dat = Dat::open("path/to/file.dat")?;
    ///
    /// let entry = fat.entries()[0];
    ///
    /// dat.unpack_to_file(entry, "path/to/dest/file.xbt")?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn unpack_to_file(
        &mut self,
        entry: Entry,
        dest: impl AsRef<Path>,
    ) -> Result<(), UnpackError> {
        // BufWriter will not help here because decompression functions write in big chunks
        let outfile = File::create(dest)?;
        self.write_decompressed(entry, outfile)
    }

    /// Reads the archived file described by an `entry`, decompresses it if needed, and copies the
    /// result to a file created in the `archive_root_dir`.
    ///
    /// The name of the created file will be automatically determined by calling the [`path`]
    /// method on the `entry` and appending the returned value to the `archive_root_dir`. For
    /// example, if the `archive_root_dir` is set to `dest_dir/` and `Entry::path` returns
    /// `credits/pc/credits.xml`, then the unpacked file will be created at
    /// `dest_dir/credits/pc/credits.xml`.
    ///
    /// Use [`Self::unpack_to_dir_iter`] if you want to unpack a lot of entries in bulk instead of
    /// calling this function for each entry manually.
    ///
    /// [`path`]: Entry::path
    ///
    /// # Errors
    ///
    /// This function will return an error under a number of different circumstances. See the
    /// documentation for [`UnpackError`] for details.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs;
    ///
    /// use disrupt_bigfile::dat::Dat;
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    ///
    /// let mut dat = Dat::open("path/to/file.dat")?;
    /// let entry = Entry {
    ///     name_hash: 0x2672_724B, // "credits/pc/credits.xml"
    ///     offset: 0x5A75607,
    ///     compression_scheme: CompressionScheme::Xmemcompress,
    ///     uncompressed_size: 288_650,
    ///     compressed_size: 58_070,
    /// };
    ///
    /// dat.unpack_to_file(entry, "dest_dir/")?;
    ///
    /// assert!(fs::exists("dest_dir/credits/pc/credits.xml")?);
    /// # Ok::<(), disrupt_bigfile::dat::UnpackError>(())
    /// ```
    pub fn unpack_to_dir(
        &mut self,
        entry: Entry,
        archive_root_dir: impl AsRef<Path>,
    ) -> Result<(), UnpackError> {
        let dest = archive_root_dir.as_ref().join(entry.path());

        #[expect(clippy::missing_panics_doc, reason = "infallible")]
        let dest_dir = dest.parent().expect("dest should always have a parent");

        fs::create_dir_all(dest_dir)?;

        self.unpack_to_file(entry, dest)
    }

    /// Returns an [`Iterator`] that bulk unpacks `entries` to the `archive_root_dir`.
    ///
    /// When the returned `Iterator` is [advanced], it unpacks one of the [`Entries`] using
    /// [`Self::unpack_to_dir`], then yields the [`Result`] of the operation and the `Entry`
    /// itself.
    ///
    /// The `Iterator` gives the user a lot of flexibility. For example, you can use the
    /// [`try_for_each`] method to stop extraction at the first error, or you can attach a progress
    /// bar to the iterator using [`indicatif`] or a similar crate.
    ///
    /// When you need to unpack a large amount of `Entries`, it is recommended to use this function
    /// instead of manually calling `unpack_to_dir` for every `Entry`, because this function
    /// unpacks the `entries` in [`offset`] order. That way, the performance is improved by
    /// minimizing the amount of seek calls and buffer flushes.
    ///
    /// [advanced]: Iterator::next
    /// [`Entries`]: Entry
    /// [`try_for_each`]: Iterator::try_for_each
    /// [`offset`]: Entry::offset
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use disrupt_bigfile::dat::Dat;
    /// use disrupt_bigfile::fat::Fat;
    ///
    /// let entries = Fat::open("path/to/file.fat")?.into_entries();
    /// let mut dat = Dat::open("path/to/file.dat")?;
    ///
    /// for (entry, result) in dat.unpack_to_dir_iter(entries, "path/to/destination/") {
    ///     match result {
    ///         Ok(()) => println!("unpacked {entry} successfully"),
    ///         Err(e) => println!("failed to unpack {entry}: {e}"),
    ///     }
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn unpack_to_dir_iter(
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
    /// Opens a file from `path` and creates a new `Dat` instance from it.
    ///
    /// This is a convinience function for opening a [`File`], wrapping it in a [`BufReader`] and
    /// using [`Self::new`] with it.
    ///
    /// # Errors
    ///
    /// This function will return an I/O error if it fails to [`open`] a file.
    ///
    /// [`open`]: File::open
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use disrupt_bigfile::dat::Dat;
    ///
    /// let dat = Dat::open("path/to/file.dat")?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn open(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let file = BufReader::new(File::open(path)?);
        Ok(Self { inner: file })
    }
}
