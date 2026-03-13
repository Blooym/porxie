// TODO: Transfer this implementation to a standalone ATProto types crate in the future.

use serde::Serialize;
use thiserror::Error;

pub mod codecs {
    pub const RAW: u64 = 0x55;
}

#[derive(Debug, Error)]
pub enum BlobCidError {
    /// The CID uses a codec other than raw (`0x55`), which is the only codec
    /// permitted for ATProto blobs.
    #[error("invalid blob codec 0x{0:x}, the only supported codec is raw (0x55)")]
    InvalidBlobCodec(u64),

    /// The underlying CID could not be parsed.
    #[error(transparent)]
    CidError(#[from] cid::Error),
}

/// A [`cid::Cid`] wrapper that guarantees the codec is raw (`0x55`), conforming
/// to the ATProto blob CID specification.
///
/// Specification: <https://atproto.com/specs/blob> (Conformant as of **13/03/26**).
#[derive(Copy, PartialEq, Eq, Clone, PartialOrd, Ord, Hash, Debug, Serialize)]
pub struct BlobCid(cid::Cid);

impl BlobCid {
    pub fn new(cid: cid::Cid) -> Result<Self, BlobCidError> {
        if cid.codec() != codecs::RAW {
            return Err(BlobCidError::InvalidBlobCodec(cid.codec()));
        }
        Ok(Self(cid))
    }
}

impl<'de> serde::Deserialize<'de> for BlobCid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let cid = cid::Cid::deserialize(deserializer)?;
        Self::new(cid).map_err(serde::de::Error::custom)
    }
}

impl core::convert::TryFrom<&str> for BlobCid {
    type Error = BlobCidError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(cid::Cid::try_from(value)?)
    }
}

impl core::convert::TryFrom<String> for BlobCid {
    type Error = BlobCidError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(cid::Cid::try_from(value)?)
    }
}

impl core::convert::TryFrom<Vec<u8>> for BlobCid {
    type Error = BlobCidError;
    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::new(cid::Cid::try_from(value)?)
    }
}

impl core::convert::AsRef<cid::Cid> for BlobCid {
    fn as_ref(&self) -> &cid::Cid {
        &self.0
    }
}

impl core::fmt::Display for BlobCid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl core::ops::Deref for BlobCid {
    type Target = cid::Cid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::borrow::Borrow<cid::Cid> for BlobCid {
    fn borrow(&self) -> &cid::Cid {
        &self.0
    }
}
