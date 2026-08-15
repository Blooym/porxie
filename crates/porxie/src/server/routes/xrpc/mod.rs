pub mod dev_blooym;
mod health;

use axum::{body::Body, http::Response, response::IntoResponse};
pub use health::xrpc_get_health_handler;

use core::any::Any;
use jacquard_axum::GenericXrpcErrorResponse;
use reqwest::StatusCode;

pub async fn xrpc_fallback_handler() -> GenericXrpcErrorResponse {
    GenericXrpcErrorResponse::new(
        axum::http::StatusCode::NOT_IMPLEMENTED,
        "MethodNotImplemented",
        Some("XRPC Method Not Implemented"),
    )
}

pub async fn xrpc_nonspec_method_handler() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

pub fn xrpc_internal_error_panic_handler(_err: Box<dyn Any + Send + 'static>) -> Response<Body> {
    GenericXrpcErrorResponse::new(
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "InternalServerError",
        Some("Internal server error"),
    )
    .into_response()
}
