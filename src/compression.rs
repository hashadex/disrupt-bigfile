use std::fmt;

use crate::{FatError, FatResult};

#[derive(Clone, Copy, Debug)]
pub enum CompressionVersion {
    V0,
    V4,
    V5,
}

impl TryFrom<u32> for CompressionVersion {
    type Error = FatError;

    fn try_from(value: u32) -> FatResult<Self> {
        match value {
            0 => Ok(Self::V0),
            4 => Ok(Self::V4),
            5 => Ok(Self::V5),
            _ => Err(FatError::UnsupportedCompressionVersion(value)),
        }
    }
}

impl fmt::Display for CompressionVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::V0 => write!(f, "V0"),
            Self::V4 => write!(f, "V4"),
            Self::V5 => write!(f, "V5"),
        }
    }
}

#[derive(Debug)]
pub enum CompressionScheme {
    None,
    LZO1x,
    Zlib,
    XMemCompress,
}

impl fmt::Display for CompressionScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "no compression"),
            Self::LZO1x => write!(f, "LZO1x"),
            Self::Zlib => write!(f, "Zlib"),
            Self::XMemCompress => write!(f, "XMemCompress"),
        }
    }
}

impl CompressionScheme {
    pub fn from_scheme_id(
        compression_scheme_id: u8,
        compression_version: CompressionVersion,
    ) -> FatResult<Self> {
        match (compression_version, compression_scheme_id) {
            (_, 0) => Ok(Self::None),
            (CompressionVersion::V4 | CompressionVersion::V5, 1) => Ok(Self::LZO1x),
            (CompressionVersion::V4 | CompressionVersion::V5, 2) => Ok(Self::Zlib),
            (CompressionVersion::V5, 3) => Ok(Self::XMemCompress),
            _ => Err(FatError::UnsupportedCompressionScheme {
                compression_scheme_id,
                compression_version,
            }),
        }
    }
}
