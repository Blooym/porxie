mod blob_service;
mod cache;
mod identity_service;
mod networking;
mod policy_client;
mod server;
mod types;

use crate::{
    blob_service::{BlobService, BlobServiceOptions},
    cache::compute_cache_sizes,
    identity_service::{IdentityService, IdentityServiceOptions},
    policy_client::{PolicyClient, PolicyClientOptions},
    server::{PorxieServer, PorxieServerOptions, SocketAddress, use_system_shutdown_signal},
};
use axum::http::{HeaderName, HeaderValue};
use bytesize::ByteSize;
use clap::{Args, Parser};
use core::num::NonZeroU64;
use dotenvy::dotenv;
use porxie_mediautil::deps::mime::Mime;
use reqwest::Url;
use tracing_subscriber::EnvFilter;

// Jemalloc seems to perform better compared to most system allocators,
// especially with multi-threading and long-lived variable-sized
// allocations.
//
// It especially performs a lot better on MUSL (when last benchmarked).
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

#[derive(Parser)]
#[clap(
    author,
    about,
    version,
    after_help = "* Use '--help' for additional information.",
    after_long_help = concat!("* Refer to the project page at ", env!("CARGO_PKG_HOMEPAGE"), " for more guidance.")
)]
struct AppArgs {
    #[command(flatten)]
    server: ServerArgs,

    #[command(flatten)]
    blob: BlobArgs,

    #[command(flatten)]
    identity: IdentityArgs,

    #[command(flatten)]
    cache: CacheArgs,

    #[command(flatten)]
    policy: PolicyServiceArgs,
}

#[derive(Args)]
#[command(next_help_heading = "Server Options")]
struct ServerArgs {
    /// Address to bind the server to.
    ///
    /// Use the 'ip:' prefix for an IP address (e.g. 'ip:127.0.0.1:6314'), or on UNIX systems,
    /// the 'unix:' prefix for a UNIX socket path (e.g. 'unix:/run/porxie.sock').
    #[arg(
        id = "SA_ADDRESS",
        long = "server-address",
        env = "PORXIE_SERVER_ADDRESS",
        default_value = "ip:127.0.0.1:6314"
    )]
    address: SocketAddress,

    /// Admin password for authenticating privileged requests.
    ///
    /// Authenticated requests always expect the username `admin` as per specification.
    ///
    /// When not set, authenticated endpoints will be unavailable.
    #[arg(
        id = "SA_SERVER_ADMIN_PASSWORD",
        long = "server-admin-password",
        env = "PORXIE_SERVER_ADMIN_PASSWORD"
    )]
    admin_password: Option<String>,
}

#[derive(Args)]
#[command(next_help_heading = "Blob Options")]
struct BlobArgs {
    /// Blob mimetypes that can be served. Wildcards are supported "*/*", "image/*", etc.
    ///
    /// Validation is done loosely via content sniffing. Further validation can be done by a layer
    /// above this proxy, such as an image transformation service. When inference fails, the blob's
    /// type falls back to `application/octet-stream`. When that type is allowed, blobs failing
    /// inference can still be served.
    ///
    /// When using the CLI, the flag can be used multiple times. When setting via environment variable,
    /// values are comma-separated (e.g. `PORXIE_BLOB_ALLOWED_MIMETYPES="video/*,image/*"`).
    #[arg(
        id = "BA_BLOB_ALLOWED_MIMETYPES",
        long = "blob-allowed-mimetypes",
        env = "PORXIE_BLOB_ALLOWED_MIMETYPES",
        default_values = ["image/jpeg", "image/png", "image/webp", "image/avif", "image/gif"],
        value_delimiter = ','
    )]
    allowed_mimetypes: Vec<Mime>,

    /// Maximum blob size that can be served.
    ///
    /// This value cannot be set higher than the system's total memory.
    #[arg(
        id = "BA_BLOB_MAX_SIZE",
        long = "blob-max-size",
        env = "PORXIE_BLOB_MAX_SIZE",
        default_value = "25mb",
        value_parser = |v: &str| -> Result<NonZeroU64, String> {
            let size: ByteSize = v.parse().map_err(|e| format!("{e}"))?;
            let total_mem = sysinfo::System::new_with_specifics(
                sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()),
            ).total_memory();
            if size.as_u64() > total_mem {
                return Err(format!(
                    "exceeds total system memory ({}) and could cause system instability",
                    ByteSize(total_mem).display().si(),
                ));
            }
            Ok(size.as_u64().try_into().map_err(|e| format!("invalid value {v}: {e}"))?)
        }
    )]
    max_size: NonZeroU64,

    /// The Cache-Control header value to send alongside blob responses.
    ///
    /// This does not affect internal cache lifetimes, only how downstream clients such as CDNs
    /// and browsers are instructed to cache responses.
    #[arg(
        id = "BA_BLOB_CACHE_HEADER",
        long = "blob-cache-header",
        env = "PORXIE_BLOB_CACHE_HEADER",
        default_value = "public, max-age=604800, immutable"
    )]
    cache_header: HeaderValue,

    /// Maximum duration a blob can be processed by this server before aborting.
    #[arg(
        id = "BA_BLOB_PROCESSING_TIMEOUT",
        long = "blob-processing-timeout",
        env = "PORXIE_BLOB_PROCESSING_TIMEOUT",
        default_value = "1m"
    )]
    processing_timeout: humantime::Duration,

    /// Maximum duration before blob fetch requests are timed out.
    #[arg(
        id = "BA_BLOB_FETCH_TIMEOUT",
        long = "blob-http-timeout",
        env = "PORXIE_BLOB_HTTP_TIMEOUT",
        default_value = "30s"
    )]
    http_timeout: humantime::Duration,
}

#[derive(Args)]
#[command(next_help_heading = "Identity Options")]
struct IdentityArgs {
    /// URL of the PLC instance used for `did:plc` lookups.
    #[arg(
        id = "IA_PLC_URL",
        long = "identity-plc-url",
        env = "PORXIE_IDENTITY_PLC_URL",
        default_value = "https://plc.directory"
    )]
    plc_url: Url,
}

#[derive(Args)]
#[command(next_help_heading = "Cache Options")]
struct CacheArgs {
    /// Total memory allocation for the internal cache.
    ///
    /// Blobs are cached using an LFU policy. The most frequently requested blobs are kept longest
    /// when the cache reaches maximum size.
    ///
    /// For production deployments, a CDN or caching layer in front of this server is recommended
    /// for lower latency and better global availability.
    ///
    /// The minimum value is 8mb and the maximum is the system's total memory.
    #[arg(
        id = "CA_CACHE_ALLOCATION",
        long = "cache-allocation",
        env = "PORXIE_CACHE_ALLOCATION",
        default_value = "512mb",
        value_parser = |v: &str| -> Result<NonZeroU64, String> {
            let size: ByteSize = v.parse().map_err(|e| format!("{e}"))?;
            if size.as_u64() < 8_000_000 {
                return Err("minimum allowed value is 8mb".to_string())
            }

            let total_mem = sysinfo::System::new_with_specifics(
                sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()),
            ).total_memory();
            if size.as_u64() > total_mem {
                return Err(format!(
                    "exceeds total system memory ({}) and could cause system instability",
                    ByteSize(total_mem).display().si(),
                ));
            }

            Ok(size.as_u64().try_into().map_err(|e| format!("invalid value {v}: {e}"))?)
        }
    )]
    size: NonZeroU64,

    /// How long blobs can be idle in the cache before expiring.
    #[arg(
        id = "CA_CACHE_BLOB_TTI",
        long = "cache-blob-tti",
        env = "PORXIE_CACHE_BLOB_TTI",
        default_value = "7days"
    )]
    blob_tti: humantime::Duration,

    /// How long blob ownership can be cached before expiring.
    #[arg(
        id = "CA_CACHE_OWNERSHIP_TTL",
        long = "cache-ownership-ttl",
        env = "PORXIE_CACHE_OWNERSHIP_TTL",
        default_value = "1day"
    )]
    ownership_ttl: humantime::Duration,

    /// How long policy decisions can be cached before expiring.
    #[arg(
        id = "CA_CACHE_POLICY_TTL",
        long = "cache-policy-ttl",
        env = "PORXIE_CACHE_POLICY_TTL",
        default_value = "1h"
    )]
    policy_ttl: humantime::Duration,

    /// How long identity lookups (DID resolution, etc) can be cached before expiring.
    #[arg(
        id = "CA_CACHE_IDENTITY_TTL",
        long = "cache-identity-ttl",
        env = "PORXIE_CACHE_IDENTITY_TTL",
        default_value = "1h"
    )]
    identity_ttl: humantime::Duration,
}

#[derive(Args)]
#[command(next_help_heading = "Policy Service Options")]
struct PolicyServiceArgs {
    /// Policy service URL that DID+CID pairs will be checked against.
    ///
    /// Requests are sent via XRPC to <url>/xrpc/dev.blooym.porxie.getBlobPolicy.
    #[arg(id = "PA_POLICY_URL", long = "policy-url", env = "PORXIE_POLICY_URL")]
    url: Option<Url>,

    /// Headers sent alongside requests to the policy service.
    ///
    /// Each header must be in the format "Name: value". When using the CLI, the flag can be used multiple times.
    /// When setting via environment variable, headers are pipe-separated (|).
    ///
    /// As pipes are used as a delimiter, they cannot be contained in headers.
    ///
    /// Example (CLI): '--policy-request-headers "X-Hello: world" --policy-request-headers "X-Foo: bar"'
    ///
    /// Example (ENV): 'PORXIE_POLICY_REQUEST_HEADERS="X-Hello: world|X-Foo: bar"'
    #[arg(
        id = "PA_POLICY_REQ_HEADERS",
        long = "policy-request-headers",
        env = "PORXIE_POLICY_REQUEST_HEADERS",
        value_delimiter = '|',
        requires = "PA_POLICY_URL",
        value_parser = |v: &str| -> Result<(HeaderName, HeaderValue), String> {
            let (name, value) = v.split_once(':')
                .ok_or_else(|| format!("invalid header {v}: expected 'Name: value'"))?;
            let name = HeaderName::try_from(name.trim())
                .map_err(|e| format!("invalid header name in {v}: {e}"))?;
            let mut value = HeaderValue::try_from(value.trim())
                .map_err(|e| format!("invalid header value in {v}: {e}"))?;
            value.set_sensitive(true);
            Ok((name, value))
        }
    )]
    request_headers: Vec<(HeaderName, HeaderValue)>,

    /// Allow requests to proceed even if the policy service is unavailable.
    ///
    /// Warning: enabling this means restricted blobs may be served when the policy service is  unavailable.
    #[arg(
        id = "PA_POLICY_FAIL_OPEN",
        long = "policy-fail-open",
        env = "PORXIE_POLICY_FAIL_OPEN",
        requires = "PA_POLICY_URL"
    )]
    fail_open: bool,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    json_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or(EnvFilter::new("info")))
        .init();
    let args = AppArgs::parse();

    let cache_sizes = compute_cache_sizes(args.cache.size)?;
    let server = PorxieServer::new(PorxieServerOptions {
        blob_processing_timeout: args.blob.processing_timeout.into(),
        identity_service: IdentityService::new(IdentityServiceOptions {
            cache_memory_allocation: cache_sizes.identity,
            cache_ttl: args.cache.identity_ttl.into(),
            plc_directory_url: args.identity.plc_url,
        })?,
        policy_client: args
            .policy
            .url
            .map(|url| {
                PolicyClient::new(PolicyClientOptions {
                    policy_service_url: url,
                    policy_service_req_headers: args.policy.request_headers,
                    cache_max_memory_allocation: cache_sizes.policy,
                    cache_ttl: args.cache.policy_ttl.into(),
                })
            })
            .transpose()?,
        blob_service: BlobService::new(BlobServiceOptions {
            allowed_mimetypes: args.blob.allowed_mimetypes.into(),
            data_cache_max_capacity: cache_sizes.blob,
            data_cache_tti: args.cache.blob_tti.into(),
            http_timeout: args.blob.http_timeout.into(),
            max_blob_size: args.blob.max_size,
            ownership_cache_max_capacity: cache_sizes.ownership,
            ownership_cache_ttl: args.cache.ownership_ttl.into(),
        })?,
        admin_password: args.server.admin_password,
        cache_control_header: args.blob.cache_header,
        policy_fail_open: args.policy.fail_open,
    });

    server
        .start(args.server.address, use_system_shutdown_signal())
        .await?;

    Ok(())
}
