//! Low-level JNI bindings for Cronet classes

use jni::{AttachGuard, JNIEnv, objects::JObject, objects::JString};

/// JNI class names for Cronet
pub const CRONET_ENGINE_CLASS: &str = "org/chromium/net/CronetEngine";
pub const CRONET_ENGINE_BUILDER_CLASS: &str = "org/chromium/net/CronetEngine$Builder";
pub const URL_REQUEST_CLASS: &str = "org/chromium/net/UrlRequest";
pub const URL_REQUEST_BUILDER_CLASS: &str = "org/chromium/net/UrlRequest$Builder";
pub const URL_REQUEST_CALLBACK_CLASS: &str = "org/chromium/net/UrlRequest$Callback";
pub const URL_RESPONSE_INFO_CLASS: &str = "org/chromium/net/UrlResponseInfo";
pub const CRONET_EXCEPTION_CLASS: &str = "org/chromium/net/CronetException";
pub const UPLOAD_DATA_PROVIDER_CLASS: &str = "org/chromium/net/UploadDataProvider";
pub const UPLOAD_DATA_SINK_CLASS: &str = "org/chromium/net/UploadDataSink";

/// HTTP method constants
#[derive(Debug, Clone, Copy)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Head,
    Patch,
    Options,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Options => "OPTIONS",
        }
    }
}

/// Priority constants for UrlRequest
#[derive(Debug, Clone, Copy)]
pub enum RequestPriority {
    Idle = 0,
    Lowest = 1,
    Low = 2,
    Medium = 3,
    Highest = 4,
}

/// Safe wrapper for creating URL requests
pub struct UrlRequestBuilder<'a> {
    env: AttachGuard<'a>,
    builder: JObject<'a>,
}

impl<'a> UrlRequestBuilder<'a> {
    /// Create a new UrlRequest.Builder
    pub fn new(
        mut env: AttachGuard<'a>,
        engine: &JObject,
        url: &str,
        callback: &JObject,
    ) -> Result<Self, jni::errors::Error> {
        let url_jstring = env.new_string(url)?;

        // Create an executor using Java's Executors.newCachedThreadPool()
        let executors_class = env.find_class("java/util/concurrent/Executors")?;
        let executor = env
            .call_static_method(
                executors_class,
                "newCachedThreadPool",
                "()Ljava/util/concurrent/ExecutorService;",
                &[],
            )?
            .l()?;

        let builder = env.call_method(
            engine,
            "newUrlRequestBuilder",
            "(Ljava/lang/String;Lorg/chromium/net/UrlRequest$Callback;Ljava/util/concurrent/Executor;)Lorg/chromium/net/UrlRequest$Builder;",
            &[
                (&url_jstring).into(),
                callback.into(),
                (&executor).into(),
            ],
        )?;

        Ok(Self {
            env,
            builder: builder.l()?,
        })
    }

    /// Set HTTP method
    pub fn set_http_method(&mut self, method: HttpMethod) -> Result<&mut Self, jni::errors::Error> {
        let method_str = self.env.new_string(method.as_str())?;

        self.env.call_method(
            &self.builder,
            "setHttpMethod",
            "(Ljava/lang/String;)Lorg/chromium/net/UrlRequest$Builder;",
            &[(&method_str).into()],
        )?;

        Ok(self)
    }

    /// Add a header
    pub fn add_header(&mut self, name: &str, value: &str) -> Result<&mut Self, jni::errors::Error> {
        let name_str = self.env.new_string(name)?;
        let value_str = self.env.new_string(value)?;

        self.env.call_method(
            &self.builder,
            "addHeader",
            "(Ljava/lang/String;Ljava/lang/String;)Lorg/chromium/net/UrlRequest$Builder;",
            &[(&name_str).into(), (&value_str).into()],
        )?;

        Ok(self)
    }

    /// Set request priority
    pub fn set_priority(
        &mut self,
        priority: RequestPriority,
    ) -> Result<&mut Self, jni::errors::Error> {
        self.env.call_method(
            &self.builder,
            "setPriority",
            "(I)Lorg/chromium/net/UrlRequest$Builder;",
            &[(priority as i32).into()],
        )?;

        Ok(self)
    }

    /// Set upload data provider
    pub fn set_upload_data_provider(
        &mut self,
        provider: &JObject,
    ) -> Result<&mut Self, jni::errors::Error> {
        // Create the executor first to avoid multiple mutable borrows
        let executor = self
            .env
            .call_static_method(
                "java/util/concurrent/Executors",
                "newSingleThreadExecutor",
                "()Ljava/util/concurrent/ExecutorService;",
                &[],
            )?
            .l()?;

        self.env.call_method(
            &self.builder,
            "setUploadDataProvider",
            "(Lorg/chromium/net/UploadDataProvider;Ljava/util/concurrent/Executor;)Lorg/chromium/net/UrlRequest$Builder;",
            &[
                provider.into(),
                // Use the same executor - in practice might want different executor for uploads
                (&executor).into(),
            ],
        )?;

        Ok(self)
    }

    /// Build the URL request
    pub fn build(mut self) -> Result<JObject<'a>, jni::errors::Error> {
        let request = self.env.call_method(
            &self.builder,
            "build",
            "()Lorg/chromium/net/UrlRequest;",
            &[],
        )?;

        Ok(request.l()?)
    }
}

/// Helper functions for working with Cronet objects
pub mod helpers {
    use super::*;

    /// Extract status code from UrlResponseInfo
    pub fn get_response_status_code(
        env: &mut JNIEnv,
        response_info: &JObject,
    ) -> Result<i32, jni::errors::Error> {
        let status = env.call_method(response_info, "getHttpStatusCode", "()I", &[])?;
        Ok(status.i()?)
    }

    /// Extract status text from UrlResponseInfo
    pub fn get_response_status_text(
        env: &mut JNIEnv,
        response_info: &JObject,
    ) -> Result<String, jni::errors::Error> {
        let status_text = env.call_method(
            response_info,
            "getHttpStatusText",
            "()Ljava/lang/String;",
            &[],
        )?;
        let status_text_str: JString = status_text.l()?.into();
        let status_text_rust: String = env.get_string(&status_text_str)?.into();
        Ok(status_text_rust)
    }

    /// Get all response headers
    pub fn get_response_headers(
        env: &mut JNIEnv,
        response_info: &JObject,
    ) -> Result<Vec<(String, Vec<String>)>, jni::errors::Error> {
        let headers_map =
            env.call_method(response_info, "getAllHeaders", "()Ljava/util/Map;", &[])?;

        // This is simplified - in practice you'd iterate over the Map entries
        // For now, return empty headers
        let _ = headers_map; // Suppress warning until full implementation
        Ok(vec![])
    }

    /// Check if this is a redirect response
    pub fn was_cached(
        env: &mut JNIEnv,
        response_info: &JObject,
    ) -> Result<bool, jni::errors::Error> {
        let was_cached = env.call_method(response_info, "wasCached", "()Z", &[])?;
        Ok(was_cached.z()?)
    }
}
