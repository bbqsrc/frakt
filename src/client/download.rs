//! Download builder for downloading files to disk

use url::Url;

use crate::backend::Backend;

/// Response from a completed download operation.
///
/// Contains information about the completed download, including where the file
/// was saved and how many bytes were downloaded.
///
/// # Examples
///
/// ```no_run
/// # use rsurlsession::Client;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let response = client
///     .download("https://httpbin.org/base64/SGVsbG8gV29ybGQ=")?
///     .to_file("hello.txt")
///     .send()
///     .await?;
///
/// println!("Downloaded to: {:?}", response.file_path);
/// println!("Downloaded {} bytes", response.bytes_downloaded);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct DownloadResponse {
    /// Path where the file was saved
    pub file_path: std::path::PathBuf,
    /// Total bytes downloaded
    pub bytes_downloaded: u64,
}

/// Builder for downloading files from URLs to local disk.
///
/// The `DownloadBuilder` provides a fluent interface for configuring downloads,
/// including the destination path, progress monitoring, and other options.
/// Downloads are streamed directly to disk to handle large files efficiently
/// without loading them entirely into memory.
///
/// # Examples
///
/// ## Basic download
///
/// ```no_run
/// # use rsurlsession::Client;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let response = client
///     .download("https://httpbin.org/base64/SGVsbG8gV29ybGQ=")?
///     .to_file("downloaded_file.txt")
///     .send()
///     .await?;
///
/// println!("Downloaded {} bytes to {:?}",
///     response.bytes_downloaded,
///     response.file_path);
/// # Ok(())
/// # }
/// ```
///
/// ## Download with progress monitoring
///
/// ```no_run
/// # use rsurlsession::Client;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let response = client
///     .download("https://httpbin.org/bytes/1048576")? // 1MB
///     .to_file("large_file.bin")
///     .progress(|downloaded, total| {
///         if let Some(total) = total {
///             let percent = (downloaded as f64 / total as f64) * 100.0;
///             println!("Download progress: {:.1}%", percent);
///         } else {
///             println!("Downloaded: {} bytes", downloaded);
///         }
///     })
///     .send()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct DownloadBuilder {
    backend: Backend,
    url: Url,
    file_path: Option<std::path::PathBuf>,
    progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
}

impl DownloadBuilder {
    /// Create a new download builder (internal use)
    pub(crate) fn new(backend: Backend, url: Url) -> Self {
        Self {
            backend,
            url,
            file_path: None,
            progress_callback: None,
        }
    }

    /// Set the destination file path for the download.
    ///
    /// The file will be created at the specified path. If the parent directories
    /// don't exist, they will be created automatically. If a file already exists
    /// at the path, it will be overwritten.
    ///
    /// # Arguments
    ///
    /// * `path` - The local file path where the downloaded content should be saved
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rsurlsession::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .download("https://httpbin.org/json")?
    ///     .to_file("data/response.json")  // Creates 'data' dir if needed
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn to_file<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.file_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set a progress callback to monitor download progress.
    ///
    /// The callback will be called periodically during the download with the number
    /// of bytes downloaded so far and the total number of bytes to download (if known).
    /// The total may be `None` if the server doesn't provide a `Content-Length` header.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that receives `(bytes_downloaded, total_bytes)`
    ///   where `total_bytes` may be `None` if the total size is unknown
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rsurlsession::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .download("https://httpbin.org/bytes/1048576")? // 1MB file
    ///     .to_file("large_download.bin")
    ///     .progress(|bytes_downloaded, total_bytes| {
    ///         match total_bytes {
    ///             Some(total) => {
    ///                 let percent = (bytes_downloaded as f64 / total as f64) * 100.0;
    ///                 println!("Download: {:.1}% ({}/{})", percent, bytes_downloaded, total);
    ///             }
    ///             None => {
    ///                 println!("Downloaded: {} bytes (total unknown)", bytes_downloaded);
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

    /// Execute the download and save the file to disk.
    ///
    /// This method performs the actual download, streaming the content directly
    /// to the specified file path. The download is performed asynchronously and
    /// efficiently handles large files by streaming rather than loading everything
    /// into memory.
    ///
    /// # Returns
    ///
    /// Returns a `DownloadResponse` containing information about the completed download,
    /// including the file path and total bytes downloaded.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No destination file path was specified using `to_file()`
    /// - The parent directory cannot be created
    /// - The file cannot be created or written to
    /// - The network request fails
    /// - The server returns an error response
    /// - File I/O operations fail during the download
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use rsurlsession::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .download("https://httpbin.org/json")?
    ///     .to_file("response.json")
    ///     .send()
    ///     .await?;
    ///
    /// println!("Download completed!");
    /// println!("File saved to: {:?}", response.file_path);
    /// println!("Downloaded {} bytes", response.bytes_downloaded);
    ///
    /// // File is now available on disk
    /// let content = std::fs::read_to_string(&response.file_path)?;
    /// println!("File content: {}", content);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send(self) -> crate::Result<DownloadResponse> {
        let file_path = self.file_path.ok_or_else(|| {
            crate::Error::Internal("Download file path not specified".to_string())
        })?;

        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                crate::Error::Internal(format!("Failed to create parent directory: {}", e))
            })?;
        }

        // Create request
        let request_builder = crate::RequestBuilder::new(http::Method::GET, self.url, self.backend);

        let response = request_builder.send().await?;

        // Get content length for progress callback before consuming response
        let total_bytes = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        // Open file for writing
        let mut file = tokio::fs::File::create(&file_path)
            .await
            .map_err(|e| crate::Error::Internal(format!("Failed to create file: {}", e)))?;

        let mut stream = response.stream();
        let mut bytes_downloaded = 0u64;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            bytes_downloaded += chunk.len() as u64;

            // Write chunk to file
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(|e| crate::Error::Internal(format!("Failed to write to file: {}", e)))?;

            // Call progress callback if provided
            if let Some(ref callback) = self.progress_callback {
                callback(bytes_downloaded, total_bytes);
            }
        }

        // Ensure file is flushed
        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .map_err(|e| crate::Error::Internal(format!("Failed to flush file: {}", e)))?;

        Ok(DownloadResponse {
            file_path,
            bytes_downloaded,
        })
    }
}
