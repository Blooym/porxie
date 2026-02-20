use reqwest::{Proxy, Url, redirect::Policy};
use std::time::Duration;

const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
const MAX_REDIRECTS: usize = 5;

pub fn build_internal_http_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(Policy::limited(MAX_REDIRECTS))
        .build()
}

pub fn build_external_http_client(
    https_only: bool,
    upstream_timeout: Duration,
    proxy_url: Option<Url>,
) -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .https_only(https_only)
        .redirect(Policy::limited(MAX_REDIRECTS))
        .timeout(upstream_timeout);

    if let Some(proxy) = proxy_url {
        builder = builder.proxy(Proxy::all(proxy)?);
    };

    builder.build()
}
