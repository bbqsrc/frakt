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
