use nom_exif::{AsyncMediaSource, MediaParser, TrackInfoTag};
use std::io::Cursor;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VideoMetadataError {
    #[error(transparent)]
    Nom(#[from] nom_exif::Error),
}

pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
}

impl VideoMetadata {
    /// Calculate video metadata from raw bytes.
    ///
    /// ## Safety
    /// Only the video's metadata tags are read during parsing.
    pub async fn from_bytes(bytes: &[u8]) -> Result<Self, VideoMetadataError> {
        let source = AsyncMediaSource::seekable(Cursor::new(bytes)).await?;
        let info = MediaParser::new().parse_track_async(source).await?;

        let (width, height, duration) = {
            (
                info.get(TrackInfoTag::Width)
                    .and_then(|v| v.as_u32())
                    .unwrap_or(0),
                info.get(TrackInfoTag::Height)
                    .and_then(|v| v.as_u32())
                    .unwrap_or(0),
                info.get(TrackInfoTag::DurationMs)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            )
        };

        Ok(Self {
            width,
            height,
            duration_ms: duration,
        })
    }
}
