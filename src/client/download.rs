use crate::Result;
use http::{HeaderMap, HeaderValue, header};
use objc2::rc::Retained;
use objc2_foundation::NSURLSession;

/// Builder for downloading files directly to disk.
///
/// `DownloadBuilder` provides a convenient interface for downloading files using
/// NSURLSessionDownloadTask, which streams content directly to disk without loading
/// it into memory. This is more efficient for large files and provides built-in
/// progress tracking.
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
///     .download("https://example.com/large-file.zip")
///     .to_file("./downloads/file.zip")
///     .progress(|downloaded, total| {
///         if let Some(total) = total {
///             let percent = (downloaded as f64 / total as f64) * 100.0;
///             println!("Downloaded: {:.1}%", percent);
///         }
///     })
///     .send()
///     .await?;
///
/// println!("Downloaded {} bytes to {}", response.bytes_downloaded, response.file_path.display());
/// # Ok(())
/// # }
/// ```
pub struct DownloadBuilder {
    url: String,
    session: Retained<NSURLSession>,
    destination: Option<std::path::PathBuf>,
    progress_callback: Option<std::sync::Arc<crate::delegate::shared_context::ProgressCallback>>,
    headers: HeaderMap,
}

impl DownloadBuilder {
    pub(crate) fn new(url: String, session: Retained<NSURLSession>) -> Self {
        Self {
            url,
            session,
            destination: None,
            progress_callback: None,
            headers: HeaderMap::new(),
        }
    }

    /// Set the destination file path where the download will be saved.
    ///
    /// The file will be created if it doesn't exist, and any parent directories
    /// will be created as needed. If the file already exists, it will be overwritten.
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
    ///     .download("https://example.com/file.pdf")
    ///     .to_file("./downloads/document.pdf")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn to_file<P: Into<std::path::PathBuf>>(mut self, path: P) -> Self {
        self.destination = Some(path.into());
        self
    }

    /// Set a progress callback to track download progress.
    ///
    /// The callback will be called periodically during the download with the number
    /// of bytes downloaded so far and the total expected size (if known).
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
    ///     .download("https://example.com/large-file.zip")
    ///     .progress(|downloaded, total| {
    ///         if let Some(total) = total {
    ///             let percent = (downloaded as f64 / total as f64) * 100.0;
    ///             println!("Progress: {:.1}%", percent);
    ///         } else {
    ///             println!("Downloaded: {} bytes", downloaded);
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

    /// Add a header to the download request.
    ///
    /// This allows you to add custom headers to the download request, such as
    /// authentication headers or custom API keys.
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
    ///     .download("https://api.example.com/files/123")
    ///     .header("X-API-Key", "your-api-key")?
    ///     .header(http::header::ACCEPT, "application/octet-stream")?
    ///     .to_file("./downloads/file.bin")
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

    /// Set authentication for the download request.
    ///
    /// This adds the appropriate `Authorization` header based on the authentication
    /// method provided.
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
    ///     .download("https://api.example.com/protected/file.zip")
    ///     .auth(Auth::bearer("your-token"))?
    ///     .to_file("./downloads/file.zip")
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

    /// Start the download and return the result.
    ///
    /// This method initiates the download using NSURLSessionDownloadTask and returns
    /// a future that resolves when the download is complete. The file will be saved
    /// to the destination path specified with `to_file()`.
    ///
    /// # Returns
    ///
    /// Returns a [`DownloadResponse`] containing the final file path and download statistics.
    ///
    /// # Errors
    ///
    /// This method can fail with various errors including:
    /// - Network connectivity issues
    /// - Invalid URLs
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
    ///     .download("https://example.com/file.zip")
    ///     .to_file("./downloads/file.zip")
    ///     .send()
    ///     .await?;
    ///
    /// println!("Downloaded {} bytes to {}",
    ///          response.bytes_downloaded,
    ///          response.file_path.display());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send(self) -> Result<DownloadResponse> {
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
                let value_str = value.to_str().map_err(|_| crate::Error::InvalidHeader)?;
                req.setValue_forHTTPHeaderField(
                    Some(&NSString::from_str(value_str)),
                    &NSString::from_str(name.as_str()),
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

/// Response from a completed download operation.
///
/// This struct contains information about a completed download, including the final
/// file location and download statistics.
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
///     .send()
///     .await?;
///
/// println!("Downloaded {} bytes", response.bytes_downloaded);
/// println!("Saved to: {}", response.file_path.display());
///
/// // Check if the file exists
/// assert!(response.file_path.exists());
/// # Ok(())
/// # }
/// ```
pub struct DownloadResponse {
    /// The final file path where the download was saved
    pub file_path: std::path::PathBuf,
    /// Total bytes downloaded
    pub bytes_downloaded: u64,
}

/// Future for handling download completion
pub(super) struct DownloadFuture {
    pub(super) download_task: Retained<objc2_foundation::NSURLSessionDownloadTask>,
    pub(super) task_context: std::sync::Arc<crate::delegate::TaskSharedContext>,
    pub(super) destination: Option<std::path::PathBuf>,
}

impl DownloadFuture {
    pub(super) fn new(
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
