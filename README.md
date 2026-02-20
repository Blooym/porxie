# Porxie

A correct and efficient ATProto Blob proxy service with caching and moderation takedowns.

## Features

- **Secure by default** - verifies blob CIDs are legitimate and serves them with strict headers.
- **Primitive mimetype filter** - auto-detects blob MIME type from content and optionally restricts which mimetypes can be served. (Note: this validation is basic and falls back to `application/octet-stream` if the mimetype filter is enabled).
- **In-memory cache** - TinyLFU-based caching for fast repeat access to frequently requested content and moderation actions.
- **Moderation service** - optional integration with an external custom moderation service to provide content takedowns. Bring your own policies.
- **Manual cache purging** - Cached content and moderation status can be purged via a simple authenticated HTTP DELETE.

## Routes

- **GET** `/did/cid` - Resolve and fetch a blob from its origin.
- **DELETE** `/did/cid` - Invalidate cached blob and moderation data. Requires configured bearer auth token.

## Usage

Please refer to the [configuration](#configuration) section for details on how to configure Porxie.

Porxie does not handle TLS termination and should be placed behind a reverse proxy such as [Caddy](https://caddyserver.com), [Traefik](https://traefik.io/traefik), or [nginx](https://nginx.org). Ensure your reverse proxy is configured to pass through `Cache-Control` and `Content-Disposition` headers from upstream responses. Please note that if you use other intermediary services you may need to configure those to pass through the headers as well.

### Directly

To run Porxie directly via CLI, you can simply compile and use the binary with [Rust and Cargo](https://rust-lang.org/tools/install/).

1. Install with

   ```sh
   cargo install --git https://codeberg.org/Blooym/porxie.git
   ```

2. Set configuration values as necessary.

3. Run the server
   ```sh
   porxie <flags>
   ```

### With Docker

To run Porxie with the Docker CLI and default settings you can run the following:

```sh
docker run -d \
  --name porxie \
  --restart unless-stopped \
  -p 6314:6314 \
  ghcr.io/blooym/porxie:latest
```

### With Docker Compose

To run Porxie with Docker Compose and default settings you can run the following:

```yaml
services:
  porxie:
    image: ghcr.io/blooym/porxie:latest
    restart: unless-stopped
    read_only: true
    ports:
      - "6314:6314"
    cap_drop:
      - ALL
    security_opt:
      - no-new-privileges
```

<details>
<summary>Pairing Porxie with Imgproxy for image post-processing</summary>

[Imgproxy](https://imgproxy.net) can be placed in front of Porxie to handle image transformations such as resizing, cropping, and format conversions. An example configuration for this would look like this:

```yaml
services:
  porxie:
    image: ghcr.io/blooym/porxie:latest
    restart: unless-stopped
    read_only: true
    cap_drop:
      - ALL
    security_opt:
      - no-new-privileges
    environment:
      PORXIE_ALLOWED_MIMETYPES: "image/*"
      PORXIE_MAX_BLOB_SIZE: 25mb

  imgproxy:
    image: darthsim/imgproxy:latest
    restart: unless-stopped
    read_only: true
    cap_drop:
      - ALL
    security_opt:
      - no-new-privileges
    depends_on:
      - porxie
    environment:
      # See https://docs.imgproxy.net/configuration/options for all options.
      IMGPROXY_BIND: ":8080"
      IMGPROXY_BASE_URL: "http://porxie:6314/"
      IMGPROXY_ALLOWED_SOURCES: "http://porxie:6314/"
      IMGPROXY_MAX_SRC_FILE_SIZE: 25000000
      IMGPROXY_CACHE_CONTROL_PASSTHROUGH: true
```

#### Replicating cdn.bsky.app

Bluesky's CDN serves images at URLs in the form of `https://cdn.bsky.app/img/{preset}/plain/{did}/{cid}@{format}`. By configuring imgproxy with matching presets and enabling preset-only mode, you can use your own server with the same URL scheme, which makes this a near drop-in replacement:

To do this, set the following presets.

```yaml
IMGPROXY_PRESETS: >-
  avatar=rs:fill:1000:1000:1:1/g:ce,
  avatar_thumbnail=rs:fill:128:128:1:1/g:ce,
  feed_thumbnail=rs:fit:0:1000,
  feed_fullsize=rs:fit:0:0
IMGPROXY_ONLY_PRESETS: true
```

Please refer to the imgproxy documentation for up-to-date details if you wish to add more or modify these. **Bluesky may change the format of their CDN at any time.**

</details>

### Configuration

All options can be set via flags, environment variables, or a `.env` file. For up-to-date and complete help, please use the `--help` flag (from which the following help is generated).

```
Usage: porxie [OPTIONS]

Options:
      --address <ADDRESS>
          Socket address to bind the server to

          [env: PORXIE_ADDRESS=]
          [default: 127.0.0.1:6314]

      --timeout <TIMEOUT>
          Maximum duration before incoming requests are timed out

          [env: PORXIE_TIMEOUT=]
          [default: 60s]

      --auth-token <AUTH_TOKEN>
          Bearer token required to authenticate admin requests.

          When unset, all authenticated endpoints are unusable.

          [env: PORXIE_AUTH_TOKEN=]

      --allowed-mimetypes <ALLOWED_MIMETYPES>
          List of mimetypes that can be served through this CDN.

          Validation is done loosely via content inference and is not foolproof. It is recommended to apply a sandboxed layer that will process the blob further to validate its type.

          [env: PORXIE_ALLOWED_MIMETYPES=]
          [default: */*]

      --cache-header-value <CACHE_CONTROL_HEADER_VALUE>
          The cache-control header value to send alongside responses.

          This header does not modify the internal cache lifetime of content, only how it instructs other clients to cache responses.

          [env: PORXIE_CACHE_HEADER_VALUE=]
          [default: "public, max-age=604800, must-revalidate"]

      --cache-size <CACHE_SIZE>
          Maximum size of cached responses in memory.

          Content is evicted using a TinyLFU policy that automatically prioritises the most frequently requested keys.

          It is recommended you deploy a dedicated caching service in front of this service for the best cache performance. The built-in cache is optimised for handling frequent requests and bursts requesting the same content.

          The default value is conservatively low; you may wish to raise it to fit your needs.

          [env: PORXIE_CACHE_SIZE=]
          [default: 512mb]

      --max-blob-size <MAX_BLOB_SIZE>
          Maximum blob size that can be served through this CDN.

          Content that exceeds this limit will return an HTTP 413 error.

          [env: PORXIE_MAX_BLOB_SIZE=]
          [default: 50mb]

      --moderation-cache-size <MODERATION_CACHE_SIZE>
          Maximum size of cached moderation responses in memory.

          Each entry is lightweight, so small allocations can hold a large number of entries.

          [env: PORXIE_MODERATION_CACHE_SIZE=]
          [default: 128mb]

      --moderation-cache-ttl <MODERATION_CACHE_TTL>
          How long moderation responses are cached before being re-checked

          [env: PORXIE_MODERATION_CACHE_TTL=]
          [default: 1h]

      --moderation-service-auth-token <MODERATION_SERVICE_AUTH_TOKEN>
          Bearer auth token sent with all requests to the moderation service

          [env: PORXIE_MODERATION_SERVICE_AUTH_TOKEN=]

      --moderation-service-fail-open <MODERATION_SERVICE_FAIL_OPEN>
          Whether to allow requests to proceed if the moderation service is unavailable or returns an unexpected status code

          [env: PORXIE_MODERATION_SERVICE_FAIL_OPEN=]
          [default: false]
          [possible values: true, false]

      --moderation-service-url <MODERATION_SERVICE_URL>
          URL of an upstream moderation service that DID+CID pairs will be checked against.

          Requests are sent as HTTP GET <url>/<did>/<cid>.

          The service is expected to return HTTP 200 if permitted or HTTP 410 if taken down.

          [env: PORXIE_MODERATION_SERVICE_URL=]

      --plc-directory-url <PLC_DIRECTORY_URL>
          URL of the PLC directory instance used for `did:plc` lookups.

          Can typically be left as default unless using a custom or test directory.

          [env: PORXIE_PLC_DIRECTORY_URL=]
          [default: https://plc.directory]

      --upstream-https-only <UPSTREAM_HTTPS_ONLY>
          Only allow HTTPS when connecting to upstreams.

          Disabling this is strongly discouraged outside of local development.

          [env: PORXIE_UPSTREAM_HTTPS_ONLY=]
          [default: true]
          [possible values: true, false]

      --upstream-proxy <UPSTREAM_PROXY>
          HTTP(S) proxy for upstream requests. Supports embedded credentials (https://user:pass@host).

          When unset, the system proxy configuration is used automatically.

          [env: PORXIE_UPSTREAM_PROXY=]

      --upstream-timeout <UPSTREAM_TIMEOUT>
          Maximum duration before upstream PDS requests are timed out

          [env: PORXIE_UPSTREAM_TIMEOUT=]
          [default: 30s]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```
