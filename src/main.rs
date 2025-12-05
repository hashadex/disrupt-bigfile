use std::error::Error;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use indicatif::ProgressIterator;

use disrupt_bigfile::dat::Dat;
use disrupt_bigfile::fat::Fat;

fn existing_file(source: &str) -> Result<PathBuf, String> {
    let path = Path::new(source);
    let metadata = path.metadata().map_err(|err| err.to_string())?;

    if metadata.is_file() {
        Ok(path.to_path_buf())
    } else {
        Err("is not a file".to_string())
    }
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Display info from a FAT's header
    Info {
        /// Path to a FAT file
        #[arg(value_parser = existing_file)]
        fat: PathBuf,
    },
    /// List files in a FAT without unpacking anything
    List {
        /// Path to a FAT file
        #[arg(value_parser = existing_file)]
        fat: PathBuf,

        /// Print out compressed size, compression scheme and offset alongside entry filename
        #[arg(short, long)]
        verbose: bool,
    },
    /// Extract files from a BigFile to a directory
    Unpack {
        /// Path to the FAT file
        #[arg(value_parser = existing_file)]
        fat: PathBuf,

        /// Path to the DAT file
        #[arg(value_parser = existing_file)]
        dat: Option<PathBuf>,

        /// Path to the output directory.
        #[arg(short, long, default_value = "./out/")]
        out: PathBuf,
    },
}

#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    #[command(subcommand)]
    action: Action,
}

fn info(fat_path: PathBuf) -> Result<(), Box<dyn Error>> {
    let fat = Fat::open(&fat_path)?;
    let metadata = fat.metadata;

    println!(
        "{}\n",
        fat_path
            .file_name()
            .expect("existing_file() should guarantee that fat_path has a filename")
            .display()
    );

    println!("FAT version:         {}", metadata.fat_version);
    println!("Entry version:       {}", metadata.entry_version);
    println!("Platform:            {}", metadata.platform);
    println!("Compression version: {}", metadata.compression_version);
    println!("Name hash version:   {}", metadata.name_hash_version);
    println!("Entry count:         {}", fat.entries.len());

    Ok(())
}

fn list(fat_path: PathBuf, verbose: bool) -> Result<(), Box<dyn Error>> {
    let fat = Fat::open(&fat_path)?;

    let mut lock = io::stdout().lock();
    for entry in fat.entries {
        if verbose {
            writeln!(lock, "{entry}")?;
        } else {
            writeln!(lock, "{}", entry.path().display())?;
        }
    }

    Ok(())
}

fn unpack(
    fat_path: PathBuf,
    dat_path: Option<PathBuf>,
    out_dir: PathBuf,
) -> Result<(), Box<dyn Error>> {
    let dat_path = dat_path.unwrap_or_else(|| {
        let dat_path_guess = fat_path.with_extension("dat");

        eprintln!(
            "Warning: no DAT path given. Assuming it's {}...",
            dat_path_guess.display()
        );

        dat_path_guess
    });

    let fat = Fat::open(&fat_path)?;
    let mut dat = Dat::open(&dat_path)?;

    eprintln!("Unpacking {} entries from...", fat.entries.len());
    eprintln!("\tFAT: {}", fat_path.display());
    eprintln!("\tDAT: {}\n", dat_path.display());
    eprintln!("FAT info: {}", fat.metadata);

    for entry in fat.entries.iter().progress() {
        dat.unpack_to_dir(*entry, &out_dir)
            .map_err(|err| format!("failed to unpack {}: {err}", entry.path().display()))?;
    }

    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();

    let action_result: Result<(), Box<dyn Error>> = match args.action {
        Action::Info { fat } => info(fat),
        Action::List { fat, verbose } => list(fat, verbose),
        Action::Unpack { fat, dat, out } => unpack(fat, dat, out),
    };

    match action_result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err}");
            ExitCode::FAILURE
        }
    }
}
