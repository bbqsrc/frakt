//! Background download builder for downloads that survive app termination

use url::Url;

use crate::backend::Backend;
use crate::client::download::DownloadResponse;

/// Background download builder for downloads that survive app termination
///
/// Platform-specific behavior:
/// - **Apple platforms**: Uses NSURLSession background downloads
/// - **Unix platforms**: Uses double-fork daemon process with reqwest
/// - **Other platforms**: Uses resumable downloads with retry logic
///
/// All platforms support:
/// - Progress callbacks
/// - Automatic resume on failure
/// - Session identifiers for tracking
pub struct BackgroundDownloadBuilder {
    backend: Backend,
    url: Url,
    session_identifier: Option<String>,
    file_path: Option<std::path::PathBuf>,
    progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
}

impl BackgroundDownloadBuilder {
    /// Create a new background download builder (internal use)
    pub(crate) fn new(backend: Backend, url: Url) -> Self {
        Self {
            backend,
            url,
            session_identifier: None,
            file_path: None,
            progress_callback: None,
        }
    }

    /// Set session identifier for background downloads.
    ///
    /// The session identifier is used to track and manage background downloads
    /// across app restarts. If not provided, a unique identifier will be
    /// automatically generated based on process ID and timestamp.
    ///
    /// # Arguments
    ///
    /// * `identifier` - A unique string to identify this download session
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use frakt::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .download_background("https://httpbin.org/bytes/10485760") // 10MB
    ///     .session_identifier("my-app-update-v1.2.3")
    ///     .to_file("updates/app-v1.2.3.zip")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn session_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.session_identifier = Some(identifier.into());
        self
    }

    /// Set the destination file path for the background download.
    ///
    /// The file will be created at the specified path. If the parent directories
    /// don't exist, they will be created automatically. The download will continue
    /// in the background and survive app termination on supported platforms.
    ///
    /// # Arguments
    ///
    /// * `path` - The local file path where the downloaded content should be saved
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use frakt::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .download_background("https://httpbin.org/bytes/104857600") // 100MB
    ///     .to_file("downloads/large_file.zip")
    ///     .progress(|downloaded, total| {
    ///         if let Some(total) = total {
    ///             println!("Background download: {}%", (downloaded * 100) / total);
    ///         }
    ///     })
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn to_file<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.file_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set a progress callback to monitor background download progress.
    ///
    /// The callback will be called periodically during the download with the number
    /// of bytes downloaded so far and the total number of bytes to download (if known).
    /// The progress callback works across app restarts on supported platforms.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that receives `(bytes_downloaded, total_bytes)`
    ///   where `total_bytes` may be `None` if the server doesn't provide Content-Length
    ///
    /// # Platform Behavior
    ///
    /// - **Apple platforms**: Progress is maintained by NSURLSession across app restarts
    /// - **Unix platforms**: Progress is tracked by the daemon process
    /// - **Other platforms**: Progress is only available while the app is running
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use frakt::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .download_background("https://httpbin.org/bytes/1073741824") // 1GB
    ///     .to_file("huge_download.bin")
    ///     .progress(|bytes_downloaded, total_bytes| {
    ///         match total_bytes {
    ///             Some(total) => {
    ///                 let percent = (bytes_downloaded as f64 / total as f64) * 100.0;
    ///                 println!("Background download: {:.1}% ({} MB / {} MB)",
    ///                     percent,
    ///                     bytes_downloaded / 1_048_576,
    ///                     total / 1_048_576);
    ///             }
    ///             None => {
    ///                 println!("Background downloaded: {} MB", bytes_downloaded / 1_048_576);
    ///             }
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
        self.progress_callback = Some(Box::new(callback));
        self
    }

    /// Start the background download and return immediately.
    ///
    /// This method initiates a background download that will continue even if the
    /// application is terminated. The download behavior varies by platform:
    ///
    /// - **Apple platforms**: Uses NSURLSession background downloads
    /// - **Unix platforms**: Spawns a daemon process using double-fork
    /// - **Other platforms**: Uses resumable downloads with retry logic
    ///
    /// # Returns
    ///
    /// Returns a `DownloadResponse` immediately with initial download information.
    /// The actual download continues in the background.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No destination file path was specified using `to_file()`
    /// - The parent directory cannot be created
    /// - The background download session cannot be started
    /// - Platform-specific background services are unavailable
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use frakt::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    ///
    /// // Start a background download that survives app termination
    /// let response = client
    ///     .download_background("https://httpbin.org/bytes/1073741824") // 1GB
    ///     .session_identifier("app-update-v2.0")
    ///     .to_file("updates/app-v2.0.dmg")
    ///     .progress(|downloaded, total| {
    ///         // This will be called even after app restart on supported platforms
    ///         if let Some(total) = total {
    ///             println!("Download progress: {}%", (downloaded * 100) / total);
    ///         }
    ///     })
    ///     .send()
    ///     .await?;
    ///
    /// println!("Background download started for: {:?}", response.file_path);
    /// // App can now terminate - download continues in background
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send(self) -> crate::Result<DownloadResponse> {
        let file_path = self.file_path.clone().ok_or_else(|| {
            crate::Error::Internal("Background download file path not specified".to_string())
        })?;

        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                crate::Error::Internal(format!("Failed to create parent directory: {}", e))
            })?;
        }

        // Delegate to backend
        self.backend
            .execute_background_download(
                self.url,
                file_path,
                self.session_identifier,
                self.progress_callback,
            )
            .await
    }
}
