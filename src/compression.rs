use std::fmt;

use crate::{Error, Result};

#[derive(Clone, Copy, Debug)]
pub enum CompressionVersion {
    V0,
    V4,
    V5,
}

impl TryFrom<u32> for CompressionVersion {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Self::V0),
            4 => Ok(Self::V4),
            5 => Ok(Self::V5),
            _ => Err(Error::UnsupportedCompressionVersion(value)),
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

impl CompressionScheme {
    pub fn from_scheme_id(
        compression_scheme_id: u8,
        compression_version: CompressionVersion,
    ) -> Result<Self> {
        match compression_version {
            CompressionVersion::V0 => match compression_scheme_id {
                0 => Ok(Self::None),
                _ => Err(Error::UnknownCompressionScheme {
                    compression_scheme_id,
                    compression_version,
                }),
            },
            CompressionVersion::V4 => match compression_scheme_id {
                0 => Ok(Self::None),
                1 => Ok(Self::LZO1x),
                2 => Ok(Self::Zlib),
                _ => Err(Error::UnknownCompressionScheme {
                    compression_scheme_id,
                    compression_version,
                }),
            },
            CompressionVersion::V5 => match compression_scheme_id {
                0 => Ok(Self::None),
                1 => Ok(Self::LZO1x),
                2 => Ok(Self::Zlib),
                3 => Ok(Self::XMemCompress),
                _ => Err(Error::UnknownCompressionScheme {
                    compression_scheme_id,
                    compression_version,
                }),
            },
        }
    }
}
