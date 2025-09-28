//! Windows backend using WinRT HTTP and BITS

pub mod error;
pub mod websocket;
pub mod http_client;
// pub mod bits;  // TODO: Fix BITS COM threading issues

pub use error::*;
// pub use bits::BitsDownloadManager;  // TODO: Fix BITS COM threading issues
pub use websocket::{WindowsWebSocket, WindowsWebSocketBuilder};

use crate::backend::types::{BackendRequest, BackendResponse};
use crate::{Error, Result};
use url::Url;
use windows::{
    core::HSTRING,
    Foundation::Uri,
    Web::Http::{HttpClient, HttpMethod, HttpRequestMessage},
    Web::Http::Filters::HttpBaseProtocolFilter,
};

/// Windows backend using WinRT HTTP client
#[derive(Clone)]
pub struct WindowsBackend {
    /// The underlying Windows HTTP client
    client: HttpClient,
    /// Optional cookie jar for this backend
    cookie_jar: Option<crate::CookieJar>,
    /// Default headers to add to all requests
    default_headers: Option<http::HeaderMap>,
    // BITS download manager for background downloads (disabled for now)
    // bits_manager: Option<std::sync::Arc<BitsDownloadManager>>,
}

impl WindowsBackend {
    /// Create a new Windows backend with default configuration
    pub fn new() -> Result<Self> {
        let client = HttpClient::new().map_err(|e| {
            Error::Internal(format!("Failed to create Windows HTTP client: {}", e))
        })?;

        // BITS disabled for now due to COM threading issues
        // let bits_manager = BitsDownloadManager::new().ok().map(std::sync::Arc::new);

        Ok(Self {
            client,
            cookie_jar: None,
            default_headers: None,
            // bits_manager,
        })
    }

    /// Create a new Windows backend with configuration
    pub fn with_config(config: crate::backend::BackendConfig) -> Result<Self> {
        // Create HTTP filter for configuration
        let filter = HttpBaseProtocolFilter::new().map_err(|e| {
            Error::Internal(format!("Failed to create HTTP filter: {}", e))
        })?;

        // Apply configuration to filter
        if config.ignore_certificate_errors.unwrap_or(false) {
            // TODO: Configure certificate validation if supported
        }

        // Create client with configured filter
        let client = HttpClient::Create(&filter).map_err(|e| {
            Error::Internal(format!("Failed to create Windows HTTP client with filter: {}", e))
        })?;

        // Configure default headers
        if let Some(ref default_headers) = config.default_headers {
            if let Ok(client_headers) = client.DefaultRequestHeaders() {
                for (name, value) in default_headers {
                    let header_name = HSTRING::from(name.as_str());
                    let header_value = HSTRING::from(
                        value.to_str().unwrap_or_default()
                    );
                    let _ = client_headers.TryAppendWithoutValidation(&header_name, &header_value);
                }
            }
        }

        // Set user agent if provided
        if let Some(ref user_agent) = config.user_agent {
            if let Ok(client_headers) = client.DefaultRequestHeaders() {
                if let Ok(user_agent_header) = client_headers.UserAgent() {
                    let _ = user_agent_header.TryParseAdd(&HSTRING::from(user_agent.as_str()));
                }
            }
        }

        // BITS disabled for now due to COM threading issues
        // let bits_manager = BitsDownloadManager::new().ok().map(std::sync::Arc::new);

        Ok(Self {
            client,
            cookie_jar: config.cookie_jar,
            default_headers: config.default_headers,
            // bits_manager,
        })
    }

    /// Execute an HTTP request using Windows HTTP client
    pub async fn execute(&self, request: BackendRequest) -> Result<BackendResponse> {
        // Convert URL to Windows URI
        let uri = Uri::CreateUri(&HSTRING::from(request.url.as_str()))
            .map_err(|_| Error::InvalidUrl)?;

        // Convert HTTP method
        let method = match request.method.as_str() {
            "GET" => HttpMethod::Get().unwrap(),
            "POST" => HttpMethod::Post().unwrap(),
            "PUT" => HttpMethod::Put().unwrap(),
            "DELETE" => HttpMethod::Delete().unwrap(),
            "HEAD" => HttpMethod::Head().unwrap(),
            "PATCH" => HttpMethod::Patch().unwrap(),
            _ => return Err(Error::Internal(format!("Unsupported HTTP method: {}", request.method))),
        };

        // Create request message
        let request_message = HttpRequestMessage::Create(&method, &uri)
            .map_err(|e| Error::Internal(format!("Failed to create request message: {}", e)))?;

        // Add headers
        let headers = request_message.Headers()
            .map_err(|e| Error::Internal(format!("Failed to get request headers: {}", e)))?;

        // Add default headers first
        if let Some(ref default_headers) = self.default_headers {
            for (name, value) in default_headers {
                let header_name = HSTRING::from(name.as_str());
                let header_value = HSTRING::from(
                    value.to_str().map_err(|_| Error::Internal("Invalid header value".to_string()))?
                );
                let _ = headers.TryAppendWithoutValidation(&header_name, &header_value);
            }
        }

        // Add request-specific headers
        for (name, value) in &request.headers {
            let header_name = HSTRING::from(name.as_str());
            let header_value = HSTRING::from(
                value.to_str().map_err(|_| Error::Internal("Invalid header value".to_string()))?
            );
            let _ = headers.TryAppendWithoutValidation(&header_name, &header_value);
        }

        // Handle request body
        if let Some(body) = request.body {
            let content = http_client::convert_body_to_http_content(body)?;
            request_message.SetContent(&content)
                .map_err(|e| Error::Internal(format!("Failed to set request content: {}", e)))?;
        }

        // Send the request using Windows async operation
        let response = self.client.SendRequestAsync(&request_message)
            .map_err(|e| Error::Network {
                code: -1,
                message: format!("Failed to send request: {}", e),
            })?
            .await
            .map_err(|e| error::map_windows_error(e))?;

        // Convert response to BackendResponse
        http_client::convert_http_response_to_backend_response(response).await
    }

    /// Execute a background download
    pub async fn execute_background_download(
        &self,
        url: Url,
        file_path: std::path::PathBuf,
        _session_identifier: Option<String>,
        progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
    ) -> Result<crate::client::download::DownloadResponse> {
        // For now, fall back to regular download
        // TODO: Implement BITS support
        let request = BackendRequest {
            method: http::Method::GET,
            url,
            headers: http::HeaderMap::new(),
            body: None,
            progress_callback: progress_callback.map(|cb| std::sync::Arc::new(cb) as std::sync::Arc<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>),
        };

        let response = self.execute(request).await?;

        // Stream response to file
        let mut file = tokio::fs::File::create(&file_path).await
            .map_err(|e| Error::Internal(format!("Failed to create file: {}", e)))?;

        let mut receiver = response.body_receiver;
        let mut bytes_downloaded = 0u64;

        while let Some(chunk_result) = receiver.recv().await {
            let chunk = chunk_result?;
            bytes_downloaded += chunk.len() as u64;

            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await
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