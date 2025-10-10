use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

fn fnv1_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF29CE484222325; // Set hash to default seed

    for &byte in bytes {
        hash = hash.wrapping_mul(0x100000001B3);
        hash ^= byte as u64;
    }

    hash
}

fn build_filelists(filelist_path_strs: &[&str]) -> Result<(), String> {
    let filelist_paths = filelist_path_strs
        .iter()
        .map(|s| Path::new("filelists").join(s));

    let mut name_hash_map = HashMap::new();
    let mut colliding_hashes = Vec::new();

    for filelist_path in filelist_paths {
        let file = BufReader::new(File::open(&filelist_path).map_err(|err| {
            format!(
                "can't open filelist {}: {err}; make sure you have cloned the repo with submodules",
                filelist_path.display()
            )
        })?);

        let mut new_entries_count = 0;

        for line in file.lines().map_while(Result::ok) {
            if line.starts_with(";") {
                continue;
            }

            let hash32 = fnv1_hash(line.to_lowercase().as_bytes()) as u32;

            if colliding_hashes.contains(&hash32) {
                continue;
            }

            match name_hash_map.entry(hash32) {
                Entry::Vacant(e) => {
                    e.insert(line);
                    new_entries_count += 1;
                }
                Entry::Occupied(e) => {
                    let other_source = e.get();
                    if line != *other_source {
                        println!("collision: {line} vs {other_source}");
                        e.remove();
                        colliding_hashes.push(hash32);
                    }
                }
            };
        }

        println!(
            "read {new_entries_count} new entries from {}",
            filelist_path.display()
        )
    }

    println!(
        "read {} entries total; {} collisions. building phf map...",
        name_hash_map.keys().len(),
        colliding_hashes.len()
    );

    let mut phf_name_hash_map = phf_codegen::Map::new();
    for (hash, source) in name_hash_map {
        let escaped_source = source.replace("\\", "\\\\");
        phf_name_hash_map.entry(hash, format!("\"{escaped_source}\""));
    }

    let outfile_path = Path::new(&env::var("OUT_DIR").expect("OUT_DIR should be set by cargo"))
        .join("name_hash_map.rs");
    let mut outfile = BufWriter::new(
        File::create(outfile_path).map_err(|err| format!("failed to create outfile: {err}"))?,
    );

    write!(outfile, "{}", phf_name_hash_map.build())
        .map_err(|err| format!("failed to write to outfile: {err}"))?;

    Ok(())
}

fn main() -> Result<(), String> {
    println!("cargo::rerun-if-changed=filelists");

    build_filelists(&[
        "wd1/common.filelist",
        "wd1/dlc/dlc_exclusive/dlc_exclusive.filelist",
        "wd1/dlc/dlc_exclusive/dlc_exclusive_brazilian.filelist",
        "wd1/dlc/dlc_exclusive/dlc_exclusive_english.filelist",
        "wd1/dlc/dlc_exclusive/dlc_exclusive_french.filelist",
        "wd1/dlc/dlc_exclusive/dlc_exclusive_german.filelist",
        "wd1/dlc/dlc_exclusive/dlc_exclusive_italian.filelist",
        "wd1/dlc/dlc_exclusive/dlc_exclusive_japanese.filelist",
        "wd1/dlc/dlc_exclusive/dlc_exclusive_russian.filelist",
        "wd1/dlc/dlc_exclusive/dlc_exclusive_spanish.filelist",
        "wd1/dlc/dlc_pill_people/dlc_pill_people.filelist",
        "wd1/dlc/dlc_solo/dlc_solo.filelist",
        "wd1/dlc/dlc_solo/dlc_solo_brazilian.filelist",
        "wd1/dlc/dlc_solo/dlc_solo_english.filelist",
        "wd1/dlc/dlc_solo/dlc_solo_french.filelist",
        "wd1/dlc/dlc_solo/dlc_solo_german.filelist",
        "wd1/dlc/dlc_solo/dlc_solo_italian.filelist",
        "wd1/dlc/dlc_solo/dlc_solo_japanese.filelist",
        "wd1/dlc/dlc_solo/dlc_solo_russian.filelist",
        "wd1/dlc/dlc_solo/dlc_solo_spanish.filelist",
        "wd1/patch.filelist",
        "wd1/shaders.filelist",
        "wd1/shadersobj.filelist",
        "wd1/sound.filelist",
        "wd1/sound_brazilian.filelist",
        "wd1/sound_english.filelist",
        "wd1/sound_french.filelist",
        "wd1/sound_german.filelist",
        "wd1/sound_italian.filelist",
        "wd1/sound_japanese.filelist",
        "wd1/sound_russian.filelist",
        "wd1/sound_spanish.filelist",
        "wd1/videos.filelist",
        "wd1/worlds/windy_city/windy_city.filelist",
        "wd1/worlds/windy_city/windy_city_brazilian.filelist",
        "wd1/worlds/windy_city/windy_city_cache.filelist",
        "wd1/worlds/windy_city/windy_city_english.filelist",
        "wd1/worlds/windy_city/windy_city_french.filelist",
        "wd1/worlds/windy_city/windy_city_german.filelist",
        "wd1/worlds/windy_city/windy_city_italian.filelist",
        "wd1/worlds/windy_city/windy_city_japanese.filelist",
        "wd1/worlds/windy_city/windy_city_russian.filelist",
        "wd1/worlds/windy_city/windy_city_spanish.filelist",
        "wd1/patch1.filelist",
    ])?;

    Ok(())
}
