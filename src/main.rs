use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use disrupt_bigfile::{Fat, FatError};

fn existing_file(source: &str) -> std::result::Result<PathBuf, String> {
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

fn main() -> ExitCode {
    let args = Args::parse();

    let action_result: Result<(), Box<dyn Error>> = match args.action {
        Action::List { fat, verbose } => list(fat, verbose),
    };

    match action_result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err}");
            ExitCode::FAILURE
        }
    }
}
