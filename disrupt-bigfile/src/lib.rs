//! Extract, create and inspect BigFile archives used by [Ubisoft's Disrupt engine] in the
//! [Watch Dogs] games.
//!
//! This project is heavily based on the research done in the [Gibbed.Disrupt] project, a tool used
//! for modding Watch Dogs games. This project aims to be an easy-to-use, performant, compatible
//! and documented alternative for it.
//!
//! <div class="warning">
//!
//! This is an unofficial, fan-made project and is not affiliated with, endorsed by, or approved by
//! Ubisoft. Watch Dogs, and all related trademarks, characters and intellectual property are owned
//! by Ubisoft.
//!
//! </div>
//!
//! <div class="warning">
//!
//! This is the API documentation for the library part of this project, meant for developers to
//! understand how to use `disrupt-bigfile` in their own projects. See the [CLI documentation] if
//! you just want to install mods for your game.
//!
//! </div>
//!
//! [Ubisoft's Disrupt engine]: https://en.wikipedia.org/wiki/Ubisoft#Disrupt
//! [Watch Dogs]: https://en.wikipedia.org/wiki/Watch_Dogs
//! [Gibbed.Disrupt]: https://github.com/gibbed/Gibbed.Disrupt
//! [CLI documentation]: https://github.com/hashadex/disrupt-bigfile/blob/main/README.md
//!
//! # About BigFile
//!
//! BigFile is an archive format used by the Watch Dogs games to store their files. A single
//! BigFile archive consists of two files: a .FAT and a .DAT.
//!
//! The DAT stores the concatenated contents of the files contained within the archive, however it
//! does not store any other information. The DAT does not store any filenames, nor does it store
//! any information about where each file starts and ends. Instead, all of that is stored in the
//! FAT file.
//!
//! The FAT serves as an index of the archive's files. Each file is described by a FAT [`Entry`],
//! which stores the file's name in the form of an [FNV-1 hash], the compression scheme used, the
//! compressed and uncompressed sizes, as well as the offset at which the file's content starts in
//! the DAT.
//!
//! Aside from the file information, the FAT also stores various metadata, like the archive's
//! version, target platform, etc. in its [header].
//!
//! [`Entry`]: entry::Entry
//! [FNV-1 hash]: https://en.wikipedia.org/wiki/Fowler%E2%80%93Noll%E2%80%93Vo_hash_function#FNV-1_hash
//! [header]: FatHeader
//!
//! # Usage examples
//!
//! These examples demonstrate the most common use cases of this library.
//!
//! ## Inspecting a FAT
//!
//! Use the [`Fat`] struct to deserialize a FAT file and inspect its metadata and file entries.
//!
//! ```no_run
//! use disrupt_bigfile::Fat;
//!
//! // Open the FAT file and deserialize it
//! let fat = Fat::open("path/to/file.fat")?;
//!
//! // Print the archive metadata
//! println!("Metadata: {}", fat.header());
//! // -> "Metadata: FAT3, Table V8, Platform Win64, Compression V5, Name hash V50"
//!
//! // Print the name of the files in the archive
//! let entries = fat.entries();
//! for entry in entries {
//!     println!("{entry}");
//!     // -> "ui/fire/bin/menu_map3d.feu"
//!
//!     // Or, use the alternate format to display more information about the entry:
//!     println!("{entry:#}");
//!     // -> "160093B (44992B XMemCompress) @ 0x468D6: ui/fire/bin/menu_map3d.feu"
//! }
//! println!("Total entries: {}", entries.len());
//! # Ok::<(), disrupt_bigfile::fat::FatDeserializationError>(())
//! ```
//!
//! ## Extracting files from an archive
//!
//! Use the [`Dat`] struct to read and decompress files represented by the FAT `Entries`.
//!
//! ```no_run
//! use disrupt_bigfile::{Dat, Fat};
//!
//! let fat = Fat::open("path/to/file.fat")?;
//! let mut dat = Dat::open("path/to/file.dat")?;
//!
//! // Use unpack_to_dir if you need to extract a small amount of entries
//! dat.unpack_to_dir(fat.entries()[0], "path/to/dest_dir")?;
//!
//! // But if you need to extract a large amount of entries in bulk, it's better to use
//! // unpack_to_dir_iter instead, because it offers better performance.
//! let entries = fat.into_entries();
//! for (entry, result) in dat.unpack_to_dir_iter(entries, "path/to/dest_dir") {
//!     match result {
//!         Ok(()) => println!("Unpacked {entry} successfully"),
//!         Err(err) => println!("Failed to unpack {entry}: {err}"),
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Creating a new archive
//!
//! Use the [`ArchiveBuilder`] to add files to a DAT and create FAT `Entries` for the added files.
//!
//! ```no_run
//! use disrupt_bigfile::{ArchiveBuilder, FatHeader};
//!
//! // First, we need to choose the metadata configuration for our archive. You can construct your
//! // own or use one of the presets provided by the library. Let's use the metadata preset used by
//! // most archives in the Windows release of Watch Dogs 1:
//! let header = FatHeader::new_wd1_win64();
//!
//! // Then, we create a new ArchiveBuilder with that metadata configuration:
//! let mut builder = ArchiveBuilder::create(header, "output_dir/file.dat")?;
//!
//! // Use the add_file method to write some files from your filesystem to the DAT. It will
//! // automatically compute name hashes and create FAT entries for these files.
//! let archive_root = "input_dir";
//!
//! // Adds from "input_dir/ui/file.xbt"
//! builder.add_file(archive_root, "ui/file.xbt")?;
//!
//! // Adds from "input_dir/domino/file.lua"
//! builder.add_file(archive_root, "domino/file.lua")?;
//!
//! // Adds from "input_dir/languages/file.loc"
//! builder.add_file(archive_root, "languages/file.loc")?;
//!
//! // After we're done writing the files, create the FAT:
//! let fat = builder.finish()?;
//! fat.create("output_dir/file.fat")?;
//! # Ok::<(), disrupt_bigfile::builder::PackError>(())
//! ```
mod name_hash_db;
mod vec;

pub mod builder;
pub mod compression;
pub mod dat;
pub mod entry;
pub mod fat;
pub mod header;

pub use builder::ArchiveBuilder;
pub use dat::Dat;
pub use fat::Fat;
pub use header::FatHeader;
