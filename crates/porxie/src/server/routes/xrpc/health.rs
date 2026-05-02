use axum::{
    Json,
    http::{StatusCode, header},
    response::IntoResponse,
};
use serde::Serialize;

use crate::server::routes::CACHE_CONTROL_NOCACHE_VALUE;

#[derive(Serialize)]
struct GetHealthResponse {
    version: &'static str,
}

pub async fn xrpc_get_health_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CACHE_CONTROL, CACHE_CONTROL_NOCACHE_VALUE)],
        Json(GetHealthResponse {
            version: concat!(
                env!("CARGO_PKG_VERSION_MAJOR"),
                ".",
                env!("CARGO_PKG_VERSION_MINOR")
            ),
        }),
    )
}
