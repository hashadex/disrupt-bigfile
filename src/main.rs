use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use disrupt_bigfile::dat::Dat;
use disrupt_bigfile::fat::Fat;
use indicatif::ProgressIterator;

fn open_fat(path: impl AsRef<Path>) -> anyhow::Result<Fat> {
    let path = path.as_ref();

    Fat::open(path)
        .with_context(|| format!("failed to deserialize a FAT from '{}'", path.display()))
}

fn suppress_broken_pipe(error: anyhow::Error) -> anyhow::Result<()> {
    if let Some(io_error) = error.downcast_ref::<io::Error>()
        && io_error.kind() == io::ErrorKind::BrokenPipe
    {
        Ok(())
    } else {
        Err(error)
    }
}

/// Print the metadata of a FAT
///
/// Display the information stored in the header of one or more FAT files, such as:
///
/// * FAT version
/// * Table version
/// * Platform
/// * Compression version
/// * Name hash version
/// * Archive hash and dependencies (FAT5 only)
#[derive(Args, Debug)]
#[command(verbatim_doc_comment)]
struct InfoArgs {
    /// Path to a FAT file
    #[arg(required = true, value_name = "FAT_PATH")]
    fat_paths: Vec<PathBuf>,

    /// Show info in a compact, one line view
    #[arg(short, long)]
    short: bool,
}

fn info(args: InfoArgs) -> anyhow::Result<()> {
    let mut lock = io::stdout().lock();

    for fat_path in args.fat_paths {
        let path_display = fat_path.display();

        let fat = open_fat(&fat_path)?;
        let header = fat.header();

        if args.short {
            writeln!(lock, "{path_display}: {header}")
        } else {
            writeln!(lock, "{path_display}\n{header:#}")
        }?
    }

    Ok(())
}

/// Print the filename of each entry in a FAT
///
/// Display the source of each entry's name hash by querying the internal name hash database. If an
/// entry with an unknown name hash is encountered, a filename in the form of
/// "__UNKNOWN/<NAME_HASH>" will be displayed instead.
#[derive(Args, Debug)]
struct ListArgs {
    /// Path to a FAT file
    #[arg(required = true, value_name = "FAT_PATH")]
    fat_paths: Vec<PathBuf>,

    /// Also display each entry's compressed and uncompressed sizes, compression scheme and offset
    #[arg(short, long)]
    verbose: bool,
}

fn list(args: ListArgs) -> anyhow::Result<()> {
    let mut lock = io::stdout().lock();
    let multiple_paths = args.fat_paths.len() > 1;

    let mut fat_paths = args.fat_paths.iter().peekable();
    while let Some(fat_path) = fat_paths.next() {
        let fat = open_fat(&fat_path)?;

        if multiple_paths {
            writeln!(lock, "{}:", fat_path.display())?;
        }
        for entry in fat.entries() {
            if args.verbose {
                writeln!(lock, "{entry:#}")
            } else {
                writeln!(lock, "{entry}")
            }?;
        }
        if multiple_paths && fat_paths.peek().is_some() {
            writeln!(lock)?;
        }
    }

    Ok(())
}

#[derive(Args, Debug)]
#[group(required = true)]
struct ArchivePaths {
    /// Path to a FAT file
    ///
    /// Will be inferred from the DAT file path if missing.
    #[arg(short, long = "fat")]
    fat_path: Option<PathBuf>,

    /// Path to a DAT file
    ///
    /// Will be inferred from the FAT file path if missing.
    #[arg(short, long = "dat")]
    dat_path: Option<PathBuf>,
}

/// Extract all files from a BigFile archive to a directory
///
/// This command will deserialize all file entries from the given FAT file and use the information
/// from those entries to locate each file's contents in the DAT and extract them.
///
/// It is not necessary to specify both the --fat and --dat flags. If only one file is specified,
/// the program will try to find the other one in the same directory.
///
/// Due to the fact that BigFile archives store only the filename hash of each entry instead of
/// their actual names, the program uses its internal name hash source database to look up entries'
/// filenames by their name hashes. If an entry with a name hash that is not present in the
/// database is encountered, it will be placed into the "__UNKNOWN" directory, like Gibbed.Disrupt
/// does.
#[derive(Args, Debug)]
struct UnpackArgs {
    #[command(flatten)]
    paths: ArchivePaths,

    /// Path to the output directory
    ///
    /// The specified directory will be automatically created if it does not exist.
    ///
    /// If this flag is missing, the program will unpack the files to a subdirectory created in
    /// the current working directory.
    #[arg(short, long = "output")]
    output_dir: Option<PathBuf>,
}

fn unpack(args: UnpackArgs) -> anyhow::Result<()> {
    let fat_path = args.paths.fat_path.unwrap_or_else(|| {
        args.paths
            .dat_path
            .as_ref()
            .expect("clap should guarantee that at least one arg from the group is present")
            .with_extension("fat")
    });
    let dat_path = args
        .paths
        .dat_path
        .unwrap_or_else(|| fat_path.with_extension("dat"));
    let output_dir = args
        .output_dir
        .as_deref()
        .or_else(|| fat_path.file_stem().map(Path::new))
        .unwrap_or("output".as_ref());

    eprintln!(
        "Unpacking archive to directory '{}' from...",
        output_dir.display()
    );
    eprintln!("\tFAT: '{}'", fat_path.display());
    eprintln!("\tDAT: '{}'\n", dat_path.display());

    let fat = open_fat(&fat_path)?;
    let (header, entries) = fat.into_inner();
    eprintln!("FAT info: {header}");

    let mut dat = Dat::open(&dat_path)
        .with_context(|| format!("failed to open DAT from '{}'", dat_path.display()))?;

    for (entry, result) in dat.unpack_to_dir_iter(entries, output_dir).progress() {
        result.with_context(|| format!("failed to unpack '{}'", entry.path().display()))?;
    }

    Ok(())
}

#[derive(Debug, Subcommand)]
enum Command {
    Info(InfoArgs),
    List(ListArgs),
    Unpack(UnpackArgs),
}

#[derive(Debug, Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    match args.command {
        Command::Info(args) => info(args).or_else(suppress_broken_pipe),
        Command::List(args) => list(args).or_else(suppress_broken_pipe),
        Command::Unpack(args) => unpack(args),
    }
}
