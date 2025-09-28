//! Backend abstraction for HTTP client implementations

pub mod types;

#[cfg(target_vendor = "apple")]
pub mod foundation;

pub mod reqwest;

use crate::Result;
use std::time::Duration;
use types::{BackendRequest, BackendResponse};

/// Configuration for backend creation
#[derive(Clone, Debug, Default)]
pub struct BackendConfig {
    /// Request timeout
    pub timeout: Option<Duration>,
    /// User agent string
    pub user_agent: Option<String>,
    /// Whether to ignore certificate errors
    pub ignore_certificate_errors: Option<bool>,
    /// Default headers to add to all requests
    pub default_headers: Option<http::HeaderMap>,
    /// Enable or disable cookies
    pub use_cookies: Option<bool>,
    /// Custom cookie jar
    pub cookie_jar: Option<crate::CookieJar>,
    /// HTTP proxy configuration
    pub http_proxy: Option<crate::client::ProxyConfig>,
    /// HTTPS proxy configuration
    pub https_proxy: Option<crate::client::ProxyConfig>,
    /// SOCKS proxy configuration
    pub socks_proxy: Option<crate::client::ProxyConfig>,
}

/// HTTP client backend implementations
#[derive(Clone)]
pub enum Backend {
    /// Native Apple implementation using NSURLSession
    #[cfg(target_vendor = "apple")]
    Foundation(foundation::FoundationBackend),

    /// Cross-platform implementation using reqwest
    Reqwest(reqwest::ReqwestBackend),
}

impl Backend {
    /// Auto-select best backend for platform
    pub fn default_for_platform() -> Result<Self> {
        #[cfg(target_vendor = "apple")]
        {
            // Default to Foundation on Apple platforms
            Ok(Backend::Foundation(foundation::FoundationBackend::new()?))
        }

        #[cfg(not(target_vendor = "apple"))]
        {
            // Use reqwest everywhere else
            Ok(Backend::Reqwest(reqwest::ReqwestBackend::new()?))
        }
    }

    /// Explicitly use reqwest backend (works on all platforms)
    pub fn reqwest() -> Result<Self> {
        Ok(Backend::Reqwest(reqwest::ReqwestBackend::new()?))
    }

    /// Use Foundation backend (Apple only)
    #[cfg(target_vendor = "apple")]
    pub fn foundation() -> Result<Self> {
        Ok(Backend::Foundation(foundation::FoundationBackend::new()?))
    }

    /// Use Foundation backend with configuration (Apple only)
    #[cfg(target_vendor = "apple")]
    pub fn foundation_with_config(config: BackendConfig) -> Result<Self> {
        Ok(Backend::Foundation(
            foundation::FoundationBackend::with_config(config)?,
        ))
    }

    /// Use reqwest backend with configuration
    pub fn reqwest_with_config(config: BackendConfig) -> Result<Self> {
        Ok(Backend::Reqwest(reqwest::ReqwestBackend::with_config(
            config,
        )?))
    }

    /// Execute an HTTP request
    pub async fn execute(&self, request: BackendRequest) -> Result<BackendResponse> {
        match self {
            #[cfg(target_vendor = "apple")]
            Backend::Foundation(f) => f.execute(request).await,

            Backend::Reqwest(r) => r.execute(request).await,
        }
    }
}
