mod extractors;
mod middlewares;
mod routes;
mod socket_address;

pub use socket_address::SocketAddress;

use crate::{
    blob_service::BlobService,
    identity_service::IdentityService,
    policy_client::PolicyClient,
    server::{
        middlewares::server_headers_middleware,
        routes::{
            get_blob_handler, get_index_handler,
            xrpc::{
                dev_blooym::porxie::{
                    cache::{xrpc_cache_purge_actor_handler, xrpc_cache_purge_blob_handler},
                    xrpc_compat_get_blob_handler, xrpc_get_blob_metadata_handler,
                },
                xrpc_fallback_handler, xrpc_get_health_handler,
            },
        },
    },
};
use anyhow::Context;
use axum::{
    Router,
    http::{HeaderValue, StatusCode},
    middleware::{self as axum_middleware},
    routing::{any, get, post},
};
use core::{num::NonZeroU64, time::Duration};
use porxie_mediautil::deps::mime::Mime;
use std::sync::Arc;
use tower_http::{
    catch_panic::CatchPanicLayer,
    cors::CorsLayer,
    normalize_path::NormalizePathLayer,
    timeout::TimeoutLayer,
    trace::{self, DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

struct ServerState {
    admin_password: Option<String>,
    allowed_mimetypes: Vec<Mime>,
    blob_service: BlobService,
    cache_control_header: HeaderValue,
    identity_service: IdentityService,
    max_blob_size: NonZeroU64,
    policy_client: Option<PolicyClient>,
    policy_fail_open: bool,
}

pub struct PorxieServerOptions {
    pub admin_password: Option<String>,
    pub allowed_mimetypes: Vec<Mime>,
    pub blob_processing_timeout: Duration,
    pub blob_service: BlobService,
    pub cache_control_header: HeaderValue,
    pub identity_service: IdentityService,
    pub max_blob_size: NonZeroU64,
    pub policy_client: Option<PolicyClient>,
    pub policy_fail_open: bool,
}

pub struct PorxieServer {
    router: Router,
}

impl PorxieServer {
    /// Create a new Porxie server.
    pub fn new(options: PorxieServerOptions) -> Self {
        let router = Router::new()
            .route("/", get(get_index_handler))
            .route(
                "/{did}/{cid}",
                get(get_blob_handler).layer(TimeoutLayer::with_status_code(
                    StatusCode::REQUEST_TIMEOUT,
                    options.blob_processing_timeout,
                )),
            )
            .nest(
                "/xrpc",
                Router::new()
                    .route("/_health", get(xrpc_get_health_handler))
                    .route(
                        "/dev.blooym.porxie.getBlob",
                        get(xrpc_compat_get_blob_handler).layer(TimeoutLayer::with_status_code(
                            StatusCode::REQUEST_TIMEOUT,
                            options.blob_processing_timeout,
                        )),
                    )
                    .route(
                        "/dev.blooym.porxie.getBlobMetadata",
                        get(xrpc_get_blob_metadata_handler).layer(TimeoutLayer::with_status_code(
                            StatusCode::REQUEST_TIMEOUT,
                            options.blob_processing_timeout,
                        )),
                    )
                    .route(
                        "/dev.blooym.porxie.cache.purgeActor",
                        post(xrpc_cache_purge_actor_handler),
                    )
                    .route(
                        "/dev.blooym.porxie.cache.purgeBlob",
                        post(xrpc_cache_purge_blob_handler),
                    )
                    // Ensure /xrpc/... routes don't fall through elsewhere.
                    .route("/{rest}", any(xrpc_fallback_handler)),
            )
            .layer(CatchPanicLayer::new())
            .layer(NormalizePathLayer::trim_trailing_slash())
            .layer(axum_middleware::from_fn(server_headers_middleware))
            .layer(CorsLayer::permissive())
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                    .on_request(DefaultOnRequest::default().level(Level::DEBUG))
                    .on_response(DefaultOnResponse::default().level(Level::INFO))
                    .on_failure(DefaultOnFailure::default().level(Level::ERROR)),
            )
            .with_state(Arc::new(ServerState {
                admin_password: options.admin_password,
                allowed_mimetypes: options.allowed_mimetypes,
                blob_service: options.blob_service,
                cache_control_header: options.cache_control_header,
                identity_service: options.identity_service,
                max_blob_size: options.max_blob_size,
                policy_client: options.policy_client,
                policy_fail_open: options.policy_fail_open,
            }));

        Self { router }
    }

    /// Start server listener on specified address.
    pub async fn start<F: Future<Output = ()> + Send + 'static>(
        self,
        address: SocketAddress,
        shutdown_signal: F,
    ) -> anyhow::Result<()> {
        match address {
            SocketAddress::Ip(ip) => {
                let listener = tokio::net::TcpListener::bind(ip)
                    .await
                    .context("failed to bind tcp listener")?;
                tracing::info!("server listening on http://{ip}");
                axum::serve(listener, self.router)
                    .with_graceful_shutdown(shutdown_signal)
                    .await?;
                Ok(())
            }
            #[cfg(unix)]
            SocketAddress::Unix(path) => {
                use anyhow::Context;

                let _ = std::fs::remove_file(&path);
                let listener = tokio::net::UnixListener::bind(&path)
                    .context("failed to bind unix listener")?;
                tracing::info!("server listening on unix:{}", path.display());
                axum::serve(listener, self.router)
                    .with_graceful_shutdown(shutdown_signal)
                    .await?;
                let _ = std::fs::remove_file(&path);
                Ok(())
            }
        }
    }
}
