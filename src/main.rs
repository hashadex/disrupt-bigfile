use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::Context;
use clap::{Args, Parser, Subcommand};

use disrupt_bigfile::fat::Fat;

/// Show the metadata of a FAT
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

        let fat = Fat::open(&fat_path)
            .with_context(|| format!("failed to deserialize a FAT from '{path_display}'"))?;
        let header = fat.header();

        let result = if args.short {
            writeln!(lock, "{path_display}: {header}")
        } else {
            writeln!(lock, "{path_display}\n{header:#}")
        };

        match result {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return Ok(()),
            Err(other) => return Err(other.into()),
        }
    }

    Ok(())
}

#[derive(Debug, Subcommand)]
enum Command {
    Info(InfoArgs),
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
        Command::Info(args) => info(args),
    }
}
