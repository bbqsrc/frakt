//! Shared types between backend implementations

use crate::body::Body;
use http::{HeaderMap, Method, StatusCode};
use std::path::PathBuf;
use tokio::sync::mpsc;

/// Platform-agnostic HTTP request
#[derive(Debug, Clone)]
pub struct BackendRequest {
    /// HTTP method for the request
    pub method: Method,
    /// URL for the request
    pub url: String,
    /// Headers for the request
    pub headers: HeaderMap,
    /// Optional body content
    pub body: Option<Body>,
}

/// Platform-agnostic HTTP response
pub struct BackendResponse {
    /// HTTP status code
    pub status: StatusCode,
    /// Response headers
    pub headers: HeaderMap,
    /// Stream of response body bytes
    pub body_receiver: mpsc::Receiver<Result<bytes::Bytes, crate::Error>>,
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
pub type ProgressCallback = Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>;
