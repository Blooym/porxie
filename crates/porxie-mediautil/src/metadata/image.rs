use image::ImageReader;
use std::io::Cursor;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImageMetadataError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
}

pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
}

impl ImageMetadata {
    /// Calculate image metadata from raw bytes.
    ///
    /// ## Safety
    /// Only a minimal chunk of the raw byte buffer is used.
    /// The bytes are not decoded, only the data needed for metadata is read.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ImageMetadataError> {
        const CHUNK_SIZE: usize = 65536;
        let (width, height) = ImageReader::new(Cursor::new(&bytes[..CHUNK_SIZE.min(bytes.len())]))
            .with_guessed_format()?
            .into_dimensions()?;

        Ok(Self { width, height })
    }
}
