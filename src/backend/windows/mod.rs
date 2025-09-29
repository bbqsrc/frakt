//! Windows backend using WinRT HTTP and BITS

pub mod bits;
pub mod cookies;
pub mod error;
pub mod http_client;
pub mod websocket;

pub use bits::BitsDownloadManager;
pub use cookies::WindowsCookieStorage;
pub use error::*;
pub use websocket::{WindowsWebSocket, WindowsWebSocketBuilder};

use crate::backend::types::{BackendRequest, BackendResponse};
use crate::{Error, Result};
use std::time::Duration;
use url::Url;

/// Windows backend using WinHTTP client
#[derive(Clone)]
pub struct WindowsBackend {
    /// User agent string for HTTP requests
    user_agent: String,
    /// Optional cookie jar for this backend
    cookie_jar: Option<crate::CookieJar>,
    /// Windows cookie storage
    cookie_storage: Option<cookies::WindowsCookieStorage>,
    /// Default headers to add to all requests
    default_headers: Option<http::HeaderMap>,
    /// Request timeout
    timeout: Option<Duration>,
    /// BITS download manager for background downloads
    bits_manager: Option<std::sync::Arc<BitsDownloadManager>>,
}

impl WindowsBackend {
    /// Create a new Windows backend with default configuration
    pub fn new() -> Result<Self> {
        // Try to create BITS manager, but don't fail if it's not available
        let bits_manager = BitsDownloadManager::new().ok().map(std::sync::Arc::new);

        Ok(Self {
            user_agent: "frakt/1.0".to_string(),
            cookie_jar: None,
            cookie_storage: None,
            default_headers: None,
            timeout: None,
            bits_manager,
        })
    }

    /// Create a new Windows backend with configuration
    pub fn with_config(config: crate::backend::BackendConfig) -> Result<Self> {
        // Try to create BITS manager, but don't fail if it's not available
        let bits_manager = BitsDownloadManager::new().ok().map(std::sync::Arc::new);

        let user_agent = config.user_agent.unwrap_or_else(|| "frakt/1.0".to_string());

        // Create cookie storage if cookies are enabled
        let cookie_storage = if config.use_cookies.unwrap_or(false) {
            cookies::WindowsCookieStorage::new().ok()
        } else {
            None
        };

        Ok(Self {
            user_agent,
            cookie_jar: config.cookie_jar,
            cookie_storage,
            default_headers: config.default_headers,
            timeout: config.timeout,
            bits_manager,
        })
    }

    /// Execute an HTTP request using WinHTTP
    pub async fn execute(&self, request: BackendRequest) -> Result<BackendResponse> {
        // For now, use the http_client module's new WinHTTP implementation
        http_client::execute_winhttp_request(
            request,
            &self.user_agent,
            &self.default_headers,
            &self.timeout,
            &self.cookie_storage,
        )
        .await
    }

    /// Execute a background download
    pub async fn execute_background_download(
        &self,
        url: Url,
        file_path: std::path::PathBuf,
        session_identifier: Option<String>,
        progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
    ) -> Result<crate::client::download::DownloadResponse> {
        // Try BITS first if available, fall back to regular download
        if let Some(ref bits_manager) = self.bits_manager {
            // For now, disable progress callbacks with BITS to avoid lifetime issues
            // TODO: Fix progress callback lifetime management
            match bits_manager
                .start_background_download(url.clone(), file_path.clone(), session_identifier, None)
                .await
            {
                Ok(response) => return Ok(response),
                Err(e) => {
                    eprintln!(
                        "BITS download failed, falling back to regular download: {}",
                        e
                    );
                }
            }
        }
        let request = BackendRequest {
            method: http::Method::GET,
            url,
            headers: http::HeaderMap::new(),
            body: None,
            progress_callback: progress_callback.map(|cb| {
                std::sync::Arc::new(cb)
                    as std::sync::Arc<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>
            }),
        };

        let response = self.execute(request).await?;

        // Stream response to file
        let mut file = tokio::fs::File::create(&file_path)
            .await
            .map_err(|e| Error::Internal(format!("Failed to create file: {}", e)))?;

        let mut receiver = response.body_receiver;
        let mut bytes_downloaded = 0u64;

        while let Some(chunk_result) = receiver.recv().await {
            let chunk = chunk_result?;
            bytes_downloaded += chunk.len() as u64;

            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(|e| Error::Internal(format!("Failed to write to file: {}", e)))?;
        }

        Ok(crate::client::download::DownloadResponse {
            file_path,
            bytes_downloaded,
        })
    }

    /// Get the cookie jar if configured
    pub fn cookie_jar(&self) -> Option<&crate::CookieJar> {
        self.cookie_jar.as_ref()
    }
}
