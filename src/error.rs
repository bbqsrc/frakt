//! Error types and NSError mapping

use thiserror::Error;

/// Result type for this crate
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for NSURLSession operations
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid URL
    #[error("Invalid URL")]
    InvalidUrl,

    /// Network error from NSURLSession
    #[error("Network error: {message} (code: {code})")]
    Network {
        /// Error code from NSURLError
        code: i64,
        /// Error message
        message: String,
    },

    /// TLS/Certificate error
    #[error("TLS error: {message}")]
    Tls {
        /// Error message
        message: String,
    },

    /// Timeout error
    #[error("Request timed out")]
    Timeout,

    /// Request was cancelled
    #[error("Request was cancelled")]
    Cancelled,

    /// UTF-8 conversion error
    #[error("UTF-8 conversion error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    /// JSON serialization/deserialization error
    #[cfg(feature = "json")]
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Response body too large
    #[error("Response body exceeds maximum size")]
    ResponseTooLarge,

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

#[cfg(target_vendor = "apple")]
impl Error {
    /// Convert NSError to our Error type
    pub(crate) fn from_ns_error(error: &objc2_foundation::NSError) -> Self {
        use objc2_foundation::NSURLErrorDomain;

        let domain = error.domain();
        let code = error.code();
        let message = unsafe {
            objc2::rc::autoreleasepool(|pool| error.localizedDescription().to_str(pool).to_string())
        };

        if unsafe { domain.isEqualToString(&NSURLErrorDomain) } {
            match code {
                -1001 => Error::Timeout,
                -999 => Error::Cancelled,
                -1200..=-1000 => Error::Tls { message },
                _ => Error::Network {
                    code: code.try_into().unwrap(),
                    message,
                },
            }
        } else {
            Error::Internal(format!(
                "Domain: {}, Code: {}, Message: {}",
                unsafe { objc2::rc::autoreleasepool(|pool| domain.to_str(pool).to_string()) },
                code,
                message
            ))
        }
    }
}
