//! FAT file serialization and deserialization.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use byteorder::{LE, ReadBytesExt, WriteBytesExt};

use crate::entry::{Entry, EntryError};
use crate::header::{
    CompressionVersion, FAT3_MAGIC, FAT5_MAGIC, FatHeader, FatVersion, Platform, TableVersion,
};
use crate::vec;

/// Errors that might happen during the deserialization of a [`Fat`] or a [`FatHeader`].
#[derive(Debug, thiserror::Error)]
pub enum FatDeserializationError {
    /// Failed to open a [`File`] or read enough bytes due to an I/O error.
    #[error("io error")]
    Io(#[from] io::Error),

    /// The signature (first four bytes) of the [`FatHeader`] was not equal to any of the known FAT
    /// file signatures ([`FAT3_MAGIC`] and [`FAT5_MAGIC`]).
    #[error("bad magic {0:#X}, expected {FAT3_MAGIC:#X} or {FAT5_MAGIC:#X}")]
    BadMagic(u32),

    /// The table version number of the [`FatHeader`] was not equal to any of the known
    /// [`TableVersion`]s.
    #[error("unknown table version {0}")]
    UnknownTableVersion(u32),

    /// The `platform_id` number of the [`FatHeader`] was not equal to any of the
    /// [supported `Platform` IDs] for the current `fat_version`.
    ///
    /// [supported `Platform` IDs]: Platform#platform-ids
    #[error("platform id {platform_id} is not supported for {fat_version}")]
    UnsupportedPlatformId {
        platform_id: u8,
        fat_version: FatVersion,
    },

    /// The compression version number of the [`FatHeader`] was not equal to any of the known
    /// [`CompressionVersion`]s.
    #[error("unknown compression version {0}")]
    UnknownCompressionVersion(u8),

    /// The hame hash version number of the [`FatHeader`] was not equal to any of the known
    /// [`NameHashVersion`]s.
    ///
    /// [`NameHashVersion`]: crate::header::NameHashVersion
    #[error("unknown name hash version {0}")]
    UnknownNameHashVersion(u8),

    /// The `0x0B` [`FatHeader`] byte (after the name hash version field) was not equal to `00`.
    #[error("unexpected padding byte {0:#X}, expected 0x00")]
    UnexpectedPaddingByte(u8),

    /// Failed to allocate enough memory to store the [`Dependencies`] of the [`FatHeader`], due to
    /// the system not having enough available memory, pointer width, or due to the `Dependency`
    /// count being ridiculous (like [`u32::MAX`]).
    ///
    /// [`Dependencies`]: crate::header::Dependency
    #[error("failed to allocate memory for {0} dependencies")]
    DependencyAllocationFailed(u32),

    /// Failed to allocate enough memory to store the [`Entries`] of the [`Fat`], due to the system
    /// not having enough available memory, pointer width, or due to the `Entry` count being
    /// ridiculous (like [`u32::MAX`]).
    ///
    /// [`Entries`]: Entry
    #[error("failed to allocate memory for {0} entries")]
    EntryAllocationFailed(u32),

    /// Failed to deserialize one of the [`Entries`] of a [`Fat`] due to it having a compression
    /// `scheme_id` that is unknown or unsupported for the current `compression_version`.
    ///
    /// [`Entries`]: Entry
    /// [`CompressionScheme` ID]: crate::entry::CompressionScheme#scheme-ids
    #[error(
        "unknown compression scheme id {scheme_id} for compression version {compression_version}"
    )]
    UnknownCompressionScheme {
        scheme_id: u8,
        compression_version: CompressionVersion,
    },
}

/// Errors that might happen during the construction of a [`Fat`] or a [`FatHeader`].
#[derive(Debug, thiserror::Error)]
pub enum FatConstructionError {
    /// Failed to construct the [`FatHeader`] due to the given `platform` not having an [ID]
    /// assigned to it on the selected `fat_version`.
    ///
    /// [ID]: Platform#platform-ids
    #[error("platform {platform} is not supported for {fat_version}")]
    UnsupportedPlatform {
        platform: Platform,
        fat_version: FatVersion,
    },

    /// Failed to construct the [`FatHeader`] due to the [`Dependency`] count being higher than
    /// [`u32::MAX`].
    ///
    /// [`Dependency`]: crate::header::Dependency
    #[error(
        "dependency count is too large to fit into header, expected {max} dependencies max, got {0}",
        max = u32::MAX
    )]
    DependencyCountWontFit(usize),

    /// Failed to construct the [`Fat`] due to the [`Entry`] count being higher than [`u32::MAX`].
    #[error(
        "entry count is too large to fit into header, expected {max} entries max, got {0}",
        max = u32::MAX
    )]
    EntryCountWontFit(usize),

    /// Failed to construct the [`Fat`] due because of an [invalid] `entry`.
    ///
    /// [invalid]: Entry::validate
    #[error("entry with name hash {:#X} is not valid", .entry.name_hash)]
    Entry {
        entry: Entry,
        #[source]
        error: EntryError,
    },
}

/// Index of archived files in a BigFile archive.
///
/// This struct represents a FAT file. It consists of a [header] that stores various archive
/// metadata, and a [list of `Entries`] that store information about archived files. You can
/// extract a file described by an [`Entry`] using the [`Dat`] struct.
///
/// `Fat` instances can be [serialized], [deserialized], [constructed manually] or created
/// automatically when using an [`ArchiveBuilder`] to pack files into a new BigFile archive.
///
/// [header]: Self::header
/// [list of `Entries`]: Self::entries
/// [`Dat`]: crate::dat::Dat
/// [serialized]: Self::serialize
/// [deserialized]: Self::deserialize
/// [constructed manually]: Self::new
/// [`ArchiveBuilder`]: crate::builder::ArchiveBuilder
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Fat {
    header: FatHeader,
    entries: Vec<Entry>,
}

impl Fat {
    /// Constructs a new `Fat` with the given `header` and `entries` **without checking if the
    /// entries are [valid] for the given `header`**.
    ///
    /// In most cases, you should just use [`Self::new`] instead of this function.
    ///
    /// The caller must guarantee that each [`Entry`] is valid for the [`TableVersion`] and
    /// [`CompressionVersion`] of the `header`, either manually or by using [`Entry::validate`].
    /// Violating this guarantee is considered a logic error and may lead to invalid outputs or
    /// even panics when serializing or otherwise using the constructed `Fat`. However, this
    /// function is completely memory safe and will not lead to [undefined behavior] under any
    /// circumstances.
    ///
    /// If needed, this function will automatically sort the `entries` by their [name hash] using
    /// [stable sorting]. This means that any `Entries` that have a duplicate name hash will retain
    /// their order.
    ///
    /// [valid]: Entry#versions-and-validity
    /// [undefined behavior]: https://doc.rust-lang.org/reference/behavior-considered-undefined.html
    /// [name hash]: Entry::name_hash
    /// [stable sorting]: slice::sort_by_key
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    /// use disrupt_bigfile::fat::Fat;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// let entry_a = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 0xCAFE_BABE,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 4096,
    ///     compressed_size: 2048,
    /// }
    /// .validate(header.table_version(), header.compression_version())?;
    /// let entry_b = Entry {
    ///     name_hash: 0xFEED_FACE,
    ///     offset: 0xCAFE_FEED,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 16384,
    ///     compressed_size: 8192,
    /// }
    /// .validate(header.table_version(), header.compression_version())?;
    ///
    /// let fat = Fat::new_unchecked(header, vec![entry_a, entry_b]);
    ///
    /// assert_eq!(fat.entries().len(), 2);
    /// # Ok::<(), disrupt_bigfile::entry::EntryError>(())
    /// ```
    #[must_use]
    pub fn new_unchecked(header: FatHeader, mut entries: Vec<Entry>) -> Self {
        let extract_name_hash = |entry: &Entry| entry.name_hash;
        if !entries.is_sorted_by_key(extract_name_hash) {
            entries.sort_by_key(extract_name_hash);
        }

        Self { header, entries }
    }

    /// Constructs a new `Fat` with the given `header` and `entries`.
    ///
    /// Before constructing the `Fat`, this function will check that each [`Entry`] is [valid] for
    /// the given `header`. If you are absolutely sure that all `Entries` are valid and want to
    /// skip validation performed by this function, use [`Self::new_unchecked`].
    ///
    /// If needed, this function will automatically sort the `entries` by their [name hash] using
    /// [stable sorting]. This means that any `Entries` that have a duplicate name hash will retain
    /// their order.
    ///
    /// [valid]: Entry#versions-and-validity
    /// [name hash]: Entry::name_hash
    /// [stable sorting]: slice::sort_by_key
    ///
    /// # Errors
    ///
    /// This function will return the following error variants:
    ///
    /// * [`FatConstructionError::EntryCountWontFit`]: if the length of `entries` is larger than
    ///   [`u32::MAX`].
    /// * [`FatConstructionError::Entry`]: if any of the given `entries` are not valid for the
    ///   [`TableVersion`] and [`CompressionVersion`] of the `header`.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    /// use disrupt_bigfile::fat::Fat;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// let entry_a = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 0xCAFE_BABE,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 4096,
    ///     compressed_size: 2048,
    /// };
    /// let entry_b = Entry {
    ///     name_hash: 0xFEED_FACE,
    ///     offset: 0xCAFE_FEED,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 16384,
    ///     compressed_size: 8192,
    /// };
    ///
    /// let fat = Fat::new(header, vec![entry_a, entry_b])?;
    ///
    /// assert_eq!(fat.entries().len(), 2);
    /// # Ok::<(), disrupt_bigfile::fat::FatConstructionError>(())
    /// ```
    pub fn new(header: FatHeader, entries: Vec<Entry>) -> Result<Self, FatConstructionError> {
        let entry_count = entries.len();
        if u32::try_from(entry_count).is_err() {
            return Err(FatConstructionError::EntryCountWontFit(entry_count));
        }

        for &entry in &entries {
            entry
                .validate(header.table_version(), header.compression_version())
                .map_err(|error| FatConstructionError::Entry { entry, error })?;
        }

        Ok(Self::new_unchecked(header, entries))
    }

    /// Reads bytes from `input` and converts them into a `Fat`.
    ///
    /// If you need to deserialize a `Fat` from a file, use [`Self::open`] instead of opening a
    /// [`File`] manually.
    ///
    /// # Errors
    ///
    /// This function will return an error under a number of different circumstances. See the
    /// documentation for [`FatDeserializationError`] for details.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Cursor;
    ///
    /// use disrupt_bigfile::fat::Fat;
    ///
    /// let mut data = Cursor::new([
    ///     0x33, 0x54, 0x41, 0x46, 0x08, 0x00, 0x00, 0x00, 0x04, 0x05, 0x32, 0x00, 0x02, 0x00,
    ///     0x00, 0x00, 0x17, 0x12, 0xF4, 0x70, 0x00, 0x00, 0x00, 0x00, 0x2B, 0x00, 0x00, 0x00,
    ///     0x00, 0x00, 0x00, 0x00, 0xFA, 0xBB, 0x02, 0xC9, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x00,
    ///     0x00, 0x60, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    /// ]);
    ///
    /// // You can pass a mutable reference to your reader in order to avoid consuming it and use
    /// // it afterwards:
    /// let fat = Fat::deserialize(&mut data)?;
    ///
    /// assert_eq!(fat.entries().len(), 2);
    /// # Ok::<(), disrupt_bigfile::fat::FatDeserializationError>(())
    /// ```
    pub fn deserialize(mut input: impl Read) -> Result<Self, FatDeserializationError> {
        let header = FatHeader::deserialize(&mut input)?;

        let entry_count = input.read_u32::<LE>()?;
        let mut entries = vec::try_with_capacity(entry_count)
            .ok_or(FatDeserializationError::EntryAllocationFailed(entry_count))?;
        for _ in 0..entry_count {
            let entry = Entry::deserialize(
                &mut input,
                header.table_version(),
                header.compression_version(),
            )?;
            entries.push(entry);
        }

        Ok(Self::new_unchecked(header, entries))
    }

    /// Deserializes a `Fat` from a file at the given `path`.
    ///
    /// This is a convinience function for opening a [`File`], wrapping it in a [`BufReader`] and
    /// using [`Self::deserialize`] with it.
    ///
    /// # Errors
    ///
    /// This function will return an error under a number of different circumstances. See the
    /// documentation for [`FatDeserializationError`] for details.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use disrupt_bigfile::fat::Fat;
    ///
    /// let fat = Fat::open("path/to/file.fat")?;
    ///
    /// assert!(!fat.entries().is_empty());
    /// # Ok::<(), disrupt_bigfile::fat::FatDeserializationError>(())
    /// ```
    pub fn open(path: impl AsRef<Path>) -> Result<Self, FatDeserializationError> {
        let file = BufReader::new(File::open(path)?);
        Self::deserialize(file)
    }

    /// Converts this `Fat` to a binary format and writes the bytes to `out`.
    ///
    /// If you need to serialize a `Fat` to a file, use [`Self::create`] instead of creating a
    /// [`File`] manually.
    ///
    /// # Panics
    ///
    /// This function may panic if this `Fat` was incorrectly constructed with
    /// [`Self::new_unchecked`].
    ///
    /// # Errors
    ///
    /// This function will return an error if it fails to write all serialized bytes to `out`.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    /// use disrupt_bigfile::fat::Fat;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// let entry_a = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 0xCAFE_BABE,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 4096,
    ///     compressed_size: 2048,
    /// };
    /// let entry_b = Entry {
    ///     name_hash: 0xFEED_FACE,
    ///     offset: 0xCAFE_FEED,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 16384,
    ///     compressed_size: 8192,
    /// };
    ///
    /// let fat = Fat::new(header, vec![entry_a, entry_b])?;
    /// let mut out = Vec::new();
    ///
    /// // You can pass a mutable reference to your writer in order to avoid consuming it and use
    /// // it afterwards:
    /// fat.serialize(&mut out)?;
    ///
    /// assert!(!out.is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn serialize(&self, mut out: impl Write) -> Result<(), io::Error> {
        let header = &self.header;
        let entries = &self.entries;

        header.serialize(&mut out)?;

        let entry_count: u32 = entries
            .len()
            .try_into()
            .expect("constructor should guarantee that entry count fits into u32");
        out.write_u32::<LE>(entry_count)?;

        for entry in entries {
            entry.serialize_unchecked(
                &mut out,
                header.table_version(),
                header.compression_version(),
            )?;
        }

        // Duplicate count
        if header.table_version() == TableVersion::V13 {
            out.write_u32::<LE>(0)?;
        }

        // Localization count
        out.write_u32::<LE>(0)?;

        out.flush()?;

        Ok(())
    }

    /// Creates a new file at `path` and serializes this `Fat` into it.
    ///
    /// This is a convinience function for creating a [`File`], wrapping it in a [`BufWriter`] and
    /// using [`Self::serialize`] with it.
    ///
    /// # Errors
    ///
    /// This function will return an error if it fails to create the file or serialize the `Fat`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    /// use disrupt_bigfile::fat::Fat;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// let entry_a = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 0xCAFE_BABE,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 4096,
    ///     compressed_size: 2048,
    /// };
    /// let entry_b = Entry {
    ///     name_hash: 0xFEED_FACE,
    ///     offset: 0xCAFE_FEED,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 16384,
    ///     compressed_size: 8192,
    /// };
    ///
    /// let fat = Fat::new(header, vec![entry_a, entry_b])?;
    ///
    /// fat.create("path/to/file.fat")?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn create(&self, path: impl AsRef<Path>) -> Result<(), io::Error> {
        let mut file = BufWriter::new(File::create(path)?);
        self.serialize(&mut file).and_then(|()| file.flush())
    }

    /// Consumes this `Fat` and returns ownership of its header and entries.
    ///
    /// The returned entries are guaranteed to be sorted by their [name hashes] in ascending order.
    /// This is useful if you want to use [binary search] to find some entry by its name hash.
    ///
    /// [name hashes]: Entry::name_hash
    /// [binary search]: slice::binary_search_by_key
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use disrupt_bigfile::dat::Dat;
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    /// use disrupt_bigfile::fat::Fat;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// let entry_a = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 0xCAFE_BABE,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 4096,
    ///     compressed_size: 2048,
    /// };
    /// let entry_b = Entry {
    ///     name_hash: 0xFEED_FACE,
    ///     offset: 0xCAFE_FEED,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 16384,
    ///     compressed_size: 8192,
    /// };
    ///
    /// let fat = Fat::new(header, vec![entry_a, entry_b])?;
    ///
    /// let (header, entries) = fat.into_inner();
    ///
    /// // Use the returned owned entries Vec wherever needed:
    /// let mut dat = Dat::open("path/to/file.dat")?;
    /// for (entry, result) in dat.unpack_to_dir_iter(entries, "path/to/destination/") {
    ///     match result {
    ///         Ok(()) => println!("unpacked {entry} successfully"),
    ///         Err(e) => println!("failed to unpack {entry}: {e}"),
    ///     }
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn into_inner(self) -> (FatHeader, Vec<Entry>) {
        (self.header, self.entries)
    }

    /// Consumes this `Fat` and returns ownership of its entries, discarding the header.
    ///
    /// This is a convinience function for calling [`Self::into_inner`] and discarding the returned
    /// [`FatHeader`].
    ///
    /// The returned entries are guaranteed to be sorted by their [name hashes] in ascending order.
    /// This is useful if you want to use [binary search] to find some entry by its name hash.
    ///
    /// [name hashes]: Entry::name_hash
    /// [binary search]: slice::binary_search_by_key
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use disrupt_bigfile::dat::Dat;
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    /// use disrupt_bigfile::fat::Fat;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// let entry_a = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 0xCAFE_BABE,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 4096,
    ///     compressed_size: 2048,
    /// };
    /// let entry_b = Entry {
    ///     name_hash: 0xFEED_FACE,
    ///     offset: 0xCAFE_FEED,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 16384,
    ///     compressed_size: 8192,
    /// };
    ///
    /// let fat = Fat::new(header, vec![entry_a, entry_b])?;
    ///
    /// let entries = fat.into_entries();
    ///
    /// // Use the returned owned entries Vec wherever needed:
    /// let mut dat = Dat::open("path/to/file.dat")?;
    /// for (entry, result) in dat.unpack_to_dir_iter(entries, "path/to/destination/") {
    ///     match result {
    ///         Ok(()) => println!("unpacked {entry} successfully"),
    ///         Err(e) => println!("failed to unpack {entry}: {e}"),
    ///     }
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn into_entries(self) -> Vec<Entry> {
        let (_, entries) = self.into_inner();

        entries
    }

    /// Returns a reference to this `Fat`'s [`FatHeader`].
    ///
    /// Use [`Self::into_inner`] if you need to get ownership of it instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::fat::Fat;
    /// use disrupt_bigfile::header::{FatHeader, Platform};
    ///
    /// let fat = Fat::new(FatHeader::new_wd1_win64(), vec![])?;
    ///
    /// let header = fat.header();
    /// assert_eq!(header.platform(), Platform::Win64);
    /// # Ok::<(), disrupt_bigfile::fat::FatConstructionError>(())
    /// ```
    #[must_use]
    pub fn header(&self) -> &FatHeader {
        &self.header
    }

    /// Returns a reference to this `Fat`'s entries.
    ///
    /// Use [`Self::into_inner`] or [`Self::into_entries`] if you need to get ownership of them
    /// instead.
    ///
    /// # Examples
    ///
    /// ```
    /// use disrupt_bigfile::entry::{CompressionScheme, Entry};
    /// use disrupt_bigfile::fat::Fat;
    /// use disrupt_bigfile::header::FatHeader;
    ///
    /// let header = FatHeader::new_wd1_win64();
    ///
    /// let entry_a = Entry {
    ///     name_hash: 0xDEAD_BEEF,
    ///     offset: 0xCAFE_BABE,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 4096,
    ///     compressed_size: 2048,
    /// };
    /// let entry_b = Entry {
    ///     name_hash: 0xFEED_FACE,
    ///     offset: 0xCAFE_FEED,
    ///     compression_scheme: CompressionScheme::None,
    ///     uncompressed_size: 16384,
    ///     compressed_size: 8192,
    /// };
    ///
    /// let fat = Fat::new(header, vec![entry_a, entry_b])?;
    ///
    /// let entries = fat.entries();
    /// assert_eq!(entries.len(), 2);
    /// # Ok::<(), disrupt_bigfile::fat::FatConstructionError>(())
    /// ```
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}
