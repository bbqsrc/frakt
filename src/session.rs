//! Session configuration and management

use crate::Result;
use http::{HeaderMap, HeaderValue, header};
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

/// Proxy configuration
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// HTTP proxy host
    pub http_host: Option<String>,
    /// HTTP proxy port
    pub http_port: Option<u16>,
    /// HTTPS proxy host
    pub https_host: Option<String>,
    /// HTTPS proxy port
    pub https_port: Option<u16>,
    /// SOCKS proxy host
    pub socks_host: Option<String>,
    /// SOCKS proxy port
    pub socks_port: Option<u16>,
    /// Proxy username for authentication
    pub username: Option<String>,
    /// Proxy password for authentication
    pub password: Option<String>,
}

/// Session configuration builder
#[derive(Debug)]
pub struct SessionConfigurationBuilder {
    timeout: Option<Duration>,
    caching: CachingBehavior,
    use_cookies: bool,
    use_default_proxy: bool,
    proxy_config: Option<ProxyConfig>,
    headers: HeaderMap,
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
            proxy_config: None,
            headers: HeaderMap::new(),
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
        let header_name: http::HeaderName = name.into().parse().expect("Invalid header name");
        let header_value = HeaderValue::from_str(&value.into()).expect("Invalid header value");
        self.headers.insert(header_name, header_value);
        self
    }

    /// Set user agent
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        let header_value =
            HeaderValue::from_str(&user_agent.into()).expect("Invalid user agent value");
        self.headers.insert(header::USER_AGENT, header_value);
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

    /// Set HTTP proxy
    pub fn http_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        let mut config = self.proxy_config.unwrap_or(ProxyConfig {
            http_host: None,
            http_port: None,
            https_host: None,
            https_port: None,
            socks_host: None,
            socks_port: None,
            username: None,
            password: None,
        });
        config.http_host = Some(host.into());
        config.http_port = Some(port);
        self.proxy_config = Some(config);
        self
    }

    /// Set HTTPS proxy
    pub fn https_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        let mut config = self.proxy_config.unwrap_or(ProxyConfig {
            http_host: None,
            http_port: None,
            https_host: None,
            https_port: None,
            socks_host: None,
            socks_port: None,
            username: None,
            password: None,
        });
        config.https_host = Some(host.into());
        config.https_port = Some(port);
        self.proxy_config = Some(config);
        self
    }

    /// Set SOCKS proxy
    pub fn socks_proxy(mut self, host: impl Into<String>, port: u16) -> Self {
        let mut config = self.proxy_config.unwrap_or(ProxyConfig {
            http_host: None,
            http_port: None,
            https_host: None,
            https_port: None,
            socks_host: None,
            socks_port: None,
            username: None,
            password: None,
        });
        config.socks_host = Some(host.into());
        config.socks_port = Some(port);
        self.proxy_config = Some(config);
        self
    }

    /// Set proxy authentication
    pub fn proxy_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        let mut config = self.proxy_config.unwrap_or(ProxyConfig {
            http_host: None,
            http_port: None,
            https_host: None,
            https_port: None,
            socks_host: None,
            socks_port: None,
            username: None,
            password: None,
        });
        config.username = Some(username.into());
        config.password = Some(password.into());
        self.proxy_config = Some(config);
        self
    }

    /// Build the NSURLSessionConfiguration
    pub(crate) fn build(mut self) -> Result<Retained<NSURLSessionConfiguration>> {
        unsafe {
            let config = match &self.session_type {
                SessionType::Default => NSURLSessionConfiguration::defaultSessionConfiguration(),
                SessionType::Background(identifier) => {
                    NSURLSessionConfiguration::backgroundSessionConfigurationWithIdentifier(
                        &NSString::from_str(identifier),
                    )
                }
            };

            // Set caching behavior
            if self.caching == CachingBehavior::Disabled {
                config.setRequestCachePolicy(NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData);
            }

            // Set proxy settings
            if let Some(proxy_config) = &self.proxy_config {
                let proxy_dict = self.build_proxy_dictionary(proxy_config)?;
                config.setConnectionProxyDictionary(Some(&*proxy_dict));
            } else if !self.use_default_proxy {
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
                let keys: Vec<_> = self
                    .headers
                    .keys()
                    .map(|k| NSString::from_str(k.as_str()))
                    .collect();
                let values: Vec<_> = self
                    .headers
                    .values()
                    .map(|v| NSString::from_str(v.to_str().expect("Invalid header value")))
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

    /// Build proxy dictionary for NSURLSessionConfiguration
    fn build_proxy_dictionary(&self, proxy_config: &ProxyConfig) -> Result<Retained<NSDictionary>> {
        let mut proxy_dict = HashMap::new();

        // HTTP proxy
        if let (Some(host), Some(port)) = (&proxy_config.http_host, &proxy_config.http_port) {
            proxy_dict.insert("HTTPEnable", "1".to_string());
            proxy_dict.insert("HTTPProxy", host.clone());
            proxy_dict.insert("HTTPPort", port.to_string());
        }

        // HTTPS proxy
        if let (Some(host), Some(port)) = (&proxy_config.https_host, &proxy_config.https_port) {
            proxy_dict.insert("HTTPSEnable", "1".to_string());
            proxy_dict.insert("HTTPSProxy", host.clone());
            proxy_dict.insert("HTTPSPort", port.to_string());
        }

        // SOCKS proxy
        if let (Some(host), Some(port)) = (&proxy_config.socks_host, &proxy_config.socks_port) {
            proxy_dict.insert("SOCKSEnable", "1".to_string());
            proxy_dict.insert("SOCKSProxy", host.clone());
            proxy_dict.insert("SOCKSPort", port.to_string());
        }

        // Proxy authentication
        if let (Some(username), Some(password)) = (&proxy_config.username, &proxy_config.password) {
            proxy_dict.insert("HTTPProxyUsername", username.clone());
            proxy_dict.insert("HTTPProxyPassword", password.clone());
            proxy_dict.insert("HTTPSProxyUsername", username.clone());
            proxy_dict.insert("HTTPSProxyPassword", password.clone());
            proxy_dict.insert("SOCKSUsername", username.clone());
            proxy_dict.insert("SOCKSPassword", password.clone());
        }

        // Convert HashMap to NSDictionary
        let keys: Vec<_> = proxy_dict.keys().map(|k| NSString::from_str(k)).collect();
        let values: Vec<_> = proxy_dict.values().map(|v| NSString::from_str(v)).collect();

        let dict = NSDictionary::from_retained_objects(
            &keys.iter().map(|s| &**s).collect::<Vec<_>>(),
            &values,
        );

        Ok(unsafe { Retained::cast_unchecked(dict) })
    }
}
