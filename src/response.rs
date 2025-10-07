//! Response types with backend abstraction

use std::fmt::Debug;

use crate::backend::types::BackendResponse;
use crate::{Error, Result};
use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use tokio::sync::mpsc;
use url::Url;

/// HTTP response from any backend
pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    url: Url,
    body_receiver: mpsc::Receiver<Result<Bytes>>,
}

impl Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("url", &self.url)
            .field("headers", &self.headers)
            .finish()
    }
}

impl Response {
    /// Create a Response from a BackendResponse
    pub(crate) fn from_backend(backend_response: BackendResponse) -> Self {
        Self {
            status: backend_response.status,
            headers: backend_response.headers,
            url: backend_response.url,
            body_receiver: backend_response.body_receiver,
        }
    }

    /// Get the status code
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Get the URL that was requested
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Get the headers
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get a specific header value
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)?.to_str().ok()
    }

    /// Check if the response status indicates success (2xx)
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Check if the response status indicates a client error (4xx)
    pub fn is_client_error(&self) -> bool {
        self.status.is_client_error()
    }

    /// Check if the response status indicates a server error (5xx)
    pub fn is_server_error(&self) -> bool {
        self.status.is_server_error()
    }

    /// Consume the response and return the body as bytes
    pub async fn bytes(mut self) -> Result<Bytes> {
        let mut body = Vec::new();

        while let Some(chunk) = self.body_receiver.recv().await {
            let chunk = chunk?;
            body.extend_from_slice(&chunk);
        }

        Ok(Bytes::from(body))
    }

    /// Consume the response and return the body as text
    pub async fn text(self) -> Result<String> {
        let bytes = self.bytes().await?;
        String::from_utf8(bytes.to_vec()).map_err(Error::Utf8)
    }

    /// Consume the response and deserialize the body as JSON
    pub async fn json<T: serde::de::DeserializeOwned>(self) -> Result<T> {
        let bytes = self.bytes().await?;
        serde_json::from_slice(&bytes).map_err(|x| Error::Json(x.to_string()))
    }

    /// Get the response body as a stream of bytes
    pub fn stream(self) -> ResponseStream {
        ResponseStream {
            receiver: self.body_receiver,
            current_chunk: None,
            chunk_offset: 0,
        }
    }
}

/// Stream of response body bytes
pub struct ResponseStream {
    receiver: mpsc::Receiver<Result<Bytes>>,
    current_chunk: Option<Bytes>,
    chunk_offset: usize,
}

impl ResponseStream {
    /// Get the next chunk of bytes
    pub async fn next(&mut self) -> Option<Result<Bytes>> {
        self.receiver.recv().await
    }
}

impl futures_util::Stream for ResponseStream {
    type Item = Result<Bytes>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;

        match self.receiver.poll_recv(cx) {
            Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl tokio::io::AsyncRead for ResponseStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::task::Poll;

        loop {
            // If we have a current chunk, try to read from it
            if let Some(ref chunk) = self.current_chunk {
                let remaining = chunk.len() - self.chunk_offset;
                if remaining > 0 {
                    let to_copy = std::cmp::min(remaining, buf.remaining());
                    let start = self.chunk_offset;
                    let end = start + to_copy;
                    buf.put_slice(&chunk[start..end]);
                    self.chunk_offset += to_copy;
                    return Poll::Ready(Ok(()));
                } else {
                    // Current chunk is exhausted
                    self.current_chunk = None;
                    self.chunk_offset = 0;
                }
            }

            // Try to get the next chunk
            match self.receiver.poll_recv(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    self.current_chunk = Some(chunk);
                    self.chunk_offset = 0;
                    // Continue loop to read from this chunk
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::Other, e)));
                }
                Poll::Ready(None) => {
                    // Stream ended
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}
