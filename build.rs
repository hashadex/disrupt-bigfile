use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

fn filelist_paths_from_dir(dir_path: &Path, path_vec: &mut Vec<PathBuf>) -> Result<(), io::Error> {
    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            filelist_paths_from_dir(&path, path_vec)?;
        } else if path.extension().is_some_and(|ext| ext == "filelist") {
            path_vec.push(path);
        }
    }
    
    Ok(())
}

fn hash_fnv1a64(data: &str) -> u64 {
    // Set hash to initial seed
    let mut hash: u64 = 0xCBF29CE484222325;

    for byte in data.as_bytes() {
        hash = hash.wrapping_mul(0x100000001B3);
        hash ^= *byte as u64;
    }

    hash
}

fn build_filelists(game_name: &str) -> Result<(), io::Error> {
    let filelist_dir_path: PathBuf = ["filelists", game_name].iter().collect();
    let mut filelist_paths = Vec::new();
    filelist_paths_from_dir(&filelist_dir_path, &mut filelist_paths)?;

    if filelist_paths.is_empty() {
        return Err(
            io::Error::new(
                io::ErrorKind::NotFound,
                "'{filelist_dir_path}' is empty. make sure you cloned the repo with submodules"
            )
        )
    }

    let mut name_hash_map = HashMap::new();
    let mut name_hash_banlist = Vec::new();

    for filelist_path in filelist_paths {
        let file = File::open(filelist_path)?;
        let buf = BufReader::new(file);

        for line in buf.lines().map_while(Result::ok) {
            if line.starts_with(";") {
                continue;
            }

            let mut name_hash = hash_fnv1a64(&line.to_lowercase()) as u32;
            if name_hash & 0xFFFF0000 == 0xFFFF0000 {
                name_hash &= !(1 << 16);
            }

            if name_hash_banlist.contains(&name_hash) {
                continue;
            }

            if name_hash_map.contains_key(&name_hash) {
                name_hash_map.remove(&name_hash);
                name_hash_banlist.push(name_hash);
                continue;
            }

            name_hash_map.insert(name_hash, line.replace("\\", "/"));
        }
    }

    let mut phf_map = phf_codegen::Map::new();
    for (hash, name) in name_hash_map {
        phf_map.entry(hash, format!("\"{name}\""));
    }

    let out_dir_path = env::var("OUT_DIR").expect("OUT_DIR should be set by cargo during build");
    let outfile_name = format!("{game_name}_map.rs");
    let outfile_path = Path::new(&out_dir_path).join(outfile_name);
    let mut outfile = BufWriter::new(File::create(outfile_path)?);

    write!(
        &mut outfile,
        "static {}_MAP: phf::Map<u32, &'static str> = {};",
        game_name.to_uppercase(),
        phf_map.build()
    )?;

    let collisions = name_hash_banlist.len();
    println!("built filelists for '{game_name}', {collisions} collisions");
    
    Ok(())
}

fn main() -> Result<(), io::Error> {
    println!("cargo::rerun-if-changed=filelists");
    build_filelists("wd1")?;

    Ok(())
}