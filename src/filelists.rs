pub type HashSourceMap = phf::Map<u64, &'static str>;

pub static HASH_SOURCE_MAP: HashSourceMap = include!(concat!(env!("OUT_DIR"), "/hash_source_map.rs"));
