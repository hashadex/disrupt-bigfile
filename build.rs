use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

fn fnv1_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF29CE484222325; // Set hash to default seed

    for &byte in bytes {
        hash = hash.wrapping_mul(0x100000001B3);
        hash ^= byte as u64;
    }

    hash
}

fn build_filelist(
    infile_path: &Path,
    outfile: &mut impl Write,
    map_name: &str,
) -> Result<(), io::Error> {
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
            }
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

    writeln!(
        outfile,
        "pub static {map_name}: NameHashMap = {};",
        phf_map.build()
    )?;

    let collisions = colliding_hashes.len();
    println!(
        "built filelist {}, {collisions} collisions",
        infile_path.display()
    );

    Ok(())
}

fn build_filelists_for_game(game_name: &str, filelists: &[&str]) -> Result<(), String> {
    let filelist_paths = filelists.iter().map(|&path_str| {
        ["filelists", game_name, path_str]
            .iter()
            .collect::<PathBuf>()
    });

    let outfile_path: PathBuf = [
        env::var("OUT_DIR").expect("OUT_DIR should be set by cargo"),
        format!("{game_name}.rs"),
    ]
    .iter()
    .collect();

    let mut outfile = BufWriter::new(
        File::create(outfile_path)
            .map_err(|err| format!("could not create outfile for {game_name}: {err}"))?,
    );

    let mut archive_name_map = phf_codegen::Map::new();

    for filelist_path in filelist_paths {
        let filelist_stem = filelist_path
            .file_stem()
            .expect("all paths provided to this function should have a stem")
            .to_string_lossy();

        let map_name = filelist_stem.to_uppercase().to_string() + "_HASHES";

        build_filelist(&filelist_path, &mut outfile, &map_name).map_err(|err| {
            format!(
                "could not build {}: {err}; make sure you have cloned the repo with submodules",
                filelist_path.to_string_lossy()
            )
        })?;

        archive_name_map.entry(filelist_stem.to_string(), format!("&{map_name}"));
    }

    writeln!(
        outfile,
        "pub static ARCHIVE_NAME_MAP: phf::Map<&'static str, &'static NameHashMap> = {};",
        archive_name_map.build()
    )
    .map_err(|err| format!("failed to write archive name map: {err}"))?;

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
            "worlds/windy_city/windy_city_spanish.filelist",
        ],
    )?;

    Ok(())
}
