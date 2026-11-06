//! [`WorkerHttp`]: the `HttpClient` port over `fetch`.

use async_trait::async_trait;
use harpseal::Result;
use worker::js_sys::Uint8Array;
use worker::send::SendFuture;
use worker::{Fetch, Headers, Method, Request, RequestInit};

use crate::platform;

/// Outgoing HTTP through `fetch`. Each call is one subrequest (Workers
/// Paid allows 1000 per request, Free 50).
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkerHttp;

#[async_trait]
impl harpseal::HttpClient for WorkerHttp {
    async fn send(&self, req: http::Request<Vec<u8>>) -> Result<http::Response<Vec<u8>>> {
        SendFuture::new(async move {
            let (parts, body) = req.into_parts();
            let headers = Headers::new();
            for (k, v) in &parts.headers {
                let v = v.to_str().map_err(platform)?;
                headers.append(k.as_str(), v).map_err(platform)?;
            }
            let mut init = RequestInit::new();
            init.with_method(Method::from(parts.method.to_string()))
                .with_headers(headers);
            if !body.is_empty() {
                init.with_body(Some(Uint8Array::from(body.as_slice()).into()));
            }
            let req = Request::new_with_init(&parts.uri.to_string(), &init).map_err(platform)?;
            let mut resp = Fetch::Request(req).send().await.map_err(platform)?;
            let mut out = http::Response::builder().status(resp.status_code());
            for (k, v) in resp.headers().entries() {
                out = out.header(k, v);
            }
            let bytes = resp.bytes().await.map_err(platform)?;
            out.body(bytes).map_err(platform)
        })
        .await
    }
}
