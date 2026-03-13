# Porxie

A correct and efficient ATProto blob proxy for secure content delivery.

## Features

- Blob validation - verifies blob content matches its CID and rejects invalid/tampered content.
- Secure serving - blobs are always served with secure headers to help improve end-user security.
- MIME filtering - detects blob content MIME-types and enforces an optional allowlist of permitted types.
- Policy enforcement - optionally integrate with an external policy service (like an AppView) to control which blobs can be served.
- In-memory cache - configurable in-memory caching for fast repeat access with support for manual cache purging via authenticated HTTP DELETE.

## Routes

- [GET] `/{did}/{cid}`: Resolve and fetch a blob from its origin.
- [DELETE] `/cache/{cid or did}`: Invalidate all valid cache items for a specific blob CID or for an entire user DID. Requires configured bearer auth token.

## Usage

> [!NOTE]
> Porxie does not handle TLS, so it should be placed behind a reverse proxy like [Caddy](https://caddyserver.com), [Traefik](https://traefik.io/traefik), or [NGINX](https://nginx.org). Ensure that any intermediaries between Porxie and the client pass through the `Cache-Control`, `Content-Security-Policy` and `Content-Disposition` headers, or otherwise set them securely.
>
> Putting a CDN in front of Porxie is also recommended for better long-term caching and worldwide latency.

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

### Run: Nix

For Nix users, a [`flake.nix`](./flake.nix) is included in this repository that provides a package and a NixOS module. Please note that this is maintained on a best-effort basis for now and may be upstreamed by another contributor later.

## Policy Service

Porxie can optionally check with an external HTTP service before serving any blob. You build and run this service yourself - Porxie just calls it and acts on the response. This is useful for things like content takedowns or blob allow lists.

For every incoming request, Porxie sends `GET <policy-service-url>/<did>/<cid>` and expects one of the following responses:

- **200 OK** - the blob is allowed and will be served.
- **410 Gone** - the blob is restricted and Porxie will refuse to serve it to the client.

Any other status code is treated as an error for now.

Policy decisions are cached per DID+CID pair, so your service won't be hit on every request. To clear a cached decision early, use the `DELETE /cache/{cid}` endpoint.

By default, Porxie will fail-closed: if the policy service errors, the blob request fails too. This can be changed to fail-open if preferred.

See the [Configuration](#configuration) section for all available policy options.

## Configuration

All options can be set via flags, environment variables, or a `.env` file. For the most detailed and up-to-date descriptions, use `porxie --help`.

### Server

```
--server-address <SA_ADDRESS>
    Address to bind the server to.

    Use the 'ip:' prefix for an IP address (e.g. 'ip:127.0.0.1:6314'), or on UNIX systems,
    the 'unix:' prefix for a UNIX socket path (e.g. 'unix:/run/porxie.sock').

    [env: PORXIE_SERVER_ADDRESS=]
    [default: ip:127.0.0.1:6314]

--server-auth-token <SA_SERVER_AUTH_TOKEN>
    Bearer token for authenticating admin requests.

    When unset, all authenticated endpoints will reject requests with HTTP 401.

    [env: PORXIE_SERVER_AUTH_TOKEN=]
```

### Blob

```
--blob-allowed-mimetypes <BA_BLOB_ALLOWED_MIMETYPES>
    Blob mimetypes that can be served.

    Validation is done loosely via content inference. Further validation can be done by a
    layer above this proxy, such as an image transformation service. When inference fails,
    the blob's type falls back to `application/octet-stream`. When that type is allowed,
    blobs failing inference can still be served.

    When using the CLI, the flag can be used multiple times. When setting via environment
    variable, values are comma-separated
    (e.g. `PORXIE_BLOB_ALLOWED_MIMETYPES="video/*,image/*"`).

    [env: PORXIE_BLOB_ALLOWED_MIMETYPES=]
    [default: image/*]

--blob-max-size <BA_BLOB_MAX_SIZE>
    Maximum blob size that can be fetched and served.

    Blobs that exceed this limit will return HTTP 413. Setting this too high can exhaust
    process or system memory. The minimum value is 512kb.

    [env: PORXIE_BLOB_MAX_SIZE=]
    [default: 50mb]

--blob-cache-header <BA_BLOB_CACHE_HEADER>
    The Cache-Control header value to send alongside blob responses.

    This does not affect internal cache lifetimes, only how downstream clients such as CDNs
    and browsers are instructed to cache responses. Intermediary caches may need to be
    cleared manually for changes to take effect quickly.

    [env: PORXIE_BLOB_CACHE_HEADER=]
    [default: "public, max-age=604800, must-revalidate, immutable"]

--blob-processing-timeout <BA_BLOB_PROCESSING_TIMEOUT>
    Maximum duration a blob can be processed by this server before aborting

    [env: PORXIE_BLOB_PROCESSING_TIMEOUT=]
    [default: 1m]

--blob-http-timeout <BA_BLOB_FETCH_TIMEOUT>
    Maximum duration before blob fetch requests are timed out

    [env: PORXIE_BLOB_HTTP_TIMEOUT=]
    [default: 30s]

--blob-http-connect-timeout <BA_BLOB_FETCH_CONNECT_TIMEOUT>
    Maximum duration before an attempted connection to a blob upstream is aborted.

    This value should be lower than --blob-http-timeout.

    [env: PORXIE_BLOB_HTTP_CONNECT_TIMEOUT=]
    [default: 10s]
```

### Identity

```
--identity-plc-url <IA_PLC_URL>
    URL of the PLC instance used for `did:plc` lookups.

    Can typically be left as default unless using a custom or local development setup.

    [env: PORXIE_IDENTITY_PLC_URL=]
    [default: https://plc.directory]

--identity-http-timeout <IA_IDENTITY_HTTP_TIMEOUT>
    Maximum duration before identity resolution requests are timed out

    [env: PORXIE_IDENTITY_HTTP_TIMEOUT=]
    [default: 10s]

--identity-http-connect-timeout <IA_IDENTITY_HTTP_CONNECT_TIMEOUT>
    Maximum duration before a connection attempt to an identity upstream is aborted.

    This value should be lower than --identity-http-timeout.

    [env: PORXIE_IDENTITY_HTTP_CONNECT_TIMEOUT=]
    [default: 8s]
```

### Cache

```
--cache-allocation <CA_CACHE_ALLOCATION>
    Total memory allocation for the internal cache.

    Blobs are cached using an LFU policy. The most frequently requested blobs are kept
    longest when the cache approaches its limit.

    For production deployments, a CDN or caching layer in front of this server is
    recommended for lower latency and better global availability.

    Setting this too high can exhaust process or system memory. The minimum value is 8mb.

    [env: PORXIE_CACHE_ALLOCATION=]
    [default: 512mb]

--cache-blob-tti <CA_CACHE_BLOB_TTI>
    How long blobs can be idle in the cache before expiring

    [env: PORXIE_CACHE_BLOB_TTI=]
    [default: 7days]

--cache-ownership-ttl <CA_CACHE_OWNERSHIP_TTL>
    How long blob ownership can be cached before expiring

    [env: PORXIE_CACHE_OWNERSHIP_TTL=]
    [default: 1day]

--cache-policy-ttl <CA_CACHE_POLICY_TTL>
    How long policy decisions can be cached before expiring

    [env: PORXIE_CACHE_POLICY_TTL=]
    [default: 1h]

--cache-identity-ttl <CA_CACHE_IDENTITY_TTL>
    How long identity lookups (DID resolution, etc) can be cached before expiring

    [env: PORXIE_CACHE_IDENTITY_TTL=]
    [default: 1h]
```

### Policy Service

```
--policy-url <PA_POLICY_URL>
    Policy service URL that DID+CID pairs will be checked against.

    Requests are sent as HTTP GET <url>/<did>/<cid>.

    The service is expected to return HTTP 200 (OK) if permitted or HTTP 410 (GONE) if
    restricted.

    [env: PORXIE_POLICY_URL=]

--policy-request-headers <PA_POLICY_REQ_HEADERS>
    Headers sent alongside all requests to the policy service.

    Each header must be in the format "Name: value". When using the CLI, the flag can be
    used multiple times. When setting via environment variable, headers are
    pipe-separated (|).

    As pipes are used as a delimiter, they cannot be contained in headers.

    Example (cli): '--policy-request-headers "Authorization: Bearer token"
    --policy-request-headers "X-Api-Key: your-key"'

    Example (env): 'PORXIE_POLICY_REQUEST_HEADERS="Authorization: Bearer
    token|X-Api-Key: your-key"'

    [env: PORXIE_POLICY_REQUEST_HEADERS=]

--policy-fail-open
    Allow requests to proceed if the policy service is unavailable or returns an
    unexpected status code.

    Warning: enabling this means restricted blobs may be served when the policy service
    is unreachable.

    [env: PORXIE_POLICY_FAIL_OPEN=]

--policy-http-timeout <PA_POLICY_HTTP_TIMEOUT>
    Maximum duration before policy service requests are timed out

    [env: PORXIE_POLICY_HTTP_TIMEOUT=]
    [default: 30s]

--policy-http-connect-timeout <PA_POLICY_HTTP_CONNECT_TIMEOUT>
    Maximum duration before an attempted connection to the policy service is aborted.

    This value should be lower than --policy-http-timeout.

    [env: PORXIE_POLICY_HTTP_CONNECT_TIMEOUT=]
    [default: 10s]
```

## Examples

> [!NOTE]
> The examples below are starting points to demonstrate what is possible with Porxie. They will likely need further modification to suit your needs and are not intended to be used as-is.

### Porxie & Imgproxy

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
      PORXIE_BLOB_ALLOWED_MIMETYPES: "image/*"
      PORXIE_BLOB_MAX_SIZE: 25mb
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
      IMGPROXY_RETURN_ATTACHMENT: true
      IMGPROXY_STRIP_METADATA: true
```

#### Replicating cdn.bsky.app

Bluesky's CDN typically serves images using URLs like `https://cdn.bsky.app/img/{preset}/plain/{did}/{cid}`. By configuring imgproxy with presets and enabling preset-only mode, you can create a compatiable service. The presets below are based on what Bluesky used at the time of writing and may not be up-to-date.

```yaml
IMGPROXY_PRESETS: >-
  avatar=rs:fill:1000:1000:1:1/g:ce/ext:webp,
  avatar_thumbnail=rs:fill:128:128:1:1/g:ce/q:70/ext:webp,
  feed_thumbnail=rs:fit:1000:0/q:70/ext:webp,
  feed_fullsize=ext:webp,
  banner=rs:fill:3000:1000:1:1/g:ce/ext:webp
IMGPROXY_ONLY_PRESETS: true
```

Refer to the [imgproxy documentation](https://docs.imgproxy.net) for details on creating and modifying presets.
