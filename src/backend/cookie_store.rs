//! Unified cookie storage implementation using cookie_store
//!
//! This module provides a common cookie storage implementation for all non-Apple
//! backends (Windows, Android, Reqwest). It wraps the cookie_store crate to provide
//! RFC 6265 compliant cookie handling with a consistent API.

use crate::{
    Error, Result,
    cookies::{Cookie, CookieAcceptPolicy},
};
use cookie_store::CookieStore;
use http::{HeaderMap, HeaderValue};
use std::sync::{Arc, Mutex};
use url::Url;

/// Unified cookie storage implementation using cookie_store
///
/// This implementation is used by Windows, Android, and Reqwest backends
/// to provide consistent cookie handling across all non-Apple platforms.
#[derive(Clone, Debug)]
pub struct CookieJar {
    /// RFC 6265 compliant cookie storage wrapped in Arc<Mutex> for thread safety
    store: Arc<Mutex<CookieStore>>,
    /// Cookie acceptance policy (stored separately as cookie_store doesn't support this)
    accept_policy: Arc<Mutex<CookieAcceptPolicy>>,
}

impl CookieJar {
    /// Create a new cookie storage
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(CookieStore::default())),
            accept_policy: Arc::new(Mutex::new(CookieAcceptPolicy::Always)),
        }
    }

    /// Get all cookies stored in the jar
    pub fn all_cookies(&self) -> Vec<Cookie> {
        let store = match self.store.lock() {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        store
            .iter_any()
            .filter_map(|cookie| Self::convert_from_cookie_store(cookie))
            .collect()
    }

    /// Get cookies for a specific URL
    pub fn cookies_for_url(&self, url: &str) -> Result<Vec<Cookie>> {
        let parsed_url = Url::parse(url).map_err(|_| Error::InvalidUrl)?;

        let store = self
            .store
            .lock()
            .map_err(|e| Error::Internal(format!("Failed to lock cookie store: {}", e)))?;

        let cookies = store
            .get_request_values(&parsed_url)
            .map(|(name, value)| {
                // Create a basic cookie with just name and value
                // The store handles domain/path/security validation
                Cookie::new(name, value)
            })
            .collect();

        Ok(cookies)
    }

    /// Add a cookie to the jar
    pub fn add_cookie(&self, cookie: Cookie) -> Result<()> {
        // Build a Set-Cookie header string from the cookie
        let mut set_cookie = format!("{}={}", cookie.name, cookie.value);

        if !cookie.domain.is_empty() {
            set_cookie.push_str(&format!("; Domain={}", cookie.domain));
        }

        if cookie.path != "/" {
            set_cookie.push_str(&format!("; Path={}", cookie.path));
        }

        if cookie.secure {
            set_cookie.push_str("; Secure");
        }

        if cookie.http_only {
            set_cookie.push_str("; HttpOnly");
        }

        if let Some(ref expires) = cookie.expires {
            set_cookie.push_str(&format!("; Expires={}", expires));
        }

        // Parse the URL from the cookie's domain and path
        let scheme = if cookie.secure { "https" } else { "http" };
        let domain = if cookie.domain.is_empty() {
            "localhost"
        } else {
            cookie.domain.trim_start_matches('.')
        };
        let url_str = format!("{}://{}{}", scheme, domain, cookie.path);
        let url = Url::parse(&url_str).map_err(|_| Error::InvalidUrl)?;

        // Add to store
        let mut store = self
            .store
            .lock()
            .map_err(|e| Error::Internal(format!("Failed to lock cookie store: {}", e)))?;

        store
            .parse(&set_cookie, &url)
            .map_err(|e| Error::Internal(format!("Failed to parse cookie: {}", e)))?;

        Ok(())
    }

    /// Remove a cookie from the jar
    pub fn remove_cookie(&self, cookie: Cookie) -> Result<()> {
        // Build a URL from the cookie's domain and path
        let scheme = if cookie.secure { "https" } else { "http" };
        let domain = if cookie.domain.is_empty() {
            "localhost"
        } else {
            cookie.domain.trim_start_matches('.')
        };
        let url_str = format!("{}://{}{}", scheme, domain, cookie.path);
        let url = Url::parse(&url_str).map_err(|_| Error::InvalidUrl)?;

        // Create an expired cookie to remove it
        let expired_cookie = format!(
            "{}=; Domain={}; Path={}; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
            cookie.name,
            if cookie.domain.is_empty() {
                domain
            } else {
                &cookie.domain
            },
            cookie.path
        );

        let mut store = self
            .store
            .lock()
            .map_err(|e| Error::Internal(format!("Failed to lock cookie store: {}", e)))?;

        // Parse the expired cookie to remove the original
        store
            .parse(&expired_cookie, &url)
            .map_err(|e| Error::Internal(format!("Failed to remove cookie: {}", e)))?;

        Ok(())
    }

    /// Clear all cookies from the jar
    pub fn clear(&self) {
        if let Ok(mut store) = self.store.lock() {
            *store = CookieStore::default();
        }
    }

    /// Set the cookie acceptance policy
    ///
    /// Note: cookie_store doesn't directly support acceptance policies,
    /// so this is stored and must be checked manually when processing cookies
    pub fn set_cookie_accept_policy(&self, policy: CookieAcceptPolicy) {
        if let Ok(mut accept_policy) = self.accept_policy.lock() {
            *accept_policy = policy;
        }
    }

    /// Get the current cookie acceptance policy
    pub fn get_cookie_accept_policy(&self) -> CookieAcceptPolicy {
        self.accept_policy
            .lock()
            .map(|p| *p)
            .unwrap_or(CookieAcceptPolicy::Always)
    }

    /// Process Set-Cookie headers from an HTTP response
    ///
    /// This is used internally by backends to store cookies from responses
    pub fn process_response_headers(&self, url: &Url, headers: &HeaderMap) -> Result<()> {
        // Check acceptance policy
        let policy = self.get_cookie_accept_policy();
        if policy == CookieAcceptPolicy::Never {
            return Ok(());
        }

        let mut store = self
            .store
            .lock()
            .map_err(|e| Error::Internal(format!("Failed to lock cookie store: {}", e)))?;

        for set_cookie_value in headers.get_all(http::header::SET_COOKIE) {
            if let Ok(header_str) = set_cookie_value.to_str() {
                // For OnlyFromMainDocumentDomain policy, we'd need to check the cookie domain
                // against the main document domain. For now, we parse all cookies when
                // policy is not Never (the store will validate domain/path/security itself)
                let _ = store.parse(header_str, url);
            }
        }

        Ok(())
    }

    /// Get cookies for a URL as HTTP headers
    ///
    /// This is used internally by backends to add Cookie headers to requests
    pub fn get_cookies_for_url(&self, url: &Url) -> Result<HeaderMap> {
        let store = self
            .store
            .lock()
            .map_err(|e| Error::Internal(format!("Failed to lock cookie store: {}", e)))?;

        let mut headers = HeaderMap::new();

        // Get cookie name=value pairs for this URL
        let cookie_pairs: Vec<String> = store
            .get_request_values(url)
            .map(|(name, value)| format!("{}={}", name, value))
            .collect();

        if !cookie_pairs.is_empty() {
            let cookie_header = cookie_pairs.join("; ");
            if let Ok(header_value) = HeaderValue::from_str(&cookie_header) {
                headers.insert(http::header::COOKIE, header_value);
            }
        }

        Ok(headers)
    }

    /// Convert from cookie_store::Cookie to our Cookie type
    fn convert_from_cookie_store(cookie: &cookie_store::Cookie) -> Option<Cookie> {
        Some(Cookie {
            name: cookie.name().to_string(),
            value: cookie.value().to_string(),
            domain: cookie.domain().unwrap_or("").to_string(),
            path: cookie.path().unwrap_or("/").to_string(),
            secure: cookie.secure().unwrap_or(false),
            http_only: cookie.http_only().unwrap_or(false),
            // cookie_store doesn't easily expose expiration in a simple format
            // We'll leave it as None for now since it's primarily used internally
            expires: None,
        })
    }
}

impl Default for CookieJar {
    fn default() -> Self {
        Self::new()
    }
}
