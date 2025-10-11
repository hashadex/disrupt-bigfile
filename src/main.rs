use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use disrupt_bigfile::{Fat3, Result};

fn existing_file(source: &str) -> std::result::Result<PathBuf, String> {
    let path = Path::new(source);
    let metadata = path
        .metadata()
        .map_err(|err| err.to_string())?;

    if !metadata.is_file() {
        Err("is not a file".to_string())
    } else {
        Ok(path.to_path_buf())
    }
}

#[derive(Debug, Subcommand)]
enum Action {
    /// List files in a FAT without unpacking anything
    List {
        /// Path to a FAT file
        #[arg(value_parser = existing_file)]
        fat: PathBuf
    }
}

#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    #[command(subcommand)]
    action: Action
}

fn list(fat_path: PathBuf) -> Result<()> {
    let mut file = BufReader::new(File::open(fat_path)?);
    let fat = Fat3::deserialize(&mut file)?;

    let names = fat.entries.iter().map(|entry| entry.path());
    for name in names {
        println!("{}", name.display())
    }

    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();

    let action_result = match args.action {
        Action::List { fat } => list(fat)
    };

    match action_result {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err}");
            ExitCode::FAILURE
        }
    }
}
