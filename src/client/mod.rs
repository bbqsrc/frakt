//! HTTP client implementation
//!
//! This module provides the main [`Client`] for making HTTP requests using NSURLSession.
//! The client supports all standard HTTP methods, authentication, cookies, proxy configuration,
//! and advanced features like WebSocket connections and background downloads.

pub mod background;
pub mod download;
pub mod upload;

pub use background::BackgroundDownloadBuilder;
pub use download::{DownloadBuilder, DownloadResponse};
pub use upload::UploadBuilder;

use crate::session::SessionConfigurationBuilder;
use crate::{Request, RequestBuilder, Result};
use http::Method;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSURLSession;
use std::time::Duration;

/// HTTP client for making requests using NSURLSession.
///
/// The `Client` provides a high-level interface for HTTP operations while leveraging
/// NSURLSession's native performance optimizations including HTTP/2, connection pooling,
/// and automatic compression.
///
/// # Examples
///
/// Basic usage:
/// ```rust,no_run
/// use rsurlsession::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::builder()
///     .user_agent("MyApp/1.0")
///     .timeout(std::time::Duration::from_secs(30))
///     .build()?;
///
/// let response = client
///     .get("https://httpbin.org/json")
///     .header(http::header::ACCEPT, "application/json")?
///     .send()
///     .await?;
///
/// println!("Status: {}", response.status());
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Client {
    session: Retained<NSURLSession>,
    delegate: Retained<crate::delegate::DataTaskDelegate>,
    base_url: Option<String>,
    cookie_jar: Option<crate::CookieJar>,
}

impl Client {
    /// Create a new client with default configuration.
    ///
    /// This is equivalent to `Client::builder().build()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the NSURLSession configuration cannot be created.
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// Create a client builder for configuring the client.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::Client;
    /// use std::time::Duration;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder()
    ///     .user_agent("MyApp/1.0")
    ///     .timeout(Duration::from_secs(30))
    ///     .use_cookies(true)
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Create a GET request to the specified URL.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to send the GET request to
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .get("https://httpbin.org/get")
    ///     .header(http::header::ACCEPT, "application/json")??
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn get(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            Method::GET,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a POST request to the specified URL.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to send the POST request to
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .post("https://httpbin.org/post")
    ///     .header(http::header::CONTENT_TYPE, "application/json")?
    ///     .body(r#"{"key": "value"}"#)
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn post(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            Method::POST,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a PUT request to the specified URL.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to send the PUT request to
    pub fn put(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            Method::PUT,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a DELETE request to the specified URL.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to send the DELETE request to
    pub fn delete(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            Method::DELETE,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a PATCH request to the specified URL.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to send the PATCH request to
    pub fn patch(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            Method::PATCH,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a HEAD request to the specified URL.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to send the HEAD request to
    pub fn head(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            Method::HEAD,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Execute a pre-built request.
    ///
    /// This is equivalent to calling `request.send().await`.
    ///
    /// # Arguments
    ///
    /// * `request` - The request to execute
    pub async fn execute(&self, request: Request) -> Result<crate::Response> {
        request.send().await
    }

    /// Download a file directly to disk using NSURLSessionDownloadTask.
    ///
    /// This method is more efficient for large files as it streams directly to disk
    /// without loading the entire file into memory.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to download from
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .download("https://example.com/file.zip")
    ///     .to_file("./downloads/file.zip")
    ///     .progress(|downloaded, total| {
    ///         println!("Downloaded: {} / {:?} bytes", downloaded, total);
    ///     })
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn download(&self, url: &str) -> DownloadBuilder {
        DownloadBuilder::new(url.to_string(), self.session.clone())
    }

    /// Download a file in the background (continues when app is suspended).
    ///
    /// Background downloads are useful for iOS apps that need to download large files
    /// even when the app is suspended or terminated.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to download from
    pub fn download_background(&self, url: &str) -> BackgroundDownloadBuilder {
        BackgroundDownloadBuilder::new(url.to_string())
    }

    /// Upload a file using NSURLSessionUploadTask.
    ///
    /// This method provides efficient file uploads with progress tracking.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to upload to
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .upload("https://httpbin.org/post")
    ///     .file("./upload.txt")
    ///     .progress(|uploaded, total| {
    ///         println!("Uploaded: {} / {:?} bytes", uploaded, total);
    ///     })
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn upload(&self, url: &str) -> UploadBuilder {
        UploadBuilder::new(url.to_string(), self.session.clone())
    }

    /// Get the cookie jar for this client.
    ///
    /// Returns `None` if cookies are disabled for this client.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::builder()
    ///     .use_cookies(true)
    ///     .build()?;
    ///
    /// if let Some(jar) = client.cookie_jar() {
    ///     let cookies = jar.all_cookies();
    ///     println!("Found {} cookies", cookies.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn cookie_jar(&self) -> Option<&crate::CookieJar> {
        self.cookie_jar.as_ref()
    }

    /// Create a WebSocket connection using NSURLSessionWebSocketTask.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::{Client, Message, CloseCode};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let websocket = client
    ///     .websocket()
    ///     .maximum_message_size(1024 * 1024)
    ///     .connect("wss://echo.websocket.org")
    ///     .await?;
    ///
    /// // Send and receive messages
    /// websocket.send(Message::text("Hello")).await?;
    /// let message = websocket.receive().await?;
    ///
    /// // Close the connection
    /// websocket.close(CloseCode::Normal, Some("Goodbye"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn websocket(&self) -> crate::websocket::WebSocketBuilder {
        crate::websocket::WebSocketBuilder::new(self.session.clone())
    }

    fn resolve_url(&self, url: &str) -> String {
        match &self.base_url {
            Some(base) => {
                if url.starts_with("http://") || url.starts_with("https://") {
                    url.to_string()
                } else {
                    format!(
                        "{}/{}",
                        base.trim_end_matches('/'),
                        url.trim_start_matches('/')
                    )
                }
            }
            None => url.to_string(),
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            self.session.finishTasksAndInvalidate();
        }
    }
}

/// Builder for creating HTTP clients
pub struct ClientBuilder {
    config_builder: SessionConfigurationBuilder,
    base_url: Option<String>,
    cookie_jar: Option<crate::CookieJar>,
}

impl ClientBuilder {
    /// Create a new client builder
    pub fn new() -> Self {
        Self {
            config_builder: SessionConfigurationBuilder::new(),
            base_url: None,
            cookie_jar: None,
        }
    }

    /// Set the base URL for all requests
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set request timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config_builder = self.config_builder.timeout(timeout);
        self
    }

    /// Set user agent
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.config_builder = self.config_builder.user_agent(user_agent);
        self
    }

    /// Add a default header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.config_builder = self.config_builder.header(name, value);
        self
    }

    /// Enable or disable cookies
    pub fn use_cookies(mut self, use_cookies: bool) -> Self {
        self.config_builder = self.config_builder.use_cookies(use_cookies);
        self
    }

    /// Ignore certificate errors (for testing only)
    pub fn ignore_certificate_errors(mut self, ignore: bool) -> Self {
        self.config_builder = self.config_builder.ignore_certificate_errors(ignore);
        self
    }

    /// Set a custom cookie jar
    pub fn cookie_jar(mut self, cookie_jar: crate::CookieJar) -> Self {
        self.cookie_jar = Some(cookie_jar);
        self
    }

    /// Set HTTP proxy
    pub fn http_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        self.config_builder = self.config_builder.http_proxy(host, port);
        self
    }

    /// Set HTTPS proxy
    pub fn https_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        self.config_builder = self.config_builder.https_proxy(host, port);
        self
    }

    /// Set SOCKS proxy
    pub fn socks_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        self.config_builder = self.config_builder.socks_proxy(host, port);
        self
    }

    /// Set proxy authentication
    pub fn proxy_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.config_builder = self.config_builder.proxy_auth(username, password);
        self
    }

    /// Build the client
    pub fn build(self) -> Result<Client> {
        let _should_ignore_certs = self.config_builder.should_ignore_certificate_errors();
        let config = self.config_builder.build()?;

        // Set cookie storage if a custom cookie jar is provided
        if let Some(ref cookie_jar) = self.cookie_jar {
            unsafe {
                config.setHTTPCookieStorage(Some(cookie_jar.storage()));
            }
        }

        // Create delegate and session with delegate
        let delegate = crate::delegate::DataTaskDelegate::new();
        let session = unsafe {
            NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &config,
                Some(ProtocolObject::from_ref(&*delegate)),
                None,
            )
        };

        Ok(Client {
            session,
            delegate,
            base_url: self.base_url,
            cookie_jar: self.cookie_jar,
        })
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}
