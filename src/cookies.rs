//! Cookie management using NSHTTPCookieStorage

use crate::{Error, Result};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{
    NSCopying, NSDictionary, NSHTTPCookie, NSHTTPCookieDomain, NSHTTPCookieName, NSHTTPCookiePath,
    NSHTTPCookieSecure, NSHTTPCookieStorage, NSHTTPCookieValue, NSMutableDictionary, NSString,
    NSURL,
};

/// Policy for cookie acceptance.
///
/// This enum defines how cookies should be handled by the HTTP client.
/// It maps directly to NSHTTPCookieAcceptPolicy values.
///
/// # Examples
///
/// ```rust
/// use rsurlsession::{Client, CookieAcceptPolicy};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::builder()
///     .use_cookies(true)
///     .build()?;
///
/// if let Some(jar) = client.cookie_jar() {
///     jar.set_cookie_accept_policy(CookieAcceptPolicy::Always);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieAcceptPolicy {
    /// Accept all cookies.
    ///
    /// Cookies will be accepted from all domains and stored in the cookie jar.
    Always,
    /// Never accept cookies.
    ///
    /// All cookies will be rejected and not stored.
    Never,
    /// Accept cookies only from the main document domain.
    ///
    /// Cookies will only be accepted if they come from the same domain as
    /// the main document URL. This helps prevent third-party tracking cookies.
    OnlyFromMainDocumentDomain,
}

/// A cookie jar that manages HTTP cookies using NSHTTPCookieStorage.
///
/// `CookieJar` provides a high-level interface for managing HTTP cookies. It wraps
/// NSHTTPCookieStorage and provides methods for adding, removing, and querying cookies.
/// Cookies are automatically sent with requests and stored from responses when enabled.
///
/// # Examples
///
/// ```rust
/// use rsurlsession::{Client, CookieJar, Cookie};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Create a client with cookies enabled
/// let client = Client::builder()
///     .use_cookies(true)
///     .build()?;
///
/// // Access the cookie jar
/// if let Some(jar) = client.cookie_jar() {
///     // Add a custom cookie
///     let cookie = Cookie::new("session", "abc123")
///         .domain("example.com")
///         .path("/")
///         .secure(true);
///     jar.add_cookie(cookie)?;
///
///     // Get all cookies
///     let cookies = jar.all_cookies();
///     println!("Found {} cookies", cookies.len());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct CookieJar {
    storage: Retained<NSHTTPCookieStorage>,
}

impl CookieJar {
    /// Create a new cookie jar with shared storage.
    ///
    /// This creates a cookie jar using the shared NSHTTPCookieStorage instance,
    /// which means cookies will be shared across all HTTP clients in the application.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::CookieJar;
    ///
    /// let jar = CookieJar::new();
    /// ```
    pub fn new() -> Self {
        Self {
            storage: unsafe { NSHTTPCookieStorage::sharedHTTPCookieStorage() },
        }
    }

    /// Create a new cookie jar with storage for a specific group container.
    ///
    /// This creates a cookie jar that uses a separate cookie storage for the specified
    /// group container identifier. This is useful for app extensions or when you need
    /// isolated cookie storage.
    ///
    /// # Arguments
    ///
    /// * `identifier` - The group container identifier
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::CookieJar;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let jar = CookieJar::for_group_container("group.com.example.app")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn for_group_container(identifier: &str) -> Result<Self> {
        let storage = unsafe {
            NSHTTPCookieStorage::sharedCookieStorageForGroupContainerIdentifier(
                &NSString::from_str(identifier),
            )
        };
        Ok(Self { storage })
    }

    /// Get all cookies
    pub fn all_cookies(&self) -> Vec<Cookie> {
        let cookies = unsafe { self.storage.cookies() };
        if let Some(cookies) = cookies {
            (0..cookies.len())
                .map(|i| cookies.objectAtIndex(i))
                .filter_map(|cookie| Cookie::from_ns_cookie(&cookie))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get cookies for a specific URL
    pub fn cookies_for_url(&self, url: &str) -> Result<Vec<Cookie>> {
        let nsurl =
            unsafe { NSURL::URLWithString(&NSString::from_str(url)).ok_or(Error::InvalidUrl)? };

        let cookies = unsafe { self.storage.cookiesForURL(&nsurl) };
        if let Some(cookies) = cookies {
            Ok((0..cookies.len())
                .map(|i| cookies.objectAtIndex(i))
                .filter_map(|cookie| Cookie::from_ns_cookie(&cookie))
                .collect())
        } else {
            Ok(Vec::new())
        }
    }

    /// Add a cookie
    pub fn add_cookie(&self, cookie: Cookie) -> Result<()> {
        let ns_cookie = cookie.to_ns_cookie()?;
        unsafe {
            self.storage.setCookie(&ns_cookie);
        }
        Ok(())
    }

    /// Remove a cookie
    pub fn remove_cookie(&self, cookie: Cookie) -> Result<()> {
        let ns_cookie = cookie.to_ns_cookie()?;
        unsafe {
            self.storage.deleteCookie(&ns_cookie);
        }
        Ok(())
    }

    /// Remove all cookies
    pub fn clear(&self) {
        let cookies = self.all_cookies();
        for cookie in cookies {
            let _ = self.remove_cookie(cookie);
        }
    }

    /// Set cookie acceptance policy
    pub fn set_cookie_accept_policy(&self, policy: CookieAcceptPolicy) {
        use objc2_foundation::NSHTTPCookieAcceptPolicy;

        let ns_policy = match policy {
            CookieAcceptPolicy::Always => NSHTTPCookieAcceptPolicy::Always,
            CookieAcceptPolicy::Never => NSHTTPCookieAcceptPolicy::Never,
            CookieAcceptPolicy::OnlyFromMainDocumentDomain => {
                NSHTTPCookieAcceptPolicy::OnlyFromMainDocumentDomain
            }
        };
        unsafe {
            self.storage.setCookieAcceptPolicy(ns_policy);
        }
    }

    /// Get the underlying NSHTTPCookieStorage
    pub(crate) fn storage(&self) -> &NSHTTPCookieStorage {
        &self.storage
    }
}

impl Default for CookieJar {
    fn default() -> Self {
        Self::new()
    }
}

/// An HTTP cookie
#[derive(Debug, Clone)]
pub struct Cookie {
    /// Cookie name
    pub name: String,
    /// Cookie value
    pub value: String,
    /// Domain
    pub domain: String,
    /// Path
    pub path: String,
    /// Whether the cookie is secure (HTTPS only)
    pub secure: bool,
    /// Whether the cookie is HTTP only
    pub http_only: bool,
    /// Expiration date as string (None for session cookies)
    pub expires: Option<String>,
}

impl Cookie {
    /// Create a new cookie
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            domain: String::new(),
            path: "/".to_string(),
            secure: false,
            http_only: false,
            expires: None,
        }
    }

    /// Set the domain
    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = domain.into();
        self
    }

    /// Set the path
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Set whether the cookie is secure
    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    /// Set whether the cookie is HTTP only
    pub fn http_only(mut self, http_only: bool) -> Self {
        self.http_only = http_only;
        self
    }

    /// Set the expiration date
    pub fn expires(mut self, expires: impl Into<String>) -> Self {
        self.expires = Some(expires.into());
        self
    }

    /// Convert to NSHTTPCookie
    fn to_ns_cookie(&self) -> Result<Retained<NSHTTPCookie>> {
        unsafe {
            // Create NSMutableDictionary with NSCopying protocol keys
            let dict: Retained<NSMutableDictionary<ProtocolObject<dyn NSCopying>, AnyObject>> =
                NSMutableDictionary::new();

            // Add required properties using the actual constants
            dict.setObject_forKey(
                &*NSString::from_str(&self.name) as &AnyObject,
                ProtocolObject::from_ref(NSHTTPCookieName),
            );

            dict.setObject_forKey(
                &*NSString::from_str(&self.value) as &AnyObject,
                ProtocolObject::from_ref(NSHTTPCookieValue),
            );

            dict.setObject_forKey(
                &*NSString::from_str(&self.domain) as &AnyObject,
                ProtocolObject::from_ref(NSHTTPCookieDomain),
            );

            dict.setObject_forKey(
                &*NSString::from_str(&self.path) as &AnyObject,
                ProtocolObject::from_ref(NSHTTPCookiePath),
            );

            // Add optional properties
            if self.secure {
                dict.setObject_forKey(
                    &*NSString::from_str("TRUE") as &AnyObject,
                    ProtocolObject::from_ref(NSHTTPCookieSecure),
                );
            }

            // Cast the dictionary to NSDictionary and create the cookie
            let cookie_dict: Retained<NSDictionary<NSString, AnyObject>> =
                objc2::rc::Retained::cast_unchecked(dict);
            NSHTTPCookie::cookieWithProperties(&*cookie_dict)
                .ok_or_else(|| Error::Internal("Failed to create NSHTTPCookie".to_string()))
        }
    }

    /// Create from NSHTTPCookie
    fn from_ns_cookie(ns_cookie: &NSHTTPCookie) -> Option<Self> {
        unsafe {
            let name = ns_cookie.name().to_string();
            let value = ns_cookie.value().to_string();
            let domain = ns_cookie.domain().to_string();
            let path = ns_cookie.path().to_string();
            let secure = ns_cookie.isSecure();
            let http_only = ns_cookie.isHTTPOnly();

            // Get expiration date as string if available
            let expires = ns_cookie
                .expiresDate()
                .map(|date| date.description().to_string());

            Some(Self {
                name,
                value,
                domain,
                path,
                secure,
                http_only,
                expires,
            })
        }
    }
}
