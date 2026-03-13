mod cache;
mod http;
mod mime;
mod routes;
mod types;

use crate::{
    cache::{CacheBuildOptions, Caches, build_caches},
    http::build_http_client,
    routes::{delete_cache_handler, get_blob_handler, get_index_handler},
};
use ::mime::Mime;
use anyhow::{Context, Result, bail};
use axum::{
    Router,
    extract::Request,
    http::{HeaderName, HeaderValue, StatusCode, header},
    middleware::{self as axum_middleware, Next},
    routing::{delete, get},
};
use bytesize::ByteSize;
use clap::{Args, Parser};
use dotenvy::dotenv;
use jacquard_identity::{
    JacquardResolver,
    resolver::{PlcSource, ResolverOptions},
};
use reqwest::Url;
use std::{net::SocketAddr, num::NonZeroU64, path::PathBuf, str::FromStr, sync::Arc};
use tower_http::{
    catch_panic::CatchPanicLayer,
    normalize_path::NormalizePathLayer,
    timeout::TimeoutLayer,
    trace::{self, DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, info};
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
enum AddressType {
    Ip(SocketAddr),
    #[cfg(unix)]
    Unix(PathBuf),
}

impl FromStr for AddressType {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        #[cfg(unix)]
        if let Some(path) = s.strip_prefix("unix:") {
            return Ok(AddressType::Unix(PathBuf::from(path)));
        }
        if let Some(ip) = s.strip_prefix("ip:") {
            return Ok(ip.parse::<SocketAddr>().map(AddressType::Ip)?);
        }

        #[cfg(unix)]
        bail!("unknown address binding type, expected 'ip:<addr>' or 'unix:<path>'".to_string(),);
        #[cfg(not(unix))]
        bail!("unknown address binding type, expected 'ip:<addr>'".to_string());
    }
}

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
    /// Use the 'ip:' prefix for an IP address (e.g. 'ip:127.0.0.1:6314'), or on Unix systems,
    /// the 'unix:' prefix for a Unix socket path (e.g. 'unix:/run/porxie.sock').
    #[arg(
        id = "SA_ADDRESS",
        long = "server-address",
        env = "PORXIE_SERVER_ADDRESS",
        default_value = "ip:127.0.0.1:6314"
    )]
    address: AddressType,

    /// Bearer token for authenticating admin requests.
    ///
    /// When unset, all authenticated endpoints will reject requests with HTTP 401.
    #[arg(
        id = "SA_SERVER_AUTH_TOKEN",
        long = "server-auth-token",
        env = "PORXIE_SERVER_AUTH_TOKEN"
    )]
    auth_token: Option<String>,
}

#[derive(Args)]
#[command(next_help_heading = "Blob Options")]
struct BlobArgs {
    /// Blob mimetypes that can be served.
    ///
    /// Validation is done loosely via content inference. Further validation can be done by a layer
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
        default_values = ["image/*"],
        value_delimiter = ','
    )]
    allowed_mimetypes: Vec<Mime>,

    /// Maximum blob size that can be fetched and served.
    ///
    /// Blobs that exceed this limit will return HTTP 413. Setting this too high can exhaust
    /// process or system memory. The minimum value is 512kb.
    #[arg(
        id = "BA_BLOB_MAX_SIZE",
        long = "blob-max-size",
        env = "PORXIE_BLOB_MAX_SIZE",
        default_value = "50mb",
        value_parser = |v: &str| -> Result<NonZeroU64, String> {
            let size: ByteSize = v.parse().map_err(|e| format!("{e}"))?;
            if size.as_u64() < 512_000 {
                return Err("minimum allowed value is 512kb".to_string())
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
    max_size: NonZeroU64,

    /// The Cache-Control header value to send alongside blob responses.
    ///
    /// This does not affect internal cache lifetimes, only how downstream clients such as CDNs
    /// and browsers are instructed to cache responses. Intermediary caches may need to be cleared
    /// manually for changes to take effect quickly.
    #[arg(
        id = "BA_BLOB_CACHE_HEADER",
        long = "blob-cache-header",
        env = "PORXIE_BLOB_CACHE_HEADER",
        default_value = "public, max-age=604800, must-revalidate, immutable"
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

    /// Maximum duration before an attempted connection to a blob upstream is aborted.
    ///
    /// This value should be lower than --blob-http-timeout.
    #[arg(
        id = "BA_BLOB_FETCH_CONNECT_TIMEOUT",
        long = "blob-http-connect-timeout",
        env = "PORXIE_BLOB_HTTP_CONNECT_TIMEOUT",
        default_value = "10s"
    )]
    http_connect_timeout: humantime::Duration,
}

#[derive(Args)]
#[command(next_help_heading = "Identity Options")]
struct IdentityArgs {
    /// URL of the PLC instance used for `did:plc` lookups.
    ///
    /// Can typically be left as default unless using a custom or local development setup.
    #[arg(
        id = "IA_PLC_URL",
        long = "identity-plc-url",
        env = "PORXIE_IDENTITY_PLC_URL",
        default_value = "https://plc.directory"
    )]
    plc_url: Url,

    /// Maximum duration before identity resolution requests are timed out.
    #[arg(
        id = "IA_IDENTITY_HTTP_TIMEOUT",
        long = "identity-http-timeout",
        env = "PORXIE_IDENTITY_HTTP_TIMEOUT",
        default_value = "10s"
    )]
    http_timeout: humantime::Duration,

    /// Maximum duration before a connection attempt to an identity upstream is aborted.
    ///
    /// This value should be lower than --identity-http-timeout.
    #[arg(
        id = "IA_IDENTITY_HTTP_CONNECT_TIMEOUT",
        long = "identity-http-connect-timeout",
        env = "PORXIE_IDENTITY_HTTP_CONNECT_TIMEOUT",
        default_value = "8s"
    )]
    http_connect_timeout: humantime::Duration,
}

#[derive(Args)]
#[command(next_help_heading = "Cache Options")]
struct CacheArgs {
    /// Total memory allocation for the internal cache.
    ///
    /// Blobs are cached using an LFU policy. The most frequently requested blobs are kept longest
    /// when the cache approaches its limit.
    ///
    /// For production deployments, a CDN or caching layer in front of this server is recommended
    /// for lower latency and better global availability.
    ///
    /// Setting this too high can exhaust process or system memory. The minimum value is 8mb.
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
    /// Requests are sent as HTTP GET <url>/<did>/<cid>.
    ///
    /// The service is expected to return HTTP 200 (OK) if permitted or HTTP 410 (GONE) if restricted.
    #[arg(id = "PA_POLICY_URL", long = "policy-url", env = "PORXIE_POLICY_URL")]
    url: Option<Url>,

    /// Headers sent alongside all requests to the policy service.
    ///
    /// Each header must be in the format "Name: value". When using the CLI, the flag can be used multiple times.
    /// When setting via environment variable, headers are pipe-separated (|).
    ///
    /// As pipes are used as a delimiter, they cannot be contained in headers.
    ///
    /// Example (cli): '--policy-request-headers "Authorization: Bearer token" --policy-request-headers "X-Api-Key: your-key"'
    ///
    /// Example (env): 'PORXIE_POLICY_REQUEST_HEADERS="Authorization: Bearer token|X-Api-Key: your-key"'
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

    /// Allow requests to proceed if the policy service is unavailable or returns an
    /// unexpected status code.
    ///
    /// Warning: enabling this means restricted blobs may be served when the policy service is unreachable.
    #[arg(
        id = "PA_POLICY_FAIL_OPEN",
        long = "policy-fail-open",
        env = "PORXIE_POLICY_FAIL_OPEN",
        requires = "PA_POLICY_URL"
    )]
    fail_open: bool,

    /// Maximum duration before policy service requests are timed out.
    #[arg(
        id = "PA_POLICY_HTTP_TIMEOUT",
        long = "policy-http-timeout",
        env = "PORXIE_POLICY_HTTP_TIMEOUT",
        default_value = "30s"
    )]
    http_timeout: humantime::Duration,

    /// Maximum duration before an attempted connection to the policy service is aborted.
    ///
    /// This value should be lower than --policy-http-timeout.
    #[arg(
        id = "PA_POLICY_HTTP_CONNECT_TIMEOUT",
        long = "policy-http-connect-timeout",
        env = "PORXIE_POLICY_HTTP_CONNECT_TIMEOUT",
        default_value = "10s"
    )]
    http_connect_timeout: humantime::Duration,
}

struct AppState {
    // Core.
    identity_resolver: JacquardResolver,
    policy_http_client: reqwest::Client,
    blob_fetch_http_client: reqwest::Client,
    cache: Caches,
    // Authentication.
    auth_token: Option<String>,
    // Blob handling.
    allowed_mimetypes: Vec<Mime>,
    max_blob_size: NonZeroU64,
    cache_control_header: HeaderValue,
    // Policy service.
    policy_service_url: Option<Url>,
    policy_service_headers: Vec<(HeaderName, HeaderValue)>,
    policy_service_fail_open: bool,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or(EnvFilter::new("info")))
        .init();
    let args = AppArgs::parse();

    // Setup state.
    let app_state = Arc::new(AppState {
        identity_resolver: JacquardResolver::new(
            build_http_client(
                args.identity.http_timeout.into(),
                args.identity.http_connect_timeout.into(),
                !cfg!(debug_assertions),
            )
            .context("failed to build identity http client")?,
            ResolverOptions {
                plc_source: PlcSource::PlcDirectory {
                    base: args.identity.plc_url,
                },
                public_fallback_for_handle: true,
                validate_doc_id: true,
                request_timeout: Some(args.identity.http_timeout.into()),
                ..Default::default()
            },
        ),
        blob_fetch_http_client: build_http_client(
            args.blob.http_timeout.into(),
            args.blob.http_connect_timeout.into(),
            !cfg!(debug_assertions),
        )
        .context("failed to build blob fetch http client")?,
        policy_http_client: build_http_client(
            args.policy.http_timeout.into(),
            args.policy.http_connect_timeout.into(),
            !cfg!(debug_assertions),
        )
        .context("failed to build policy http client")?,
        cache: build_caches(&CacheBuildOptions {
            memory_capacity: args.cache.size,
            blob_content_ttl: args.cache.blob_tti.into(),
            blob_ownership_ttl: args.cache.ownership_ttl.into(),
            blob_policy_ttl: args.cache.policy_ttl.into(),
            identity_cache_ttl: args.cache.identity_ttl.into(),
        })
        .context("failed to build caches")?,
        auth_token: args.server.auth_token,
        allowed_mimetypes: args.blob.allowed_mimetypes,
        max_blob_size: args.blob.max_size,
        cache_control_header: args.blob.cache_header,
        policy_service_url: args.policy.url,
        policy_service_headers: args.policy.request_headers,
        policy_service_fail_open: args.policy.fail_open,
    });

    // Setup router.
    let router = Router::new()
        .route("/", get(get_index_handler))
        .route(
            "/{did}/{cid}",
            get(get_blob_handler).layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                args.blob.processing_timeout.into(),
            )),
        )
        .route("/cache/{id}", delete(delete_cache_handler))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::default().level(Level::DEBUG))
                .on_response(DefaultOnResponse::default().level(Level::INFO))
                .on_failure(DefaultOnFailure::default().level(Level::ERROR)),
        )
        .layer(NormalizePathLayer::trim_trailing_slash())
        .layer(CatchPanicLayer::new())
        .layer(axum_middleware::from_fn(
            async |req: Request, next: Next| {
                let mut res = next.run(req).await;
                let res_headers = res.headers_mut();
                res_headers.insert(
                    header::SERVER,
                    const { HeaderValue::from_static(env!("CARGO_PKG_NAME")) },
                );
                res_headers.insert("X-Robots-Tag", const { HeaderValue::from_static("none") });
                res
            },
        ))
        .with_state(app_state);

    // Start server listener on specified address.
    match args.server.address {
        AddressType::Ip(ip) => {
            let listener = tokio::net::TcpListener::bind(ip)
                .await
                .context("failed to bind tcp listener")?;
            info!("listening on http://{ip}");
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown_signal())
                .await?;
        }
        #[cfg(unix)]
        AddressType::Unix(path) => {
            let _ = std::fs::remove_file(&path);
            let listener =
                tokio::net::UnixListener::bind(&path).context("failed to bind unix listener")?;
            info!("listening on unix:{}", path.display());
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown_signal())
                .await?;
            let _ = std::fs::remove_file(&path);
        }
    }

    Ok(())
}

// https://github.com/tokio-rs/axum/blob/15917c6dbcb4a48707a20e9cfd021992a279a662/examples/graceful-shutdown/src/main.rs#L55
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
