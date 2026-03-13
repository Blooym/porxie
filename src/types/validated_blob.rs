// // TODO: Consider transferring this implementation to a standalone ATProto crate in the future.

// use crate::types::blob_cid::{self};
// use multihash_codetable::{Code, MultihashDigest};
// use thiserror::Error;

// #[derive(Debug, Error)]
// pub enum ValidatedBlobError {
//     /// The CID's multihash codec is not supported by the codetable.
//     #[error("unsupported multihash codec 0x{0:x}")]
//     CidUnsupportedMultihash(u64),
//     /// The computed CID of the blob content does not match the expected CID.
//     #[error("CID mismatch: computed {computed} but expected {expected}")]
//     CidMismatch {
//         computed: blob_cid::BlobCid,
//         expected: blob_cid::BlobCid,
//     },
// }

// /// Blob content whose integrity has been verified against a [`blob_cid::BlobCid`].
// #[derive(Debug, PartialEq, Eq, Clone, PartialOrd, Ord, Hash)]
// pub struct ValidatedBlob(bytes::Bytes);

// impl ValidatedBlob {
//     /// Verify that `bytes` matches the expected `checksum` CID.
//     pub fn new<B: Into<bytes::Bytes>>(
//         bytes: B,
//         checksum: blob_cid::BlobCid,
//     ) -> Result<Self, ValidatedBlobError> {
//         let bytes = bytes.into();

//         // Enabled Multihashes are set in the multihash-codetable crate features.
//         let hash_code = checksum.hash().code();
//         let computed_cid = match Code::try_from(hash_code) {
//             Ok(code) => Ok(blob_cid::BlobCid::new(cid::Cid::new_v1(
//                 blob_cid::codecs::RAW,
//                 code.digest(&bytes),
//             ))
//             .expect("computed CID with raw codec should always be a valid BlobCid")),
//             Err(err) => {
//                 tracing::warn!("failed to compute CID: {err:?}");
//                 Err(ValidatedBlobError::CidUnsupportedMultihash(hash_code))
//             }
//         }?;

//         if computed_cid != checksum {
//             tracing::warn!("cid mismatch: computed {computed_cid} expected {checksum}");
//             return Err(ValidatedBlobError::CidMismatch {
//                 computed: computed_cid,
//                 expected: checksum,
//             });
//         }

//         Ok(Self(bytes))
//     }

//     #[must_use]
//     pub fn into_inner(self) -> bytes::Bytes {
//         self.0
//     }
// }

// impl core::convert::AsRef<bytes::Bytes> for ValidatedBlob {
//     fn as_ref(&self) -> &bytes::Bytes {
//         &self.0
//     }
// }

// impl core::ops::Deref for ValidatedBlob {
//     type Target = bytes::Bytes;
//     fn deref(&self) -> &Self::Target {
//         &self.0
//     }
// }

// impl core::borrow::Borrow<bytes::Bytes> for ValidatedBlob {
//     fn borrow(&self) -> &bytes::Bytes {
//         &self.0
//     }
// }
