use crate::AppState;
use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use axum_extra::{
    TypedHeader,
    headers::{Authorization, authorization::Basic},
};
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// Enforce a valid admin XRPC authentication and reject the request if invalid.
///
/// Uses the password configured the Router's [`AppState`].
///
/// Specification: <https://atproto.com/specs/xrpc#admin-token-temporary-specification>.
pub struct AdminXrpcAuth;

impl FromRequestParts<Arc<AppState>> for AdminXrpcAuth {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let Ok(basic_auth) =
            TypedHeader::<Authorization<Basic>>::from_request_parts(parts, state).await
        else {
            return Err(StatusCode::UNAUTHORIZED);
        };

        // Enforce admin as username as per specification.
        if basic_auth.username() != "admin" {
            return Err(StatusCode::UNAUTHORIZED);
        }

        // Check password with a constant time check.
        if !state
            .admin_password
            .as_ref()
            .map(|expected| {
                expected
                    .as_bytes()
                    .ct_eq(basic_auth.password().as_bytes())
                    .into()
            })
            .unwrap_or(false)
        {
            return Err(StatusCode::UNAUTHORIZED);
        }

        Ok(AdminXrpcAuth)
    }
}
