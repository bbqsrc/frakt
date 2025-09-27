//! HTTP client implementation

pub mod background;
pub mod download;
pub mod upload;

pub use background::BackgroundDownloadBuilder;
pub use download::{DownloadBuilder, DownloadResponse};
pub use upload::UploadBuilder;

use crate::session::SessionConfigurationBuilder;
use crate::{Request, RequestBuilder, Result};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSURLSession;
use std::time::Duration;

/// HTTP client for making requests
#[derive(Clone)]
pub struct Client {
    session: Retained<NSURLSession>,
    delegate: Retained<crate::delegate::DataTaskDelegate>,
    base_url: Option<String>,
    cookie_jar: Option<crate::CookieJar>,
}

impl Client {
    /// Create a new client with default configuration
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// Create a client builder
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    /// Create a GET request
    pub fn get(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::GET,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a POST request
    pub fn post(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::POST,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a PUT request
    pub fn put(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::PUT,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a DELETE request
    pub fn delete(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::DELETE,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a PATCH request
    pub fn patch(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::PATCH,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Create a HEAD request
    pub fn head(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(
            crate::request::Method::HEAD,
            self.resolve_url(url),
            self.session.clone(),
            self.delegate.clone(),
        )
    }

    /// Execute a request
    pub async fn execute(&self, request: Request) -> Result<crate::Response> {
        request.send().await
    }

    /// Download a file directly to disk
    pub fn download(&self, url: &str) -> DownloadBuilder {
        DownloadBuilder::new(url.to_string(), self.session.clone())
    }

    /// Download a file in the background (continues when app is suspended)
    pub fn download_background(&self, url: &str) -> BackgroundDownloadBuilder {
        BackgroundDownloadBuilder::new(url.to_string())
    }

    /// Upload a file using NSURLSessionUploadTask
    pub fn upload(&self, url: &str) -> UploadBuilder {
        UploadBuilder::new(url.to_string(), self.session.clone())
    }

    /// Get the cookie jar for this client
    pub fn cookie_jar(&self) -> Option<&crate::CookieJar> {
        self.cookie_jar.as_ref()
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
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            self.session.finishTasksAndInvalidate();
        }
    }
}

/// Builder for creating HTTP clients
pub struct ClientBuilder {
    config_builder: SessionConfigurationBuilder,
    base_url: Option<String>,
    cookie_jar: Option<crate::CookieJar>,
}

impl ClientBuilder {
    /// Create a new client builder
    pub fn new() -> Self {
        Self {
            config_builder: SessionConfigurationBuilder::new(),
            base_url: None,
            cookie_jar: None,
        }
    }

    /// Set the base URL for all requests
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set request timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config_builder = self.config_builder.timeout(timeout);
        self
    }

    /// Set user agent
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.config_builder = self.config_builder.user_agent(user_agent);
        self
    }

    /// Add a default header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.config_builder = self.config_builder.header(name, value);
        self
    }

    /// Enable or disable cookies
    pub fn use_cookies(mut self, use_cookies: bool) -> Self {
        self.config_builder = self.config_builder.use_cookies(use_cookies);
        self
    }

    /// Ignore certificate errors (for testing only)
    pub fn ignore_certificate_errors(mut self, ignore: bool) -> Self {
        self.config_builder = self.config_builder.ignore_certificate_errors(ignore);
        self
    }

    /// Set a custom cookie jar
    pub fn cookie_jar(mut self, cookie_jar: crate::CookieJar) -> Self {
        self.cookie_jar = Some(cookie_jar);
        self
    }

    /// Set HTTP proxy
    pub fn http_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        self.config_builder = self.config_builder.http_proxy(host, port);
        self
    }

    /// Set HTTPS proxy
    pub fn https_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        self.config_builder = self.config_builder.https_proxy(host, port);
        self
    }

    /// Set SOCKS proxy
    pub fn socks_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        self.config_builder = self.config_builder.socks_proxy(host, port);
        self
    }

    /// Set proxy authentication
    pub fn proxy_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.config_builder = self.config_builder.proxy_auth(username, password);
        self
    }

    /// Build the client
    pub fn build(self) -> Result<Client> {
        let _should_ignore_certs = self.config_builder.should_ignore_certificate_errors();
        let config = self.config_builder.build()?;

        // Set cookie storage if a custom cookie jar is provided
        if let Some(ref cookie_jar) = self.cookie_jar {
            unsafe {
                config.setHTTPCookieStorage(Some(cookie_jar.storage()));
            }
        }

        // Create delegate and session with delegate
        let delegate = crate::delegate::DataTaskDelegate::new();
        let session = unsafe {
            NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &config,
                Some(ProtocolObject::from_ref(&*delegate)),
                None,
            )
        };

        Ok(Client {
            session,
            delegate,
            base_url: self.base_url,
            cookie_jar: self.cookie_jar,
        })
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}
