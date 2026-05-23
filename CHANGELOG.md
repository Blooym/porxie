# Changelog

## [0.3.2] - 2026-05-23

- Requests to PDS endpoints under `#atproto_pds` now follow a maximum of 3 redirects
  due to some PDS implementations redirecting blobs to alternative download URLs instead
  of serving directly. This reverts the change in 0.3.1 where redirects were no longer followed.
  - The redirect limit for policy service and identity requests has been set to 3 to provide a consistent behaviour.
  - The prevention of internal IP ranges will apply to all redirects in the chain.

- Several per-service HTTP timeout configuration options have been removed in favour of sensible defaults. Please open an issue if this breaks your setup or the limits are not ideal. As a result, the following flags and environment variables are no longer accepted:
  - `--blob-http-connect-timeout` / `PORXIE_BLOB_HTTP_CONNECT_TIMEOUT`
  - `--identity-http-timeout` / `PORXIE_IDENTITY_HTTP_TIMEOUT`
  - `--identity-http-connect-timeout` / `PORXIE_IDENTITY_HTTP_CONNECT_TIMEOUT`
  - `--policy-http-timeout` / `PORXIE_POLICY_HTTP_TIMEOUT`
  - `--policy-http-connect-timeout` / `PORXIE_POLICY_HTTP_CONNECT_TIMEOUT`

- The minimum size value for `--blob-max-size` has been removed.

## [0.3.1] - 2026-05-16

- Usage of raw IPv4 and IPv6 addresses in `#atproto_pds` services are now explicitly blocked.

- Requests to PDS endpoints under `#atproto_pds` will no longer follow any redirects and will fail instantly if one is provided.

- The redirect limit for identity resolutions has been changed from 2 to 4 to be generally less restrictive.

- Blob fetches and identity resolution will now refuse to use private IP ranges when making requests to prevent malicious actors from proxying requests to the internal network.
  - HTTPS was already enforced for all blob and identity requests; this change adds an additional guard on top of that.

## [0.3.0] - 2026-05-12

- Added `/xrpc/dev.blooym.porxie.getBlobMetadata` with query parameters `?did=did&cid=cid` that returns format-specific metadata about a blob.
  - This endpoint shares an internal cache with `/xrpc/dev.blooym.porxie.getBlob` for content, ownership and policy information.
  - All blobs can return metadata for their MIME type and size.
  - Images additionally include their calculated aspect ratio (width and height). These are calculated from the image's metadata without decoding, so results may be inaccurate for malformed or tampered images. Not all image types are supported.
  - Videos additionally include their calculated aspect ratio (width and height) and duration in milliseconds. These are calculated from the video's metadata without decoding, so results may be inaccurate for malformed or tampered videos. Not all video types are supported.

## [0.2.0] - 2026-05-02

- The configuration flag `--server-auth-token` has been changed to `--server-admin-password`.

- Most endpoints have been migrated to use XRPC.

  - [GET] `/:did/:cid` now has an alias endpoint available at `/xrpc/dev.blooym.porxie.getBlob` with the query parameters `?did=did&cid=cid`. The original endpoint remains fully supported for the foreseeable future, and it is the caller's decision of which endpoint is preferred for now.
  - [DELETE] `/cache/:did` has been moved to [POST] `/xrpc/dev.blooym.porxie.cache.purgeActor` with a JSON body containing `{ "did": "did" }`
  - [DELETE] `/cache/:cid` has been moved to [POST] `/xrpc/dev.blooym.porxie.cache.purgeBlob` with a JSON body containing `{ "cid": "cid" }`.
  - Authentication for all administrative endpoints now use authentication type 'Basic' instead of 'Bearer'. Per the temporary ATProtocol specification, the username field is expected to be set to `admin`.

- The policy service has been migrated to use XRPC.
  - Calls will now be made to `/xrpc/dev.blooym.porxie.getBlobPolicy` with the query parameters `?did=did&cid=cid`.
  - As part of this change, Porxie will now expect a JSON response containing the status of the blob instead of using the previous method of handling based on status code. You can find the permitted responses in the [lexicon definition](lexicons/dev/blooym/porxie/getBlobPolicy.json).
  - The configuration option to append custom headers remains as-is and can be used to use whatever authentication scheme you see fit. Please note that Porxie does not support service authentication at this time, so your best choice would be using [admin tokens](https://atproto.com/specs/xrpc#admin-token-temporary-specification).

## [0.1.2] - 2026-04-30

### Security Fixes:

- **Fix broken logic for enabling HTTPS only in release mode.**
- **Fix broken authentication check logic that allows invalid tokens to clear the internal cache.**

## [0.1.1] - 2026-04-30

- Improve compliance with blob cid specification by only accepting v1 hashes with accepted codecs

- Use jemallocator instead of the system allocator on all platforms.

- Make authentication checks constant time.

- Refactored codebase for future maintainability

## [0.1.0] - 2026-03-26

- Initial Release
