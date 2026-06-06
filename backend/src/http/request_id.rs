use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

use axum::{extract::Request, http::header::HeaderValue, middleware::Next, response::Response};

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct RequestId(String);

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub async fn attach(mut request: Request, next: Next) -> Response {
    let request_id = RequestId(format!(
        "req_{:016x}",
        NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    ));

    request.extensions_mut().insert(request_id.clone());

    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&request_id.to_string()) {
        response.headers_mut().insert("x-request-id", value);
    }

    response
}
