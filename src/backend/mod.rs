//! Backend abstraction for HTTP client implementations

pub mod types;

#[cfg(target_vendor = "apple")]
pub mod foundation;

#[cfg(windows)]
pub mod windows;

pub mod reqwest;

use crate::{
    Result,
    cookies::{Cookie, CookieAcceptPolicy},
};
use std::time::Duration;
use types::{BackendRequest, BackendResponse};
use url::Url;

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

    /// Native Windows implementation using WinRT HTTP
    #[cfg(windows)]
    Windows(windows::WindowsBackend),

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

        #[cfg(all(windows, not(target_vendor = "apple")))]
        {
            // Default to Windows on Windows platforms
            Ok(Backend::Windows(windows::WindowsBackend::new()?))
        }

        #[cfg(not(any(target_vendor = "apple", windows)))]
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

    /// Use Windows backend (Windows only)
    #[cfg(windows)]
    pub fn windows() -> Result<Self> {
        Ok(Backend::Windows(windows::WindowsBackend::new()?))
    }

    /// Use Windows backend with configuration (Windows only)
    #[cfg(windows)]
    pub fn windows_with_config(config: BackendConfig) -> Result<Self> {
        Ok(Backend::Windows(
            windows::WindowsBackend::with_config(config)?,
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

            #[cfg(windows)]
            Backend::Windows(w) => w.execute(request).await,

            Backend::Reqwest(r) => r.execute(request).await,
        }
    }

    /// Execute a background download that survives app termination
    pub async fn execute_background_download(
        &self,
        url: Url,
        file_path: std::path::PathBuf,
        session_identifier: Option<String>,
        progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
    ) -> Result<crate::client::download::DownloadResponse> {
        match self {
            #[cfg(target_vendor = "apple")]
            Backend::Foundation(f) => {
                f.execute_background_download(url, file_path, session_identifier, progress_callback)
                    .await
            }

            #[cfg(windows)]
            Backend::Windows(w) => {
                w.execute_background_download(url, file_path, session_identifier, progress_callback)
                    .await
            }

            Backend::Reqwest(r) => {
                r.execute_background_download(url, file_path, session_identifier, progress_callback)
                    .await
            }
        }
    }

    /// Get the cookie jar if configured
    pub fn cookie_jar(&self) -> Option<&crate::CookieJar> {
        match self {
            #[cfg(target_vendor = "apple")]
            Backend::Foundation(f) => f.cookie_jar(),

            #[cfg(windows)]
            Backend::Windows(w) => w.cookie_jar(),

            Backend::Reqwest(r) => r.cookie_jar(),
        }
    }
}

/// Cookie storage backend implementations
#[derive(Clone, Debug)]
pub enum CookieStorage {
    /// Native Apple implementation using NSHTTPCookieStorage
    #[cfg(target_vendor = "apple")]
    Foundation(foundation::FoundationCookieStorage),

    /// Cross-platform implementation using reqwest cookie jar
    Reqwest(reqwest::ReqwestCookieStorage),
}

impl CookieStorage {
    /// Create a new cookie storage with default configuration
    pub fn new() -> Self {
        #[cfg(target_vendor = "apple")]
        {
            CookieStorage::Foundation(foundation::FoundationCookieStorage::new())
        }

        #[cfg(not(target_vendor = "apple"))]
        {
            CookieStorage::Reqwest(reqwest::ReqwestCookieStorage::new())
        }
    }

    /// Create a new cookie storage for a group container (Apple only)
    #[cfg(target_vendor = "apple")]
    pub fn for_group_container(identifier: &str) -> Result<Self> {
        Ok(CookieStorage::Foundation(
            foundation::FoundationCookieStorage::for_group_container(identifier),
        ))
    }

    /// Get all cookies
    pub fn all_cookies(&self) -> Vec<Cookie> {
        match self {
            #[cfg(target_vendor = "apple")]
            CookieStorage::Foundation(storage) => storage.all_cookies(),
            CookieStorage::Reqwest(storage) => storage.all_cookies(),
        }
    }

    /// Get cookies for a specific URL
    pub fn cookies_for_url(&self, url: &str) -> Result<Vec<Cookie>> {
        match self {
            #[cfg(target_vendor = "apple")]
            CookieStorage::Foundation(storage) => storage.cookies_for_url(url),
            CookieStorage::Reqwest(storage) => storage.cookies_for_url(url),
        }
    }

    /// Add a cookie
    pub fn add_cookie(&self, cookie: Cookie) -> Result<()> {
        match self {
            #[cfg(target_vendor = "apple")]
            CookieStorage::Foundation(storage) => storage.add_cookie(cookie),
            CookieStorage::Reqwest(storage) => storage.add_cookie(cookie),
        }
    }

    /// Remove a cookie
    pub fn remove_cookie(&self, cookie: Cookie) -> Result<()> {
        match self {
            #[cfg(target_vendor = "apple")]
            CookieStorage::Foundation(storage) => storage.remove_cookie(cookie),
            CookieStorage::Reqwest(storage) => storage.remove_cookie(cookie),
        }
    }

    /// Clear all cookies
    pub fn clear(&self) {
        match self {
            #[cfg(target_vendor = "apple")]
            CookieStorage::Foundation(storage) => storage.clear(),
            CookieStorage::Reqwest(storage) => storage.clear(),
        }
    }

    /// Set cookie acceptance policy
    pub fn set_cookie_accept_policy(&self, policy: CookieAcceptPolicy) {
        match self {
            #[cfg(target_vendor = "apple")]
            CookieStorage::Foundation(storage) => storage.set_cookie_accept_policy(policy),
            CookieStorage::Reqwest(storage) => storage.set_cookie_accept_policy(policy),
        }
    }
}
