use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::header::{HeaderName, HeaderValue};
use hyper::Method;
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use tokio::time::timeout;

type HyperClient = Client<hyper_rustls::HttpsConnector<HttpConnector>, Full<Bytes>>;

#[derive(Clone)]
pub struct HttpClient {
    inner: HyperClient,
    default_timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("timeout")]
    Timeout,
    #[error("errore di rete: {0}")]
    Network(String),
    #[error("errore costruzione request: {0}")]
    Build(String),
}

impl HttpError {
    pub fn is_timeout(&self) -> bool {
        matches!(self, HttpError::Timeout)
    }
}

impl HttpClient {
    pub fn new(default_timeout: Duration) -> Self {
        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .expect("TLS roots non trovati")
            .https_or_http()
            .enable_http1()
            .build();

        let inner = Client::builder(TokioExecutor::new()).build(https);

        Self { inner, default_timeout }
    }

    pub async fn post_raw(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
        request_timeout: Duration,
    ) -> Result<(u16, String), HttpError> {
        let mut builder = hyper::Request::builder()
            .method(Method::POST)
            .uri(url);

        for (k, v) in headers {
            builder = builder.header(
                HeaderName::from_bytes(k.as_bytes())
                    .map_err(|e| HttpError::Build(e.to_string()))?,
                HeaderValue::from_str(v)
                    .map_err(|e| HttpError::Build(e.to_string()))?,
            );
        }

        let req = builder
            .body(Full::new(Bytes::from(body)))
            .map_err(|e| HttpError::Build(e.to_string()))?;

        let t = if request_timeout.is_zero() { self.default_timeout } else { request_timeout };

        let resp = timeout(t, self.inner.request(req))
            .await
            .map_err(|_| HttpError::Timeout)?
            .map_err(|e| HttpError::Network(e.to_string()))?;

        let status = resp.status().as_u16();
        let bytes = resp
            .into_body()
            .collect()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?
            .to_bytes();

        Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
    }
}