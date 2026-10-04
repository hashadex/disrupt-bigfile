use std::sync::LazyLock;

use rkyv::Archive;
use rkyv::util::Align;

#[expect(
    dead_code,
    reason = "this struct is needed to generate the archived variant of it for rkyv"
)]
#[derive(Archive)]
struct NameHashDb {
    entries: Vec<(u64, String)>,
}

macro_rules! include_archive_bytes {
    () => {
        include_bytes!(concat!(env!("OUT_DIR"), "/name_hash_db.rkyv"))
    };
}
static ALIGNED_ARCHIVE_BYTES: Align<[u8; include_archive_bytes!().len()]> =
    Align(*include_archive_bytes!());

static NAME_HASH_DB: LazyLock<&ArchivedNameHashDb> = LazyLock::new(|| {
    // SAFETY: build.rs guarantees that name_hash_db.rkyv contains valid data.
    // rkyv::util::Align guarantees that the buffer is properly aligned.
    unsafe { rkyv::access_unchecked(&ALIGNED_ARCHIVE_BYTES.0) }
});

pub fn get(hash: u64) -> Option<&'static str> {
    let idx = NAME_HASH_DB
        .entries
        .binary_search_by_key(&hash.into(), |entry| entry.0)
        .ok()?;

    Some(NAME_HASH_DB.entries[idx].1.as_str())
}
