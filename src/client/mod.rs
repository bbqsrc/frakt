//! Client implementation using backend abstraction

// TODO: Re-implement these with backend abstraction
// pub mod background;
// pub mod download;
// pub mod upload;

// pub use background::BackgroundDownloadBuilder;
// pub use download::{DownloadBuilder, DownloadResponse};
// pub use upload::UploadBuilder;

/// Download builder for downloading files to disk
pub struct DownloadBuilder {
    backend: Backend,
    url: String,
    file_path: Option<std::path::PathBuf>,
    progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
}

impl DownloadBuilder {
    /// Create a new download builder (internal use)
    pub(crate) fn new(backend: Backend, url: String) -> Self {
        Self {
            backend,
            url,
            file_path: None,
            progress_callback: None,
        }
    }

    /// Set destination file path
    pub fn to_file<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.file_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set progress callback
    pub fn progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(u64, Option<u64>) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Box::new(callback));
        self
    }

    /// Start the download
    pub async fn send(self) -> crate::Result<DownloadResponse> {
        let file_path = self.file_path.ok_or_else(|| {
            crate::Error::Internal("Download file path not specified".to_string())
        })?;

        // Create a GET request using the backend
        let request = crate::backend::types::BackendRequest {
            method: http::Method::GET,
            url: self.url,
            headers: http::HeaderMap::new(),
            body: None,
        };

        let response = self.backend.execute(request).await?;

        // Stream response body to file
        let mut file = tokio::fs::File::create(&file_path)
            .await
            .map_err(|e| crate::Error::Internal(format!("Failed to create file: {}", e)))?;

        let mut receiver = response.body_receiver;
        let mut bytes_downloaded = 0u64;

        // Get content length for progress callback
        let total_bytes = response.headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        while let Some(chunk_result) = receiver.recv().await {
            let chunk = chunk_result?;
            bytes_downloaded += chunk.len() as u64;

            // Call progress callback if provided
            if let Some(ref callback) = self.progress_callback {
                callback(bytes_downloaded, total_bytes);
            }

            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(|e| crate::Error::Internal(format!("Failed to write to file: {}", e)))?;
        }

        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .map_err(|e| crate::Error::Internal(format!("Failed to flush file: {}", e)))?;

        Ok(DownloadResponse {
            file_path,
            bytes_downloaded,
        })
    }
}

/// Background download builder for downloads that continue when app is suspended
pub struct BackgroundDownloadBuilder {
    backend: Backend,
    url: String,
    session_identifier: Option<String>,
    file_path: Option<std::path::PathBuf>,
    progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
}

impl BackgroundDownloadBuilder {
    /// Create a new background download builder (internal use)
    pub(crate) fn new(backend: Backend, url: String) -> Self {
        Self {
            backend,
            url,
            session_identifier: None,
            file_path: None,
            progress_callback: None,
        }
    }

    /// Set session identifier for background downloads
    pub fn session_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.session_identifier = Some(identifier.into());
        self
    }

    /// Set destination file path
    pub fn to_file<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.file_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set progress callback
    pub fn progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(u64, Option<u64>) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Box::new(callback));
        self
    }

    /// Start the background download
    pub async fn send(self) -> crate::Result<DownloadResponse> {
        let file_path = self.file_path.clone().ok_or_else(|| {
            crate::Error::Internal("Background download file path not specified".to_string())
        })?;

        match &self.backend {
            #[cfg(target_vendor = "apple")]
            Backend::Foundation(_) => {
                self.send_foundation_background(file_path).await
            }
            Backend::Reqwest(_) => {
                // Reqwest doesn't support true background downloads, fall back to regular download
                let mut download_builder = DownloadBuilder::new(self.backend, self.url);
                download_builder = download_builder.to_file(&file_path);
                if let Some(callback) = self.progress_callback {
                    download_builder = download_builder.progress(callback);
                }
                download_builder.send().await
            }
        }
    }

    #[cfg(target_vendor = "apple")]
    async fn send_foundation_background(self, file_path: std::path::PathBuf) -> crate::Result<DownloadResponse> {
        // For background downloads, we need to create a background session
        use crate::backend::foundation::delegate::background_session::BackgroundSessionDelegate;
        use crate::backend::foundation::delegate::shared_context::TaskSharedContext;
        use objc2_foundation::{NSURLSessionConfiguration, NSURLSession, NSString, NSURL};
        use objc2::runtime::ProtocolObject;
        use std::sync::Arc;

        // Create background session configuration
        let session_id = self.session_identifier.unwrap_or_else(|| {
            format!("rsurlsession-bg-{}", std::process::id())
        });

        let session_config = unsafe {
            NSURLSessionConfiguration::backgroundSessionConfigurationWithIdentifier(
                &NSString::from_str(&session_id)
            )
        };

        // Create background delegate
        let delegate = BackgroundSessionDelegate::new();
        let session = unsafe {
            NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &session_config,
                Some(ProtocolObject::from_ref(&*delegate)),
                None,
            )
        };

        // Create download task
        let nsurl = unsafe {
            NSURL::URLWithString(&NSString::from_str(&self.url))
                .ok_or(crate::Error::InvalidUrl)?
        };

        let download_task = unsafe { session.downloadTaskWithURL(&nsurl) };
        let task_id = unsafe { download_task.taskIdentifier() } as usize;

        // Create task context
        let mut task_context = if let Some(callback) = self.progress_callback {
            TaskSharedContext::with_progress_callback(Arc::new(callback))
        } else {
            TaskSharedContext::new()
        };

        task_context.download_context = Some(Arc::new(
            crate::backend::foundation::delegate::shared_context::DownloadContext::new(Some(file_path.clone()))
        ));

        let task_context = Arc::new(task_context);

        // Register task with delegate
        delegate.register_task(task_id, task_context.clone());

        // Start download
        unsafe {
            download_task.resume();
        }

        // Wait for completion
        while !task_context.is_completed() {
            tokio::task::yield_now().await;
        }

        // Check for errors
        if let Some(error) = task_context.error.load_full() {
            return Err(crate::Error::from_ns_error(&*error));
        }

        // Calculate bytes downloaded
        let bytes_downloaded = task_context.bytes_downloaded.load(std::sync::atomic::Ordering::Relaxed);

        Ok(DownloadResponse {
            file_path,
            bytes_downloaded,
        })
    }
}

/// Placeholder for upload builder - needs backend abstraction
pub struct UploadBuilder {
    backend: Backend,
    url: String,
    body: Option<crate::body::Body>,
    headers: http::HeaderMap,
    file_path: Option<(std::path::PathBuf, String)>, // (path, content_type)
}

impl UploadBuilder {
    /// Create a new upload builder (internal use)
    pub(crate) fn new(backend: Backend, url: String) -> Self {
        Self {
            backend,
            url,
            body: None,
            headers: http::HeaderMap::new(),
            file_path: None,
        }
    }

    /// Upload from file
    pub fn from_file<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        let path = path.as_ref().to_path_buf();

        // Determine content type from file extension
        let content_type = match path.extension().and_then(|ext| ext.to_str()) {
            Some("txt") => "text/plain",
            Some("json") => "application/json",
            Some("xml") => "application/xml",
            Some("html") => "text/html",
            Some("css") => "text/css",
            Some("js") => "application/javascript",
            Some("pdf") => "application/pdf",
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("zip") => "application/zip",
            _ => "application/octet-stream",
        }.to_string();

        // Store the file path for later async reading in send()
        self.file_path = Some((path, content_type));
        self
    }

    /// Upload from data
    pub fn from_data(mut self, data: Vec<u8>) -> Self {
        self.body = Some(crate::body::Body::bytes(data, "application/octet-stream"));
        self
    }

    /// Set authentication
    pub fn auth(mut self, auth: crate::Auth) -> Self {
        let header_value = http::HeaderValue::from_str(&auth.to_header_value())
            .expect("Invalid auth header value");
        self.headers
            .insert(http::header::AUTHORIZATION, header_value);
        self
    }

    /// Add a header to the request
    pub fn header(
        mut self,
        name: impl TryInto<http::HeaderName>,
        value: impl Into<String>,
    ) -> crate::Result<Self> {
        let header_name = name.try_into().map_err(|_| crate::Error::InvalidHeader)?;
        let header_value =
            http::HeaderValue::from_str(&value.into()).map_err(|_| crate::Error::InvalidHeader)?;
        self.headers.insert(header_name, header_value);
        Ok(self)
    }

    /// Set progress callback
    pub fn progress<F>(self, _callback: F) -> Self
    where
        F: Fn(u64, Option<u64>) + Send + Sync + 'static,
    {
        // TODO: Implement progress callbacks
        self
    }

    /// Start the upload
    pub async fn send(self) -> crate::Result<crate::Response> {
        let mut request_builder =
            crate::RequestBuilder::new(http::Method::POST, self.url, self.backend);

        // Add headers
        for (name, value) in self.headers {
            if let Some(name) = name {
                request_builder = request_builder.header(name, value.to_str().unwrap())?;
            }
        }

        // Handle body - either from data or from file
        if let Some((file_path, content_type)) = self.file_path {
            // Read file and create body
            let body = crate::body::Body::from_file(file_path, Some(content_type)).await?;
            request_builder = request_builder.body(body);
        } else if let Some(body) = self.body {
            request_builder = request_builder.body(body);
        }

        request_builder.send().await
    }
}

/// Response from a download operation
pub struct DownloadResponse {
    /// Path to the downloaded file
    pub file_path: std::path::PathBuf,
    /// Number of bytes downloaded
    pub bytes_downloaded: u64,
}

use crate::Result;
use crate::backend::Backend;
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use std::time::Duration;

/// HTTP client that works with any backend
#[derive(Clone)]
pub struct Client {
    backend: Backend,
    base_url: Option<String>,
    cookie_jar: Option<crate::CookieJar>,
    default_headers: HeaderMap,
}

impl Client {
    /// Create a new client with default backend for the platform
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// Create a client builder
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Create a client with a specific backend
    pub fn with_backend(backend: Backend) -> Self {
        Self {
            backend,
            base_url: None,
            cookie_jar: None,
            default_headers: HeaderMap::new(),
        }
    }

    /// Create a GET request
    pub fn get(&self, url: &str) -> crate::RequestBuilder {
        let mut builder =
            crate::RequestBuilder::new(Method::GET, self.resolve_url(url), self.backend.clone());
        builder.add_default_headers(self.default_headers.clone());
        builder
    }

    /// Create a POST request
    pub fn post(&self, url: &str) -> crate::RequestBuilder {
        let mut builder =
            crate::RequestBuilder::new(Method::POST, self.resolve_url(url), self.backend.clone());
        builder.add_default_headers(self.default_headers.clone());
        builder
    }

    /// Create a PUT request
    pub fn put(&self, url: &str) -> crate::RequestBuilder {
        let mut builder =
            crate::RequestBuilder::new(Method::PUT, self.resolve_url(url), self.backend.clone());
        builder.add_default_headers(self.default_headers.clone());
        builder
    }

    /// Create a DELETE request
    pub fn delete(&self, url: &str) -> crate::RequestBuilder {
        let mut builder =
            crate::RequestBuilder::new(Method::DELETE, self.resolve_url(url), self.backend.clone());
        builder.add_default_headers(self.default_headers.clone());
        builder
    }

    /// Create a PATCH request
    pub fn patch(&self, url: &str) -> crate::RequestBuilder {
        let mut builder =
            crate::RequestBuilder::new(Method::PATCH, self.resolve_url(url), self.backend.clone());
        builder.add_default_headers(self.default_headers.clone());
        builder
    }

    /// Create a HEAD request
    pub fn head(&self, url: &str) -> crate::RequestBuilder {
        let mut builder =
            crate::RequestBuilder::new(Method::HEAD, self.resolve_url(url), self.backend.clone());
        builder.add_default_headers(self.default_headers.clone());
        builder
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

    /// Get the cookie jar for this client
    pub fn cookie_jar(&self) -> Option<&crate::CookieJar> {
        self.cookie_jar.as_ref()
    }

    /// Download a file directly to disk
    pub fn download(&self, url: &str) -> DownloadBuilder {
        DownloadBuilder::new(self.backend.clone(), self.resolve_url(url))
    }

    /// Download a file in the background (continues when app is suspended)
    pub fn download_background(&self, url: &str) -> BackgroundDownloadBuilder {
        BackgroundDownloadBuilder::new(self.backend.clone(), self.resolve_url(url))
    }

    /// Upload a file
    pub fn upload(&self, url: &str) -> UploadBuilder {
        UploadBuilder::new(self.backend.clone(), self.resolve_url(url))
    }

    /// Create a WebSocket connection
    pub fn websocket(&self) -> crate::websocket::WebSocketBuilder {
        match &self.backend {
            #[cfg(target_vendor = "apple")]
            Backend::Foundation(foundation_backend) => {
                // Extract the NSURLSession from the Foundation backend
                crate::websocket::WebSocketBuilder::new(foundation_backend.session().clone())
            }
            Backend::Reqwest(_) => {
                // For Reqwest backend, we need to create a temporary Foundation session
                // since WebSocket is only implemented for Foundation
                #[cfg(target_vendor = "apple")]
                {
                    let foundation_backend = crate::backend::foundation::FoundationBackend::new()
                        .expect("Failed to create Foundation backend for WebSocket");
                    crate::websocket::WebSocketBuilder::new(foundation_backend.session().clone())
                }
                #[cfg(not(target_vendor = "apple"))]
                {
                    panic!("WebSocket is only available on Apple platforms")
                }
            }
        }
    }
}

/// Proxy configuration
#[derive(Clone)]
pub struct ProxyConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Builder for creating clients with backend abstraction
pub struct ClientBuilder {
    backend: Option<Backend>,
    base_url: Option<String>,
    timeout: Option<Duration>,
    user_agent: Option<String>,
    headers: HeaderMap,
    use_cookies: Option<bool>,
    cookie_jar: Option<crate::CookieJar>,
    ignore_certificate_errors: Option<bool>,
    http_proxy: Option<ProxyConfig>,
    https_proxy: Option<ProxyConfig>,
    socks_proxy: Option<ProxyConfig>,
}

impl ClientBuilder {
    /// Create a new client builder
    pub fn new() -> Self {
        Self {
            backend: None,
            base_url: None,
            timeout: None,
            user_agent: None,
            headers: HeaderMap::new(),
            use_cookies: None,
            cookie_jar: None,
            ignore_certificate_errors: None,
            http_proxy: None,
            https_proxy: None,
            socks_proxy: None,
        }
    }

    /// Set a specific backend
    pub fn backend(mut self, backend: Backend) -> Self {
        self.backend = Some(backend);
        self
    }

    /// Set the base URL for all requests
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set request timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set user agent
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    /// Add a default header
    pub fn header(
        mut self,
        name: impl TryInto<HeaderName>,
        value: impl Into<String>,
    ) -> Result<Self> {
        let header_name = name.try_into().map_err(|_| crate::Error::InvalidHeader)?;
        let header_value =
            HeaderValue::from_str(&value.into()).map_err(|_| crate::Error::InvalidHeader)?;
        self.headers.insert(header_name, header_value);
        Ok(self)
    }

    /// Enable or disable cookies
    pub fn use_cookies(mut self, use_cookies: bool) -> Self {
        self.use_cookies = Some(use_cookies);
        self
    }

    /// Set a custom cookie jar
    pub fn cookie_jar(mut self, cookie_jar: crate::CookieJar) -> Self {
        self.cookie_jar = Some(cookie_jar);
        self
    }

    /// Ignore certificate errors (for testing only)
    pub fn ignore_certificate_errors(mut self, ignore: bool) -> Self {
        self.ignore_certificate_errors = Some(ignore);
        self
    }

    /// Set HTTP proxy
    pub fn http_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        self.http_proxy = Some(ProxyConfig {
            host: host.into(),
            port,
            username: None,
            password: None,
        });
        self
    }

    /// Set HTTPS proxy
    pub fn https_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        self.https_proxy = Some(ProxyConfig {
            host: host.into(),
            port,
            username: None,
            password: None,
        });
        self
    }

    /// Set SOCKS proxy
    pub fn socks_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        self.socks_proxy = Some(ProxyConfig {
            host: host.into(),
            port,
            username: None,
            password: None,
        });
        self
    }

    /// Set proxy authentication
    pub fn proxy_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        let username = username.into();
        let password = password.into();

        // Apply to all configured proxies
        if let Some(ref mut proxy) = self.http_proxy {
            proxy.username = Some(username.clone());
            proxy.password = Some(password.clone());
        }
        if let Some(ref mut proxy) = self.https_proxy {
            proxy.username = Some(username.clone());
            proxy.password = Some(password.clone());
        }
        if let Some(ref mut proxy) = self.socks_proxy {
            proxy.username = Some(username);
            proxy.password = Some(password);
        }
        self
    }

    /// Build the client
    pub fn build(self) -> Result<Client> {
        let backend = match self.backend {
            Some(backend) => backend,
            None => {
                // Create backend with configuration
                let config = crate::backend::BackendConfig {
                    timeout: self.timeout,
                    user_agent: self.user_agent,
                    ignore_certificate_errors: self.ignore_certificate_errors,
                };

                if cfg!(target_vendor = "apple") {
                    crate::backend::Backend::foundation_with_config(config)?
                } else {
                    crate::backend::Backend::reqwest_with_config(config)?
                }
            }
        };

        Ok(Client {
            backend,
            base_url: self.base_url,
            cookie_jar: self.cookie_jar,
            default_headers: self.headers,
        })
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}
