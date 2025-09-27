use super::download::{DownloadFuture, DownloadResponse};
use crate::Result;
use http::{HeaderMap, HeaderValue, header};

/// Builder for downloading files in background sessions.
///
/// `BackgroundDownloadBuilder` provides a specialized interface for downloading files
/// that continue even when the app is suspended or terminated. This is particularly
/// useful on iOS for large file downloads that need to complete in the background.
///
/// Background downloads require a unique session identifier and have special handling
/// for app lifecycle events.
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
///     .download_background("https://example.com/large-file.zip")
///     .session_identifier("com.myapp.background-downloads")
///     .to_file("./downloads/large-file.zip")
///     .progress(|downloaded, total| {
///         if let Some(total) = total {
///             let percent = (downloaded as f64 / total as f64) * 100.0;
///             println!("Background download: {:.1}%", percent);
///         }
///     })
///     .send()
///     .await?;
///
/// println!("Background download completed: {}", response.file_path.display());
/// # Ok(())
/// # }
/// ```
pub struct BackgroundDownloadBuilder {
    url: String,
    destination: Option<std::path::PathBuf>,
    progress_callback: Option<std::sync::Arc<crate::delegate::shared_context::ProgressCallback>>,
    headers: HeaderMap,
    session_identifier: Option<String>,
    background_completion_handler: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl BackgroundDownloadBuilder {
    pub(crate) fn new(url: String) -> Self {
        Self {
            url,
            destination: None,
            progress_callback: None,
            headers: HeaderMap::new(),
            session_identifier: None,
            background_completion_handler: None,
        }
    }

    /// Set the destination file path where the background download will be saved.
    ///
    /// The file will be created if it doesn't exist, and any parent directories
    /// will be created as needed. If the file already exists, it will be overwritten.
    /// Background downloads require a destination file path to be specified.
    ///
    /// # Arguments
    ///
    /// * `path` - The file path where the download should be saved
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
    ///     .download_background("https://example.com/large-video.mp4")
    ///     .session_identifier("com.myapp.downloads")
    ///     .to_file("./downloads/video.mp4")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn to_file<P: Into<std::path::PathBuf>>(mut self, path: P) -> Self {
        self.destination = Some(path.into());
        self
    }

    /// Set a progress callback to track background download progress.
    ///
    /// The callback will be called periodically during the download with the number
    /// of bytes downloaded so far and the total expected size (if known). This callback
    /// will continue to be invoked even when the app is suspended, as background downloads
    /// continue in the background.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that takes (downloaded_bytes, total_bytes) parameters
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
    ///     .download_background("https://example.com/large-file.zip")
    ///     .session_identifier("com.myapp.downloads")
    ///     .progress(|downloaded, total| {
    ///         if let Some(total) = total {
    ///             let percent = (downloaded as f64 / total as f64) * 100.0;
    ///             println!("Background download: {:.1}%", percent);
    ///         } else {
    ///             println!("Background downloaded: {} bytes", downloaded);
    ///         }
    ///     })
    ///     .to_file("./downloads/file.zip")
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

    /// Add a header to the background download request.
    ///
    /// This allows you to add custom headers to the download request, such as
    /// authentication headers, API keys, or custom request headers that the server
    /// may require for the download.
    ///
    /// # Arguments
    ///
    /// * `name` - The header name
    /// * `value` - The header value
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
    ///     .download_background("https://api.example.com/files/video.mp4")
    ///     .session_identifier("com.myapp.downloads")
    ///     .header("X-API-Key", "your-api-key")?
    ///     .header(http::header::USER_AGENT, "MyApp/1.0")?
    ///     .to_file("./downloads/video.mp4")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn header(
        mut self,
        name: impl TryInto<http::HeaderName>,
        value: impl Into<String>,
    ) -> crate::Result<Self> {
        let header_name = name.try_into().map_err(|_| crate::Error::InvalidHeader)?;
        let header_value =
            HeaderValue::from_str(&value.into()).map_err(|_| crate::Error::InvalidHeader)?;
        self.headers.insert(header_name, header_value);
        Ok(self)
    }

    /// Set multiple headers at once using a HeaderMap.
    ///
    /// This method replaces all existing headers with the provided HeaderMap.
    /// This is more efficient than chaining multiple `.header()` calls when you
    /// need to set many headers for the background download. All headers will be
    /// preserved during the background download even if the app is suspended.
    ///
    /// # Arguments
    ///
    /// * `headers` - A HeaderMap containing all headers to set
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::Client;
    /// use http::{HeaderMap, header};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    ///
    /// let mut headers = HeaderMap::new();
    /// headers.insert(header::AUTHORIZATION, "Bearer token".parse()?);
    /// headers.insert(header::USER_AGENT, "MyApp/1.0 Background".parse()?);
    /// headers.insert("X-Background-Priority", "low".parse()?);
    ///
    /// let response = client
    ///     .download_background("https://example.com/large-file.zip")
    ///     .session_identifier("com.myapp.downloads")
    ///     .headers(headers)
    ///     .to_file("./downloads/large-file.zip")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn headers(mut self, headers: HeaderMap) -> Self {
        self.headers = headers;
        self
    }

    /// Set authentication for the background download request.
    ///
    /// This adds the appropriate `Authorization` header based on the authentication
    /// method provided. The authentication will be preserved during the background
    /// download even if the app is suspended.
    ///
    /// # Arguments
    ///
    /// * `auth` - The authentication method to use
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::{Client, Auth};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .download_background("https://api.example.com/protected/large-file.zip")
    ///     .session_identifier("com.myapp.downloads")
    ///     .auth(Auth::bearer("your-token"))?
    ///     .to_file("./downloads/protected-file.zip")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn auth(mut self, auth: crate::Auth) -> crate::Result<Self> {
        let header_value = HeaderValue::from_str(&auth.to_header_value())
            .map_err(|_| crate::Error::InvalidHeader)?;
        self.headers.insert(header::AUTHORIZATION, header_value);
        Ok(self)
    }

    /// Set the background session identifier (required for background downloads).
    ///
    /// The session identifier is a unique string that identifies this background session.
    /// It should follow reverse-DNS naming conventions (e.g., "com.yourapp.downloads").
    /// This identifier is used by the system to associate the background download with
    /// your app and ensure it can continue even when the app is terminated.
    ///
    /// **This method is required for background downloads and the download will fail
    /// if no session identifier is provided.**
    ///
    /// # Arguments
    ///
    /// * `identifier` - A unique session identifier following reverse-DNS conventions
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
    ///     .download_background("https://example.com/large-file.zip")
    ///     .session_identifier("com.mycompany.myapp.downloads")
    ///     .to_file("./downloads/file.zip")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn session_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.session_identifier = Some(identifier.into());
        self
    }

    /// Set a completion handler that's called when all background events finish.
    ///
    /// This handler will be invoked when all background download tasks for this session
    /// have completed, failed, or been cancelled. This is useful for performing cleanup
    /// operations or updating UI state when background downloads are finished.
    ///
    /// **Note:** Background completion handler registration is not yet fully implemented
    /// due to complexity with objc2 block conversions. This method will accept the handler
    /// but emit a warning that registration is pending implementation.
    ///
    /// # Arguments
    ///
    /// * `handler` - A function to call when all background downloads complete
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
    ///     .download_background("https://example.com/file.zip")
    ///     .session_identifier("com.myapp.downloads")
    ///     .on_background_completion(|| {
    ///         println!("All background downloads completed!");
    ///     })
    ///     .to_file("./downloads/file.zip")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn on_background_completion<F>(mut self, handler: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.background_completion_handler = Some(std::sync::Arc::new(handler));
        self
    }

    /// Start the background download and return the result.
    ///
    /// This method initiates the background download using NSURLSessionDownloadTask with
    /// a background session configuration. The download will continue even if the app
    /// is suspended or terminated, making it ideal for large file downloads on iOS.
    ///
    /// A session identifier must be provided via [`session_identifier()`] before calling
    /// this method, or the download will fail with an error.
    ///
    /// # Returns
    ///
    /// Returns a [`DownloadResponse`] containing the final file path and download statistics
    /// when the download completes successfully.
    ///
    /// # Errors
    ///
    /// This method can fail with various errors including:
    /// - [`Error::Internal`] if no session identifier was provided
    /// - [`Error::InvalidUrl`] if the URL is malformed
    /// - Network connectivity issues
    /// - File system errors when writing to the destination
    /// - Authentication failures
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
    ///     .download_background("https://example.com/large-movie.mp4")
    ///     .session_identifier("com.myapp.media-downloads")
    ///     .to_file("./downloads/movie.mp4")
    ///     .progress(|downloaded, total| {
    ///         if let Some(total) = total {
    ///             let percent = (downloaded as f64 / total as f64) * 100.0;
    ///             println!("Background download: {:.1}%", percent);
    ///         }
    ///     })
    ///     .send()
    ///     .await?;
    ///
    /// println!("Background download completed: {}", response.file_path.display());
    /// println!("Downloaded {} bytes", response.bytes_downloaded);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`session_identifier()`]: Self::session_identifier
    /// [`DownloadResponse`]: crate::client::DownloadResponse
    /// [`Error::Internal`]: crate::Error::Internal
    /// [`Error::InvalidUrl`]: crate::Error::InvalidUrl
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
                let value_str = value.to_str().map_err(|_| crate::Error::InvalidHeader)?;
                req.setValue_forHTTPHeaderField(
                    Some(&NSString::from_str(value_str)),
                    &NSString::from_str(name.as_str()),
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
