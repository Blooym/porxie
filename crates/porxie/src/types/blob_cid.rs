// TODO: Transfer this implementation to a standalone ATProto types crate in the future.

use cid::Version;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BlobCidError {
    /// The CID uses an invalid codec type.
    #[error("invalid blob codec 0x{0:x}, the only supported codec is raw (0x55)")]
    InvalidBlobCodec(u64),
    /// The CID uses an invalid version.
    #[error("invalid blob version {0:?}, the only supported version is v1")]
    InvalidBlobVersion(Version),
    /// The CID uses an invalid multihash.
    #[error("invalid multihash {0:?}, the only supported version is sha256")]
    InvalidMultihash(multihash_codetable::Multihash),
    /// An error from the CID crate.
    #[error(transparent)]
    CidError(#[from] cid::Error),
}

/// A [`cid::Cid`] wrapper that guarantees that  data conforms to the
/// ATProto blob CID specification where possible.
///
/// Note: BlobCid does not currently attempt to validate the
///  encoding representation of the given value.
///
/// Specification: <https://atproto.com/specs/blob>.
#[derive(Copy, PartialEq, Eq, Clone, PartialOrd, Ord, Hash, Debug, Serialize)]
pub struct BlobCid(cid::Cid);

impl BlobCid {
    pub fn try_from_cid(cid: cid::Cid) -> Result<Self, BlobCidError> {
        // Ensure the cid uses an accepted codec.
        if !matches!(
            cid.codec(),
            0x55 // Raw
        ) {
            return Err(BlobCidError::InvalidBlobCodec(cid.codec()));
        }

        // Ensure the cid uses an accepted version.
        if !matches!(cid.version(), Version::V1) {
            return Err(BlobCidError::InvalidBlobVersion(cid.version()));
        }

        // Ensure the cid uses an accepted multihash.
        if !matches!(
            multihash_codetable::Code::try_from(cid.hash().code()),
            Ok(multihash_codetable::Code::Sha2_256)
        ) {
            return Err(BlobCidError::InvalidMultihash(*cid.hash()));
        }

        Ok(Self(cid))
    }
}

impl<'de> serde::Deserialize<'de> for BlobCid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let cid = cid::Cid::deserialize(deserializer)?;
        Self::try_from_cid(cid).map_err(serde::de::Error::custom)
    }
}

impl core::convert::TryFrom<&str> for BlobCid {
    type Error = BlobCidError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from_cid(cid::Cid::try_from(value)?)
    }
}

impl core::convert::TryFrom<String> for BlobCid {
    type Error = BlobCidError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from_cid(cid::Cid::try_from(value)?)
    }
}

impl core::convert::TryFrom<Vec<u8>> for BlobCid {
    type Error = BlobCidError;
    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_from_cid(cid::Cid::try_from(value)?)
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
