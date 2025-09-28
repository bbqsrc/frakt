//! Reqwest cookie storage implementation using reqwest cookie jar

use crate::{
    Error, Result,
    cookies::{Cookie, CookieAcceptPolicy},
};
use reqwest::cookie::CookieStore;
use std::sync::Arc;
use url::Url;

/// Reqwest cookie storage using reqwest cookie jar
#[derive(Debug, Clone)]
pub struct ReqwestCookieStorage {
    jar: Arc<reqwest::cookie::Jar>,
}

impl ReqwestCookieStorage {
    /// Create a new cookie storage
    pub fn new() -> Self {
        Self {
            jar: Arc::new(reqwest::cookie::Jar::default()),
        }
    }

    /// Get all cookies (reqwest doesn't support this directly, so return empty for now)
    pub fn all_cookies(&self) -> Vec<Cookie> {
        // reqwest doesn't provide a way to list all cookies
        // This is a limitation of the reqwest cookie jar API
        Vec::new()
    }

    /// Get cookies for a specific URL
    pub fn cookies_for_url(&self, url: &str) -> Result<Vec<Cookie>> {
        let url = Url::parse(url).map_err(|_| Error::InvalidUrl)?;
        let mut cookies = Vec::new();

        // Get cookie header for the URL
        if let Some(cookie_header) = self.jar.cookies(&url) {
            if let Some(cookie_str) = cookie_header.to_str().ok() {
                // Parse cookies from header
                for cookie_part in cookie_str.split(';') {
                    let cookie_part = cookie_part.trim();
                    if let Some((name, value)) = cookie_part.split_once('=') {
                        cookies.push(Cookie::new(name.trim(), value.trim()));
                    }
                }
            }
        }

        Ok(cookies)
    }

    /// Add a cookie
    pub fn add_cookie(&self, cookie: Cookie) -> Result<()> {
        // Create a URL for the cookie domain
        let scheme = if cookie.secure { "https" } else { "http" };
        let domain = if cookie.domain.is_empty() {
            "localhost"
        } else {
            &cookie.domain
        };

        let url_str = format!("{}://{}{}", scheme, domain, cookie.path);
        let url = Url::parse(&url_str).map_err(|_| Error::InvalidUrl)?;

        // Build reqwest cookie string manually
        let mut cookie_str = format!("{}={}", cookie.name, cookie.value);

        if !cookie.domain.is_empty() {
            cookie_str.push_str(&format!("; Domain={}", cookie.domain));
        }

        if cookie.path != "/" {
            cookie_str.push_str(&format!("; Path={}", cookie.path));
        }

        if cookie.secure {
            cookie_str.push_str("; Secure");
        }

        if cookie.http_only {
            cookie_str.push_str("; HttpOnly");
        }

        if let Some(expires) = cookie.expires {
            cookie_str.push_str(&format!("; Expires={}", expires));
        }

        self.jar.add_cookie_str(&cookie_str, &url);
        Ok(())
    }

    /// Remove a cookie (not supported by reqwest directly)
    pub fn remove_cookie(&self, _cookie: Cookie) -> Result<()> {
        // reqwest doesn't provide a direct way to remove specific cookies
        // This is a limitation of the reqwest cookie jar API
        Ok(())
    }

    /// Clear all cookies (not supported by reqwest directly)
    pub fn clear(&self) {
        // reqwest doesn't provide a way to clear all cookies
        // This is a limitation of the reqwest cookie jar API
    }

    /// Set cookie acceptance policy (not supported by reqwest)
    pub fn set_cookie_accept_policy(&self, _policy: CookieAcceptPolicy) {
        // reqwest doesn't expose cookie acceptance policy configuration
        // This is a limitation of the reqwest cookie jar API
    }

    /// Get the underlying reqwest cookie jar
    pub(crate) fn jar(&self) -> &reqwest::cookie::Jar {
        &self.jar
    }
}
