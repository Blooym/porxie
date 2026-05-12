use std::io::Cursor;

use image::ImageReader;

pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
}

impl ImageMetadata {
    /// Calculate image metadata from a raw byte array.
    ///
    /// ## Safety
    /// Only a minimal chunk of the raw byte buffer is used.
    /// The bytes are not decoded, only the data needed for metadata is read.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, image::ImageError> {
        const CHUNK_SIZE: usize = 65536;
        let (width, height) = ImageReader::new(Cursor::new(&bytes[..CHUNK_SIZE.min(bytes.len())]))
            .with_guessed_format()?
            .into_dimensions()?;
        Ok(Self { width, height })
    }
}
