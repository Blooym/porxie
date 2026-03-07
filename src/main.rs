mod cache;
mod http;
mod mime;
mod routes;

use crate::{
    cache::{CacheBuildOptions, Caches, build_caches},
    http::{build_external_http_client, build_internal_http_client},
    routes::{delete_cache_handler, get_blob_handler, get_index_handler},
};
use ::mime::Mime;
use anyhow::Result;
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
use std::{net::SocketAddr, num::NonZeroU64, sync::Arc, time::Duration};
use tokio::{net::TcpListener, signal};
use tower_http::{
    catch_panic::CatchPanicLayer,
    normalize_path::NormalizePathLayer,
    timeout::TimeoutLayer,
    trace::{self, DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, info};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[clap(
    author,
    about,
    version,
    after_help = "* Use '--help' for additional information.",
    after_long_help = concat!("* Refer to the project README found at ", env!("CARGO_PKG_REPOSITORY"), " for more guidance.")
)]
struct AppArgs {
    /// Socket address (IPv4 or IPv6) to bind the server to.
    #[arg(
        long = "address",
        env = "PORXIE_ADDRESS",
        default_value = "127.0.0.1:6314"
    )]
    address: SocketAddr,

    /// Timeout applied to incoming requests.
    #[arg(
        long = "request-timeout",
        env = "PORXIE_REQUEST_TIMEOUT",
        default_value = "2m"
    )]
    timeout: humantime::Duration,

    /// Bearer token for authenticating admin requests.
    ///
    /// When unset, all authenticated endpoints will reject requests with HTTP 401.
    #[arg(long = "auth-token", env = "PORXIE_AUTH_TOKEN")]
    auth_token: Option<String>,

    /// List of mimetypes that can be served.
    ///
    /// Validation is done loosely via content inference, further validation can be done by using another
    /// layer that strictly processes content above this proxy, such as an image transformation service.
    ///
    /// By default only image and video are allowed.
    /// Unknown blobs fall back to `application/octet-stream`, which also has to be explicitly enabled.
    ///
    /// When using the CLI, the flag can be used multiple times. When setting via environment variable,
    /// values are comma-separated (e.g. `PORXIE_ALLOWED_MIMETYPES="video/*,image/*"`).
    #[arg(
        long = "allowed-mimetypes",
        env = "PORXIE_ALLOWED_MIMETYPES",
        default_values = ["video/*", "image/*"],
        value_delimiter = ','
    )]
    allowed_mimetypes: Vec<Mime>,

    /// Maximum blob size that can be served.
    ///
    /// Blobs that exceed this limit will return an HTTP 413 error.
    ///
    /// Be aware that setting this value too high can lead to the process or system running out of memory, so adjust accordingly.
    /// The minimum max blob size is 512kb.
    #[arg(
        long = "max-blob-size",
        env = "PORXIE_MAX_BLOB_SIZE",
        default_value = "50mb",
        value_parser = |v: &str| -> Result<ByteSize, String> {
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

            Ok(size)
        }
    )]
    max_blob_size: ByteSize,

    /// URL of the PLC directory instance used for `did:plc` lookups.
    ///
    /// Can typically be left as default unless using a custom or test directory.
    #[arg(
        long = "plc-directory-url",
        env = "PORXIE_PLC_DIRECTORY_URL",
        default_value = "https://plc.directory"
    )]
    plc_directory_url: Url,

    /// HTTP(S) or SOCKS5(h) proxy for upstream requests. Supports embedded credentials (e.g. http://user:pass@host).
    ///
    /// When unset, the system's proxy configuration is used automatically.
    #[arg(long = "upstream-proxy", env = "PORXIE_UPSTREAM_PROXY", value_parser = |v: &str| {
        let url = Url::parse(v).map_err(|e| e.to_string())?;
        if !matches!(url.scheme(), "http" | "https" |"socks5" | "socks5h") {
            return Err("proxy URL must use http://, https://, socks5://, or socks5h://".to_string());
        }
        Ok(url)
    })]
    upstream_proxy: Option<Url>,

    /// Maximum duration before upstream requests are timed out.
    ///
    /// This value should be lower than --request-timeout to allow time for error handling.
    #[arg(
        long = "upstream-timeout",
        env = "PORXIE_UPSTREAM_TIMEOUT",
        default_value = "30s"
    )]
    upstream_timeout: humantime::Duration,

    #[command(flatten)]
    cache: CacheArgs,

    #[command(flatten)]
    policy: PolicyServiceArgs,
}

#[derive(Args)]
#[command(next_help_heading = "Cache Options")]
struct CacheArgs {
    /// Total memory allocation for the internal cache.
    ///
    /// Blobs are cached using an LFU policy, the most frequently requested blobs will be kept the longest
    /// when the cache begins to exceed its maximum size.
    ///
    /// You may wish to adjust this to fit your needs.
    ///
    /// It is recommended to use a CDN or caching layer in front of Porxie for production deployments for lower
    /// latency, better global availability and better response caching.
    ///
    /// Be aware that setting this value too high can lead to the process or system running out of memory, so adjust accordingly.
    /// The minimum cache size is 8mb.
    #[arg(
        long = "cache-size",
        env = "PORXIE_CACHE_SIZE",
        default_value = "512mb",
        value_parser = |v: &str| -> Result<ByteSize, String> {
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

            Ok(size)
        }
    )]
    size: ByteSize,

    /// How long fetched blobs are cached before expiring.
    #[arg(
        long = "blob-cache-ttl",
        env = "PORXIE_BLOB_CACHE_TTL",
        default_value = "7days"
    )]
    content_ttl: humantime::Duration,

    /// How long blob ownership data is cached before being re-checked.
    #[arg(
        long = "ownership-cache-ttl",
        env = "PORXIE_OWNERSHIP_CACHE_TTL",
        default_value = "1day"
    )]
    ownership_ttl: humantime::Duration,

    /// How long policy decisions are cached before being re-checked.
    #[arg(
        long = "policy-cache-ttl",
        env = "PORXIE_POLICY_CACHE_TTL",
        default_value = "1h"
    )]
    policy_ttl: humantime::Duration,

    /// The Cache-Control header value to send alongside responses.
    ///
    /// This header does not modify internal cache lifetimes, only how other clients are instructed to cache responses
    /// (such as CDNs and browsers). You should adjust this according to your own infrastructure needs.
    ///
    /// Be aware that you may also need to clear intermediary caches manually if you want a policy change to apply quickly.
    #[arg(
        long = "cache-control-header",
        env = "PORXIE_CACHE_CONTROL_HEADER",
        default_value = "public, max-age=604800, must-revalidate, immutable"
    )]
    cache_control_header_value: HeaderValue,
}

#[derive(Args)]
#[command(next_help_heading = "Policy Service Options")]
struct PolicyServiceArgs {
    /// Policy service URL that DID+CID pairs will be checked against.
    ///
    /// Requests are sent as HTTP GET <url>/<did>/<cid>.
    ///
    /// The service is expected to return HTTP 200 (OK) if permitted or HTTP 410 (GONE) if restricted.
    #[arg(long = "policy-service-url", env = "PORXIE_POLICY_SERVICE_URL")]
    url: Option<Url>,

    /// Headers sent alongside all requests to the policy service.
    ///
    /// Each header must be in the format "Name: value". When using the CLI, the flag can be used multiple times.
    /// When setting via environment variable, headers are pipe-separated (|).
    ///
    /// As pipes are used as a delimiter, they cannot be contained in headers.
    ///
    /// Example (cli): '--policy-service-headers "Authorization: Bearer token" --policy-service-headers "X-Api-Key: your-key"'
    ///
    /// Example (env): 'PORXIE_POLICY_SERVICE_HEADERS="Authorization: Bearer token|X-Api-Key: your-key"'
    #[arg(
        long = "policy-service-headers",
        env = "PORXIE_POLICY_SERVICE_HEADERS",
        value_delimiter = '|',
        requires = "url",
        value_parser = |v: &str| -> Result<(HeaderName, HeaderValue), String> {
            let (name, value) = v.split_once(':')
                .ok_or_else(|| format!("invalid header {v:?}: expected 'Name: value'"))?;
            let name = HeaderName::try_from(name.trim())
                .map_err(|e| format!("invalid header name in {v:?}: {e}"))?;
            let mut value = HeaderValue::try_from(value.trim())
                .map_err(|e| format!("invalid header value in {v:?}: {e}"))?;
            value.set_sensitive(true);
            Ok((name, value))
        }
    )]
    headers: Vec<(HeaderName, HeaderValue)>,

    /// Allow requests to proceed if the policy service is unavailable or returns an
    /// unexpected status code.
    ///
    /// Warning: enabling this means restricted blobs may be served when the policy service is unreachable.
    #[arg(
        long = "policy-service-fail-open",
        env = "PORXIE_POLICY_SERVICE_FAIL_OPEN",
        requires = "url"
    )]
    fail_open: bool,
}

struct AppState {
    // Core.
    identity_resolver: JacquardResolver,
    internal_http_client: reqwest::Client,
    external_http_client: reqwest::Client,
    cache: Caches,
    // Authentication.
    auth_token: Option<String>,
    // Blobs handling.
    allowed_mimetypes: Vec<Mime>,
    max_blob_size: NonZeroU64,
    cache_control_header: HeaderValue,
    // Policy service.
    policy_service_url: Option<Url>,
    policy_service_headers: Vec<(HeaderName, HeaderValue)>,
    policy_service_fail_open: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or(EnvFilter::new("info")))
        .init();
    let args = AppArgs::parse();

    // Setup state.
    let external_http_client =
        build_external_http_client(args.upstream_timeout.into(), args.upstream_proxy)?;
    let app_state = Arc::new(AppState {
        identity_resolver: JacquardResolver::new(
            external_http_client.clone(),
            ResolverOptions {
                plc_source: PlcSource::PlcDirectory {
                    base: args.plc_directory_url,
                },
                public_fallback_for_handle: true,
                validate_doc_id: true,
                ..Default::default()
            },
        ),
        external_http_client,
        internal_http_client: build_internal_http_client(Duration::from_secs(15))?,
        cache: build_caches(&CacheBuildOptions {
            memory_capacity: args.cache.size.as_u64(),
            blob_content_ttl: args.cache.content_ttl.into(),
            blob_ownership_ttl: args.cache.ownership_ttl.into(),
            blob_policy_ttl: args.cache.policy_ttl.into(),
        })?,
        auth_token: args.auth_token,
        allowed_mimetypes: args.allowed_mimetypes,
        max_blob_size: args.max_blob_size.as_u64().try_into()?,
        cache_control_header: args.cache.cache_control_header_value,
        policy_service_url: args.policy.url,
        policy_service_headers: args.policy.headers,
        policy_service_fail_open: args.policy.fail_open,
    });

    // Setup router.
    let router = Router::new()
        .route("/", get(get_index_handler))
        .route("/{did}/{cid}", get(get_blob_handler))
        .route("/cache/{id}", delete(delete_cache_handler))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::default().level(Level::INFO))
                .on_response(DefaultOnResponse::default().level(Level::INFO))
                .on_failure(DefaultOnFailure::default()),
        )
        .layer(NormalizePathLayer::trim_trailing_slash())
        .layer(CatchPanicLayer::new())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            args.timeout.into(),
        ))
        .layer(axum_middleware::from_fn(
            async |req: Request, next: Next| {
                const SERVER_HV: HeaderValue = HeaderValue::from_static(env!("CARGO_PKG_NAME"));
                const ROBOTS_HV: HeaderValue = HeaderValue::from_static("none");
                let mut res = next.run(req).await;
                let res_headers = res.headers_mut();
                res_headers.insert(header::SERVER, SERVER_HV);
                res_headers.insert("X-Robots-Tag", ROBOTS_HV);
                res
            },
        ))
        .with_state(app_state);

    // Start server.
    let tcp_listener = TcpListener::bind(args.address).await?;
    info!(
        "Internal server started - listening on: http://{}",
        args.address,
    );
    axum::serve(tcp_listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

// https://github.com/tokio-rs/axum/blob/15917c6dbcb4a48707a20e9cfd021992a279a662/examples/graceful-shutdown/src/main.rs#L55
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
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
