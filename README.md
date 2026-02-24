# Porxie

A correct and efficient ATProto Blob proxy service with caching and policy enforcement.

## Features

- **Secure by default** - verifies blob CIDs are legitimate and serves them with strict headers.
- **Primitive MIME type filter** - auto-detects blob MIME type from content and optionally restricts which MIME types can be served. (Note: this validation is basic and falls back to `application/octet-stream` if that MIME type is enabled).
- **Policy enforcement** - optional integration with an external policy service to control which blobs can be served. Bring your own rules.
- **In-memory cache** - TinyLFU-based caching for fast repeat access to frequently requested content and policy decisions. Manual cache purging is supported via a simple authenticated HTTP DELETE request.

## Routes

- **GET** `/did/cid`: Resolve and fetch a blob from its origin.
- **DELETE** `/did/cid`: Invalidate cached blob and policy data. Requires a configured bearer auth token.

## Usage

Porxie does not handle TLS, so it should be placed behind a reverse proxy such as [Caddy](https://caddyserver.com), [Traefik](https://traefik.io/traefik), or [nginx](https://nginx.org). Make sure your reverse proxy (and any other intermediaries) pass through `Cache-Control` and `Content-Disposition` headers from upstream responses.

It is also recommended to put a CDN in front of Porxie for long-term caching and faster responses. Additionally, as Porxie is stateless, you can deploy in several regions for better availability.

### Run: Binary

To run Porxie directly, install [Rust and Cargo](https://rust-lang.org/tools/install/) and then:

1. Install the binary:

   ```sh
   cargo install --git https://codeberg.org/Blooym/porxie.git
   ```

2. Run the server with your chosen [configuration](#configuration) options:

   ```sh
   porxie
   ```

### Run: Docker

To run Porxie with the Docker CLI and default settings, use the following command:

```sh
docker run -d \
  --name porxie \
  --restart unless-stopped \
  -p 6314:6314 \
  ghcr.io/blooym/porxie:latest
```

### Run: Docker Compose

To run Porxie with Docker Compose, you can start with the following `compose.yml` template:

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

### Run: Docker Compose & Imgproxy

[Imgproxy](https://imgproxy.net) can be placed in front of Porxie to handle image transformations such as resizing, cropping, and format conversions.

Using Docker Compose, an example `compose.yml` would look like this:

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

Bluesky's CDN serves images at URLs in the form of `https://cdn.bsky.app/img/{preset}/plain/{did}/{cid}@{format}`. By configuring imgproxy with matching presets and enabling preset-only mode, you can use the same URL scheme as a near drop-in replacement. Set the following presets:

```yaml
IMGPROXY_PRESETS: >-
  avatar=rs:fill:1000:1000:1:1/g:ce,
  avatar_thumbnail=rs:fill:128:128:1:1/g:ce,
  feed_thumbnail=rs:fit:0:1000,
  feed_fullsize=rs:fit:0:0
IMGPROXY_ONLY_PRESETS: true
```

Please refer to the imgproxy documentation for up-to-date details if you wish to add more or modify these. **Bluesky may change the format of their CDN at any time.**

## Policy Service

The policy service is an optional external HTTP service that Porxie consults before serving any blob. You build and run the service yourself - Porxie just calls it and acts on the response. This lets you implement domain-specific policy for your service such as takedowns, allow lists, account-level bans, or anything else.

### How it works

For every incoming request, Porxie sends `GET <policy-service-url>/<did>/<cid>` to your policy service. The response code received will determine what Porxie does:

- **200 OK**: the blob is allowed and will be served
- **410 Gone**: the blob is restricted; Porxie returns 410 to the client

Any other status code is treated as an error.

Policy decisions are cached per DID+CID pair for the duration set by `--policy-cache-ttl`, so your service will not be hit on every request. To clear a cached decision immediately, use the `DELETE /<did>/<cid>` endpoint.

### Authentication

If your policy service requires authentication, set `--policy-service-auth-token` to a bearer token. Porxie will include it as an `Authorization: Bearer <token>` header on every request.

### Fail-open vs fail-closed

By default, if the policy service is unreachable or returns an unexpected status, Porxie blocks the request and returns a 500. This is fail-closed behaviour. When `--policy-service-fail-open` is set to true, requests will go through as normal. Use this if uptime matters more to you than strict enforcement.

## Configuration

All options can be set via flags, environment variables, or a `.env` file. For up-to-date and complete help, please use the `--help` flag.

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

      --cache-control-header <CACHE_CONTROL_HEADER_VALUE>
          The cache-control header value to send alongside responses.

          This header does not modify the internal cache lifetime of content, only how it instructs other clients to cache responses.

          [env: PORXIE_CACHE_CONTROL_HEADER=]
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

          Content that exceeds this limit will return an HTTP 422 error.

          [env: PORXIE_MAX_BLOB_SIZE=]
          [default: 50mb]

      --policy-cache-size <POLICY_CACHE_SIZE>
          Maximum size of cached policy decisions in memory.

          Each entry is lightweight, so small allocations can hold a large number of entries.

          [env: PORXIE_POLICY_CACHE_SIZE=]
          [default: 256mb]

      --policy-cache-ttl <POLICY_CACHE_TTL>
          How long policy decisions are cached before being re-checked

          [env: PORXIE_POLICY_CACHE_TTL=]
          [default: 1h]

      --policy-service-auth-token <POLICY_SERVICE_AUTH_TOKEN>
          Authorization bearer token sent alongside all requests to the policy service

          [env: PORXIE_POLICY_SERVICE_AUTH_TOKEN=]

      --policy-service-fail-open <POLICY_SERVICE_FAIL_OPEN>
          Whether to allow requests to proceed if the policy service is unavailable or returns an unexpected status code

          [env: PORXIE_POLICY_SERVICE_FAIL_OPEN=]
          [default: false]
          [possible values: true, false]

      --policy-service-url <POLICY_SERVICE_URL>
          URL of an upstream policy service that DID+CID pairs will be checked against.

          Requests are sent as HTTP GET <url>/<did>/<cid>.

          The service is expected to return HTTP 200 (OK) if permitted or HTTP 410 (GONE) if restricted.

          [env: PORXIE_POLICY_SERVICE_URL=]

      --plc-directory-url <PLC_DIRECTORY_URL>
          URL of the PLC directory instance used for `did:plc` lookups.

          Can typically be left as default unless using a custom or test directory.

          [env: PORXIE_PLC_DIRECTORY_URL=]
          [default: https://plc.directory]

      --upstream-https-only <UPSTREAM_HTTPS_ONLY>
          Only allow HTTPS when connecting to upstreams.

          Disabling this is strongly discouraged.

          [env: PORXIE_UPSTREAM_HTTPS_ONLY=]
          [default: true]
          [possible values: true, false]

      --upstream-proxy <UPSTREAM_PROXY>
          HTTP(S) proxy for upstream requests. Supports embedded credentials (https://user:pass@host).

          When unset, the system proxy configuration is used automatically.

          [env: PORXIE_UPSTREAM_PROXY=]

      --upstream-timeout <UPSTREAM_TIMEOUT>
          Maximum duration before upstream requests are timed out

          [env: PORXIE_UPSTREAM_TIMEOUT=]
          [default: 30s]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```
