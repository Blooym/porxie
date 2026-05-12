use nom_exif::{AsyncMediaSource, MediaParser, TrackInfoTag};
use std::io::Cursor;

pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub length: u64,
}

impl VideoMetadata {
    /// Calculate video metadata from a raw byte array.
    ///
    /// ## Safety
    /// Only the video's metadata tags are read during parsing. The underlying
    /// video frames are left untouched.
    pub async fn from_bytes(bytes: &[u8]) -> Result<Self, nom_exif::Error> {
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

        Ok(VideoMetadata {
            width,
            height,
            length: duration,
        })
    }
}
