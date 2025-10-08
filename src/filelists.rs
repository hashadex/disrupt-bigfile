pub type NameHashMap = phf::Map<u32, &'static str>;

pub mod wd1 {
    use super::NameHashMap;

    include!(concat!(env!("OUT_DIR"), "/wd1.rs"));
}