//! Response handling

use crate::{Error, Result};
use objc2::rc::Retained;
use objc2_foundation::{NSHTTPURLResponse, NSURLResponse};
use std::collections::HashMap;
use std::sync::Arc;

/// HTTP response
pub struct Response {
    response: Retained<NSURLResponse>,
    task_context: Arc<crate::delegate::TaskSharedContext>,
}

impl Response {
    pub(crate) fn new(
        response: Retained<NSURLResponse>,
        task_context: Arc<crate::delegate::TaskSharedContext>,
    ) -> Self {
        Self {
            response,
            task_context,
        }
    }

    /// Get the response status code
    pub fn status(&self) -> u16 {
        if let Some(http_response) = self.http_response() {
            unsafe { http_response.statusCode() as u16 }
        } else {
            200 // Non-HTTP responses default to 200
        }
    }

    /// Check if the response status indicates success (2xx)
    pub fn is_success(&self) -> bool {
        let status = self.status();
        (200..300).contains(&status)
    }

    /// Check if the response status indicates a client error (4xx)
    pub fn is_client_error(&self) -> bool {
        let status = self.status();
        (400..500).contains(&status)
    }

    /// Check if the response status indicates a server error (5xx)
    pub fn is_server_error(&self) -> bool {
        let status = self.status();
        (500..600).contains(&status)
    }

    /// Get the content length from headers
    pub fn content_length(&self) -> Option<u64> {
        if let Some(http_response) = self.http_response() {
            unsafe {
                let length = http_response.expectedContentLength();
                if length >= 0 {
                    Some(length as u64)
                } else {
                    None
                }
            }
        } else {
            None
        }
    }

    /// Get a header value
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers().get(name).cloned()
    }

    /// Get all headers
    pub fn headers(&self) -> HashMap<String, String> {
        if let Some(http_response) = self.http_response() {
            unsafe {
                let headers_dict = http_response.allHeaderFields();
                let mut result = HashMap::new();

                objc2::rc::autoreleasepool(|pool| {
                    // Get all keys and iterate through them
                    let keys = headers_dict.allKeys();
                    for i in 0..keys.count() {
                        let key = keys.objectAtIndex(i);
                        if let Some(key_str) = key.downcast_ref::<objc2_foundation::NSString>() {
                            if let Some(value) = headers_dict.objectForKey(&key) {
                                if let Some(value_str) = value.downcast_ref::<objc2_foundation::NSString>() {
                                    let key_string = key_str.to_str(pool).to_string();
                                    let value_string = value_str.to_str(pool).to_string();
                                    result.insert(key_string, value_string);
                                }
                            }
                        }
                    }
                });

                result
            }
        } else {
            HashMap::new()
        }
    }

    /// Get the response URL
    pub fn url(&self) -> Option<String> {
        unsafe {
            self.response.URL().and_then(|url| {
                url.absoluteString().map(|abs_str| {
                    objc2::rc::autoreleasepool(|pool| abs_str.to_str(pool).to_string())
                })
            })
        }
    }

    /// Get the MIME type
    pub fn content_type(&self) -> Option<String> {
        unsafe {
            self.response.MIMEType().map(|mime| {
                objc2::rc::autoreleasepool(|pool| mime.to_str(pool).to_string())
            })
        }
    }

    /// Consume the response and return the body as bytes
    pub async fn bytes(self) -> Result<Vec<u8>> {
        self.task_context.take_response_buffer().await
    }

    /// Consume the response and return the body as text
    pub async fn text(self) -> Result<String> {
        let bytes = self.bytes().await?;
        String::from_utf8(bytes).map_err(Error::from)
    }

    /// Consume the response and parse the body as JSON
    #[cfg(feature = "json")]
    pub async fn json<T: serde::de::DeserializeOwned>(self) -> Result<T> {
        let bytes = self.bytes().await?;
        serde_json::from_slice(&bytes).map_err(Error::from)
    }

    /// Create a streaming reader for the response body
    pub fn stream(self) -> ResponseStream {
        ResponseStream::new(self.task_context)
    }

    fn http_response(&self) -> Option<&NSHTTPURLResponse> {
        self.response.downcast_ref::<NSHTTPURLResponse>()
    }
}

/// Streaming response reader
pub struct ResponseStream {
    task_context: Arc<crate::delegate::TaskSharedContext>,
    bytes_read: usize,
    finished: bool,
}

impl ResponseStream {
    fn new(task_context: Arc<crate::delegate::TaskSharedContext>) -> Self {
        Self {
            task_context,
            bytes_read: 0,
            finished: false,
        }
    }
}

impl tokio::io::AsyncRead for ResponseStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.finished {
            return std::task::Poll::Ready(Ok(()));
        }

        // Try to read data from the shared response buffer
        let (has_data, to_copy) = if let Ok(shared_buffer) = self.task_context.response_buffer.try_lock() {
            let available_data = shared_buffer.len().saturating_sub(self.bytes_read);

            if available_data > 0 {
                // We have new data to read
                let to_copy = std::cmp::min(available_data, buf.remaining());
                let start_pos = self.bytes_read;
                let end_pos = start_pos + to_copy;

                buf.put_slice(&shared_buffer[start_pos..end_pos]);
                (true, to_copy)
            } else {
                (false, 0)
            }
        } else {
            (false, 0)
        };

        if has_data {
            self.bytes_read += to_copy;
            return std::task::Poll::Ready(Ok(()));
        }

        // Check if task is completed
        if self.task_context.is_completed() {
            self.finished = true;
            return std::task::Poll::Ready(Ok(()));
        }

        // No data available and task not completed, register waker
        let waker = cx.waker().clone();
        let task_context = self.task_context.clone();
        tokio::spawn(async move {
            task_context.waker.register(waker).await;
        });

        std::task::Poll::Pending
    }
}