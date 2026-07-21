use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};

use disrupt_bigfile::fat::Fat;

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

#[derive(Debug, Subcommand)]
enum Command {
    Info(InfoArgs),
    List(ListArgs),
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
    }
}
