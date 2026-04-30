pub mod dev_blooym;
mod health;

pub use health::xrpc_get_health_handler;

pub async fn xrpc_fallback_handler() -> axum::http::StatusCode {
    axum::http::StatusCode::NOT_IMPLEMENTED
}
