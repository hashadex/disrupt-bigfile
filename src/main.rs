use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use indicatif::ProgressIterator;

use disrupt_bigfile::Fat;

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

fn list(fat_path: PathBuf, verbose: bool) -> Result<(), Box<dyn Error>> {
    let mut fat_file = BufReader::new(File::open(fat_path)?);
    let fat = Fat::deserialize(&mut fat_file)?;

    for entry in fat.entries {
        if verbose {
            println!(
                "{}B {} @ 0x{:X}: {}",
                entry.compressed_size,
                entry.compression_scheme,
                entry.offset,
                entry.path().display(),
            );
        } else {
            println!("{}", entry.path().display());
        }
    }

    Ok(())
}

fn unpack(
    fat_path: PathBuf,
    dat_path: Option<PathBuf>,
    out_dir: PathBuf,
) -> Result<(), Box<dyn Error>> {
    let mut fat_file = BufReader::new(File::open(&fat_path)?);
    let fat = Fat::deserialize(&mut fat_file)?;

    let dat_path = dat_path.unwrap_or_else(|| {
        let dat_path_guess = fat_path.with_extension("dat");
        eprintln!(
            "Warning: no DAT path given. Assuming it's '{}'.",
            dat_path_guess.display()
        );
        dat_path_guess
    });

    let mut dat_file =
        BufReader::new(File::open(dat_path).map_err(|err| format!("failed to open DAT: {err}"))?);

    for entry in fat.entries.iter().progress() {
        entry
            .unpack_to_dir(&mut dat_file, &out_dir)
            .map_err(|err| format!("failed to unpack '{}': {err}", entry.path().display()))?;
    }

    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();

    let action_result: Result<(), Box<dyn Error>> = match args.action {
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
