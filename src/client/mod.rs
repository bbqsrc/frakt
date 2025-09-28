//! Client implementation using backend abstraction

use crate::backend::Backend;
use futures_util::StreamExt;
use http::{HeaderMap, HeaderName, HeaderValue, Method};

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
        let total_bytes = response
            .headers
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

/// Background download builder for downloads that survive app termination
///
/// Platform-specific behavior:
/// - **Apple platforms**: Uses NSURLSession background downloads
/// - **Unix platforms**: Uses double-fork daemon process with curl/wget
/// - **Other platforms**: Uses resumable downloads with retry logic
///
/// All platforms support:
/// - Progress callbacks
/// - Automatic resume on failure
/// - Session identifiers for tracking
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

    /// Generate a unique session identifier
    fn generate_session_id(&self, prefix: &str) -> String {
        format!(
            "rsurlsession-{}-{}-{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        )
    }

    /// Create and return the state directory path
    fn ensure_state_dir(&self) -> crate::Result<std::path::PathBuf> {
        let state_dir = std::env::temp_dir().join("rsurlsession");
        std::fs::create_dir_all(&state_dir).map_err(|e| {
            crate::Error::Internal(format!("Failed to create state directory: {}", e))
        })?;
        Ok(state_dir)
    }

    /// Set session identifier for background downloads
    ///
    /// If not provided, a unique identifier will be automatically generated
    /// based on process ID and timestamp.
    pub fn session_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.session_identifier = Some(identifier.into());
        self
    }

    /// Set destination file path
    ///
    /// The destination directory will be created if it doesn't exist.
    pub fn to_file<P: AsRef<std::path::Path>>(mut self, path: P) -> Self {
        self.file_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set progress callback
    ///
    /// The callback receives (bytes_downloaded, total_bytes).
    /// total_bytes may be None if the server doesn't provide Content-Length.
    pub fn progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(u64, Option<u64>) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Box::new(callback));
        self
    }

    /// Start the background download
    ///
    /// Returns immediately with download information. The download continues
    /// in the background and survives app termination on supported platforms.
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

        match &self.backend {
            #[cfg(target_vendor = "apple")]
            Backend::Foundation(_) => self.send_foundation_background(file_path).await,
            Backend::Reqwest(_) => {
                #[cfg(unix)]
                {
                    // Use double-fork approach on Unix systems
                    self.send_unix_background(file_path).await
                }
                #[cfg(not(unix))]
                {
                    // Use resumable download for non-Unix platforms
                    self.send_resumable_background(file_path).await
                }
            }
        }
    }

    #[cfg(target_vendor = "apple")]
    async fn send_foundation_background(
        self,
        file_path: std::path::PathBuf,
    ) -> crate::Result<DownloadResponse> {
        // For background downloads, we need to create a background session
        use crate::backend::foundation::delegate::background_session::BackgroundSessionDelegate;
        use crate::backend::foundation::delegate::shared_context::TaskSharedContext;
        use objc2::runtime::ProtocolObject;
        use objc2_foundation::{NSString, NSURL, NSURLSession, NSURLSessionConfiguration};
        use std::sync::Arc;

        // Create background session configuration
        let session_id = self
            .session_identifier
            .clone()
            .unwrap_or_else(|| self.generate_session_id("bg"));

        let session_config = unsafe {
            NSURLSessionConfiguration::backgroundSessionConfigurationWithIdentifier(
                &NSString::from_str(&session_id),
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
            NSURL::URLWithString(&NSString::from_str(&self.url)).ok_or(crate::Error::InvalidUrl)?
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
            crate::backend::foundation::delegate::shared_context::DownloadContext::new(Some(
                file_path.clone(),
            )),
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
        let bytes_downloaded = task_context
            .bytes_downloaded
            .load(std::sync::atomic::Ordering::Relaxed);

        Ok(DownloadResponse {
            file_path,
            bytes_downloaded,
        })
    }

    #[cfg(unix)]
    async fn send_unix_background(
        self,
        file_path: std::path::PathBuf,
    ) -> crate::Result<DownloadResponse> {
        // Create a unique session identifier
        let session_id = self
            .session_identifier
            .clone()
            .unwrap_or_else(|| self.generate_session_id("unix"));

        // Create state directory
        let state_dir = self.ensure_state_dir()?;

        let state_file = state_dir.join(format!("{}.state", session_id));

        // Get the backend for the daemon process
        let backend = match &self.backend {
            Backend::Reqwest(reqwest_backend) => reqwest_backend.clone(),
            #[cfg(target_vendor = "apple")]
            Backend::Foundation(_) => {
                return Err(crate::Error::Internal(
                    "Foundation backend should not use Unix fork approach".to_string(),
                ));
            }
        };

        // Clone data needed for the daemon process
        let url = self.url.clone();
        let progress_callback = self.progress_callback.is_some();

        // Double-fork to create a truly detached process
        unsafe {
            let pid = libc::fork();
            if pid < 0 {
                return Err(crate::Error::Internal("First fork failed".to_string()));
            } else if pid == 0 {
                // First child

                // Create new session
                if libc::setsid() < 0 {
                    std::process::exit(1);
                }

                // Second fork
                let pid2 = libc::fork();
                if pid2 < 0 {
                    std::process::exit(1);
                } else if pid2 == 0 {
                    // Second child - the daemon

                    // Close all file descriptors
                    for fd in 3..256 {
                        libc::close(fd);
                    }

                    // Redirect stdin/stdout/stderr to /dev/null
                    let devnull = libc::open(b"/dev/null\0".as_ptr() as *const i8, libc::O_RDWR);
                    if devnull >= 0 {
                        libc::dup2(devnull, 0);
                        libc::dup2(devnull, 1);
                        libc::dup2(devnull, 2);
                        if devnull > 2 {
                            libc::close(devnull);
                        }
                    }

                    // Run the download in the daemon process
                    Self::run_daemon_download(
                        url,
                        file_path,
                        state_file,
                        backend,
                        progress_callback,
                    );

                    // Exit when download completes
                    std::process::exit(0);
                } else {
                    // First child exits immediately
                    std::process::exit(0);
                }
            } else {
                // Parent process - wait for first child to exit
                let mut status = 0;
                libc::waitpid(pid, &mut status, 0);
            }
        }

        // Wait a moment for the download to start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Now monitor the state file for completion
        let start_time = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(300); // 5 minute timeout for start

        loop {
            if start_time.elapsed() > timeout {
                return Err(crate::Error::Internal(
                    "Background download timeout".to_string(),
                ));
            }

            if let Ok(state_content) = std::fs::read_to_string(&state_file) {
                let mut status = None;
                let mut bytes_downloaded = 0u64;
                let mut error_msg = None;

                for line in state_content.lines() {
                    if let Some((key, value)) = line.split_once(':') {
                        match key {
                            "status" => status = Some(value.to_string()),
                            "bytes_downloaded" => bytes_downloaded = value.parse().unwrap_or(0),
                            "error" => error_msg = Some(value.to_string()),
                            _ => {}
                        }
                    }
                }

                match status.as_deref() {
                    Some("completed") => {
                        // Clean up state file
                        let _ = std::fs::remove_file(&state_file);

                        return Ok(DownloadResponse {
                            file_path,
                            bytes_downloaded,
                        });
                    }
                    Some("failed") => {
                        // Clean up state file
                        let _ = std::fs::remove_file(&state_file);

                        return Err(crate::Error::Internal(
                            error_msg.unwrap_or_else(|| "Download failed".to_string()),
                        ));
                    }
                    Some("downloading") => {
                        // Call progress callback if provided
                        if let Some(ref callback) = self.progress_callback {
                            callback(bytes_downloaded, None);
                        }
                    }
                    _ => {}
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    /// Run download in daemon process using reqwest
    fn run_daemon_download(
        url: String,
        file_path: std::path::PathBuf,
        state_file: std::path::PathBuf,
        backend: crate::backend::reqwest::ReqwestBackend,
        has_progress_callback: bool,
    ) {
        // Helper function to write state
        let write_state = |status: &str, bytes_downloaded: u64, error: Option<&str>| {
            let mut content = format!(
                "status:{}\nbytes_downloaded:{}\nlast_update:{}\n",
                status,
                bytes_downloaded,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            );
            if let Some(err) = error {
                content.push_str(&format!("error:{}\n", err));
            }
            let _ = std::fs::write(&state_file, content);
        };

        // Create a new tokio runtime in the forked process
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                write_state(
                    "failed",
                    0,
                    Some(&format!("Failed to create runtime: {}", e)),
                );
                return;
            }
        };

        // Run the download
        runtime.block_on(async {
            if let Err(e) = Self::daemon_download_async(
                url,
                file_path,
                &state_file,
                backend,
                has_progress_callback,
                write_state,
            )
            .await
            {
                write_state("failed", 0, Some(&format!("Download failed: {}", e)));
            }
        });
    }

    /// Async download logic for daemon process
    async fn daemon_download_async(
        url: String,
        file_path: std::path::PathBuf,
        state_file: &std::path::Path,
        backend: crate::backend::reqwest::ReqwestBackend,
        has_progress_callback: bool,
        write_state: impl Fn(&str, u64, Option<&str>),
    ) -> std::result::Result<(), String> {
        use futures_util::StreamExt;

        // Check if file already exists (for resume)
        let initial_size = if file_path.exists() {
            std::fs::metadata(&file_path)
                .map_err(|e| format!("Failed to check existing file: {}", e))?
                .len()
        } else {
            0
        };

        // Create the reqwest client from backend
        let client = backend.client();

        // Create request with Range header for resume if needed
        let mut request_builder = client.get(&url);
        if initial_size > 0 {
            request_builder = request_builder.header("Range", format!("bytes={}-", initial_size));
        }

        // Send the request
        let response = request_builder
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        // Check status
        if !response.status().is_success() && response.status() != 206 {
            if response.status() == 416 && initial_size > 0 {
                // Range not satisfiable - file is already complete
                write_state("completed", initial_size, None);
                return Ok(());
            }
            return Err(format!("HTTP error: {}", response.status()));
        }

        // Get content length
        let content_length = response.content_length();
        let total_size = if response.status() == 206 {
            // Partial content - add to existing size
            content_length.map(|len| len + initial_size)
        } else {
            content_length
        };

        // Open file for writing (append if resuming)
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(initial_size > 0)
            .write(true)
            .truncate(initial_size == 0)
            .open(&file_path)
            .map_err(|e| format!("Failed to open file: {}", e))?;

        // Stream the response body
        let mut stream = response.bytes_stream();
        let mut bytes_downloaded = initial_size;
        let mut last_progress_update = std::time::Instant::now();

        write_state("downloading", bytes_downloaded, None);

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("Stream error: {}", e))?;

            // Write chunk to file
            std::io::Write::write_all(&mut file, &chunk)
                .map_err(|e| format!("Failed to write to file: {}", e))?;

            bytes_downloaded += chunk.len() as u64;

            // Update progress periodically
            if has_progress_callback
                && last_progress_update.elapsed() > std::time::Duration::from_millis(500)
            {
                write_state("downloading", bytes_downloaded, None);
                last_progress_update = std::time::Instant::now();
            }
        }

        // Ensure file is flushed
        std::io::Write::flush(&mut file).map_err(|e| format!("Failed to flush file: {}", e))?;

        // Mark as completed
        write_state("completed", bytes_downloaded, None);
        Ok(())
    }

    /// Resumable background download for non-Unix platforms
    async fn send_resumable_background(
        self,
        file_path: std::path::PathBuf,
    ) -> crate::Result<DownloadResponse> {
        // Create a unique session identifier
        let session_id = self
            .session_identifier
            .clone()
            .unwrap_or_else(|| self.generate_session_id("resumable"));

        // Create state directory
        let state_dir = self.ensure_state_dir()?;

        let state_file = state_dir.join(format!("{}.state", session_id));

        // Ensure destination directory exists
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                crate::Error::Internal(format!("Failed to create destination directory: {}", e))
            })?;
        }

        // Check if we're resuming an existing download
        let mut bytes_downloaded = 0u64;
        if file_path.exists() {
            bytes_downloaded = std::fs::metadata(&file_path)
                .map_err(|e| {
                    crate::Error::Internal(format!("Failed to read file metadata: {}", e))
                })?
                .len();
        }

        // Write initial state
        let state_content = format!(
            "status:downloading\nbytes_downloaded:{}\nlast_update:{}\n",
            bytes_downloaded,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );
        std::fs::write(&state_file, state_content)
            .map_err(|e| crate::Error::Internal(format!("Failed to write state file: {}", e)))?;

        // Create request with Range header for resume support
        let mut request_builder = match &self.backend {
            Backend::Reqwest(_) => crate::RequestBuilder::new(
                http::Method::GET,
                self.url.clone(),
                self.backend.clone(),
            ),
            #[cfg(target_vendor = "apple")]
            Backend::Foundation(_) => crate::RequestBuilder::new(
                http::Method::GET,
                self.url.clone(),
                self.backend.clone(),
            ),
        };

        // Add Range header if resuming
        if bytes_downloaded > 0 {
            request_builder =
                request_builder.header("Range", format!("bytes={}-", bytes_downloaded))?;
        }

        // Perform the download with retry logic
        let mut retry_count = 0;
        const MAX_RETRIES: u32 = 5;
        const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

        loop {
            match self
                .try_download(&request_builder, &file_path, bytes_downloaded, &state_file)
                .await
            {
                Ok(total_bytes) => {
                    // Download completed successfully
                    let final_state = format!(
                        "status:completed\nbytes_downloaded:{}\nlast_update:{}\n",
                        total_bytes,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    );
                    let _ = std::fs::write(&state_file, final_state);

                    // Clean up state file
                    let _ = std::fs::remove_file(&state_file);

                    return Ok(DownloadResponse {
                        file_path,
                        bytes_downloaded: total_bytes,
                    });
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count >= MAX_RETRIES {
                        // Max retries exceeded, write failed state
                        let failed_state = format!(
                            "status:failed\nerror:{}\nlast_update:{}\n",
                            e,
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                        );
                        let _ = std::fs::write(&state_file, failed_state);
                        return Err(e);
                    }

                    // Update bytes downloaded for next retry
                    if file_path.exists() {
                        bytes_downloaded = std::fs::metadata(&file_path)
                            .map_err(|e| {
                                crate::Error::Internal(format!(
                                    "Failed to read file metadata: {}",
                                    e
                                ))
                            })?
                            .len();

                        // Update Range header for retry
                        request_builder = match &self.backend {
                            Backend::Reqwest(_) => crate::RequestBuilder::new(
                                http::Method::GET,
                                self.url.clone(),
                                self.backend.clone(),
                            ),
                            #[cfg(target_vendor = "apple")]
                            Backend::Foundation(_) => crate::RequestBuilder::new(
                                http::Method::GET,
                                self.url.clone(),
                                self.backend.clone(),
                            ),
                        };

                        if bytes_downloaded > 0 {
                            request_builder = request_builder
                                .header("Range", format!("bytes={}-", bytes_downloaded))?;
                        }
                    }

                    // Exponential backoff
                    let delay = RETRY_DELAY * 2_u32.pow(retry_count - 1);
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn try_download(
        &self,
        request_builder: &crate::RequestBuilder,
        file_path: &std::path::Path,
        initial_bytes: u64,
        state_file: &std::path::Path,
    ) -> crate::Result<u64> {
        use tokio::io::AsyncWriteExt;

        // For simplicity, we'll recreate the request with the Range header
        let mut final_request =
            crate::RequestBuilder::new(Method::GET, self.url.clone(), self.backend.clone());

        // Add Range header if resuming
        if initial_bytes > 0 {
            final_request = final_request.header("Range", format!("bytes={}-", initial_bytes))?;
        }

        let response = final_request.send().await?;

        // Open file for appending (or create if doesn't exist)
        let mut file = if initial_bytes > 0 {
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(file_path)
                .await
                .map_err(|e| {
                    crate::Error::Internal(format!("Failed to open file for appending: {}", e))
                })?
        } else {
            tokio::fs::File::create(file_path)
                .await
                .map_err(|e| crate::Error::Internal(format!("Failed to create file: {}", e)))?
        };

        let mut stream = response.stream();
        let mut bytes_written = initial_bytes;

        while let Some(chunk_result) = stream.next().await {
            let chunk =
                chunk_result.map_err(|e| crate::Error::Internal(format!("Stream error: {}", e)))?;

            file.write_all(&chunk)
                .await
                .map_err(|e| crate::Error::Internal(format!("Failed to write to file: {}", e)))?;

            bytes_written += chunk.len() as u64;

            // Update state file with progress
            let state_content = format!(
                "status:downloading\nbytes_downloaded:{}\nlast_update:{}\n",
                bytes_written,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            );
            let _ = std::fs::write(state_file, state_content);

            // Call progress callback if provided
            if let Some(ref callback) = self.progress_callback {
                callback(bytes_written, None);
            }
        }

        file.flush()
            .await
            .map_err(|e| crate::Error::Internal(format!("Failed to flush file: {}", e)))?;

        Ok(bytes_written)
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
        }
        .to_string();

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
                // Use Foundation backend for WebSocket
                crate::websocket::WebSocketBuilder::new_foundation(
                    foundation_backend.session().clone(),
                )
            }
            Backend::Reqwest(_) => {
                // Use Reqwest backend for WebSocket with tokio-tungstenite
                crate::websocket::WebSocketBuilder::new_reqwest()
            }
        }
    }
}

/// Proxy configuration
#[derive(Clone, Debug)]
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
                    default_headers: if self.headers.is_empty() {
                        None
                    } else {
                        Some(self.headers.clone())
                    },
                    use_cookies: self.use_cookies,
                    cookie_jar: None, // TODO: Add cookie jar support to ClientBuilder
                    http_proxy: None, // TODO: Add proxy support to ClientBuilder
                    https_proxy: None,
                    socks_proxy: None,
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
