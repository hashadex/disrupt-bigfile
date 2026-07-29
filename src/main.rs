use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::num::ParseIntError;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, ensure};
use clap::builder::NonEmptyStringValueParser;
use clap::{Args, Parser, Subcommand, ValueEnum};
use disrupt_bigfile::builder::ArchiveBuilder;
use disrupt_bigfile::dat::Dat;
use disrupt_bigfile::fat::Fat;
use disrupt_bigfile::header::{
    CompressionVersion, Dependency, FatHeader, FatVersion, NameHashVersion, Platform, TableVersion,
};
use indicatif::{ProgressBar, ProgressIterator, ProgressStyle};
use walkdir::WalkDir;

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

#[derive(Clone, Debug, ValueEnum)]
enum HeaderPreset {
    Wd1Win64,
    Wd1Win64Sound,
    Wd1WiiU,
    Wd1WiiUSound,
    Wd2Win64,
    Wd2Ps4,
    Wd2Sound,
    WdlWin64,
    WdlWin64London,
    WdlWin64LondonCache,
}

impl From<HeaderPreset> for FatHeader {
    fn from(preset: HeaderPreset) -> Self {
        match preset {
            HeaderPreset::Wd1Win64 => Self::new_wd1_win64(),
            HeaderPreset::Wd1Win64Sound => Self::new_wd1_win64_sound(),
            HeaderPreset::Wd1WiiU => Self::new_wd1_wiiu(),
            HeaderPreset::Wd1WiiUSound => Self::new_wd1_wiiu_sound(),
            HeaderPreset::Wd2Win64 => Self::new_wd2_win64(),
            HeaderPreset::Wd2Ps4 => Self::new_wd2_ps4(),
            HeaderPreset::Wd2Sound => Self::new_wd2_sound(),
            HeaderPreset::WdlWin64 => Self::new_wdl_win64(),
            HeaderPreset::WdlWin64London => Self::new_wdl_win64_london(),
            HeaderPreset::WdlWin64LondonCache => Self::new_wdl_win64_london_cache(),
        }
    }
}

fn hex_u64_parser(source: &str) -> Result<u64, ParseIntError> {
    u64::from_str_radix(source, 16)
}

fn dependency_parser(source: &str) -> anyhow::Result<Dependency> {
    let parts = source
        .split(':')
        .map(|part| {
            hex_u64_parser(part)
                .map_err(|err| anyhow!("failed to parse a hexadecimal number from '{part}': {err}"))
        })
        .collect::<anyhow::Result<Vec<u64>>>()?;
    ensure!(
        parts.len() == 2,
        "expected two colon-separated hexadecimal values"
    );

    let archive_hash = parts[0];
    let name_hash = parts[1];

    Ok(Dependency {
        archive_hash,
        name_hash,
    })
}

#[derive(Args, Debug)]
struct PackArgs {
    input_dir: PathBuf,

    #[arg(short, long = "output", default_value = ".")]
    output_dir: PathBuf,

    #[arg(short = 'N', long = "name", value_parser = NonEmptyStringValueParser::new())]
    archive_name: Option<String>,

    #[arg(short = 'P', long, default_value = "wd1-win64")]
    preset: HeaderPreset,

    #[arg(short, long)]
    fat_version: Option<FatVersion>,

    #[arg(short, long)]
    table_version: Option<TableVersion>,

    #[arg(short, long)]
    platform: Option<Platform>,

    #[arg(short, long)]
    compression_version: Option<CompressionVersion>,

    #[arg(short, long)]
    name_hash_version: Option<NameHashVersion>,

    #[arg(short, long, value_parser = hex_u64_parser)]
    archive_hash: Option<u64>,

    #[arg(
        short,
        long = "dependency",
        value_parser = dependency_parser,
        value_name = "ARCHIVE_HASH:NAME_HASH"
    )]
    dependencies: Option<Vec<Dependency>>,
}

fn pack(args: PackArgs) -> anyhow::Result<()> {
    let preset: FatHeader = args.preset.into();
    let fat_version = args.fat_version.unwrap_or(preset.fat_version());
    let table_version = args.table_version.unwrap_or(preset.table_version());
    let platform = args.platform.unwrap_or(preset.platform());
    let compression_version = args
        .compression_version
        .unwrap_or(preset.compression_version());
    let name_hash_version = args.name_hash_version.unwrap_or(preset.name_hash_version());

    let fat_header = match fat_version {
        FatVersion::Fat3 => {
            if args.archive_hash.is_some() {
                eprintln!("Warning: archive hash is ignored as it is not supported for FAT3");
            }
            if args.dependencies.is_some() {
                eprintln!("Warning: dependencies are ignored as they are not supported for FAT3");
            }

            FatHeader::new_fat3(
                table_version,
                platform,
                compression_version,
                name_hash_version,
            )
        }
        FatVersion::Fat5 => {
            let archive_hash = args
                .archive_hash
                .or(preset.archive_hash())
                .unwrap_or(0xFFFF_FFFF_FFFF_FFFF);
            let dependencies = args
                .dependencies
                .or(preset.dependencies().map(Vec::from))
                .unwrap_or_default();

            FatHeader::new_fat5(
                table_version,
                platform,
                compression_version,
                name_hash_version,
                archive_hash,
                dependencies,
            )
            .context("invalid FAT header configuration")?
        }
    };

    let input_dir = args.input_dir.canonicalize().with_context(|| {
        format!(
            "failed to access the input directory at '{}'",
            args.input_dir.display()
        )
    })?;
    let output_dir = fs::create_dir_all(&args.output_dir)
        .and_then(|()| args.output_dir.canonicalize())
        .with_context(|| {
            format!(
                "failed to access the output directory at {}",
                args.output_dir.display()
            )
        })?;
    let archive_name = args
        .archive_name
        .as_ref()
        .map(OsStr::new)
        .or(input_dir.file_name())
        .unwrap_or("packed".as_ref());

    let outfile_base_path = output_dir.join(archive_name);
    let fat_path = outfile_base_path.with_added_extension("fat");
    let dat_path = outfile_base_path.with_added_extension("dat");

    eprintln!("Packing directory '{}' to...", input_dir.display());
    eprintln!("\tFAT: '{}'", fat_path.display());
    eprintln!("\tDAT: '{}'\n", dat_path.display());
    eprintln!("FAT info: {fat_header}");

    let mut builder =
        ArchiveBuilder::create(fat_header, &dat_path).context("failed to create the DAT")?;

    let bar = ProgressBar::no_length().with_style(
        ProgressStyle::with_template("{spinner} Packed files: {pos}")
            .expect("hardcoded template should always be valid"),
    );
    for entry_result in WalkDir::new(&input_dir).into_iter().progress_with(bar) {
        let entry = entry_result.context("failed to walk the input directory")?;
        let path = entry.path();
        if entry.file_type().is_dir() || path == dat_path {
            continue;
        }

        let relative_entry_path = path
            .strip_prefix(&input_dir)
            .expect("walkdir should guarantee that path always starts with input_dir");
        builder
            .add_file(&input_dir, relative_entry_path)
            .with_context(|| format!("failed to add file '{}'", path.display()))?;
    }

    let fat = builder.finish().context("failed to flush the DAT file")?;
    fat.create(fat_path)
        .context("failed to serialize the FAT file")?;

    Ok(())
}

#[derive(Debug, Subcommand)]
enum Command {
    Info(InfoArgs),
    List(ListArgs),
    Unpack(UnpackArgs),
    Pack(PackArgs),
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
        Command::Pack(args) => pack(args),
    }
}
