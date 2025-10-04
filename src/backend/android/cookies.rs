//! Android cookie storage using cookie_store

use crate::{Error, Result};
use cookie_store::CookieStore;
use http::{HeaderMap, HeaderValue};
use std::sync::{Arc, Mutex};
use url::Url;

/// Android cookie storage using cookie_store
#[derive(Clone, Debug)]
pub struct AndroidCookieStorage {
    /// RFC6265 compliant cookie storage wrapped in a mutex for thread safety
    store: Arc<Mutex<CookieStore>>,
}

impl AndroidCookieStorage {
    /// Create a new Android cookie storage
    pub fn new() -> Result<Self> {
        Ok(Self {
            store: Arc::new(Mutex::new(CookieStore::default())),
        })
    }

    /// Process Set-Cookie headers from an HTTP response
    pub fn process_response_headers(&self, url: &Url, headers: &HeaderMap) -> Result<()> {
        println!("🍪 Processing Set-Cookie headers for URL: {}", url);
        let mut cookie_count = 0;

        for set_cookie_value in headers.get_all(http::header::SET_COOKIE) {
            if let Ok(header_str) = set_cookie_value.to_str() {
                println!("🍪 Set-Cookie header: {}", header_str);
                let mut store = self
                    .store
                    .lock()
                    .map_err(|e| Error::Internal(format!("Failed to lock cookie store: {}", e)))?;

                // Parse the Set-Cookie header and insert into store
                if let Err(e) = store.parse(header_str, url) {
                    println!("🍪 Failed to parse cookie: {}", e);
                } else {
                    cookie_count += 1;
                    println!("🍪 Successfully stored cookie");
                }
            }
        }

        println!("🍪 Stored {} cookies total", cookie_count);
        Ok(())
    }

    /// Get cookies for a URL as HTTP headers
    pub fn get_cookies_for_url(&self, url: &Url) -> Result<HeaderMap> {
        println!("🍪 Getting cookies for URL: {}", url);
        let store = self
            .store
            .lock()
            .map_err(|e| Error::Internal(format!("Failed to lock cookie store: {}", e)))?;

        let mut headers = HeaderMap::new();

        // Get cookie name=value pairs for this URL
        let cookie_pairs: Vec<String> = store
            .get_request_values(url)
            .map(|(name, value)| {
                println!("🍪 Found cookie: {}={}", name, value);
                format!("{}={}", name, value)
            })
            .collect();

        if !cookie_pairs.is_empty() {
            let cookie_header = cookie_pairs.join("; ");
            println!("🍪 Sending Cookie header: {}", cookie_header);
            if let Ok(header_value) = HeaderValue::from_str(&cookie_header) {
                headers.insert(http::header::COOKIE, header_value);
            }
        } else {
            println!("🍪 No cookies found for this URL");
        }

        Ok(headers)
    }

    /// Get all cookies (required by CookieStorage trait)
    pub fn all_cookies(&self) -> Vec<crate::cookies::Cookie> {
        // cookie_store doesn't provide easy iteration, return empty for now
        vec![]
    }

    /// Get cookies for a URL (required by CookieStorage trait)
    pub fn cookies_for_url(&self, url: &str) -> Result<Vec<crate::cookies::Cookie>> {
        // cookie_store doesn't easily convert to our Cookie type, return empty for now
        Ok(vec![])
    }

    /// Add a cookie (required by CookieStorage trait)
    pub fn add_cookie(&self, _cookie: crate::cookies::Cookie) -> Result<()> {
        // Would need to convert our Cookie type to cookie_store format
        Ok(())
    }

    /// Remove a cookie (required by CookieStorage trait)
    pub fn remove_cookie(&self, _cookie: crate::cookies::Cookie) -> Result<()> {
        Ok(())
    }

    /// Clear all cookies (required by CookieStorage trait)
    pub fn clear(&self) {
        if let Ok(mut store) = self.store.lock() {
            *store = CookieStore::default();
        }
    }

    /// Set cookie accept policy (required by CookieStorage trait)
    pub fn set_cookie_accept_policy(&self, _policy: crate::cookies::CookieAcceptPolicy) {
        // cookie_store doesn't have this concept
    }
}
