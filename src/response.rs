//! Response handling

use crate::{Error, Result};
use objc2::rc::Retained;
use objc2_foundation::{NSHTTPURLResponse, NSURLResponse};
use std::collections::HashMap;
use std::sync::Arc;

/// HTTP response from an NSURLSession request.
///
/// This struct represents an HTTP response received from a server. It provides methods
/// to access the response status, headers, body content, and other metadata.
///
/// # Examples
///
/// ```rust
/// use rsurlsession::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let response = client.get("https://api.example.com/data").send().await?;
///
/// // Check response status
/// println!("Status: {}", response.status());
/// println!("Success: {}", response.is_success());
///
/// // Access headers
/// if let Some(content_type) = response.header("content-type") {
///     println!("Content-Type: {}", content_type);
/// }
///
/// // Read response body
/// let body = response.text().await?;
/// println!("Body: {}", body);
/// # Ok(())
/// # }
/// ```
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

    /// Get the HTTP response status code.
    ///
    /// Returns the HTTP status code for the response. For non-HTTP responses,
    /// this method returns 200 by default.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client.get("https://httpbin.org/status/404").send().await?;
    /// assert_eq!(response.status(), 404);
    /// # Ok(())
    /// # }
    /// ```
    pub fn status(&self) -> u16 {
        if let Some(http_response) = self.http_response() {
            unsafe { http_response.statusCode() as u16 }
        } else {
            200 // Non-HTTP responses default to 200
        }
    }

    /// Check if the response status indicates success (2xx).
    ///
    /// Returns `true` if the status code is in the 200-299 range.
    pub fn is_success(&self) -> bool {
        let status = self.status();
        (200..300).contains(&status)
    }

    /// Check if the response status indicates a client error (4xx).
    ///
    /// Returns `true` if the status code is in the 400-499 range.
    pub fn is_client_error(&self) -> bool {
        let status = self.status();
        (400..500).contains(&status)
    }

    /// Check if the response status indicates a server error (5xx).
    ///
    /// Returns `true` if the status code is in the 500-599 range.
    pub fn is_server_error(&self) -> bool {
        let status = self.status();
        (500..600).contains(&status)
    }

    /// Get the expected content length from headers.
    ///
    /// Returns the expected content length if known from the `Content-Length` header
    /// or if NSURLSession can determine it. Returns `None` if the length is unknown
    /// or if this is a non-HTTP response.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client.get("https://httpbin.org/get").send().await?;
    ///
    /// if let Some(length) = response.content_length() {
    ///     println!("Expected {} bytes", length);
    /// } else {
    ///     println!("Content length unknown");
    /// }
    /// # Ok(())
    /// # }
    /// ```
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

    /// Get a specific header value by name.
    ///
    /// Returns the value of the specified header if it exists. Header names are
    /// case-insensitive. Returns `None` if the header is not present.
    ///
    /// # Arguments
    ///
    /// * `name` - The header name to look up
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client.get("https://httpbin.org/get").send().await?;
    ///
    /// if let Some(content_type) = response.header("content-type") {
    ///     println!("Content-Type: {}", content_type);
    /// }
    ///
    /// if let Some(server) = response.header("server") {
    ///     println!("Server: {}", server);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers().get(name).cloned()
    }

    /// Get all response headers as a HashMap.
    ///
    /// Returns a HashMap containing all HTTP response headers. The keys are header names
    /// and values are header values. For non-HTTP responses, returns an empty HashMap.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client.get("https://httpbin.org/get").send().await?;
    ///
    /// for (name, value) in response.headers() {
    ///     println!("{}: {}", name, value);
    /// }
    /// # Ok(())
    /// # }
    /// ```
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
                                if let Some(value_str) =
                                    value.downcast_ref::<objc2_foundation::NSString>()
                                {
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

    /// Get the final response URL.
    ///
    /// Returns the final URL after any redirects. This may be different from the
    /// original request URL if the server sent redirect responses.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client.get("https://httpbin.org/redirect/1").send().await?;
    ///
    /// if let Some(final_url) = response.url() {
    ///     println!("Final URL: {}", final_url);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn url(&self) -> Option<String> {
        unsafe {
            self.response.URL().and_then(|url| {
                url.absoluteString().map(|abs_str| {
                    objc2::rc::autoreleasepool(|pool| abs_str.to_str(pool).to_string())
                })
            })
        }
    }

    /// Get the MIME type of the response.
    ///
    /// Returns the MIME type (Content-Type) of the response as determined by NSURLResponse.
    /// This may be derived from the Content-Type header or inferred from the response data.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client.get("https://httpbin.org/json").send().await?;
    ///
    /// if let Some(mime_type) = response.content_type() {
    ///     println!("MIME type: {}", mime_type);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn content_type(&self) -> Option<String> {
        unsafe {
            self.response
                .MIMEType()
                .map(|mime| objc2::rc::autoreleasepool(|pool| mime.to_str(pool).to_string()))
        }
    }

    /// Consume the response and return the body as bytes.
    ///
    /// This method reads the entire response body into a `Vec<u8>`. The response
    /// is consumed and cannot be used again after this call.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client.get("https://httpbin.org/bytes/1024").send().await?;
    ///
    /// let bytes = response.bytes().await?;
    /// println!("Received {} bytes", bytes.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn bytes(self) -> Result<Vec<u8>> {
        self.task_context.take_response_buffer().await
    }

    /// Consume the response and return the body as text.
    ///
    /// This method reads the entire response body and attempts to decode it as UTF-8 text.
    /// The response is consumed and cannot be used again after this call.
    ///
    /// # Errors
    ///
    /// Returns an error if the response body is not valid UTF-8.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client.get("https://httpbin.org/get").send().await?;
    ///
    /// let text = response.text().await?;
    /// println!("Response body: {}", text);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn text(self) -> Result<String> {
        let bytes = self.bytes().await?;
        String::from_utf8(bytes).map_err(Error::from)
    }

    /// Consume the response and parse the body as JSON.
    ///
    /// This method reads the entire response body and attempts to deserialize it as JSON
    /// into the specified type. The response is consumed and cannot be used again after this call.
    /// This feature requires the "json" feature flag.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The type to deserialize the JSON into. Must implement `serde::Deserialize`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The response body cannot be read
    /// - The response body is not valid JSON
    /// - The JSON cannot be deserialized into type `T`
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct ApiResponse {
    ///     origin: String,
    ///     url: String,
    /// }
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client.get("https://httpbin.org/get").send().await?;
    ///
    /// let data: ApiResponse = response.json().await?;
    /// println!("Origin: {}", data.origin);
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "json")]
    pub async fn json<T: serde::de::DeserializeOwned>(self) -> Result<T> {
        let bytes = self.bytes().await?;
        serde_json::from_slice(&bytes).map_err(Error::from)
    }

    /// Create a streaming reader for the response body.
    ///
    /// This method returns a [`ResponseStream`] that implements [`tokio::io::AsyncRead`],
    /// allowing you to read the response body in chunks instead of loading it entirely
    /// into memory. This is particularly useful for large responses.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    /// use tokio::io::AsyncReadExt;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client.get("https://httpbin.org/stream/10").send().await?;
    ///
    /// let mut stream = response.stream();
    /// let mut buffer = [0u8; 1024];
    ///
    /// while let bytes_read = stream.read(&mut buffer).await? {
    ///     if bytes_read == 0 { break; }
    ///     // Process chunk
    ///     println!("Read {} bytes", bytes_read);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn stream(self) -> ResponseStream {
        ResponseStream::new(self.task_context)
    }

    fn http_response(&self) -> Option<&NSHTTPURLResponse> {
        self.response.downcast_ref::<NSHTTPURLResponse>()
    }
}

/// Streaming response reader that implements [`tokio::io::AsyncRead`].
///
/// This struct provides a streaming interface for reading response bodies in chunks,
/// which is memory-efficient for large responses. It implements the tokio AsyncRead trait,
/// allowing it to be used with any tokio I/O utilities.
///
/// # Examples
///
/// ```rust
/// use rsurlsession::Client;
/// use tokio::io::AsyncReadExt;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let response = client.get("https://httpbin.org/stream/5").send().await?;
///
/// let mut stream = response.stream();
/// let mut buffer = Vec::new();
///
/// // Read the entire stream into the buffer
/// stream.read_to_end(&mut buffer).await?;
/// println!("Read {} total bytes", buffer.len());
/// # Ok(())
/// # }
/// ```
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
        let (has_data, to_copy) =
            if let Ok(shared_buffer) = self.task_context.response_buffer.try_lock() {
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
