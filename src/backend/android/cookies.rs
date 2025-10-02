//! Android cookie storage using CookieManager

use crate::cookies::{Cookie, CookieAcceptPolicy};
use crate::{Error, Result};
use jni::{JNIEnv, JavaVM, objects::GlobalRef};
use std::sync::Arc;

/// Android cookie storage using the system CookieManager
#[derive(Clone, Debug)]
pub struct AndroidCookieStorage {
    jvm: &'static JavaVM,
    cookie_manager: Arc<GlobalRef>,
}

impl AndroidCookieStorage {
    /// Create a new Android cookie storage
    pub fn new(jvm: &'static JavaVM) -> Result<Self> {
        let cookie_manager = get_cookie_manager(jvm)?;

        Ok(Self {
            jvm,
            cookie_manager: Arc::new(cookie_manager),
        })
    }

    /// Get all cookies from the cookie manager
    pub fn all_cookies(&self) -> Vec<Cookie> {
        // CookieManager doesn't provide a direct way to get all cookies
        // This would need to be implemented by tracking cookies we've set
        // or using a different approach
        tracing::warn!("all_cookies() not fully implemented for Android CookieManager");
        vec![]
    }

    /// Get cookies for a specific URL
    pub fn cookies_for_url(&self, url: &str) -> Result<Vec<Cookie>> {
        let mut env = self
            .jvm
            .attach_current_thread()
            .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

        let url_string = env
            .new_string(url)
            .map_err(|e| Error::Internal(format!("Failed to create URL string: {}", e)))?;

        let cookie_string = env
            .call_method(
                self.cookie_manager.as_obj(),
                "getCookie",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[(&url_string).into()],
            )
            .map_err(|e| Error::Internal(format!("Failed to get cookies for URL: {}", e)))?
            .l()
            .map_err(|e| Error::Internal(format!("Failed to get cookie string: {}", e)))?;

        if cookie_string.is_null() {
            return Ok(vec![]);
        }

        let cookie_str: String = env
            .get_string(&cookie_string.into())
            .map_err(|e| Error::Internal(format!("Failed to convert cookie string: {}", e)))?
            .into();

        // Parse cookie string (format: "name1=value1; name2=value2; ...")
        let cookies = parse_cookie_string(&cookie_str)?;
        Ok(cookies)
    }

    /// Add a cookie to the cookie manager
    pub fn add_cookie(&self, cookie: Cookie) -> Result<()> {
        let mut env = self
            .jvm
            .attach_current_thread()
            .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

        // Convert cookie to string format
        let cookie_string = format_cookie_for_android(&cookie)?;
        let cookie_str = env
            .new_string(&cookie_string)
            .map_err(|e| Error::Internal(format!("Failed to create cookie string: {}", e)))?;

        // Construct URL for the cookie (needed for setCookie)
        let url = construct_cookie_url(&cookie)?;
        let url_str = env
            .new_string(&url)
            .map_err(|e| Error::Internal(format!("Failed to create URL string: {}", e)))?;

        env.call_method(
            self.cookie_manager.as_obj(),
            "setCookie",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[(&url_str).into(), (&cookie_str).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to set cookie: {}", e)))?;

        // Sync cookies to persistent storage
        self.sync_cookies(&mut env)?;

        Ok(())
    }

    /// Remove a cookie from the cookie manager
    pub fn remove_cookie(&self, cookie: Cookie) -> Result<()> {
        let mut env = self
            .jvm
            .attach_current_thread()
            .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

        // To remove a cookie in Android, we set it with an expired date
        let mut expired_cookie = cookie;
        expired_cookie.expires = Some("Thu, 01 Jan 1970 00:00:00 GMT".to_string());

        let cookie_string = format_cookie_for_android(&expired_cookie)?;
        let cookie_str = env.new_string(&cookie_string).map_err(|e| {
            Error::Internal(format!("Failed to create expired cookie string: {}", e))
        })?;

        let url = construct_cookie_url(&expired_cookie)?;
        let url_str = env
            .new_string(&url)
            .map_err(|e| Error::Internal(format!("Failed to create URL string: {}", e)))?;

        env.call_method(
            self.cookie_manager.as_obj(),
            "setCookie",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[(&url_str).into(), (&cookie_str).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to remove cookie: {}", e)))?;

        self.sync_cookies(&mut env)?;

        Ok(())
    }

    /// Clear all cookies
    pub fn clear(&self) {
        if let Ok(mut env) = self.jvm.attach_current_thread() {
            if let Err(e) =
                env.call_method(self.cookie_manager.as_obj(), "removeAllCookie", "()V", &[])
            {
                tracing::error!("Failed to clear cookies: {}", e);
                return;
            }

            if let Err(e) = self.sync_cookies(&mut env) {
                tracing::error!("Failed to sync after clearing cookies: {}", e);
            }
        } else {
            tracing::error!("Failed to attach to JVM thread for clearing cookies");
        }
    }

    /// Set cookie acceptance policy
    pub fn set_cookie_accept_policy(&self, policy: CookieAcceptPolicy) {
        if let Ok(mut env) = self.jvm.attach_current_thread() {
            let accept = match policy {
                CookieAcceptPolicy::Always => true,
                CookieAcceptPolicy::Never => false,
                CookieAcceptPolicy::OnlyFromMainDocumentDomain => true, // Android doesn't distinguish this
            };

            if let Err(e) = env.call_method(
                self.cookie_manager.as_obj(),
                "setAcceptCookie",
                "(Z)V",
                &[accept.into()],
            ) {
                tracing::error!("Failed to set cookie accept policy: {}", e);
            }
        } else {
            tracing::error!("Failed to attach to JVM thread for setting cookie policy");
        }
    }

    /// Sync cookies to persistent storage
    fn sync_cookies(&self, env: &mut JNIEnv) -> Result<()> {
        // Note: CookieSyncManager is deprecated in newer Android versions
        // but we'll try to call it for compatibility
        if let Ok(cookie_sync_manager_class) = env.find_class("android/webkit/CookieSyncManager") {
            if let Ok(instance) = env.call_static_method(
                cookie_sync_manager_class,
                "getInstance",
                "()Landroid/webkit/CookieSyncManager;",
                &[],
            ) {
                if let Ok(sync_manager) = instance.l() {
                    let _ = env.call_method(sync_manager, "sync", "()V", &[]);
                }
            }
        }

        // For newer Android versions, try to flush
        let _ = env.call_method(self.cookie_manager.as_obj(), "flush", "()V", &[]);

        Ok(())
    }
}

/// Get the Android CookieManager instance
fn get_cookie_manager(jvm: &JavaVM) -> Result<GlobalRef> {
    let mut env = jvm
        .attach_current_thread()
        .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

    let cookie_manager = env
        .call_static_method(
            "android/webkit/CookieManager",
            "getInstance",
            "()Landroid/webkit/CookieManager;",
            &[],
        )
        .map_err(|e| Error::Internal(format!("Failed to get CookieManager instance: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to get CookieManager object: {}", e)))?;

    env.new_global_ref(&cookie_manager).map_err(|e| {
        Error::Internal(format!(
            "Failed to create global reference to CookieManager: {}",
            e
        ))
    })
}

/// Parse a cookie string from Android CookieManager
fn parse_cookie_string(cookie_str: &str) -> Result<Vec<Cookie>> {
    let mut cookies = Vec::new();

    for cookie_part in cookie_str.split(';') {
        let cookie_part = cookie_part.trim();
        if let Some(equals_pos) = cookie_part.find('=') {
            let name = cookie_part[..equals_pos].trim().to_string();
            let value = cookie_part[equals_pos + 1..].trim().to_string();

            // Create basic cookie (Android's getCookie doesn't return full cookie attributes)
            let cookie = Cookie {
                name,
                value,
                domain: String::new(),
                path: "/".to_string(),
                expires: None,
                secure: false,
                http_only: false,
            };

            cookies.push(cookie);
        }
    }

    Ok(cookies)
}

/// Format a cookie for Android's setCookie method
fn format_cookie_for_android(cookie: &Cookie) -> Result<String> {
    let mut cookie_string = format!("{}={}", cookie.name, cookie.value);

    if !cookie.domain.is_empty() {
        cookie_string.push_str(&format!("; Domain={}", cookie.domain));
    }

    if !cookie.path.is_empty() && cookie.path != "/" {
        cookie_string.push_str(&format!("; Path={}", cookie.path));
    }

    if let Some(expires) = &cookie.expires {
        cookie_string.push_str(&format!("; Expires={}", expires));
    }

    if cookie.secure {
        cookie_string.push_str("; Secure");
    }

    if cookie.http_only {
        cookie_string.push_str("; HttpOnly");
    }

    Ok(cookie_string)
}

/// Construct a URL for cookie operations
fn construct_cookie_url(cookie: &Cookie) -> Result<String> {
    let scheme = if cookie.secure { "https" } else { "http" };
    let domain = if cookie.domain.is_empty() {
        "example.com"
    } else {
        &cookie.domain
    };
    let path = if cookie.path.is_empty() {
        "/"
    } else {
        &cookie.path
    };

    Ok(format!("{}://{}{}", scheme, domain, path))
}
