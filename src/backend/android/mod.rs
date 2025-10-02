//! Android backend using Cronet (Chromium Network Stack)

mod callback;
mod cronet;
mod download;
mod jni_bindings;
mod request;
mod response;

pub mod cookies;
pub use cookies::AndroidCookieStorage;

#[cfg(test)]
mod tests;

use crate::backend::BackendConfig;
use crate::backend::types::{BackendRequest, BackendResponse};
use crate::{Error, Result};
use jni::{JavaVM, objects::GlobalRef};
use once_cell::sync::Lazy;
use std::sync::Arc;
use url::Url;

// Global JavaVM instance for Android - lives forever
static ANDROID_JVM: Lazy<&'static JavaVM> = Lazy::new(|| {
    Box::leak(Box::new(unsafe {
        jni::JavaVM::from_raw(vampire::java_vm() as *mut _).unwrap()
    }))
});

/// Get the global JavaVM instance
pub(crate) fn get_global_vm() -> Result<&'static JavaVM> {
    Ok(*ANDROID_JVM)
}

/// Android backend using Cronet for HTTP requests
#[derive(Clone)]
pub struct AndroidBackend {
    pub(crate) jvm: &'static JavaVM,
    cronet_engine: Arc<GlobalRef>,
    cookie_jar: Option<crate::CookieJar>,
}

impl AndroidBackend {
    /// Create a new Android backend with default configuration
    pub fn new() -> Result<Self> {
        let jvm = get_global_vm()?;
        let cronet_engine = cronet::create_cronet_engine(jvm)?;

        Ok(Self {
            jvm,
            cronet_engine: Arc::new(cronet_engine),
            cookie_jar: None,
        })
    }

    /// Create a new Android backend with configuration
    pub fn with_config(config: BackendConfig) -> Result<Self> {
        let jvm = get_global_vm()?;
        let cronet_engine = cronet::create_cronet_engine_with_config(jvm, &config)?;

        Ok(Self {
            jvm,
            cronet_engine: Arc::new(cronet_engine),
            cookie_jar: config.cookie_jar,
        })
    }

    /// Execute an HTTP request using Cronet
    pub async fn execute(&self, request: BackendRequest) -> Result<BackendResponse> {
        // Validate URL scheme
        match request.url.scheme() {
            "http" | "https" => {}
            _ => {
                return Err(Error::InvalidUrl);
            }
        }

        request::execute_request(self.jvm, &self.cronet_engine, request).await
    }

    /// Execute a background download using DownloadManager
    pub async fn execute_background_download(
        &self,
        url: Url,
        file_path: std::path::PathBuf,
        session_identifier: Option<String>,
        headers: http::HeaderMap,
        progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
    ) -> Result<crate::client::download::DownloadResponse> {
        download::execute_background_download(
            self.jvm,
            url,
            file_path,
            session_identifier,
            headers,
            progress_callback,
        )
        .await
    }

    /// Get the cookie jar if configured
    pub fn cookie_jar(&self) -> Option<&crate::CookieJar> {
        self.cookie_jar.as_ref()
    }
}
