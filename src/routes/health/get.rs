use crate::routes::CACHE_CONTROL_NOCACHE_VALUE;
use axum::{
    Json,
    http::{StatusCode, header},
    response::IntoResponse,
};
use serde::Serialize;

#[derive(Serialize)]
struct GetHealthResponse {
    version: &'static str,
}

pub async fn get_health_handler() -> impl IntoResponse {
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
