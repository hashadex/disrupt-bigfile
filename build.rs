use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

fn recurse_get_filelist_paths(dir: &Path) -> Result<Vec<PathBuf>, io::Error> {
    let mut filelist_paths = Vec::new();

    for entry in fs::read_dir(dir)?.map_while(Result::ok) {
        let path = entry.path();

        if entry.metadata()?.is_dir() {
            let mut child_dir_paths = recurse_get_filelist_paths(&path)?;
            filelist_paths.append(&mut child_dir_paths);
        } else if path.extension().is_some_and(|ext| ext == "filelist") {
            filelist_paths.push(path);
        }
    }

    Ok(filelist_paths)
}

fn fnv1_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF29CE484222325; // Set hash to default seed

    for &byte in bytes {
        hash = hash.wrapping_mul(0x100000001B3);
        hash ^= byte as u64;
    }

    hash
}

fn build_filelist(infile_path: &Path) -> Result<(), io::Error> {
    let infile = BufReader::new(File::open(infile_path)?);

    let mut colliding_hashes = Vec::new();
    let mut hashes = HashMap::new();

    for name in infile.lines().map_while(Result::ok) {
        if name.starts_with(";") {
            continue;
        }

        let name_hash = fnv1_hash(name.to_lowercase().as_bytes()) as u32;

        if colliding_hashes.contains(&name_hash) {
            continue;
        }
        
        match hashes.entry(name_hash) {
            Entry::Occupied(e) => {
                e.remove_entry();
                colliding_hashes.push(name_hash);
            },
            Entry::Vacant(e) => {
                e.insert(name);
            }
        }
    }

    let mut phf_map = phf_codegen::Map::new();
    for (name_hash, name) in hashes {
        let escaped_name = name.replace("\\", "\\\\");
        phf_map.entry(name_hash, format!("\"{escaped_name}\""));
    }

    let outdir_env = env::var("OUT_DIR").expect("OUT_DIR should be set by cargo during build");
    let outdir_path = Path::new(&outdir_env);

    let outfile_stem = infile_path.file_stem().unwrap();
    let outfile_name = format!("{}.rs", outfile_stem.display());

    let outfile_path = outdir_path.join(outfile_name);

    let mut outfile = BufWriter::new(File::create(outfile_path)?);
    write!(
        &mut outfile,
        "pub static {}_HASHES: phf::Map<u32, &'static str> = {};",
        outfile_stem.to_ascii_uppercase().display(),
        phf_map.build()
    )?;

    let collisions = colliding_hashes.len();
    println!("built filelist {}, {collisions} collisions", infile_path.display());

    Ok(())
}

fn build_filelists_in_dir(dir_path: &Path) -> Result<(), io::Error> {
    let filelist_paths = recurse_get_filelist_paths(dir_path)?;

    for filelist_path in filelist_paths {
        build_filelist(&filelist_path)?;
    }

    Ok(())
}

fn main() -> Result<(), io::Error> {
    println!("cargo::rerun-if-changed=filelists");
    
    build_filelists_in_dir(Path::new("filelists/wd1"))?;

    Ok(())
}