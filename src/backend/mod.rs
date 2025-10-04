//! Backend abstraction for HTTP client implementations

pub mod cookie_store_impl;
pub mod types;

#[cfg(target_vendor = "apple")]
pub mod foundation;

#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "android")]
pub mod android;

#[cfg(target_os = "android")]
pub use android::{check_permission, list_permissions, start_netlog, stop_netlog, test_dns};

pub mod reqwest;

pub use cookie_store_impl::CookieStoreImpl;

use crate::{
    BackendType, Result,
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

    /// Native Android implementation using Cronet
    #[cfg(target_os = "android")]
    Android(android::AndroidBackend),

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

        #[cfg(all(target_os = "android", not(any(target_vendor = "apple", windows))))]
        {
            // Default to Android backend on Android
            Ok(Backend::Android(android::AndroidBackend::new()?))
        }

        #[cfg(not(any(target_vendor = "apple", windows, target_os = "android")))]
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
        Ok(Backend::Windows(windows::WindowsBackend::with_config(
            config,
        )?))
    }

    /// Use Android backend (Android only)
    #[cfg(target_os = "android")]
    pub fn android() -> Result<Self> {
        Ok(Backend::Android(android::AndroidBackend::new()?))
    }

    /// Use Android backend with configuration (Android only)
    #[cfg(target_os = "android")]
    pub fn android_with_config(config: BackendConfig) -> Result<Self> {
        Ok(Backend::Android(android::AndroidBackend::with_config(
            config,
        )?))
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

            #[cfg(target_os = "android")]
            Backend::Android(a) => a.execute(request).await,

            Backend::Reqwest(r) => r.execute(request).await,
        }
    }

    /// Execute a background download that survives app termination
    pub async fn execute_background_download(
        &self,
        url: Url,
        file_path: std::path::PathBuf,
        session_identifier: Option<String>,
        headers: http::HeaderMap,
        progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
        error_for_status: bool,
    ) -> Result<crate::client::download::DownloadResponse> {
        match self {
            #[cfg(target_vendor = "apple")]
            Backend::Foundation(f) => {
                f.execute_background_download(
                    url,
                    file_path,
                    session_identifier,
                    headers,
                    progress_callback,
                    error_for_status,
                )
                .await
            }

            #[cfg(windows)]
            Backend::Windows(w) => {
                w.execute_background_download(
                    url,
                    file_path,
                    session_identifier,
                    headers,
                    progress_callback,
                    error_for_status,
                )
                .await
            }

            #[cfg(target_os = "android")]
            Backend::Android(a) => {
                a.execute_background_download(
                    url,
                    file_path,
                    session_identifier,
                    headers,
                    progress_callback,
                    error_for_status,
                )
                .await
            }

            Backend::Reqwest(r) => {
                r.execute_background_download(
                    url,
                    file_path,
                    session_identifier,
                    headers,
                    progress_callback,
                    error_for_status,
                )
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

            #[cfg(target_os = "android")]
            Backend::Android(a) => a.cookie_jar(),

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

    /// RFC 6265 compliant implementation using cookie_store
    /// Used by Windows, Android, and Reqwest backends
    CookieStore(CookieStoreImpl),
}

impl CookieStorage {
    /// Create a new cookie storage with default configuration
    pub fn new(backend: Backend) -> Self {
        match backend {
            #[cfg(target_vendor = "apple")]
            Backend::Foundation(_) => {
                CookieStorage::Foundation(foundation::FoundationCookieStorage::new())
            }
            // All non-Apple backends use the unified CookieStore implementation
            #[cfg(windows)]
            Backend::Windows(_) => CookieStorage::CookieStore(CookieStoreImpl::new()),

            #[cfg(target_os = "android")]
            Backend::Android(_) => CookieStorage::CookieStore(CookieStoreImpl::new()),

            Backend::Reqwest(_) => CookieStorage::CookieStore(CookieStoreImpl::new()),
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

            CookieStorage::CookieStore(storage) => storage.all_cookies(),
        }
    }

    /// Get cookies for a specific URL
    pub fn cookies_for_url(&self, url: &str) -> Result<Vec<Cookie>> {
        match self {
            #[cfg(target_vendor = "apple")]
            CookieStorage::Foundation(storage) => storage.cookies_for_url(url),

            CookieStorage::CookieStore(storage) => storage.cookies_for_url(url),
        }
    }

    /// Add a cookie
    pub fn add_cookie(&self, cookie: Cookie) -> Result<()> {
        match self {
            #[cfg(target_vendor = "apple")]
            CookieStorage::Foundation(storage) => storage.add_cookie(cookie),

            CookieStorage::CookieStore(storage) => storage.add_cookie(cookie),
        }
    }

    /// Remove a cookie
    pub fn remove_cookie(&self, cookie: Cookie) -> Result<()> {
        match self {
            #[cfg(target_vendor = "apple")]
            CookieStorage::Foundation(storage) => storage.remove_cookie(cookie),

            CookieStorage::CookieStore(storage) => storage.remove_cookie(cookie),
        }
    }

    /// Clear all cookies
    pub fn clear(&self) {
        match self {
            #[cfg(target_vendor = "apple")]
            CookieStorage::Foundation(storage) => storage.clear(),

            CookieStorage::CookieStore(storage) => storage.clear(),
        }
    }

    /// Set cookie acceptance policy
    pub fn set_cookie_accept_policy(&self, policy: CookieAcceptPolicy) {
        match self {
            #[cfg(target_vendor = "apple")]
            CookieStorage::Foundation(storage) => storage.set_cookie_accept_policy(policy),

            CookieStorage::CookieStore(storage) => storage.set_cookie_accept_policy(policy),
        }
    }
}
