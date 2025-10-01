use crate::{Error, Result};

#[derive(Debug)]
pub enum CompressionScheme {
    None,
    LZO1x,
    Zlib,
    XMemCompress
}

impl CompressionScheme {
    pub fn from_scheme_id(compression_scheme_id: u8, compression_version: u8) -> Result<CompressionScheme> {
        match compression_version {
            0 => match compression_scheme_id {
                0 => Ok(CompressionScheme::None),
                _ => Err(Error::UnknownCompressionScheme { compression_scheme_id, compression_version })
            },
            4 => match compression_scheme_id {
                0 => Ok(CompressionScheme::None),
                1 => Ok(CompressionScheme::LZO1x),
                2 => Ok(CompressionScheme::Zlib),
                _ => Err(Error::UnknownCompressionScheme { compression_scheme_id, compression_version })
            },
            5 => match compression_scheme_id {
                0 => Ok(CompressionScheme::None),
                1 => Ok(CompressionScheme::LZO1x),
                2 => Ok(CompressionScheme::Zlib),
                3 => Ok(CompressionScheme::XMemCompress),
                _ => Err(Error::UnknownCompressionScheme { compression_scheme_id, compression_version })
            },
            _ => Err(Error::UnsupportedCompressionVersion(compression_version))
        }
    }
}