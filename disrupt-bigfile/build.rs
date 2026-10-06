use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::path::PathBuf;

use anyhow::Context;
use fnv1::Fnv1BuildHasher;
use rkyv::rancor::Error;
use rkyv::{Archive, Serialize};

#[allow(
    dead_code,
    reason = "function may become unused if all filelist features are disabled"
)]
fn read_filelists(
    filelists: &[&'static str],
    fat3_hash: bool,
) -> impl Iterator<Item = (u64, &'static str)> {
    let builder = Fnv1BuildHasher::new();

    filelists
        .iter()
        .flat_map(|filelist| filelist.lines())
        .filter(|filename| !filename.starts_with(';'))
        .map(move |filename| {
            let mut hasher = builder.build_hasher();
            hasher.write(filename.as_bytes());
            let mut hash = hasher.finish();

            if fat3_hash {
                hash &= 0xFFFF_FFFF;
            } else {
                hash &= 0x1FFF_FFFF_FFFF_FFFF;
                hash |= 0xA000_0000_0000_0000;
            }

            (hash, filename)
        })
}

#[derive(Archive, Serialize)]
struct NameHashDb {
    entries: Vec<(u64, String)>,
}

fn main() -> anyhow::Result<()> {
    println!("cargo::rerun-if-changed=build.rs");

    let filelists_wd1 = cfg_select! {
        feature = "wd1" => read_filelists(&disrupt_bigfile_filelists_wd1::FILELISTS, true),
        _ => std::iter::empty(),
    };
    let filelists_wd2 = cfg_select! {
        feature = "wd2" => read_filelists(&disrupt_bigfile_filelists_wd2::FILELISTS, false),
        _ => std::iter::empty(),
    };
    let filelists_wdl = cfg_select! {
        feature = "wdl" => read_filelists(&disrupt_bigfile_filelists_wdl::FILELISTS, false),
        _ => std::iter::empty(),
    };

    let filenames = filelists_wd1.chain(filelists_wd2).chain(filelists_wdl);

    let mut hash_filename_map = HashMap::new();
    let mut colliding_hashes = HashSet::new();

    for (name_hash, filename) in filenames {
        if colliding_hashes.contains(&name_hash) {
            continue;
        }

        match hash_filename_map.entry(name_hash) {
            Entry::Vacant(e) => {
                e.insert(filename);
            }
            Entry::Occupied(e) => {
                let other_filename = e.get();

                if filename != *other_filename {
                    println!("collision: '{filename}' vs '{other_filename}");

                    e.remove();
                    colliding_hashes.insert(name_hash);
                }
            }
        };
    }

    println!(
        "read {} filenames, {} collisions. building db file...",
        hash_filename_map.len(),
        colliding_hashes.len()
    );

    let mut entries: Vec<(u64, String)> = hash_filename_map
        .into_iter()
        .map(|(hash, filename)| (hash, filename.replace('\\', "/")))
        .collect();
    entries.sort_unstable_by_key(|entry| entry.0);

    let db = NameHashDb { entries };
    let db_bytes = rkyv::to_bytes::<Error>(&db).context("failed to serialize name hash db")?;

    let db_file_path: PathBuf = [
        &env::var("OUT_DIR").expect("OUT_DIR should be set by cargo"),
        "name_hash_db.rkyv",
    ]
    .iter()
    .collect();

    fs::write(db_file_path, db_bytes).context("failed to write to db file")
}
