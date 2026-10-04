use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::num::ParseIntError;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, ensure};
use clap::builder::NonEmptyStringValueParser;
use clap::{Args, Parser, Subcommand, ValueEnum};
use disrupt_bigfile::header::{
    CompressionVersion, Dependency, FatHeader, FatVersion, NameHashVersion, Platform, TableVersion,
};
use disrupt_bigfile::{ArchiveBuilder, Dat, Fat};
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

/// Show the metadata for one or more FAT files.
///
/// Print the FAT version, table version, platform, compression version, name hash version for the
/// specified FAT files to stdout. The archive hash and dependencies will also be printed for FAT5
/// archives.
#[derive(Args, Debug)]
struct InfoArgs {
    /// Path to the FAT file.
    #[arg(required = true, value_name = "FAT_PATH")]
    fat_paths: Vec<PathBuf>,

    /// Print the information in a compact format, one line per FAT file.
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
        }?;
    }

    Ok(())
}

/// Show the entries of one or more FAT files.
///
/// Print the filename (or the name hash, if the filename could not be resolved) of each entry of
/// each given FAT file to stdout.
///
/// If multiple FAT files are specified, the entries will be printed out in blocks, similar to how
/// the `ls` command on Linux groups output.
#[derive(Args, Debug)]
struct ListArgs {
    /// Path to the FAT file.
    #[arg(required = true, value_name = "FAT_PATH")]
    fat_paths: Vec<PathBuf>,

    /// Also print each entry's compressed and uncompressed sizes, compression scheme and offset.
    #[arg(short, long)]
    verbose: bool,
}

fn list(args: &ListArgs) -> anyhow::Result<()> {
    let mut lock = io::stdout().lock();
    let multiple_paths = args.fat_paths.len() > 1;

    let mut fat_paths = args.fat_paths.iter().peekable();
    while let Some(fat_path) = fat_paths.next() {
        let fat = open_fat(fat_path)?;

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
    /// Path to the FAT file.
    #[arg(short, long = "fat")]
    fat_path: Option<PathBuf>,

    /// Path to the DAT file.
    #[arg(short, long = "dat")]
    dat_path: Option<PathBuf>,
}

/// Extract all files from a BigFile archive to a directory.
///
/// Decompress and copy every file stored in the given BigFile archive (FAT+DAT file pair) to the
/// given destination.
///
/// It is not necessary to specify both the --fat and --dat flags. If only one file is specified,
/// the program will try to find the other one in the same directory.
///
/// For compatibility with Gibbed.Disrupt, any files for which the filename could not be resolved
/// due to them having an unknown name hash will be placed into the special "__UNKNOWN" directory.
/// However, unlike Gibbed.Disrupt, this program will not extract files with a duplicate name hash
/// separately. If multiple entries represent some file in the FAT, only the last occurrence of
/// that file will actually be extracted, and all other duplicates of that file will be ignored.
#[derive(Args, Debug)]
struct UnpackArgs {
    #[command(flatten)]
    paths: ArchivePaths,

    /// Path to the destination directory.
    ///
    /// This directory will be automatically created if it does not exist.
    ///
    /// If this flag is missing, the program will automatically create a subdirectory in the
    /// current working directory and extract the files there.
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
        result.with_context(|| format!("failed to unpack '{entry}'"))?;
    }

    Ok(())
}

#[derive(Clone, Debug, ValueEnum)]
enum HeaderPreset {
    /// All archives except for "sound*" in the Windows release of WD1.
    Wd1Win64,

    /// "sound*" archives in the Windows release of WD1.
    Wd1Win64Sound,

    /// All archives except for "sound*" in the Wii U release of WD1.
    Wd1WiiU,

    /// "sound*" archives in the Wii U release of WD1.
    Wd1WiiUSound,

    /// All archives except for "sound*" in the Windows release of WD2.
    Wd2Win64,

    /// All archives except for "sound*" in the PS4 release of WD2.
    Wd2Ps4,

    /// "sound*" archives in WD2.
    Wd2Sound,

    /// All archives except for "london" and "london_cache" in the Windows release of WDL.
    WdlWin64,

    /// "worlds/london/london" archive in the Windows release of WDL.
    WdlWin64London,

    /// "worlds/london/london_cache" archive in the Windows release of WDL.
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

/// Create a new BigFile archive from a directory.
///
/// Recursively add all files in a given input directory to a new BigFile archive
/// (FAT+DAT file pair). The new files will be created with the given name and placed into the
/// specified destination directory.
///
/// You can use the provided FAT metadata presets to easily modify archives for most releases of
/// the games, and use flags like `--platform`, `--table-version`, etc. to override the fields from
/// the preset.
///
/// For compatibility with Gibbed.Disrupt, the files in the "__UNKNOWN" and "__DUPLICATE"
/// subdirectories will be processed in a special way. Files in the "__UNKNOWN" directory will have
/// their name hashes read verbatim as a hexadecimal number from their filenames. Files in the
/// "__DUPLICATE" directory will have the "__DUPLICATE_<number>" suffix removed from their file
/// stems, although the number in the prefix will not be respected -- "__DUPLICATE" files may
/// appear in any order in the created FAT file. That way any mod that was meant to be installed
/// with Gibbed.Disrupt can be installed with disrupt-bigfile, and vice versa.
#[derive(Args, Debug)]
struct PackArgs {
    /// Path to the directory to be packed.
    input_dir: PathBuf,

    /// Path to the directory where the FAT and DAT files will be created.
    ///
    /// This directory will be created if it does not exist.
    #[arg(short, long = "output", default_value = ".")]
    output_dir: PathBuf,

    /// Name of the created FAT and DAT files.
    #[arg(short = 'N', long = "name", value_parser = NonEmptyStringValueParser::new())]
    archive_name: Option<String>,

    /// Preset for the FAT metadata fields.
    ///
    /// Fields from the preset will be overridden by flags like `--platform`, `--table-version` and
    /// other.
    #[arg(short = 'P', long, default_value = "wd1-win64")]
    preset: HeaderPreset,

    /// Version/type of the FAT file.
    #[arg(short, long)]
    fat_version: Option<FatVersion>,

    /// Version of the binary format of the FAT file's body.
    #[arg(short, long)]
    table_version: Option<TableVersion>,

    /// Target platform of the archive.
    #[arg(short, long)]
    platform: Option<Platform>,

    /// Compression version.
    ///
    /// Affects which compression schemes can be used in the archive.
    #[arg(short, long, verbatim_doc_comment)]
    compression_version: Option<CompressionVersion>,

    /// Name hash version.
    #[arg(short, long)]
    name_hash_version: Option<NameHashVersion>,

    /// Archive hash.
    ///
    /// Specified as a 64-bit hexadecimal number, for example "--archive-hash A7E2977F3F32B98E".
    ///
    /// Supported only on FAT5 and will be ignored if used when creating a FAT3 archive.
    #[arg(short, long, value_parser = hex_u64_parser)]
    archive_hash: Option<u64>,

    /// Add a dependency entry to the metadata.
    ///
    /// A dependency is specified as two hexadecimal numbers, the dependency archive hash and the
    /// name hash, separated by a colon; for example
    /// "--dependency B78228C0B350CC14:BE38E2B5954E5FA4".
    ///
    /// Supported only on FAT5 and will be ignored if used when creating a FAT3 archive.
    ///
    /// This flag can be used multiple times to add multiple dependencies.
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
        }
    }
    .context("invalid FAT header configuration")?;

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

/// Extract, create and inspect BigFile archives used by Disrupt, Ubisoft's game engine for the
/// Watch Dogs games.
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
        Command::List(args) => list(&args).or_else(suppress_broken_pipe),
        Command::Unpack(args) => unpack(args),
        Command::Pack(args) => pack(args),
    }
}
