//! Background download implementations for reqwest backend

use crate::{Error, Result};
use futures_util::StreamExt;
use std::path::PathBuf;
use url::Url;

/// Generate a unique session identifier
fn generate_session_id(prefix: &str) -> String {
    format!(
        "frakt-{}-{}-{}",
        prefix,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    )
}

/// Create and return the state directory path
fn ensure_state_dir() -> Result<PathBuf> {
    let state_dir = std::env::temp_dir().join("frakt");
    std::fs::create_dir_all(&state_dir)
        .map_err(|e| Error::Internal(format!("Failed to create state directory: {}", e)))?;
    Ok(state_dir)
}

/// Execute background download on Unix using double-fork
#[cfg(unix)]
pub async fn execute_unix_background_download(
    client: &reqwest::Client,
    url: Url,
    file_path: PathBuf,
    session_identifier: Option<String>,
    headers: http::HeaderMap,
    progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
    _error_for_status: bool,
) -> Result<crate::client::download::DownloadResponse> {
    let session_id = session_identifier.unwrap_or_else(|| generate_session_id("unix"));
    let state_dir = ensure_state_dir()?;
    let state_file = state_dir.join(format!("{}.state", session_id));
    let has_progress_callback = progress_callback.is_some();

    // Clone client for daemon process
    let client = client.clone();

    // Double-fork to create a truly detached process
    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            return Err(Error::Internal("First fork failed".to_string()));
        } else if pid == 0 {
            // First child - create new session
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
                let devnull =
                    libc::open(b"/dev/null\0".as_ptr() as *const libc::c_char, libc::O_RDWR);
                if devnull >= 0 {
                    libc::dup2(devnull, 0);
                    libc::dup2(devnull, 1);
                    libc::dup2(devnull, 2);
                    if devnull > 2 {
                        libc::close(devnull);
                    }
                }

                // Run the download in the daemon process
                run_daemon_download(
                    url,
                    file_path,
                    state_file,
                    client,
                    headers,
                    has_progress_callback,
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

    // Monitor the state file for completion and call progress callback if provided
    monitor_background_download_with_progress(state_file, file_path, progress_callback).await
}

/// Monitor state file for completion with progress callback support
async fn monitor_background_download_with_progress(
    state_file: PathBuf,
    file_path: PathBuf,
    progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
) -> Result<crate::client::download::DownloadResponse> {
    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(300); // 5 minute timeout

    loop {
        if start_time.elapsed() > timeout {
            return Err(Error::Internal("Background download timeout".to_string()));
        }

        if let Ok(state_content) = std::fs::read_to_string(&state_file) {
            let mut status = None;
            let mut bytes_downloaded = 0u64;
            let mut error_msg = None;

            for line in state_content.lines() {
                if let Some((key, value)) = line.split_once(':') {
                    match key {
                        "status" => status = Some(value.to_string()),
                        "bytes_downloaded" => {
                            bytes_downloaded = value.parse().unwrap_or(0);
                        }
                        "error" => error_msg = Some(value.to_string()),
                        _ => {}
                    }
                }
            }

            // Call progress callback if we have one and we're downloading
            if let (Some(callback), Some("downloading")) = (&progress_callback, status.as_deref()) {
                // For Unix daemon downloads, we don't know the total size easily,
                // so we'll pass None for total_bytes
                callback(bytes_downloaded, None);
            }

            match status.as_deref() {
                Some("completed") => {
                    // Call final progress callback if provided
                    if let Some(callback) = &progress_callback {
                        callback(bytes_downloaded, Some(bytes_downloaded));
                    }

                    // Clean up state file
                    let _ = std::fs::remove_file(&state_file);

                    // TODO: Capture actual status and headers from background download
                    return Ok(crate::client::download::DownloadResponse {
                        file_path,
                        bytes_downloaded,
                        status: http::StatusCode::OK,
                        headers: http::HeaderMap::new(),
                    });
                }
                Some("failed") => {
                    // Clean up state file
                    let _ = std::fs::remove_file(&state_file);

                    return Err(Error::Internal(
                        error_msg.unwrap_or_else(|| "Download failed".to_string()),
                    ));
                }
                _ => {
                    // Still downloading or unknown status
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Execute background download on non-Unix platforms using resumable downloads
#[cfg(not(unix))]
pub async fn execute_resumable_background_download(
    client: &reqwest::Client,
    url: Url,
    file_path: PathBuf,
    session_identifier: Option<String>,
    headers: http::HeaderMap,
    progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
) -> Result<crate::client::download::DownloadResponse> {
    let session_id = session_identifier.unwrap_or_else(|| generate_session_id("resumable"));
    let state_dir = ensure_state_dir()?;
    let state_file = state_dir.join(format!("{}.state", session_id));

    // Check if we're resuming an existing download
    let mut bytes_downloaded = 0u64;
    if file_path.exists() {
        bytes_downloaded = std::fs::metadata(&file_path)
            .map_err(|e| Error::Internal(format!("Failed to read file metadata: {}", e)))?
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
        .map_err(|e| Error::Internal(format!("Failed to write state file: {}", e)))?;

    // Perform the download with retry logic
    let mut retry_count = 0;
    const MAX_RETRIES: u32 = 5;
    const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

    loop {
        match try_download_with_resume(
            client,
            &url,
            &file_path,
            bytes_downloaded,
            &headers,
            progress_callback.as_deref(),
        )
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

                return Ok(crate::client::download::DownloadResponse {
                    file_path,
                    bytes_downloaded: total_bytes,
                });
            }
            Err(e) => {
                retry_count += 1;
                if retry_count >= MAX_RETRIES {
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
                            Error::Internal(format!("Failed to read file metadata: {}", e))
                        })?
                        .len();
                }

                // Exponential backoff
                let delay = RETRY_DELAY * 2_u32.pow(retry_count - 1);
                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// Run download in daemon process using reqwest
fn run_daemon_download(
    url: Url,
    file_path: PathBuf,
    state_file: PathBuf,
    client: reqwest::Client,
    headers: http::HeaderMap,
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
        if let Err(e) = daemon_download_async(
            url,
            file_path,
            &state_file,
            client,
            headers,
            has_progress_callback,
            write_state,
        )
        .await
        {
            write_state("failed", 0, Some(&format!("Download failed: {}", e)));
        }
    });
}

/// Attempt download with resume capability
#[cfg(not(unix))]
async fn try_download_with_resume(
    client: &reqwest::Client,
    url: &Url,
    file_path: &std::path::Path,
    start_byte: u64,
    headers: &http::HeaderMap,
    progress_callback: Option<&(dyn Fn(u64, Option<u64>) + Send + Sync)>,
) -> Result<u64> {
    // Create request with Range header for resume if needed
    let mut request_builder = client.get(url.clone());

    // Add all custom headers first
    for (name, value) in headers {
        request_builder = request_builder.header(name, value);
    }

    // Add Range header for resume if needed (this can override Range in custom headers)
    if start_byte > 0 {
        request_builder = request_builder.header("Range", format!("bytes={}-", start_byte));
    }

    // Send the request
    let response = request_builder
        .send()
        .await
        .map_err(|e| Error::Internal(format!("Request failed: {}", e)))?;

    // Check status
    if !response.status().is_success() && response.status() != 206 {
        if response.status() == 416 && start_byte > 0 {
            // Range not satisfiable - file is already complete
            return Ok(start_byte);
        }
        return Err(Error::Internal(format!(
            "HTTP error: {}",
            response.status()
        )));
    }

    // Get total size if available
    let total_size = response.content_length().map(|len| len + start_byte);

    // Open file for writing (append if resuming)
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(start_byte > 0)
        .write(true)
        .truncate(start_byte == 0)
        .open(file_path)
        .map_err(|e| Error::Internal(format!("Failed to open file: {}", e)))?;

    // Stream the response body
    let mut stream = response.bytes_stream();
    let mut bytes_downloaded = start_byte;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| Error::Internal(format!("Stream error: {}", e)))?;

        // Write chunk to file
        std::io::Write::write_all(&mut file, &chunk)
            .map_err(|e| Error::Internal(format!("Failed to write to file: {}", e)))?;

        bytes_downloaded += chunk.len() as u64;

        // Call progress callback if provided
        if let Some(callback) = progress_callback {
            callback(bytes_downloaded, total_size);
        }
    }

    // Ensure file is flushed
    std::io::Write::flush(&mut file)
        .map_err(|e| Error::Internal(format!("Failed to flush file: {}", e)))?;

    Ok(bytes_downloaded)
}

/// Async download logic for daemon process
async fn daemon_download_async(
    url: Url,
    file_path: PathBuf,
    _state_file: &std::path::Path,
    client: reqwest::Client,
    headers: http::HeaderMap,
    has_progress_callback: bool,
    write_state: impl Fn(&str, u64, Option<&str>),
) -> std::result::Result<(), String> {
    // Check if file already exists (for resume)
    let initial_size = if file_path.exists() {
        std::fs::metadata(&file_path)
            .map_err(|e| format!("Failed to check existing file: {}", e))?
            .len()
    } else {
        0
    };

    // Create request with Range header for resume if needed
    let mut request_builder = client.get(url);

    // Add all custom headers first
    for (name, value) in &headers {
        request_builder = request_builder.header(name, value);
    }

    // Add Range header for resume if needed (this can override Range in custom headers)
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
