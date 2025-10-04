//! Shared types between backend implementations

use crate::body::Body;
use http::{HeaderMap, Method, StatusCode};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use url::Url;

/// Platform-agnostic HTTP request
pub struct BackendRequest {
    /// HTTP method for the request
    pub method: Method,
    /// URL for the request
    pub url: Url,
    /// Headers for the request
    pub headers: HeaderMap,
    /// Optional body content
    pub body: Option<Body>,
    /// Optional progress callback for uploads
    pub progress_callback: Option<ProgressCallback>,
    /// Optional timeout for the request
    pub timeout: Option<Duration>,
}

/// Platform-agnostic HTTP response
pub struct BackendResponse {
    /// HTTP status code
    pub status: StatusCode,
    /// Response headers
    pub headers: HeaderMap,
    /// The URL that was requested
    pub url: Url,
    /// Stream of response body bytes
    pub body_receiver: mpsc::Receiver<Result<bytes::Bytes, crate::Error>>,
    /// Headers from redirect responses (for cookie processing)
    pub redirect_headers: Vec<HeaderMap>,
}

/// Platform-agnostic download handle
pub struct BackendDownloadHandle {
    /// Unique identifier for the download
    pub id: String,
    /// Platform-specific handle data
    pub platform_handle: PlatformHandle,
}

/// Platform-specific download handle data
pub enum PlatformHandle {
    /// Foundation (NSURLSession) download task on Apple platforms
    #[cfg(target_vendor = "apple")]
    Foundation {
        /// NSURLSession download task
        task: objc2::rc::Retained<objc2_foundation::NSURLSessionDownloadTask>,
    },

    /// Reqwest-based download task for cross-platform support
    Reqwest {
        /// Tokio task handle for the download
        task_handle: tokio::task::JoinHandle<Result<PathBuf, crate::Error>>,
    },
}

/// Progress information for downloads/uploads
#[derive(Debug, Clone)]
pub struct ProgressInfo {
    /// Number of bytes transferred so far
    pub bytes_transferred: u64,
    /// Total bytes to transfer (if known)
    pub total_bytes: Option<u64>,
}

/// Callback type for progress reporting
pub type ProgressCallback = Arc<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>;
