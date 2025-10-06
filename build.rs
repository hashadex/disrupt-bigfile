use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::Path;

fn fnv1_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF29CE484222325; // Set hash to default seed

    for &byte in bytes {
        hash = hash.wrapping_mul(0x100000001B3);
        hash ^= byte as u64;
    }

    hash
}

fn build_filelist(infile_path: &Path, outfile: &mut impl Write) -> Result<(), io::Error> {
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

    let infile_stem = infile_path.file_stem()
        .unwrap(); // It's safe to unwrap at this point in the function
    let map_name = format!("{}_HASHES", infile_stem.to_ascii_uppercase().display());

    writeln!(
        outfile,
        "pub static {map_name}: phf::Map<u32, &'static str> = {};",
        phf_map.build()
    )?;

    let collisions = colliding_hashes.len();
    println!("built filelist {}, {collisions} collisions", infile_path.display());

    Ok(())
}

fn build_filelists_for_game(game_name: &str, filelists: &[&str]) -> Result<(), String> {
    let outdir_path = env::var("OUT_DIR").expect("OUT_DIR should be set by cargo");
    let outfile_path = Path::new(&outdir_path).join(game_name).with_extension("rs");

    let outfile = File::create(outfile_path)
        .map_err(|err| format!("could not create outfile for {game_name}: {err}"))?;
    let mut outfile_writer = BufWriter::new(outfile);

    let filelists_src_dir_path = Path::new("filelists").join(game_name);

    for &filelist_path_str in filelists {
        let filelist_path = filelists_src_dir_path.join(filelist_path_str);

        build_filelist(&filelist_path, &mut outfile_writer)
            .map_err(|err| format!(
                "could not build filelist {}: {err}; make sure you cloned the repo with submodules",
                filelist_path.display()
            ))?;
    }

    Ok(())
}

fn main() -> Result<(), String> {
    println!("cargo::rerun-if-changed=filelists");

    // Watch Dogs 1
    build_filelists_for_game(
        "wd1",
        &[
            "common.filelist",
            "dlc/dlc_exclusive/dlc_exclusive.filelist",
            "dlc/dlc_exclusive/dlc_exclusive_brazilian.filelist",
            "dlc/dlc_exclusive/dlc_exclusive_english.filelist",
            "dlc/dlc_exclusive/dlc_exclusive_french.filelist",
            "dlc/dlc_exclusive/dlc_exclusive_german.filelist",
            "dlc/dlc_exclusive/dlc_exclusive_italian.filelist",
            "dlc/dlc_exclusive/dlc_exclusive_japanese.filelist",
            "dlc/dlc_exclusive/dlc_exclusive_russian.filelist",
            "dlc/dlc_exclusive/dlc_exclusive_spanish.filelist",
            "dlc/dlc_pill_people/dlc_pill_people.filelist",
            "dlc/dlc_solo/dlc_solo.filelist",
            "dlc/dlc_solo/dlc_solo_brazilian.filelist",
            "dlc/dlc_solo/dlc_solo_english.filelist",
            "dlc/dlc_solo/dlc_solo_french.filelist",
            "dlc/dlc_solo/dlc_solo_german.filelist",
            "dlc/dlc_solo/dlc_solo_italian.filelist",
            "dlc/dlc_solo/dlc_solo_japanese.filelist",
            "dlc/dlc_solo/dlc_solo_russian.filelist",
            "dlc/dlc_solo/dlc_solo_spanish.filelist",
            "patch.filelist",
            "patch1.filelist",
            "shaders.filelist",
            "shadersobj.filelist",
            "sound.filelist",
            "sound_brazilian.filelist",
            "sound_english.filelist",
            "sound_french.filelist",
            "sound_german.filelist",
            "sound_italian.filelist",
            "sound_japanese.filelist",
            "sound_russian.filelist",
            "sound_spanish.filelist",
            "videos.filelist",
            "worlds/windy_city/windy_city.filelist",
            "worlds/windy_city/windy_city_brazilian.filelist",
            "worlds/windy_city/windy_city_cache.filelist",
            "worlds/windy_city/windy_city_english.filelist",
            "worlds/windy_city/windy_city_french.filelist",
            "worlds/windy_city/windy_city_german.filelist",
            "worlds/windy_city/windy_city_italian.filelist",
            "worlds/windy_city/windy_city_japanese.filelist",
            "worlds/windy_city/windy_city_russian.filelist",
            "worlds/windy_city/windy_city_spanish.filelist"
        ]
    )?;

    Ok(())
}