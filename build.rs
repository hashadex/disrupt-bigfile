use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::{env, fs};

use rkyv::rancor::Error;
use rkyv::{Archive, Serialize};

fn fnv1_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325; // Set hash to default seed

    for &byte in bytes {
        hash = hash.wrapping_mul(0x0100_0000_01B3);
        hash ^= u64::from(byte);
    }

    hash
}

const FILELIST_PATHS: [&str; 157] = [
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
    "wd1/patch1.filelist",
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
    "wd2/common.filelist",
    "wd2/dlc/dlc_ultra_textures/dlc_ultra_textures.filelist",
    "wd2/installpackage.filelist",
    "wd2/installpackage_brazilian.filelist",
    "wd2/installpackage_english.filelist",
    "wd2/installpackage_french.filelist",
    "wd2/installpackage_german.filelist",
    "wd2/installpackage_italian.filelist",
    "wd2/installpackage_japanese.filelist",
    "wd2/installpackage_mexican.filelist",
    "wd2/installpackage_russian.filelist",
    "wd2/installpackage_spanish.filelist",
    "wd2/patch.filelist",
    "wd2/patch2.filelist",
    "wd2/patch2_brazilian.filelist",
    "wd2/patch2_english.filelist",
    "wd2/patch2_french.filelist",
    "wd2/patch2_german.filelist",
    "wd2/patch2_italian.filelist",
    "wd2/patch2_japanese.filelist",
    "wd2/patch2_mexican.filelist",
    "wd2/patch2_russian.filelist",
    "wd2/patch2_spanish.filelist",
    "wd2/patch_brazilian.filelist",
    "wd2/patch_english.filelist",
    "wd2/patch_french.filelist",
    "wd2/patch_german.filelist",
    "wd2/patch_italian.filelist",
    "wd2/patch_japanese.filelist",
    "wd2/patch_mexican.filelist",
    "wd2/patch_russian.filelist",
    "wd2/patch_spanish.filelist",
    "wd2/shadersobj.filelist",
    "wd2/sound.filelist",
    "wd2/sound_brazilian.filelist",
    "wd2/sound_english.filelist",
    "wd2/sound_french.filelist",
    "wd2/sound_german.filelist",
    "wd2/sound_italian.filelist",
    "wd2/sound_japanese.filelist",
    "wd2/sound_mexican.filelist",
    "wd2/sound_russian.filelist",
    "wd2/sound_spanish.filelist",
    "wd2/videos.filelist",
    "wd2/worlds/san_francisco/san_francisco.filelist",
    "wd2/worlds/san_francisco/san_francisco_brazilian.filelist",
    "wd2/worlds/san_francisco/san_francisco_cache.filelist",
    "wd2/worlds/san_francisco/san_francisco_cache_patch.filelist",
    "wd2/worlds/san_francisco/san_francisco_english.filelist",
    "wd2/worlds/san_francisco/san_francisco_french.filelist",
    "wd2/worlds/san_francisco/san_francisco_german.filelist",
    "wd2/worlds/san_francisco/san_francisco_hires.filelist",
    "wd2/worlds/san_francisco/san_francisco_italian.filelist",
    "wd2/worlds/san_francisco/san_francisco_japanese.filelist",
    "wd2/worlds/san_francisco/san_francisco_mexican.filelist",
    "wd2/worlds/san_francisco/san_francisco_preload.filelist",
    "wd2/worlds/san_francisco/san_francisco_russian.filelist",
    "wd2/worlds/san_francisco/san_francisco_sound.filelist",
    "wd2/worlds/san_francisco/san_francisco_sound_brazilian.filelist",
    "wd2/worlds/san_francisco/san_francisco_sound_english.filelist",
    "wd2/worlds/san_francisco/san_francisco_sound_french.filelist",
    "wd2/worlds/san_francisco/san_francisco_sound_german.filelist",
    "wd2/worlds/san_francisco/san_francisco_sound_italian.filelist",
    "wd2/worlds/san_francisco/san_francisco_sound_japanese.filelist",
    "wd2/worlds/san_francisco/san_francisco_sound_mexican.filelist",
    "wd2/worlds/san_francisco/san_francisco_sound_russian.filelist",
    "wd2/worlds/san_francisco/san_francisco_sound_spanish.filelist",
    "wd2/worlds/san_francisco/san_francisco_spanish.filelist",
    "wdl/common.filelist",
    "wdl/commonengine.filelist",
    "wdl/patch.filelist",
    "wdl/patch_brazilian.filelist",
    "wdl/patch_english.filelist",
    "wdl/patch_french.filelist",
    "wdl/patch_german.filelist",
    "wdl/patch_italian.filelist",
    "wdl/patch_japanese.filelist",
    "wdl/patch_russian.filelist",
    "wdl/patch_spanish.filelist",
    "wdl/shadersobj.filelist",
    "wdl/sound.filelist",
    "wdl/sound_brazilian.filelist",
    "wdl/sound_english.filelist",
    "wdl/sound_french.filelist",
    "wdl/sound_german.filelist",
    "wdl/sound_italian.filelist",
    "wdl/sound_japanese.filelist",
    "wdl/sound_russian.filelist",
    "wdl/sound_spanish.filelist",
    "wdl/videos.filelist",
    "wdl/videos_ultra.filelist",
    "wdl/worlds/london/london.filelist",
    "wdl/worlds/london/london_brazilian.filelist",
    "wdl/worlds/london/london_cache.filelist",
    "wdl/worlds/london/london_english.filelist",
    "wdl/worlds/london/london_french.filelist",
    "wdl/worlds/london/london_german.filelist",
    "wdl/worlds/london/london_hires.filelist",
    "wdl/worlds/london/london_italian.filelist",
    "wdl/worlds/london/london_japanese.filelist",
    "wdl/worlds/london/london_preload.filelist",
    "wdl/worlds/london/london_russian.filelist",
    "wdl/worlds/london/london_sound.filelist",
    "wdl/worlds/london/london_sound_brazilian.filelist",
    "wdl/worlds/london/london_sound_english.filelist",
    "wdl/worlds/london/london_sound_french.filelist",
    "wdl/worlds/london/london_sound_german.filelist",
    "wdl/worlds/london/london_sound_italian.filelist",
    "wdl/worlds/london/london_sound_japanese.filelist",
    "wdl/worlds/london/london_sound_russian.filelist",
    "wdl/worlds/london/london_sound_spanish.filelist",
    "wdl/worlds/london/london_spanish.filelist",
    "wdl/worlds/london/london_ultra.filelist",
];

#[derive(Archive, Serialize)]
struct NameHashDb {
    entries: Vec<(u64, String)>,
}

fn main() -> Result<(), String> {
    println!("cargo::rerun-if-changed=filelists");

    let mut hash_filename_map = HashMap::new();
    let mut colliding_hashes = HashSet::new();

    for filelist_path in FILELIST_PATHS {
        let mut new_filenames_count = 0;

        let fat3_hash = filelist_path.starts_with("wd1");

        let filelist_path = Path::new("filelists").join(filelist_path);
        let filelist_file = BufReader::new(File::open(&filelist_path).map_err(|err| format!("failed to open filelist {}: {err}; make sure you have cloned the repo with submodules", filelist_path.display()))?);

        for filename in filelist_file.lines() {
            let filename = filename.map_err(|err| {
                format!(
                    "failed to read all filenames from {}: {err}",
                    filelist_path.display()
                )
            })?;

            if filename.starts_with(';') {
                continue;
            }

            let mut name_hash = fnv1_hash(filename.as_bytes());
            if fat3_hash {
                name_hash &= 0xFFFF_FFFF;
            } else {
                name_hash &= 0x1FFF_FFFF_FFFF_FFFF;
                name_hash |= 0xA000_0000_0000_0000;
            }

            if colliding_hashes.contains(&name_hash) {
                continue;
            }

            match hash_filename_map.entry(name_hash) {
                Entry::Occupied(entry) => {
                    let other_filename = entry.get();

                    if *other_filename != filename {
                        println!("collision: '{other_filename}' vs '{filename}'");

                        colliding_hashes.insert(name_hash);
                        entry.remove();
                    }
                }
                Entry::Vacant(entry) => {
                    entry.insert(filename);
                    new_filenames_count += 1;
                }
            }
        }

        println!(
            "read {new_filenames_count} new filenames from {}",
            filelist_path.display()
        );
    }

    println!(
        "read {} filenames, {} collisions. building db file...",
        hash_filename_map.len(),
        colliding_hashes.len(),
    );

    let mut entries: Vec<(u64, String)> = hash_filename_map
        .into_iter()
        .map(|(hash, filename)| (hash, filename.replace('\\', "/")))
        .collect();
    entries.sort_unstable_by_key(|entry| entry.0);

    let db = NameHashDb { entries };
    let db_bytes = rkyv::to_bytes::<Error>(&db)
        .map_err(|err| format!("failed to serialize name hash db: {err}"))?;

    let out_dir_path =
        env::var("OUT_DIR").map_err(|err| format!("failed to get env var 'OUT_DIR': {err}"))?;
    let archive_path: PathBuf = [&out_dir_path, "name_hash_db.rkyv"].iter().collect();

    fs::write(&archive_path, db_bytes)
        .map_err(|err| format!("failed to write db to file: {err}"))?;

    println!("all done!");

    Ok(())
}
