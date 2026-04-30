use axum::{
    extract::Request,
    http::{HeaderValue, header},
    middleware::Next,
    response::Response,
};

pub async fn server_headers_middleware(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let res_headers = res.headers_mut();
    res_headers.insert(
        header::SERVER,
        const { HeaderValue::from_static(env!("CARGO_PKG_NAME")) },
    );
    res_headers.insert("X-Robots-Tag", const { HeaderValue::from_static("none") });
    res
}
