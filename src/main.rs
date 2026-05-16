use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use indicatif::{ProgressBar, ProgressIterator, ProgressStyle};
use walkdir::WalkDir;

use disrupt_bigfile::builder::ArchiveBuilder;
use disrupt_bigfile::dat::Dat;
use disrupt_bigfile::fat::Fat;
use disrupt_bigfile::header::{
    CompressionVersion, Dependency, FatHeader, FatVersion, NameHashVersion, Platform, TableVersion,
};

fn existing_file_parser(source: &str) -> Result<PathBuf, io::Error> {
    let path = PathBuf::from(source).canonicalize()?;
    let metadata = path.metadata()?;

    if metadata.is_file() {
        Ok(path)
    } else {
        Err(io::ErrorKind::IsADirectory.into())
    }
}

fn existing_dir_parser(source: &str) -> Result<PathBuf, io::Error> {
    let path = PathBuf::from(source).canonicalize()?;
    let metadata = path.metadata()?;

    if metadata.is_dir() {
        Ok(path)
    } else {
        Err(io::ErrorKind::NotADirectory.into())
    }
}

fn hex_u64_parser(source: &str) -> Result<u64, String> {
    u64::from_str_radix(source, 16)
        .map_err(|err| format!("failed to parse hex 64-bit integer: {err}"))
}

fn dependency_parser(source: &str) -> Result<Dependency, String> {
    let parts: Vec<&str> = source.split(',').collect();
    if parts.len() != 2 {
        return Err("expected two comma-separated hexadecimal integers".to_string());
    }

    let archive_hash = hex_u64_parser(parts[0])?;
    let name_hash = hex_u64_parser(parts[1])?;

    Ok(Dependency {
        archive_hash,
        name_hash,
    })
}

#[derive(Clone, Debug, ValueEnum)]
enum Preset {
    /// All archives except "sound*" in the Windows version of Watch Dogs 1
    Wd1Win64,

    /// "sound*" archives in the Windows version of Watch Dogs 1
    Wd1Win64Sound,

    /// All archives except "sound*" in the Wii U version of Watch Dogs 1
    Wd1WiiU,

    /// "sound*" archives in the Wii U version of Watch Dogs 1
    Wd1WiiUSound,

    /// All archives except "sound*" in the Windows version of Watch Dogs 2
    Wd2Win64,

    /// All archives except "sound*" in the PS4 version of Watch Dogs 2
    Wd2Ps4,

    /// "sound*" archives in the Windows and PS4 versions of Watch Dogs 2
    Wd2Sound,

    /// All archives except "london" and "london_cache" in the Windows version of Watch Dogs:
    /// Legion
    WdlWin64,

    /// "london" archive in the Windows version of Watch Dogs: Legion
    WdlWin64London,

    /// "london_cache" archive in the Windows version of Watch Dogs: Legion
    WdlWin64LondonCache,
}

impl From<Preset> for FatHeader {
    fn from(preset: Preset) -> Self {
        match preset {
            Preset::Wd1Win64 => Self::new_wd1_win64(),
            Preset::Wd1Win64Sound => Self::new_wd1_win64_sound(),
            Preset::Wd1WiiU => Self::new_wd1_wiiu(),
            Preset::Wd1WiiUSound => Self::new_wd1_wiiu_sound(),
            Preset::Wd2Win64 => Self::new_wd2_win64(),
            Preset::Wd2Sound => Self::new_wd2_sound(),
            Preset::Wd2Ps4 => Self::new_wd2_ps4(),
            Preset::WdlWin64 => Self::new_wdl_win64(),
            Preset::WdlWin64London => Self::new_wdl_win64_london(),
            Preset::WdlWin64LondonCache => Self::new_wdl_win64_london_cache(),
        }
    }
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Display info from a FAT's header
    Info {
        /// Path to a FAT file
        #[arg(value_parser = existing_file_parser)]
        fat: PathBuf,

        /// Print out info in a short one-line format
        #[arg(short, long)]
        short: bool,
    },
    /// List files in a FAT without unpacking anything
    List {
        /// Path to a FAT file
        #[arg(value_parser = existing_file_parser)]
        fat: PathBuf,

        /// Print out compressed size, compression scheme and offset alongside entry filename
        #[arg(short, long)]
        verbose: bool,
    },
    /// Extract files from a BigFile to a directory
    Unpack {
        /// Path to the FAT file
        #[arg(value_parser = existing_file_parser)]
        fat: PathBuf,

        /// Path to the DAT file
        #[arg(value_parser = existing_file_parser)]
        dat: Option<PathBuf>,

        /// Path to the output directory
        ///
        /// If this directory does not exist, it will be created.
        #[arg(short, long, default_value = "./out/")]
        out: PathBuf,
    },
    /// Create a BigFile from a directory
    Pack {
        /// Path to the directory
        #[arg(value_parser = existing_dir_parser)]
        dir: PathBuf,

        /// Path to the directory where the DAT and FAT will be created
        ///
        /// If this directory does not exist, it will be created.
        #[arg(short, long, default_value = "./out/")]
        out: PathBuf,

        /// Filename of the DAT and FAT
        #[arg(long)]
        name: Option<OsString>,

        /// Affects which fields are present in the FAT header
        ///
        /// FAT3 is used in WD1, and FAT5 is used in WD2 and Legion.
        #[arg(short, long)]
        fat_version: Option<FatVersion>,

        /// Affects the binary layout of the FAT's contents
        #[arg(short, long)]
        table_version: Option<TableVersion>,

        /// Platform that the archive targets
        #[arg(short, long)]
        platform: Option<Platform>,

        /// Changes which compression schemes are available and their IDs
        #[arg(short, long)]
        compression_version: Option<CompressionVersion>,

        /// Does not seem to affect anything
        #[arg(short, long)]
        name_hash_version: Option<NameHashVersion>,

        /// Does not seem to affect anything. Available only for FAT5 archives.
        ///
        /// In Watch Dogs 2, it is always set to 0xFFFF_FFFF_FFFF_FFFF in all archives.
        ///
        /// In Legion, it is set to the same value in all archives except london.fat, where it is
        /// set to 0xA7E2_977F_3F32_B98E; and london_cache.fat, where it is set to
        /// 0xB782_28C0_B350_CC14.
        #[arg(short, long, value_parser = hex_u64_parser)]
        archive_hash: Option<u64>,

        /// Add a dependency to the archive, as two comma-separated hexadecimal 64-bit integers,
        /// for example "--dependency B78228C0B350CC14,BE38E2B5954E5FA4". Available only in FAT5
        /// archives.
        ///
        /// The first value is the dependency's archive hash and the second value is the
        /// dependency's name hash.
        ///
        /// This option may be used more than once.
        ///
        /// In Watch Dogs 2, dependencies are not present in any of the archives.
        ///
        /// In Legion, dependencies are present only in london.fat and london_cache.fat.
        #[arg(short, long = "dependency", value_parser = dependency_parser)]
        dependencies: Option<Vec<Dependency>>,

        /// Preset for the FAT header
        ///
        /// Manually specifying a header field using a flag such as --fat-version, --platform
        /// will override the field from the preset.
        #[arg(short = 'P', long, default_value = "wd1-win64")]
        preset: Preset,
    },
}

#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    #[command(subcommand)]
    action: Action,
}

fn info(fat_path: PathBuf, short: bool) -> Result<(), Box<dyn Error>> {
    let fat = Fat::open(&fat_path)?;
    let header = fat.header();

    if short {
        println!("{header}");
    } else {
        println!(
            "{}\n",
            fat_path
                .file_name()
                .expect("existing_file_parser() should guarantee that fat_path has a filename")
                .display()
        );

        println!("FAT version:         {}", header.fat_version());
        println!("Table version:       {}", header.table_version());
        println!("Platform:            {}", header.platform());
        println!("Compression version: {}", header.compression_version());
        println!("Name hash version:   {}", header.name_hash_version());
        println!("Entry count:         {}", fat.entries().len());

        if header.fat_version() == FatVersion::Fat5 {
            let archive_hash = header
                .archive_hash()
                .expect("archive hash should be always present on FAT5");
            let dependencies = header
                .dependencies()
                .expect("dependencies should be always present on FAT5");

            println!("Archive hash:        0x{archive_hash:X}");

            println!("Dependencies:");
            for dependency in dependencies {
                println!("\t-> {dependency}");
            }
        }
    }

    Ok(())
}

fn list(fat_path: PathBuf, verbose: bool) -> Result<(), Box<dyn Error>> {
    let fat = Fat::open(fat_path)?;

    let mut lock = io::stdout().lock();
    for entry in fat.entries() {
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

    eprintln!("Unpacking {} entries from...", fat.entries().len());
    eprintln!("\tFAT: {}", fat_path.display());
    eprintln!("\tDAT: {}\n", dat_path.display());
    eprintln!("FAT info: {}", fat.header());

    let (_, entries) = fat.into_inner();
    for (entry, result) in dat.bulk_unpack_to_dir(entries, out_dir).progress() {
        result.map_err(|err| format!("failed to unpack {}: {err}", entry.path().display()))?;
    }

    Ok(())
}

fn pack(
    in_dir: PathBuf,
    out_dir: PathBuf,
    archive_name: Option<OsString>,
    preset: Preset,
    fat_version: Option<FatVersion>,
    table_version: Option<TableVersion>,
    platform: Option<Platform>,
    compression_version: Option<CompressionVersion>,
    name_hash_version: Option<NameHashVersion>,
    archive_hash: Option<u64>,
    dependencies: Option<Vec<Dependency>>,
) -> Result<(), Box<dyn Error>> {
    let preset: FatHeader = preset.into();

    let fat_version = fat_version.unwrap_or(preset.fat_version());
    let table_version = table_version.unwrap_or(preset.table_version());
    let compression_version = compression_version.unwrap_or(preset.compression_version());
    let platform = platform.unwrap_or(preset.platform());
    let name_hash_version = name_hash_version.unwrap_or(preset.name_hash_version());

    let archive_hash = archive_hash.or(preset.archive_hash());
    let dependencies = dependencies.or_else(|| preset.dependencies().map(Vec::from));

    let header = match fat_version {
        FatVersion::Fat3 => {
            if archive_hash.is_some() {
                eprintln!("Warning: ignoring archive hash as it is not supported for FAT3");
            }
            if dependencies.is_some() {
                eprintln!("Warning: ignoring dependencies as they are not supported for FAT3");
            }

            FatHeader::new_fat3(
                table_version,
                platform,
                compression_version,
                name_hash_version,
            )
        }
        FatVersion::Fat5 => {
            let archive_hash =
                archive_hash.ok_or("archive hash is required for FAT5, but it is missing")?;
            let dependencies = dependencies.unwrap_or(vec![]);

            FatHeader::new_fat5(
                table_version,
                platform,
                compression_version,
                name_hash_version,
                archive_hash,
                dependencies,
            )?
        }
    };

    eprintln!("FAT info: {header}\n");

    let archive_name = archive_name
        .or_else(|| out_dir.file_stem().map(OsString::from))
        .unwrap_or("out".into());

    fs::create_dir_all(&out_dir)?;
    let out_dir = out_dir.canonicalize()?;

    let outfiles_base_path = out_dir.join(archive_name);
    let fat_path = outfiles_base_path.with_extension("fat");
    let dat_path = outfiles_base_path.with_extension("dat");

    eprintln!("Packing {} to...", in_dir.display());
    eprintln!("\tFAT: {}", fat_path.display());
    eprintln!("\tDAT: {}", dat_path.display());

    let mut builder = ArchiveBuilder::create(header, &dat_path)?;

    let spinner = ProgressBar::no_length().with_style(
        ProgressStyle::with_template("{spinner} Packed files: {pos}")
            .expect("hardcoded template should always be valid"),
    );
    for entry in WalkDir::new(&in_dir) {
        let entry = entry?;
        if entry.file_type().is_dir() || entry.path() == out_dir {
            continue;
        }

        let relative_entry_path = entry.path().strip_prefix(&in_dir)?;

        builder
            .add_file(&in_dir, relative_entry_path)
            .map_err(|err| format!("failed to add {}: {err}", entry.path().display()))?;

        spinner.inc(1);
    }

    builder.finish()?.create(fat_path)?;

    spinner.finish_and_clear();

    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();

    let action_result: Result<(), Box<dyn Error>> = match args.action {
        Action::Info { fat, short } => info(fat, short),
        Action::List { fat, verbose } => list(fat, verbose),
        Action::Unpack { fat, dat, out } => unpack(fat, dat, out),
        Action::Pack {
            dir,
            out,
            name,
            fat_version,
            table_version,
            platform,
            compression_version,
            name_hash_version,
            archive_hash,
            dependencies,
            preset,
        } => pack(
            dir,
            out,
            name,
            preset,
            fat_version,
            table_version,
            platform,
            compression_version,
            name_hash_version,
            archive_hash,
            dependencies,
        ),
    };

    match action_result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Error: {err}");
            ExitCode::FAILURE
        }
    }
}
