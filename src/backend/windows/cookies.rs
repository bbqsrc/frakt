//! Windows cookie management using cookie_store

use crate::{CookieJar, Error, Result};
use cookie_store::{Cookie as StoreCookie, CookieStore};
use http::{HeaderMap, HeaderValue};
use std::sync::{Arc, Mutex};
use url::Url;

/// Windows cookie storage backend using cookie_store for RFC6265 compliance
#[derive(Clone)]
pub struct WindowsCookieStorage {
    /// RFC6265 compliant cookie storage wrapped in a mutex for thread safety
    store: Arc<Mutex<CookieStore>>,
}

impl WindowsCookieStorage {
    /// Create a new Windows cookie storage
    pub fn new() -> Result<Self> {
        Ok(Self {
            store: Arc::new(Mutex::new(CookieStore::default())),
        })
    }

    /// Add a cookie from a Set-Cookie header
    pub fn add_cookie_from_header(&self, url: &Url, set_cookie_header: &str) -> Result<()> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| Error::Internal(format!("Failed to lock cookie store: {}", e)))?;

        // Parse the Set-Cookie header and insert into store
        if let Err(e) = store.parse(set_cookie_header, url) {
            return Err(Error::Internal(format!("Failed to parse cookie: {}", e)));
        }

        Ok(())
    }

    /// Add a simple cookie by name and value
    pub fn add_cookie(&self, url: &Url, name: &str, value: &str) -> Result<()> {
        // Create a simple Set-Cookie header string
        let set_cookie_header = format!("{}={}", name, value);
        self.add_cookie_from_header(url, &set_cookie_header)
    }

    /// Process Set-Cookie headers from an HTTP response
    pub fn process_response_headers(&self, url: &Url, headers: &HeaderMap) -> Result<()> {
        for set_cookie_value in headers.get_all(http::header::SET_COOKIE) {
            if let Ok(header_str) = set_cookie_value.to_str() {
                if let Err(e) = self.add_cookie_from_header(url, header_str) {
                    // Log the error but don't fail the entire response processing
                    eprintln!(
                        "Warning: Failed to process Set-Cookie header '{}': {}",
                        header_str, e
                    );
                }
            }
        }
        Ok(())
    }

    /// Get cookies for a URL as HTTP headers
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

    /// Delete a specific cookie by name for a domain
    pub fn delete_cookie(&self, url: &Url, name: &str) -> Result<()> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| Error::Internal(format!("Failed to lock cookie store: {}", e)))?;

        // Find and remove cookies matching the name and domain
        // Note: cookie_store doesn't have a direct delete method, so we need to
        // collect cookies to remove and then remove them
        let domain = url.host_str().unwrap_or("localhost");
        let path = url.path();

        // Create a list of cookies to remove
        let cookies_to_remove: Vec<StoreCookie> = store
            .iter_any()
            .filter(|cookie| {
                cookie.name() == name
                    && (cookie.domain().is_none() || cookie.domain() == Some(domain))
                    && path.starts_with(cookie.path().unwrap_or("/"))
            })
            .cloned()
            .collect();

        // Remove the matching cookies
        for cookie in cookies_to_remove {
            // Cookie store doesn't have direct removal, so we'll add an expired version
            // This is a limitation of the cookie_store crate
            let expired_cookie = format!(
                "{}=; Domain={}; Path={}; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
                cookie.name(),
                cookie.domain().unwrap_or(domain),
                cookie.path().unwrap_or("/")
            );
            let _ = store.parse(&expired_cookie, url);
        }

        Ok(())
    }

    /// Clear all cookies
    pub fn clear_all_cookies(&self) -> Result<()> {
        let mut store = self
            .store
            .lock()
            .map_err(|e| Error::Internal(format!("Failed to lock cookie store: {}", e)))?;

        // Create a new empty store
        *store = CookieStore::default();
        Ok(())
    }

    /// Get all cookies as a vector for debugging/inspection
    pub fn get_all_cookies(&self) -> Result<Vec<StoreCookie<'_>>> {
        let store = self
            .store
            .lock()
            .map_err(|e| Error::Internal(format!("Failed to lock cookie store: {}", e)))?;

        Ok(store.iter_any().cloned().collect())
    }
}

/// Convert Windows cookie storage to library CookieJar
impl From<WindowsCookieStorage> for CookieJar {
    fn from(_storage: WindowsCookieStorage) -> Self {
        CookieJar::new()
    }
}
