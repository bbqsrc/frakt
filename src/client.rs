//! HTTP client implementation

use crate::{Request, RequestBuilder, Result};
use crate::session::SessionConfigurationBuilder;
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
        RequestBuilder::new(crate::request::Method::GET, self.resolve_url(url), self.session.clone(), self.delegate.clone())
    }

    /// Create a POST request
    pub fn post(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(crate::request::Method::POST, self.resolve_url(url), self.session.clone(), self.delegate.clone())
    }

    /// Create a PUT request
    pub fn put(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(crate::request::Method::PUT, self.resolve_url(url), self.session.clone(), self.delegate.clone())
    }

    /// Create a DELETE request
    pub fn delete(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(crate::request::Method::DELETE, self.resolve_url(url), self.session.clone(), self.delegate.clone())
    }

    /// Create a PATCH request
    pub fn patch(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(crate::request::Method::PATCH, self.resolve_url(url), self.session.clone(), self.delegate.clone())
    }

    /// Create a HEAD request
    pub fn head(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(crate::request::Method::HEAD, self.resolve_url(url), self.session.clone(), self.delegate.clone())
    }

    /// Execute a request
    pub async fn execute(&self, request: Request) -> Result<crate::Response> {
        request.send().await
    }

    fn resolve_url(&self, url: &str) -> String {
        match &self.base_url {
            Some(base) => {
                if url.starts_with("http://") || url.starts_with("https://") {
                    url.to_string()
                } else {
                    format!("{}/{}", base.trim_end_matches('/'), url.trim_start_matches('/'))
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
}

impl ClientBuilder {
    /// Create a new client builder
    pub fn new() -> Self {
        Self {
            config_builder: SessionConfigurationBuilder::new(),
            base_url: None,
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

    /// Build the client
    pub fn build(self) -> Result<Client> {
        let _should_ignore_certs = self.config_builder.should_ignore_certificate_errors();
        let config = self.config_builder.build()?;

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
        })
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}