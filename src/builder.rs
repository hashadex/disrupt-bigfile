//! BigFile archive creation.

use std::borrow::Cow;
use std::fs::File;
use std::hash::Hasher;
use std::hint;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use fnv1::Fnv1Hasher;

use crate::entry::{CompressionScheme, Entry, EntryError};
use crate::fat::Fat;
use crate::header::{FatHeader, FatVersion};

/// Errors that might happen when adding a file to an archive using [`ArchiveBuilder`].
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    /// Failed to seek or write to the DAT due to an I/O error.
    #[error("io error")]
    Io(#[from] io::Error),

    /// Failed to create a [valid] [`Entry`] for the added file due to its [offset] or [size] being
    /// too large.
    ///
    /// [valid]: Entry#versions-and-validity
    /// [offset]: Entry::offset
    /// [size]: Entry::uncompressed_size
    #[error("failed to create an entry")]
    Entry(#[from] EntryError),

    /// Can't add more than [`u32::MAX`] files to an archive.
    #[error("can't add more than {} entries", u32::MAX)]
    TableIsFull,

    /// Failed to compute the [name hash] for a [special path] due to it being in an invalid
    /// format.
    ///
    /// [name hash]: Entry::name_hash
    /// [special path]: ArchiveBuilder#special-paths
    #[error("can't compute name hash for invalid special path '{0}'")]
    InvalidSpecialPath(PathBuf),
}

/// A builder used for creating new [`Fat`]/DAT pairs.
///
/// This struct wraps a DAT writer and allows you to write files to it, automatically creating and
/// storing [`Fat`] [`Entries`] for them. When you're done adding files to the archive, use
/// [`Self::finish`] to create a new [`Fat`] instance containing those `Entries`. Then, you can use
/// [`Fat::create`] to serialize and save the `Fat` to the filesystem.
///
/// File compression is not yet supported, meaning that all added files will be uncompressed.
///
/// [`Entries`]: Entry
pub struct ArchiveBuilder<W: Write + Seek> {
    fat_header: FatHeader,
    entries: Vec<Entry>,
    dat: W,
    dat_position: u64,
    last_write_failed: bool,
}

impl<W: Write + Seek> ArchiveBuilder<W> {
    /// Creates a new builder that will write the contents of the files being archived to `dat` and
    /// build a [`Fat`] with the given `fat_header`.
    ///
    /// If you want to write the packed files to a DAT file on the filesystem, use [`Self::create`]
    /// instead.
    ///
    /// # Errors
    ///
    /// This function will return an I/O error if it fails to get the [`stream_position`] of the
    /// `dat`.
    ///
    /// [`stream_position`]: Seek::stream_position
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Cursor;
    ///
    /// use disrupt_bigfile::builder::ArchiveBuilder;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let fat_header = FatHeader::new_wd1_win64();
    /// let dat = Cursor::new(vec![]);
    ///
    /// let builder = ArchiveBuilder::new(fat_header, dat)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn new(fat_header: FatHeader, mut dat: W) -> Result<Self, io::Error> {
        let dat_position = dat.stream_position()?;

        Ok(ArchiveBuilder {
            fat_header,
            entries: Vec::new(),
            dat,
            dat_position,
            last_write_failed: false,
        })
    }

    fn compute_name_hash(&self, path: &Path) -> Option<u64> {
        if path.starts_with("__UNKNOWN") {
            let stem = path.file_stem()?.to_str()?;
            u64::from_str_radix(stem, 16).ok()
        } else {
            // By avoiding calling functions like to_str when hashing regular paths and using
            // cold_path hints, we get a 50% performance boost.

            let mut hasher = Fnv1Hasher::new();

            let clean_path = if path.starts_with("__DUPLICATE") {
                hint::cold_path();

                let (clean_stem, _) = path.file_stem()?.to_str()?.split_once("__DUPLICATE_")?;
                let extension = path.extension();

                let mut clean_path = path
                    .strip_prefix("__DUPLICATE")
                    .expect("just checked that path starts with __DUPLICATE")
                    .with_file_name(clean_stem);
                if let Some(extension) = extension {
                    clean_path.set_extension(extension);
                }

                Cow::Owned(clean_path)
            } else {
                Cow::Borrowed(path)
            };

            for &byte in clean_path.as_os_str().as_encoded_bytes() {
                let windows_byte = if byte == b'/' {
                    hint::cold_path();

                    b'\\'
                } else {
                    byte
                };

                hasher.write_u8(windows_byte);
            }

            let mut hash = hasher.finish();
            match self.fat_header.fat_version() {
                FatVersion::Fat3 => hash &= 0xFFFF_FFFF,
                FatVersion::Fat5 => {
                    // The three highest bits in all FAT5 name hashes seem to be always set to 101.
                    hash &= 0x1FFF_FFFF_FFFF_FFFF; // 0b0001_1111...
                    hash |= 0xA000_0000_0000_0000; // 0b1010_0000...
                }
            }

            Some(hash)
        }
    }

    /// Adds a new file to the archive, copying its contents from `input` to the DAT and creating
    /// an [`Entry`] for it with the [`name_hash`] computed from the [`relative_entry_path`].
    ///
    /// Use [`Self::add_file`] instead if you want to archive a file from the filesystem.
    ///
    /// [`name_hash`]: Entry::name_hash
    /// [`relative_entry_path`]: #relative_entry_path
    ///
    /// # `relative_entry_path`
    ///
    /// The `relative_entry_path` should be the [`Path`] to the file being added, relative to the
    /// root directory of the archive.
    ///
    /// For example, if you have a directory named `root_dir` that contains all the files you want
    /// to archive, like this:
    ///
    /// ```text
    /// root_dir/
    /// ├── domino
    /// │   └── script.lua
    /// ├── generated
    /// │   └── database.obj
    /// └── ui
    ///     └── texture.xbt
    /// ```
    ///
    /// Then the `relative_entry_path` for each of those files should be `domino/script.lua`,
    /// `generated/database.obj` and `ui/texture.xbt`.
    ///
    /// If the `relative_entry_path` uses UNIX path separators (`/`), they will automatically be
    /// converted to Windows path separators (`\`).
    ///
    /// ## Special paths
    ///
    /// For compatibility with [Gibbed.Disrupt], `relative_entry_path`s starting with the special
    /// `__UNKNOWN` or `__DUPLICATE` directories will be handled in a different way.
    ///
    /// ### `__UNKNOWN`
    ///
    /// `__UNKNOWN` paths must have the form `__UNKNOWN/<name_hash>`, where `<name_hash>` is a 32
    /// or 64 bit hexadecimal number (without the `0x` prefix).
    ///
    /// The name hash for these files will be parsed verbatim from `<name_hash>`. For example, for
    /// a file with the path `__UNKNOWN/DEADBEEF` the name hash will be `0xDEAD_BEEF`.
    ///
    /// ### `__DUPLICATE`
    ///
    /// Paths in the `__DUPLICATE` directory must have the form
    /// `__DUPLICATE/<original_path>__DUPLICATE_<number>.<extension>`.
    ///
    /// These paths will be cleaned from all the `DUPLICATE` stuff and the name hash for them will
    /// be computed from their `<original_path>`. For example, the name hash for
    /// `__DUPLICATE/credits/pc/credits__DUPLICATE_1.xml` will be the same as for
    /// `credits/pc/credits.xml`. Note that unlike Gibbed.Disrupt, the duplicate `<number>` will
    /// not be respected.
    ///
    /// [Gibbed.Disrupt]: https://github.com/gibbed/Gibbed.Disrupt
    ///
    /// # Errors
    ///
    /// This function will return an error under a number of different circumstances. See the
    /// documentation for [`PackError`] for details.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io::Cursor;
    ///
    /// use disrupt_bigfile::builder::ArchiveBuilder;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wd1_win64();
    /// let mut builder = ArchiveBuilder::create(header, "out_dir/file.dat")?;
    ///
    /// let mut data = Cursor::new("<credits>John Doe</credits>");
    ///
    /// // If you need to, you can pass a mutable reference to your reader in order to avoid
    /// // consuming it:
    /// builder.add(&mut data, "credits/pc/credits.xml")?;
    /// # Ok::<(), disrupt_bigfile::builder::PackError>(())
    /// ```
    pub fn add(
        &mut self,
        mut input: impl Read,
        relative_entry_path: impl AsRef<Path>,
    ) -> Result<(), PackError> {
        let entry_count: u32 = self
            .entries
            .len()
            .try_into()
            .expect("add() should guarantee that entries.len() <= u32::MAX");
        if entry_count == u32::MAX {
            return Err(PackError::TableIsFull);
        }

        // If the last write failed, dat_position might not accurately reflect dat's actual
        // position. Let's fix this by rewinding dat to the position after the last successful
        // add() call.
        if self.last_write_failed {
            self.dat.seek(SeekFrom::Start(self.dat_position))?;
            self.last_write_failed = false;
        }

        let relative_entry_path = relative_entry_path.as_ref();
        let name_hash = self
            .compute_name_hash(relative_entry_path)
            .ok_or_else(|| PackError::InvalidSpecialPath(relative_entry_path.to_path_buf()))?;
        let offset = self.dat_position;

        let copied =
            io::copy(&mut input, &mut self.dat).inspect_err(|_| self.last_write_failed = true)?;
        self.dat_position += copied;

        let entry = Entry {
            name_hash,
            offset,
            compression_scheme: CompressionScheme::None,
            uncompressed_size: copied,
            compressed_size: copied,
        }
        .validate(
            self.fat_header.table_version(),
            self.fat_header.compression_version(),
        )?;

        self.entries.push(entry);

        Ok(())
    }

    /// Adds a new file to the archive, copying its contents from a file located at the
    /// [`relative_entry_path`] in the `archive_root` directory to the DAT and creating an
    /// [`Entry`] for it with the [`name_hash`] computed from the `relative_entry_path`
    ///
    /// For example, if the `archive_root` is `root_dir/` and the `relative_entry_path` is set
    /// `credits/pc/credits.xml`, then the file will be copied from
    /// `root_dir/credits/pc/credits.xml`.
    ///
    /// This is a convinience function for opening a [`File`] and archiving it using [`Self::add`].
    ///
    /// [`relative_entry_path`]: Self#relative_entry_path
    /// [`name_hash`]: Entry::name_hash
    ///
    /// # Errors
    ///
    /// This function will return an error under a number of different circumstances. See the
    /// documentation for [`PackError`] for details.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use disrupt_bigfile::builder::ArchiveBuilder;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wd1_win64();
    /// let mut builder = ArchiveBuilder::create(header, "out_dir/file.dat")?;
    ///
    /// let archive_root = "root_dir/";
    ///
    /// // Adds a file from root_dir/ui/texture.xbt
    /// builder.add_file(archive_root, "ui/texture.xbt")?;
    ///
    /// // Adds a file from root_dir/generated/database.obj
    /// builder.add_file(archive_root, "generated/database.obj")?;
    /// # Ok::<(), disrupt_bigfile::builder::PackError>(())
    /// ```
    pub fn add_file(
        &mut self,
        archive_root: impl AsRef<Path>,
        relative_entry_path: impl AsRef<Path>,
    ) -> Result<(), PackError> {
        let file_path = archive_root.as_ref().join(&relative_entry_path);
        let file = File::open(file_path)?;

        self.add(file, relative_entry_path)
    }

    /// Finishes building this archive, flushing any remaining buffered data to the wrapped DAT
    /// writer and returning the [`Fat`] constructed for it.
    ///
    /// # Errors
    ///
    /// This function will return an I/O error if it fails to [`flush`] the wrapped DAT writer.
    ///
    /// [`flush`]: Write::flush
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use disrupt_bigfile::builder::ArchiveBuilder;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wd1_win64();
    /// let mut builder = ArchiveBuilder::create(header, "out_dir/file.dat")?;
    ///
    /// builder.add_file("root_dir/", "ui/texture.xbt")?;
    ///
    /// let fat = builder.finish()?;
    /// fat.create("out_dir/file.fat")?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn finish(mut self) -> Result<Fat, io::Error> {
        self.dat.flush()?;

        Ok(Fat::new_unchecked(self.fat_header, self.entries))
    }
}

impl ArchiveBuilder<BufWriter<File>> {
    /// Creates a new builder that will write the contents of the files being archived to a new
    /// file at `dat_path` and build a [`Fat`] with the given `fat_header`.
    ///
    /// If a file at `dat_path` already exists, it will be overwritten.
    ///
    /// # Errors
    ///
    /// This function will return an I/O error if it fails to open or seek the file at `dat_path`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use disrupt_bigfile::builder::ArchiveBuilder;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let fat_header = FatHeader::new_wd1_win64();
    ///
    /// let builder = ArchiveBuilder::create(fat_header, "out_dir/file.dat")?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn create(fat_header: FatHeader, dat_path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let dat = BufWriter::new(File::create(dat_path)?);

        Self::new(fat_header, dat)
    }
}
