//! HTTP client implementation

use crate::session::SessionConfigurationBuilder;
use crate::{Request, RequestBuilder, Result};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSURLSession;
use std::time::Duration;

/// HTTP client for making requests
#[derive(Clone)]
pub struct Client {
    session: Retained<NSURLSession>,
    delegate: Retained<crate::delegate::DataTaskDelegate>,
    base_url: Option<String>,
}

impl Client {
    /// Create a new client with default configuration
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// Create a client builder
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Create a GET request
    pub fn get(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::GET,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a POST request
    pub fn post(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::POST,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a PUT request
    pub fn put(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::PUT,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a DELETE request
    pub fn delete(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::DELETE,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a PATCH request
    pub fn patch(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::PATCH,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a HEAD request
    pub fn head(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::HEAD,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Execute a request
    pub async fn execute(&self, request: Request) -> Result<crate::Response> {
        request.send().await
    }

    /// Download a file directly to disk
    pub fn download(&self, url: &str) -> DownloadBuilder {
        DownloadBuilder::new(url.to_string(), self.session.clone())
    }

    /// Download a file in the background (continues when app is suspended)
    pub fn download_background(&self, url: &str) -> BackgroundDownloadBuilder {
        BackgroundDownloadBuilder::new(url.to_string())
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
}

impl ClientBuilder {
    /// Create a new client builder
    pub fn new() -> Self {
        Self {
            config_builder: SessionConfigurationBuilder::new(),
            base_url: None,
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

    /// Build the client
    pub fn build(self) -> Result<Client> {
        let _should_ignore_certs = self.config_builder.should_ignore_certificate_errors();
        let config = self.config_builder.build()?;

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
        })
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for downloading files
pub struct DownloadBuilder {
    url: String,
    session: Retained<NSURLSession>,
    destination: Option<std::path::PathBuf>,
    progress_callback: Option<std::sync::Arc<crate::delegate::shared_context::ProgressCallback>>,
    headers: std::collections::HashMap<String, String>,
}

impl DownloadBuilder {
    pub(crate) fn new(url: String, session: Retained<NSURLSession>) -> Self {
        Self {
            url,
            session,
            destination: None,
            progress_callback: None,
            headers: std::collections::HashMap::new(),
        }
    }

    /// Set the destination file path
    pub fn to_file<P: Into<std::path::PathBuf>>(mut self, path: P) -> Self {
        self.destination = Some(path.into());
        self
    }

    /// Set a progress callback
    pub fn progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(u64, Option<u64>) + Send + Sync + 'static,
    {
        self.progress_callback = Some(std::sync::Arc::new(callback));
        self
    }

    /// Add a header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Start the download
    pub async fn send(self) -> Result<DownloadResponse> {
        use crate::delegate::shared_context::ProgressCallback;
        use objc2::runtime::ProtocolObject;
        use objc2_foundation::{NSMutableURLRequest, NSString, NSURL};

        // Create NSURLRequest
        let nsurl = unsafe {
            NSURL::URLWithString(&NSString::from_str(&self.url)).ok_or(crate::Error::InvalidUrl)?
        };

        let nsrequest = unsafe {
            let req = NSMutableURLRequest::requestWithURL(&nsurl);

            // Set headers
            for (name, value) in &self.headers {
                req.setValue_forHTTPHeaderField(
                    Some(&NSString::from_str(value)),
                    &NSString::from_str(name),
                );
            }

            req
        };

        // Create download delegate and task context
        let download_delegate = crate::delegate::DownloadTaskDelegate::new();
        let task_context = std::sync::Arc::new(crate::delegate::TaskSharedContext::with_download(
            self.destination.clone(),
            self.progress_callback,
        ));

        // Create download session with delegate
        let download_session = unsafe {
            objc2_foundation::NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &self.session.configuration(),
                Some(ProtocolObject::from_ref(&*download_delegate)),
                None,
            )
        };

        // Create download task
        let download_task = unsafe { download_session.downloadTaskWithRequest(&nsrequest) };

        // Register the task context with the delegate
        let task_id = unsafe { download_task.taskIdentifier() } as usize;
        download_delegate.register_task(task_id, task_context.clone());

        // Create download future
        let download_future = DownloadFuture::new(download_task, task_context, self.destination);

        // Start the download
        unsafe {
            download_future.download_task.resume();
        }

        download_future.await
    }
}

/// Response from a download operation
pub struct DownloadResponse {
    /// The final file path where the download was saved
    pub file_path: std::path::PathBuf,
    /// Total bytes downloaded
    pub bytes_downloaded: u64,
}

/// Future for handling download completion
struct DownloadFuture {
    download_task: Retained<objc2_foundation::NSURLSessionDownloadTask>,
    task_context: std::sync::Arc<crate::delegate::TaskSharedContext>,
    destination: Option<std::path::PathBuf>,
}

impl DownloadFuture {
    fn new(
        download_task: Retained<objc2_foundation::NSURLSessionDownloadTask>,
        task_context: std::sync::Arc<crate::delegate::TaskSharedContext>,
        destination: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            download_task,
            task_context,
            destination,
        }
    }
}

impl std::future::Future for DownloadFuture {
    type Output = Result<DownloadResponse>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.task_context.is_completed() {
            // Check for errors
            if let Some(error) = self.task_context.error.load_full() {
                return std::task::Poll::Ready(Err(crate::Error::from_ns_error(&*error)));
            }

            // Get the final file location (already copied by delegate)
            if let Some(download_context) = &self.task_context.download_context {
                if let Some(final_location) = download_context.final_location.load_full() {
                    let bytes_downloaded = self
                        .task_context
                        .bytes_downloaded
                        .load(std::sync::atomic::Ordering::Acquire);

                    return std::task::Poll::Ready(Ok(DownloadResponse {
                        file_path: (**final_location).to_path_buf(),
                        bytes_downloaded,
                    }));
                }
            }

            return std::task::Poll::Ready(Err(crate::Error::Internal(
                "No download location received".to_string(),
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

/// Builder for downloading files in background sessions
pub struct BackgroundDownloadBuilder {
    url: String,
    destination: Option<std::path::PathBuf>,
    progress_callback: Option<std::sync::Arc<crate::delegate::shared_context::ProgressCallback>>,
    headers: std::collections::HashMap<String, String>,
    session_identifier: Option<String>,
    background_completion_handler: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl BackgroundDownloadBuilder {
    pub(crate) fn new(url: String) -> Self {
        Self {
            url,
            destination: None,
            progress_callback: None,
            headers: std::collections::HashMap::new(),
            session_identifier: None,
            background_completion_handler: None,
        }
    }

    /// Set the destination file path
    pub fn to_file<P: Into<std::path::PathBuf>>(mut self, path: P) -> Self {
        self.destination = Some(path.into());
        self
    }

    /// Set a progress callback
    pub fn progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(u64, Option<u64>) + Send + Sync + 'static,
    {
        self.progress_callback = Some(std::sync::Arc::new(callback));
        self
    }

    /// Add a header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set the background session identifier (required for background downloads)
    pub fn session_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.session_identifier = Some(identifier.into());
        self
    }

    /// Set a completion handler that's called when all background events finish
    pub fn on_background_completion<F>(mut self, handler: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.background_completion_handler = Some(std::sync::Arc::new(handler));
        self
    }

    /// Start the background download
    pub async fn send(self) -> Result<DownloadResponse> {
        use objc2::runtime::ProtocolObject;
        use objc2_foundation::{NSMutableURLRequest, NSString, NSURL};

        // Background downloads require a session identifier
        let session_identifier = self.session_identifier.ok_or_else(|| {
            crate::Error::Internal("Background downloads require a session identifier".to_string())
        })?;

        // Create NSURLRequest
        let nsurl = unsafe {
            NSURL::URLWithString(&NSString::from_str(&self.url)).ok_or(crate::Error::InvalidUrl)?
        };

        let nsrequest = unsafe {
            let req = NSMutableURLRequest::requestWithURL(&nsurl);

            // Set headers
            for (name, value) in &self.headers {
                req.setValue_forHTTPHeaderField(
                    Some(&NSString::from_str(value)),
                    &NSString::from_str(name),
                );
            }

            req
        };

        // Create background session configuration
        let session_config = crate::session::SessionConfigurationBuilder::new()
            .background_session(&session_identifier)
            .build()?;

        // Create background delegate and task context
        let background_delegate = crate::delegate::BackgroundSessionDelegate::new();
        let task_context = std::sync::Arc::new(crate::delegate::TaskSharedContext::with_download(
            self.destination.clone(),
            self.progress_callback,
        ));

        // Create background session with delegate
        let background_session = unsafe {
            objc2_foundation::NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &session_config,
                Some(ProtocolObject::from_ref(&*background_delegate)),
                None,
            )
        };

        // TODO: Register background completion handler if provided
        // Note: This is complex due to block2/objc2 type conversions
        // For now, focusing on basic background download functionality
        if let Some(_handler) = self.background_completion_handler {
            // Will implement completion handler registration in a future iteration
            eprintln!("Warning: Background completion handler registration not yet implemented");
        }

        // Create download task
        let download_task = unsafe { background_session.downloadTaskWithRequest(&nsrequest) };

        // Register the task context with the delegate
        let task_id = unsafe { download_task.taskIdentifier() } as usize;
        background_delegate.register_task(task_id, task_context.clone());

        // Create download future
        let download_future = DownloadFuture::new(download_task, task_context, self.destination);

        // Start the download
        unsafe {
            download_future.download_task.resume();
        }

        download_future.await
    }
}
