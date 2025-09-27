//! Session configuration and management

use crate::Result;
use objc2::rc::Retained;
use objc2_foundation::{
    NSDictionary, NSString, NSURLRequestCachePolicy, NSURLSessionConfiguration,
};
use std::collections::HashMap;
use std::time::Duration;

/// Caching behavior for requests
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachingBehavior {
    /// Use default caching behavior
    Default,
    /// Disable caching
    Disabled,
}

/// Session configuration type
#[derive(Debug, Clone)]
pub enum SessionType {
    /// Default foreground session
    Default,
    /// Background session with identifier
    Background(String),
}

/// Session configuration builder
#[derive(Debug)]
pub struct SessionConfigurationBuilder {
    timeout: Option<Duration>,
    caching: CachingBehavior,
    use_cookies: bool,
    use_default_proxy: bool,
    headers: HashMap<String, String>,
    ignore_certificate_errors: bool,
    session_type: SessionType,
}

impl Default for SessionConfigurationBuilder {
    fn default() -> Self {
        Self {
            timeout: None,
            caching: CachingBehavior::Default,
            use_cookies: true,
            use_default_proxy: true,
            headers: HashMap::new(),
            ignore_certificate_errors: false,
            session_type: SessionType::Default,
        }
    }
}

impl SessionConfigurationBuilder {
    /// Create a new configuration builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set request timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set caching behavior
    pub fn caching(mut self, caching: CachingBehavior) -> Self {
        self.caching = caching;
        self
    }

    /// Enable or disable cookies
    pub fn use_cookies(mut self, use_cookies: bool) -> Self {
        self.use_cookies = use_cookies;
        self
    }

    /// Enable or disable default proxy settings
    pub fn use_default_proxy(mut self, use_proxy: bool) -> Self {
        self.use_default_proxy = use_proxy;
        self
    }

    /// Add a default header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set user agent
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.headers
            .insert("User-Agent".to_string(), user_agent.into());
        self
    }

    /// Ignore certificate errors (for testing only)
    pub fn ignore_certificate_errors(mut self, ignore: bool) -> Self {
        self.ignore_certificate_errors = ignore;
        self
    }

    /// Create a background session with the given identifier
    /// Background sessions allow downloads/uploads to continue when the app is suspended
    pub fn background_session(mut self, identifier: impl Into<String>) -> Self {
        self.session_type = SessionType::Background(identifier.into());
        self
    }

    /// Build the NSURLSessionConfiguration
    pub(crate) fn build(self) -> Result<Retained<NSURLSessionConfiguration>> {
        unsafe {
            let config = match self.session_type {
                SessionType::Default => NSURLSessionConfiguration::defaultSessionConfiguration(),
                SessionType::Background(identifier) => {
                    NSURLSessionConfiguration::backgroundSessionConfigurationWithIdentifier(
                        &NSString::from_str(&identifier),
                    )
                }
            };

            // Set caching behavior
            if self.caching == CachingBehavior::Disabled {
                config.setRequestCachePolicy(NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData);
            }

            // Set proxy settings
            if !self.use_default_proxy {
                config.setConnectionProxyDictionary(Some(&*NSDictionary::new()));
            }

            // Set cookie handling
            if !self.use_cookies {
                config.setHTTPShouldSetCookies(false);
            }

            // Set timeout
            if let Some(timeout) = self.timeout {
                let timeout_interval = timeout.as_secs_f64();
                config.setTimeoutIntervalForRequest(timeout_interval);
            }

            // Set default headers
            if !self.headers.is_empty() {
                let keys: Vec<_> = self.headers.keys().map(|k| NSString::from_str(k)).collect();
                let values: Vec<_> = self
                    .headers
                    .values()
                    .map(|v| NSString::from_str(v))
                    .collect();

                let dict = NSDictionary::from_retained_objects(
                    &keys.iter().map(|s| &**s).collect::<Vec<_>>(),
                    &values,
                );

                config.setHTTPAdditionalHeaders(Some(
                    Retained::cast_unchecked::<NSDictionary>(dict).as_ref(),
                ));
            }

            Ok(config)
        }
    }

    /// Get whether certificate errors should be ignored
    pub(crate) fn should_ignore_certificate_errors(&self) -> bool {
        self.ignore_certificate_errors
    }

    /// Get whether this is a background session
    pub(crate) fn is_background_session(&self) -> bool {
        matches!(self.session_type, SessionType::Background(_))
    }

    /// Get the session type
    pub(crate) fn session_type(&self) -> &SessionType {
        &self.session_type
    }
}
