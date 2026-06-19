pub mod dev_blooym;

mod health;
pub use health::xrpc_get_health_handler;

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
