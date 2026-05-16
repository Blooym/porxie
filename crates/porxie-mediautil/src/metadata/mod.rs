#[cfg(feature = "image-metadata")]
mod image;
#[cfg(feature = "video-metadata")]
mod video;

#[cfg(feature = "image-metadata")]
pub use image::*;
#[cfg(feature = "video-metadata")]
pub use video::*;
