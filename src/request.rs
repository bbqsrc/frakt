//! Request types and builders

use crate::delegate::shared_context::ProgressCallback;
use crate::{Error, Result, body::Body};
use objc2::rc::Retained;
use objc2_foundation::{NSMutableURLRequest, NSString, NSURL, NSURLSession};
use std::collections::HashMap;

/// HTTP request methods.
///
/// This enum represents all standard HTTP methods plus support for custom methods.
/// All methods are supported by NSURLSession and can be used with any request type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    /// GET method - retrieve data from the server
    GET,
    /// POST method - send data to the server
    POST,
    /// PUT method - update or create a resource
    PUT,
    /// DELETE method - delete a resource
    DELETE,
    /// PATCH method - partially update a resource
    PATCH,
    /// HEAD method - retrieve headers only
    HEAD,
    /// Custom HTTP method
    Custom(String),
}

impl Method {
    /// Convert the HTTP method to a string representation.
    pub fn as_str(&self) -> &str {
        match self {
            Method::GET => "GET",
            Method::POST => "POST",
            Method::PUT => "PUT",
            Method::DELETE => "DELETE",
            Method::PATCH => "PATCH",
            Method::HEAD => "HEAD",
            Method::Custom(method) => method,
        }
    }
}

/// An HTTP request ready to be executed.
///
/// This represents a fully configured HTTP request that can be sent using NSURLSession.
/// Requests are typically created through the [`RequestBuilder`] using methods on [`Client`].
///
/// [`Client`]: crate::Client
pub struct Request {
    pub(crate) method: Method,
    pub(crate) url: String,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) body: Option<Body>,
    pub(crate) session: Retained<NSURLSession>,
    pub(crate) delegate: Retained<crate::delegate::DataTaskDelegate>,
    pub(crate) progress_callback: Option<std::sync::Arc<ProgressCallback>>,
}

impl Request {
    /// Send the request and get a response.
    ///
    /// This method executes the HTTP request using NSURLSession and returns the response.
    /// The request is sent asynchronously and the future can be awaited.
    ///
    /// # Returns
    ///
    /// Returns a [`Result`] containing the [`Response`] on success, or an [`Error`] on failure.
    ///
    /// # Errors
    ///
    /// This method can fail with various errors including:
    /// - Network connectivity issues
    /// - Invalid URLs
    /// - Timeout errors
    /// - Server errors
    ///
    /// [`Response`]: crate::Response
    /// [`Error`]: crate::Error
    pub async fn send(self) -> Result<crate::Response> {
        // Create NSURLRequest
        let nsurl = unsafe {
            NSURL::URLWithString(&NSString::from_str(&self.url)).ok_or(Error::InvalidUrl)?
        };

        let nsrequest = unsafe {
            let req = NSMutableURLRequest::requestWithURL(&nsurl);
            req.setHTTPMethod(&NSString::from_str(self.method.as_str()));

            // Set headers
            for (name, value) in &self.headers {
                req.setValue_forHTTPHeaderField(
                    Some(&NSString::from_str(value)),
                    &NSString::from_str(name),
                );
            }

            // Set body
            if let Some(body) = &self.body {
                match body {
                    Body::Empty => {}
                    Body::Bytes {
                        content,
                        content_type,
                    } => {
                        req.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str(content_type)),
                            &NSString::from_str("Content-Type"),
                        );
                        let nsdata = objc2_foundation::NSData::from_vec(content.to_vec());
                        req.setHTTPBody(Some(&nsdata));
                    }
                    Body::Form { fields } => {
                        req.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str("application/x-www-form-urlencoded")),
                            &NSString::from_str("Content-Type"),
                        );
                        let encoded = encode_form_fields(fields);
                        let nsdata = objc2_foundation::NSData::from_vec(encoded.into_bytes());
                        req.setHTTPBody(Some(&nsdata));
                    }
                    #[cfg(feature = "json")]
                    Body::Json { value } => {
                        req.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str("application/json")),
                            &NSString::from_str("Content-Type"),
                        );
                        let json_bytes = serde_json::to_vec(value)?;
                        let nsdata = objc2_foundation::NSData::from_vec(json_bytes);
                        req.setHTTPBody(Some(&nsdata));
                    }
                    #[cfg(feature = "multipart")]
                    Body::Multipart { parts } => {
                        let boundary = generate_boundary();
                        let content_type = format!("multipart/form-data; boundary={}", boundary);
                        req.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str(&content_type)),
                            &NSString::from_str("Content-Type"),
                        );
                        let multipart_data = encode_multipart_data(&boundary, parts)?;
                        let nsdata = objc2_foundation::NSData::from_vec(multipart_data);
                        req.setHTTPBody(Some(&nsdata));
                    }
                }
            }

            req
        };

        // Create task context
        let task_context = if let Some(progress_callback) = self.progress_callback {
            std::sync::Arc::new(crate::delegate::TaskSharedContext::with_progress_callback(
                progress_callback,
            ))
        } else {
            std::sync::Arc::new(crate::delegate::TaskSharedContext::new())
        };

        // Create data task
        let data_task = unsafe { self.session.dataTaskWithRequest(&nsrequest) };

        // Register the task context with the delegate using the task identifier
        let task_id = unsafe { data_task.taskIdentifier() } as usize;
        self.delegate.register_task(task_id, task_context.clone());

        // Create response future
        let response_future = ResponseFuture::new(data_task, task_context, self.delegate);

        unsafe {
            response_future.data_task.resume();
        }

        response_future.await
    }
}

/// Builder for constructing HTTP requests.
///
/// `RequestBuilder` provides a fluent interface for configuring HTTP requests before sending them.
/// It supports setting headers, request bodies, authentication, and progress callbacks.
///
/// # Examples
///
/// Basic request with headers:
/// ```rust
/// use rsurlsession::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let response = client
///     .get("https://api.example.com/data")
///     .header("Accept", "application/json")
///     .header("User-Agent", "MyApp/1.0")
///     .send()
///     .await?;
/// # Ok(())
/// # }
/// ```
///
/// POST request with JSON body:
/// ```rust
/// use rsurlsession::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let response = client
///     .post("https://api.example.com/users")
///     .header("Content-Type", "application/json")
///     .body(r#"{"name": "John", "email": "john@example.com"}"#)
///     .send()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct RequestBuilder {
    method: Method,
    url: String,
    headers: HashMap<String, String>,
    body: Option<Body>,
    session: Retained<NSURLSession>,
    delegate: Retained<crate::delegate::DataTaskDelegate>,
    progress_callback: Option<std::sync::Arc<ProgressCallback>>,
}

impl RequestBuilder {
    pub(crate) fn new(
        method: Method,
        url: String,
        session: Retained<NSURLSession>,
        delegate: Retained<crate::delegate::DataTaskDelegate>,
    ) -> Self {
        Self {
            method,
            url,
            headers: HashMap::new(),
            body: None,
            session,
            delegate,
            progress_callback: None,
        }
    }

    /// Add a header to the request.
    ///
    /// This method adds a single header field to the request. Headers can be called multiple
    /// times to add different header fields. If the same header name is used multiple times,
    /// the last value will be used.
    ///
    /// # Arguments
    ///
    /// * `name` - The header name (e.g., "Content-Type", "Authorization")
    /// * `value` - The header value
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .get("https://api.example.com/data")
    ///     .header("Accept", "application/json")
    ///     .header("Authorization", "Bearer token123")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set the request body.
    ///
    /// This method sets the request body using any type that can be converted into a [`Body`].
    /// Convenient `From` implementations are provided for common types like `String`, `&str`,
    /// and `Vec<u8>`.
    ///
    /// # Arguments
    ///
    /// * `body` - The request body content
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    ///
    /// // String body
    /// let response = client
    ///     .post("https://api.example.com/data")
    ///     .body("Hello, World!")
    ///     .send()
    ///     .await?;
    ///
    /// // Binary body
    /// let data = vec![1, 2, 3, 4];
    /// let response = client
    ///     .post("https://api.example.com/upload")
    ///     .body(data)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`Body`]: crate::Body
    pub fn body(mut self, body: impl Into<Body>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set a JSON body from a serializable value.
    ///
    /// This method serializes the provided value to JSON and sets it as the request body
    /// with the appropriate `Content-Type` header. This feature requires the "json" feature flag.
    ///
    /// # Arguments
    ///
    /// * `value` - Any value that implements `serde::Serialize`
    ///
    /// # Errors
    ///
    /// Returns an error if the value cannot be serialized to JSON.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    /// use serde_json::json;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .post("https://api.example.com/users")
    ///     .json(json!({
    ///         "name": "John Doe",
    ///         "email": "john@example.com"
    ///     }))?
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "json")]
    pub fn json(mut self, value: impl serde::Serialize) -> Result<Self> {
        self.body = Some(Body::json(value)?);
        Ok(self)
    }

    /// Set a form-urlencoded body from field/value pairs.
    ///
    /// This method creates a form-urlencoded request body from a list of field/value pairs
    /// and sets the appropriate `Content-Type` header to `application/x-www-form-urlencoded`.
    ///
    /// # Arguments
    ///
    /// * `fields` - A vector of (field, value) tuples to encode as form data
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .post("https://api.example.com/login")
    ///     .form(vec![
    ///         ("username", "john_doe"),
    ///         ("password", "secret123"),
    ///     ])
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn form(
        mut self,
        fields: Vec<(
            impl Into<std::borrow::Cow<'static, str>>,
            impl Into<std::borrow::Cow<'static, str>>,
        )>,
    ) -> Self {
        self.body = Some(Body::form(fields));
        self
    }

    /// Set a plain text body.
    ///
    /// This method sets the request body to plain text with the `Content-Type` header
    /// set to `text/plain; charset=utf-8`.
    ///
    /// # Arguments
    ///
    /// * `content` - The text content to use as the request body
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .post("https://api.example.com/notes")
    ///     .text("This is a plain text note")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn text(mut self, content: impl Into<String>) -> Self {
        self.body = Some(Body::text(content));
        self
    }

    /// Set authentication for the request.
    ///
    /// This method adds the appropriate `Authorization` header based on the authentication
    /// method provided. Supported authentication types include Basic, Bearer, and Custom.
    ///
    /// # Arguments
    ///
    /// * `auth` - The authentication method to use
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::{Client, Auth};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    ///
    /// // Basic authentication
    /// let response = client
    ///     .get("https://api.example.com/protected")
    ///     .auth(Auth::Basic {
    ///         username: "user".to_string(),
    ///         password: "pass".to_string(),
    ///     })
    ///     .send()
    ///     .await?;
    ///
    /// // Bearer token
    /// let response = client
    ///     .get("https://api.example.com/data")
    ///     .auth(Auth::Bearer {
    ///         token: "your-token".to_string(),
    ///     })
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn auth(mut self, auth: crate::Auth) -> Self {
        self.headers
            .insert("Authorization".to_string(), auth.to_header_value());
        self
    }

    /// Set a progress callback for tracking download progress.
    ///
    /// This method sets a callback function that will be called periodically during
    /// the request to report download progress. The callback receives the number of
    /// bytes downloaded so far and the total expected bytes (if known).
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that takes (downloaded_bytes, total_bytes) parameters
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .get("https://example.com/large-file.zip")
    ///     .progress(|downloaded, total| {
    ///         if let Some(total) = total {
    ///             let percent = (downloaded as f64 / total as f64) * 100.0;
    ///             println!("Downloaded: {:.1}%", percent);
    ///         } else {
    ///             println!("Downloaded: {} bytes", downloaded);
    ///         }
    ///     })
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(u64, Option<u64>) + Send + Sync + 'static,
    {
        self.progress_callback = Some(std::sync::Arc::new(callback));
        self
    }

    /// Send the request and return the response.
    ///
    /// This method executes the configured HTTP request using NSURLSession.
    /// The request is sent asynchronously and this method returns a future
    /// that resolves to the response.
    ///
    /// # Returns
    ///
    /// Returns a [`Result`] containing the [`Response`] on success, or an [`Error`] on failure.
    ///
    /// # Errors
    ///
    /// This method can fail with various errors including:
    /// - Network connectivity issues
    /// - Invalid URLs or malformed requests
    /// - Timeout errors
    /// - Server errors (4xx, 5xx status codes are still successful responses)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .get("https://api.example.com/data")
    ///     .header("Accept", "application/json")
    ///     .send()
    ///     .await?;
    ///
    /// println!("Status: {}", response.status());
    /// let body = response.text().await?;
    /// println!("Body: {}", body);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`Response`]: crate::Response
    /// [`Error`]: crate::Error
    pub async fn send(self) -> Result<crate::Response> {
        let request = Request {
            method: self.method,
            url: self.url,
            headers: self.headers,
            body: self.body,
            session: self.session,
            delegate: self.delegate,
            progress_callback: self.progress_callback,
        };
        request.send().await
    }
}

/// Future for handling response
struct ResponseFuture {
    data_task: Retained<objc2_foundation::NSURLSessionDataTask>,
    task_context: std::sync::Arc<crate::delegate::TaskSharedContext>,
    _delegate: Retained<crate::delegate::DataTaskDelegate>,
}

impl ResponseFuture {
    fn new(
        data_task: Retained<objc2_foundation::NSURLSessionDataTask>,
        task_context: std::sync::Arc<crate::delegate::TaskSharedContext>,
        delegate: Retained<crate::delegate::DataTaskDelegate>,
    ) -> Self {
        Self {
            data_task,
            task_context,
            _delegate: delegate,
        }
    }
}

impl std::future::Future for ResponseFuture {
    type Output = Result<crate::Response>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.task_context.is_completed() {
            // Check for errors
            if let Some(error) = self.task_context.error.load_full() {
                return std::task::Poll::Ready(Err(Error::from_ns_error(&*error)));
            }

            // Get response
            if let Some(response) = self.task_context.response.load_full() {
                let response = crate::Response::new((*response).clone(), self.task_context.clone());
                return std::task::Poll::Ready(Ok(response));
            }

            return std::task::Poll::Ready(Err(Error::Internal(
                "No response received".to_string(),
            )));
        }

        // Register waker
        let waker = cx.waker().clone();
        let task_context = self.task_context.clone();
        tokio::spawn(async move {
            task_context.waker.register(waker).await;
        });

        std::task::Poll::Pending
    }
}

fn encode_form_fields(
    fields: &[(
        std::borrow::Cow<'static, str>,
        std::borrow::Cow<'static, str>,
    )],
) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(feature = "multipart")]
fn generate_boundary() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("----formdata-rsurlsession-{}", timestamp)
}

#[cfg(feature = "multipart")]
fn encode_multipart_data(boundary: &str, parts: &[crate::body::MultipartPart]) -> Result<Vec<u8>> {
    let mut data = Vec::new();

    for part in parts {
        data.extend_from_slice(format!("\r\n--{}\r\n", boundary).as_bytes());

        let mut disposition = format!("Content-Disposition: form-data; name=\"{}\"", part.name);
        if let Some(filename) = &part.filename {
            disposition.push_str(&format!("; filename=\"{}\"", filename));
        }
        data.extend_from_slice(disposition.as_bytes());
        data.extend_from_slice(b"\r\n");

        if let Some(content_type) = &part.content_type {
            data.extend_from_slice(format!("Content-Type: {}\r\n", content_type).as_bytes());
        }

        data.extend_from_slice(b"\r\n");
        data.extend_from_slice(&part.content);
    }

    data.extend_from_slice(format!("\r\n--{}--\r\n", boundary).as_bytes());
    Ok(data)
}
