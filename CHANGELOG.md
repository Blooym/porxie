# Changelog

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
