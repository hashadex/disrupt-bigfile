use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
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
    "filelists/wd1/common.filelist",
    "filelists/wd1/dlc/dlc_exclusive/dlc_exclusive.filelist",
    "filelists/wd1/dlc/dlc_exclusive/dlc_exclusive_brazilian.filelist",
    "filelists/wd1/dlc/dlc_exclusive/dlc_exclusive_english.filelist",
    "filelists/wd1/dlc/dlc_exclusive/dlc_exclusive_french.filelist",
    "filelists/wd1/dlc/dlc_exclusive/dlc_exclusive_german.filelist",
    "filelists/wd1/dlc/dlc_exclusive/dlc_exclusive_italian.filelist",
    "filelists/wd1/dlc/dlc_exclusive/dlc_exclusive_japanese.filelist",
    "filelists/wd1/dlc/dlc_exclusive/dlc_exclusive_russian.filelist",
    "filelists/wd1/dlc/dlc_exclusive/dlc_exclusive_spanish.filelist",
    "filelists/wd1/dlc/dlc_pill_people/dlc_pill_people.filelist",
    "filelists/wd1/dlc/dlc_solo/dlc_solo.filelist",
    "filelists/wd1/dlc/dlc_solo/dlc_solo_brazilian.filelist",
    "filelists/wd1/dlc/dlc_solo/dlc_solo_english.filelist",
    "filelists/wd1/dlc/dlc_solo/dlc_solo_french.filelist",
    "filelists/wd1/dlc/dlc_solo/dlc_solo_german.filelist",
    "filelists/wd1/dlc/dlc_solo/dlc_solo_italian.filelist",
    "filelists/wd1/dlc/dlc_solo/dlc_solo_japanese.filelist",
    "filelists/wd1/dlc/dlc_solo/dlc_solo_russian.filelist",
    "filelists/wd1/dlc/dlc_solo/dlc_solo_spanish.filelist",
    "filelists/wd1/patch.filelist",
    "filelists/wd1/patch1.filelist",
    "filelists/wd1/shaders.filelist",
    "filelists/wd1/shadersobj.filelist",
    "filelists/wd1/sound.filelist",
    "filelists/wd1/sound_brazilian.filelist",
    "filelists/wd1/sound_english.filelist",
    "filelists/wd1/sound_french.filelist",
    "filelists/wd1/sound_german.filelist",
    "filelists/wd1/sound_italian.filelist",
    "filelists/wd1/sound_japanese.filelist",
    "filelists/wd1/sound_russian.filelist",
    "filelists/wd1/sound_spanish.filelist",
    "filelists/wd1/videos.filelist",
    "filelists/wd1/worlds/windy_city/windy_city.filelist",
    "filelists/wd1/worlds/windy_city/windy_city_brazilian.filelist",
    "filelists/wd1/worlds/windy_city/windy_city_cache.filelist",
    "filelists/wd1/worlds/windy_city/windy_city_english.filelist",
    "filelists/wd1/worlds/windy_city/windy_city_french.filelist",
    "filelists/wd1/worlds/windy_city/windy_city_german.filelist",
    "filelists/wd1/worlds/windy_city/windy_city_italian.filelist",
    "filelists/wd1/worlds/windy_city/windy_city_japanese.filelist",
    "filelists/wd1/worlds/windy_city/windy_city_russian.filelist",
    "filelists/wd1/worlds/windy_city/windy_city_spanish.filelist",
    "filelists/wd2/common.filelist",
    "filelists/wd2/dlc/dlc_ultra_textures/dlc_ultra_textures.filelist",
    "filelists/wd2/installpackage.filelist",
    "filelists/wd2/installpackage_brazilian.filelist",
    "filelists/wd2/installpackage_english.filelist",
    "filelists/wd2/installpackage_french.filelist",
    "filelists/wd2/installpackage_german.filelist",
    "filelists/wd2/installpackage_italian.filelist",
    "filelists/wd2/installpackage_japanese.filelist",
    "filelists/wd2/installpackage_mexican.filelist",
    "filelists/wd2/installpackage_russian.filelist",
    "filelists/wd2/installpackage_spanish.filelist",
    "filelists/wd2/patch.filelist",
    "filelists/wd2/patch2.filelist",
    "filelists/wd2/patch2_brazilian.filelist",
    "filelists/wd2/patch2_english.filelist",
    "filelists/wd2/patch2_french.filelist",
    "filelists/wd2/patch2_german.filelist",
    "filelists/wd2/patch2_italian.filelist",
    "filelists/wd2/patch2_japanese.filelist",
    "filelists/wd2/patch2_mexican.filelist",
    "filelists/wd2/patch2_russian.filelist",
    "filelists/wd2/patch2_spanish.filelist",
    "filelists/wd2/patch_brazilian.filelist",
    "filelists/wd2/patch_english.filelist",
    "filelists/wd2/patch_french.filelist",
    "filelists/wd2/patch_german.filelist",
    "filelists/wd2/patch_italian.filelist",
    "filelists/wd2/patch_japanese.filelist",
    "filelists/wd2/patch_mexican.filelist",
    "filelists/wd2/patch_russian.filelist",
    "filelists/wd2/patch_spanish.filelist",
    "filelists/wd2/shadersobj.filelist",
    "filelists/wd2/sound.filelist",
    "filelists/wd2/sound_brazilian.filelist",
    "filelists/wd2/sound_english.filelist",
    "filelists/wd2/sound_french.filelist",
    "filelists/wd2/sound_german.filelist",
    "filelists/wd2/sound_italian.filelist",
    "filelists/wd2/sound_japanese.filelist",
    "filelists/wd2/sound_mexican.filelist",
    "filelists/wd2/sound_russian.filelist",
    "filelists/wd2/sound_spanish.filelist",
    "filelists/wd2/videos.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_brazilian.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_cache.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_cache_patch.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_english.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_french.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_german.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_hires.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_italian.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_japanese.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_mexican.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_preload.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_russian.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_sound.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_sound_brazilian.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_sound_english.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_sound_french.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_sound_german.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_sound_italian.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_sound_japanese.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_sound_mexican.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_sound_russian.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_sound_spanish.filelist",
    "filelists/wd2/worlds/san_francisco/san_francisco_spanish.filelist",
    "filelists/wdl/common.filelist",
    "filelists/wdl/commonengine.filelist",
    "filelists/wdl/patch.filelist",
    "filelists/wdl/patch_brazilian.filelist",
    "filelists/wdl/patch_english.filelist",
    "filelists/wdl/patch_french.filelist",
    "filelists/wdl/patch_german.filelist",
    "filelists/wdl/patch_italian.filelist",
    "filelists/wdl/patch_japanese.filelist",
    "filelists/wdl/patch_russian.filelist",
    "filelists/wdl/patch_spanish.filelist",
    "filelists/wdl/shadersobj.filelist",
    "filelists/wdl/sound.filelist",
    "filelists/wdl/sound_brazilian.filelist",
    "filelists/wdl/sound_english.filelist",
    "filelists/wdl/sound_french.filelist",
    "filelists/wdl/sound_german.filelist",
    "filelists/wdl/sound_italian.filelist",
    "filelists/wdl/sound_japanese.filelist",
    "filelists/wdl/sound_russian.filelist",
    "filelists/wdl/sound_spanish.filelist",
    "filelists/wdl/videos.filelist",
    "filelists/wdl/videos_ultra.filelist",
    "filelists/wdl/worlds/london/london.filelist",
    "filelists/wdl/worlds/london/london_brazilian.filelist",
    "filelists/wdl/worlds/london/london_cache.filelist",
    "filelists/wdl/worlds/london/london_english.filelist",
    "filelists/wdl/worlds/london/london_french.filelist",
    "filelists/wdl/worlds/london/london_german.filelist",
    "filelists/wdl/worlds/london/london_hires.filelist",
    "filelists/wdl/worlds/london/london_italian.filelist",
    "filelists/wdl/worlds/london/london_japanese.filelist",
    "filelists/wdl/worlds/london/london_preload.filelist",
    "filelists/wdl/worlds/london/london_russian.filelist",
    "filelists/wdl/worlds/london/london_sound.filelist",
    "filelists/wdl/worlds/london/london_sound_brazilian.filelist",
    "filelists/wdl/worlds/london/london_sound_english.filelist",
    "filelists/wdl/worlds/london/london_sound_french.filelist",
    "filelists/wdl/worlds/london/london_sound_german.filelist",
    "filelists/wdl/worlds/london/london_sound_italian.filelist",
    "filelists/wdl/worlds/london/london_sound_japanese.filelist",
    "filelists/wdl/worlds/london/london_sound_russian.filelist",
    "filelists/wdl/worlds/london/london_sound_spanish.filelist",
    "filelists/wdl/worlds/london/london_spanish.filelist",
    "filelists/wdl/worlds/london/london_ultra.filelist",
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

        let filelist_file = BufReader::new(File::open(&filelist_path).map_err(|err| {
            format!(
                "failed to open filelist {filelist_path}: {err}; make sure you have cloned the repo with submodules"
            )
        })?);

        let filenames = filelist_file
            .lines()
            .collect::<io::Result<Vec<_>>>()
            .map_err(|err| format!("failed to read all filenames from {filelist_path}: {err}"))?
            .into_iter()
            .filter(|filename| !filename.starts_with(';'));

        for filename in filenames {
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

        println!("read {new_filenames_count} new filenames from {filelist_path}");
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
