pub type HashSourceMap = phf::Map<u64, &'static str>;

#[allow(clippy::unreadable_literal)]
#[rust_analyzer::skip]
pub static HASH_SOURCE_MAP: HashSourceMap =
    include!(concat!(env!("OUT_DIR"), "/hash_source_map.rs"));
