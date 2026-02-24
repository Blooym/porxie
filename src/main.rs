mod cache;
mod http;
mod mime;
mod routes;

use crate::{
    cache::{PolicyCache, ResponseCache, build_policy_cache, build_response_cache},
    http::{build_external_http_client, build_internal_http_client},
    routes::{delete_blob_handler, get_blob_handler, get_index_handler},
};
use ::mime::{Mime, STAR_STAR};
use anyhow::Result;
use axum::{
    Router,
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::{self as axum_middleware, Next},
    routing::{delete, get},
};
use bytesize::ByteSize;
use clap::Parser;
use dotenvy::dotenv;
use jacquard_identity::{
    JacquardResolver,
    resolver::{PlcSource, ResolverOptions},
};
use reqwest::Url;
use std::{net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, signal};
use tower_http::{
    catch_panic::CatchPanicLayer,
    normalize_path::NormalizePathLayer,
    timeout::TimeoutLayer,
    trace::{self, DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::{Level, info};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Parser)]
#[clap(author, about, long_about, version)]
struct Arguments {
    /// Socket address to bind the server to.
    #[arg(
        long = "address",
        env = "PORXIE_ADDRESS",
        default_value = "127.0.0.1:6314"
    )]
    address: SocketAddr,

    /// Maximum duration before incoming requests are timed out.
    #[arg(long = "timeout", env = "PORXIE_TIMEOUT", default_value = "60s")]
    timeout: humantime::Duration,

    /// Bearer token required to authenticate admin requests.
    ///
    /// When unset, all authenticated endpoints are unusable.
    #[arg(long = "auth-token", env = "PORXIE_AUTH_TOKEN")]
    auth_token: Option<String>,

    /// List of mimetypes that can be served through this CDN.
    ///
    /// Validation is done loosely via content inference and is not foolproof.
    /// It is recommended to apply a sandboxed layer that will process the blob
    /// further to validate its type.
    #[arg(
        long = "allowed-mimetypes",
        env = "PORXIE_ALLOWED_MIMETYPES",
        default_values_t = [STAR_STAR],
        value_delimiter = ','
    )]
    allowed_mimetypes: Vec<Mime>,

    /// The cache-control header value to send alongside responses.
    ///
    /// This header does not modify the internal cache lifetime of content, only how it instructs
    /// other clients to cache responses.
    #[arg(
        long = "cache-control-header",
        env = "PORXIE_CACHE_CONTROL_HEADER",
        default_value = "public, max-age=604800, must-revalidate"
    )]
    cache_control_header_value: HeaderValue,

    /// Maximum size of cached responses in memory.
    ///
    /// Content is evicted using a TinyLFU policy that automatically prioritises the most
    /// frequently requested keys.
    ///
    /// It is recommended you deploy a dedicated caching service in front of this service for the
    /// best cache performance. The built-in cache is optimised for handling frequent requests and bursts
    /// requesting the same content.
    ///
    /// The default value is conservatively low; you may wish to raise it to fit your needs.
    #[arg(
        long = "cache-size",
        env = "PORXIE_CACHE_SIZE",
        default_value = "512mb"
    )]
    cache_size: ByteSize,

    /// Maximum blob size that can be served through this CDN.
    ///
    /// Content that exceeds this limit will return an HTTP 422 error.
    #[arg(
        long = "max-blob-size",
        env = "PORXIE_MAX_BLOB_SIZE",
        default_value = "50mb"
    )]
    max_blob_size: ByteSize,

    /// Maximum size of cached policy decisions in memory.
    ///
    /// Each entry is lightweight, so small allocations can hold a large number of entries.
    #[arg(
        long = "policy-cache-size",
        env = "PORXIE_POLICY_CACHE_SIZE",
        default_value = "256mb"
    )]
    policy_cache_size: ByteSize,

    /// How long policy decisions are cached before being re-checked.
    #[arg(
        long = "policy-cache-ttl",
        env = "PORXIE_POLICY_CACHE_TTL",
        default_value = "1h"
    )]
    policy_cache_ttl: humantime::Duration,

    /// Authorization bearer token sent alongside all requests to the policy service.
    #[arg(
        long = "policy-service-auth-token",
        env = "PORXIE_POLICY_SERVICE_AUTH_TOKEN",
        requires = "policy_service_url"
    )]
    policy_service_auth_token: Option<String>,

    /// Whether to allow requests to proceed if the policy service is unavailable or returns an
    /// unexpected status code.
    #[arg(
        long = "policy-service-fail-open",
        env = "PORXIE_POLICY_SERVICE_FAIL_OPEN",
        default_value_t = false,
        requires = "policy_service_url"
    )]
    policy_service_fail_open: core::primitive::bool,

    /// URL of an upstream policy service that DID+CID pairs will be checked against.
    ///
    /// Requests are sent as HTTP GET <url>/<did>/<cid>.
    ///
    /// The service is expected to return HTTP 200 (OK) if permitted or HTTP 410 (GONE) if restricted.
    #[arg(long = "policy-service-url", env = "PORXIE_POLICY_SERVICE_URL")]
    policy_service_url: Option<Url>,

    /// URL of the PLC directory instance used for `did:plc` lookups.
    ///
    /// Can typically be left as default unless using a custom or test directory.
    #[arg(
        long = "plc-directory-url",
        env = "PORXIE_PLC_DIRECTORY_URL",
        default_value = "https://plc.directory"
    )]
    plc_directory_url: Url,

    /// Only allow HTTPS when connecting to upstreams.
    ///
    /// Disabling this is strongly discouraged.
    #[arg(
        long = "upstream-https-only",
        env = "PORXIE_UPSTREAM_HTTPS_ONLY",
        default_value_t = true
    )]
    upstream_https_only: core::primitive::bool,

    /// HTTP(S) proxy for upstream requests. Supports embedded credentials (https://user:pass@host).
    ///
    /// When unset, the system proxy configuration is used automatically.
    #[arg(long = "upstream-proxy", env = "PORXIE_UPSTREAM_PROXY", value_parser = |v: &str| {
        let url = Url::parse(v).map_err(|e| e.to_string())?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("proxy URL must use http:// or https://".to_string());
        }
        Ok(url)
    })]
    upstream_proxy: Option<Url>,

    /// Maximum duration before upstream requests are timed out.
    #[arg(
        long = "upstream-timeout",
        env = "PORXIE_UPSTREAM_TIMEOUT",
        default_value = "30s"
    )]
    upstream_timeout: humantime::Duration,
}

struct AppState {
    // Internals..
    resolver: JacquardResolver,
    internal_http_client: reqwest::Client,
    external_http_client: reqwest::Client,
    // Auth.
    auth_token: Option<String>,
    // Content.
    allowed_mimetypes: Vec<Mime>,
    max_blob_size: u64,
    cache_control_header: HeaderValue,
    response_cache: ResponseCache,
    // Policy.
    policy_service_url: Option<Url>,
    policy_service_auth_header: Option<HeaderValue>,
    policy_service_fail_open: bool,
    policy_cache: PolicyCache,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or(EnvFilter::new("info")))
        .init();
    let args = Arguments::parse();

    // Setup state.
    let external_http_client = build_external_http_client(
        args.upstream_https_only,
        args.upstream_timeout.into(),
        args.upstream_proxy,
    )?;
    let app_state = Arc::new(AppState {
        resolver: JacquardResolver::new(
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
        internal_http_client: build_internal_http_client()?,
        auth_token: args.auth_token,
        allowed_mimetypes: args.allowed_mimetypes,
        max_blob_size: args.max_blob_size.as_u64(),
        cache_control_header: args.cache_control_header_value,
        response_cache: build_response_cache(args.cache_size.as_u64()),
        policy_service_url: args.policy_service_url,
        policy_service_auth_header: args.policy_service_auth_token.map(|token| {
            let mut header = HeaderValue::from_str(&format!("Bearer {token}"))
                .expect("policy service token should be a valid header value");
            header.set_sensitive(true);
            header
        }),
        policy_service_fail_open: args.policy_service_fail_open,
        policy_cache: build_policy_cache(
            args.policy_cache_size.as_u64(),
            args.policy_cache_ttl.into(),
        ),
    });

    // Setup router.
    let router = Router::new()
        .route("/", get(get_index_handler))
        .route("/{did}/{cid}", get(get_blob_handler))
        .route("/{did}/{cid}", delete(delete_blob_handler))
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
                let mut res = next.run(req).await;
                let res_headers = res.headers_mut();
                res_headers.insert(
                    header::SERVER,
                    HeaderValue::from_static(env!("CARGO_PKG_NAME")),
                );
                res_headers.insert("X-Robots-Tag", HeaderValue::from_static("none"));
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
