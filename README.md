# Porxie

A correct and efficient ATProto Blob proxy service with caching and policy enforcement.

## Features

- **Secure by default** - verifies blob CIDs are legitimate and serves them with strict headers.
- **Primitive MIME type filter** - auto-detects blob MIME type from content and optionally restricts which MIME types can be served. (Note: this validation is basic and falls back to `application/octet-stream` if that MIME type is enabled).
- **Policy enforcement** - optional integration with an external policy service to control which blobs can be served. Bring your own rules.
- **In-memory cache** - TinyLFU-based caching for fast repeat access to frequently requested content and policy decisions. Manual cache purging is supported via a simple authenticated HTTP DELETE request.

## Routes

- **GET** `/{did}/{cid}` - Resolve and fetch a blob from its origin.
- **DELETE** `/cache/{cid or did}` - Invalidate cache for either a CID (blob, policy, ownership) or for a DID (ownerships and policies). Requires configured bearer auth token.

## Usage

Porxie does not handle TLS, so it should be placed behind a reverse proxy such as [Caddy](https://caddyserver.com), [Traefik](https://traefik.io/traefik), or [nginx](https://nginx.org). Make sure your reverse proxy (and any other intermediaries) pass through, at minimum, the `Cache-Control`, `Content-Security-Policy` and `Content-Disposition` headers from upstream responses.

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

### Headers

Custom headers can be attached to every request Porxie sends to the policy service, for example to pass authentication credentials. See the [Configuration](#configuration) section for details.

### Fail-open vs fail-closed

By default, if the policy service is unreachable or returns an unexpected status, Porxie blocks the request and returns a 500. This is fail-closed behaviour. When `--policy-service-fail-open` is set to true, requests will go through as normal. Use this if uptime matters more to you than strict enforcement.

## Configuration

All options can be set via flags, environment variables, or a `.env` file. For up-to-date and complete help, please use the `--help` flag.

```
Usage: porxie [OPTIONS]

Options:
      --address <ADDRESS>
          Socket address (IPv4 or IPv6) to bind the server to

          [env: PORXIE_ADDRESS=]
          [default: 127.0.0.1:6314]

      --request-timeout <TIMEOUT>
          Timeout applied to incoming requests

          [env: PORXIE_REQUEST_TIMEOUT=]
          [default: 2m]

      --auth-token <AUTH_TOKEN>
          Bearer token for authenticating admin requests.

          When unset, all authenticated endpoints will reject requests with HTTP 401.

          [env: PORXIE_AUTH_TOKEN=]

      --allowed-mimetypes <ALLOWED_MIMETYPES>
          List of mimetypes that can be served.

          Validation is done loosely via content inference, further validation can be done by using another layer that strictly processes content above this proxy, such as an image transformation service.

          By default only image and video are allowed. Unknown blobs fall back to `application/octet-stream`, which also has to be explicitly enabled.

          When using the CLI, the flag can be used multiple times. When setting via environment variable, values are comma-separated (e.g. `PORXIE_ALLOWED_MIMETYPES="video/*,image/*"`).

          [env: PORXIE_ALLOWED_MIMETYPES=]
          [default: video/* image/*]

      --max-blob-size <MAX_BLOB_SIZE>
          Maximum blob size that can be served.

          Blobs that exceed this limit will return an HTTP 413 error.

          Be aware that setting this value too high can lead to the process or system running out of memory, so adjust accordingly. The minimum max blob size is 512kb.

          [env: PORXIE_MAX_BLOB_SIZE=]
          [default: 50mb]

      --plc-directory-url <PLC_DIRECTORY_URL>
          URL of the PLC directory instance used for `did:plc` lookups.

          Can typically be left as default unless using a custom or test directory.

          [env: PORXIE_PLC_DIRECTORY_URL=]
          [default: https://plc.directory]

      --upstream-proxy <UPSTREAM_PROXY>
          HTTP(S) or SOCKS5(h) proxy for upstream requests. Supports embedded credentials (e.g. http://user:pass@host).

          When unset, the system's proxy configuration is used automatically.

          [env: PORXIE_UPSTREAM_PROXY=]

      --upstream-timeout <UPSTREAM_TIMEOUT>
          Maximum duration before upstream requests are timed out.

          This value should be lower than --request-timeout to allow time for error handling.

          [env: PORXIE_UPSTREAM_TIMEOUT=]
          [default: 30s]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Cache Options:
      --cache-size <SIZE>
          Total memory allocation for the internal cache.

          Blobs are cached using an LFU policy, the most frequently requested blobs will be kept the longest when the cache begins to exceed its maximum size.

          You may wish to adjust this to fit your needs.

          It is recommended to use a CDN or caching layer in front of Porxie for production deployments for lower latency, better global availability and better response caching.

          Be aware that setting this value too high can lead to the process or system running out of memory, so adjust accordingly. The minimum cache size is 8mb.

          [env: PORXIE_CACHE_SIZE=]
          [default: 512mb]

      --blob-cache-ttl <CONTENT_TTL>
          How long fetched blobs are cached before expiring

          [env: PORXIE_BLOB_CACHE_TTL=]
          [default: 7days]

      --ownership-cache-ttl <OWNERSHIP_TTL>
          How long blob ownership data is cached before being re-checked

          [env: PORXIE_OWNERSHIP_CACHE_TTL=]
          [default: 1day]

      --policy-cache-ttl <POLICY_TTL>
          How long policy decisions are cached before being re-checked

          [env: PORXIE_POLICY_CACHE_TTL=]
          [default: 1h]

      --cache-control-header <CACHE_CONTROL_HEADER_VALUE>
          The Cache-Control header value to send alongside responses.

          This header does not modify internal cache lifetimes, only how other clients are instructed to cache responses (such as CDNs and browsers). You should adjust this according to your own infrastructure needs.

          Be aware that you may also need to clear intermediary caches manually if you want a policy change to apply quickly.

          [env: PORXIE_CACHE_CONTROL_HEADER=]
          [default: "public, max-age=604800, must-revalidate, immutable"]

Policy Service Options:
      --policy-service-url <URL>
          Policy service URL that DID+CID pairs will be checked against.

          Requests are sent as HTTP GET <url>/<did>/<cid>.

          The service is expected to return HTTP 200 (OK) if permitted or HTTP 410 (GONE) if restricted.

          [env: PORXIE_POLICY_SERVICE_URL=]

      --policy-service-headers <HEADERS>
          Headers sent alongside all requests to the policy service.

          Each header must be in the format "Name: value". When using the CLI, the flag can be used multiple times. When setting via environment variable, headers are pipe-separated (|).

          As pipes are used as a delimiter, they cannot be contained in headers.

          Example (cli): '--policy-service-headers "Authorization: Bearer token" --policy-service-headers "X-Api-Key: your-key"'

          Example (env): 'PORXIE_POLICY_SERVICE_HEADERS="Authorization: Bearer token|X-Api-Key: your-key"'

          [env: PORXIE_POLICY_SERVICE_HEADERS=]

      --policy-service-fail-open
          Allow requests to proceed if the policy service is unavailable or returns an unexpected status code.

          Warning: enabling this means restricted blobs may be served when the policy service is unreachable.

          [env: PORXIE_POLICY_SERVICE_FAIL_OPEN=]
```
