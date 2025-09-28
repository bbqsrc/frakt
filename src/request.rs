//! Request types and builders with backend abstraction

use std::sync::Arc;

use crate::backend::{Backend, types::BackendRequest};
use crate::{Result, body::Body};
use http::{HeaderMap, HeaderValue, Method, header};
use url::Url;

/// An HTTP request ready to be executed using any backend
pub struct Request {
    pub(crate) method: Method,
    pub(crate) url: Url,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Option<Body>,
    pub(crate) backend: Backend,
    pub(crate) progress_callback: Option<Arc<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
}

impl Request {
    /// Send the request and get a response
    pub async fn send(self) -> Result<crate::Response> {
        let backend_request = BackendRequest {
            method: self.method,
            url: self.url,
            headers: self.headers,
            body: self.body,
            progress_callback: self.progress_callback,
        };

        let backend_response = self.backend.execute(backend_request).await?;
        Ok(crate::Response::from_backend(backend_response))
    }
}

/// Builder for constructing HTTP requests with backend abstraction
pub struct RequestBuilder {
    method: Method,
    url: Url,
    headers: HeaderMap,
    body: Option<Body>,
    backend: Backend,
    progress_callback: Option<Arc<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
}

impl RequestBuilder {
    pub(crate) fn new(method: Method, url: Url, backend: Backend) -> Self {
        Self {
            method,
            url,
            headers: HeaderMap::new(),
            body: None,
            backend,
            progress_callback: None,
        }
    }

    /// Add a header to the request
    pub fn header(
        mut self,
        name: impl TryInto<http::HeaderName>,
        value: impl Into<String>,
    ) -> crate::Result<Self> {
        let header_name = name.try_into().map_err(|_| crate::Error::InvalidHeader)?;
        let header_value =
            HeaderValue::from_str(&value.into()).map_err(|_| crate::Error::InvalidHeader)?;
        self.headers.insert(header_name, header_value);
        Ok(self)
    }

    /// Set the request body
    pub fn body(mut self, body: impl Into<Body>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set a JSON body from a serializable value
    #[cfg(feature = "json")]
    pub fn json(mut self, value: impl serde::Serialize) -> Result<Self> {
        self.body = Some(Body::json(value)?);
        Ok(self)
    }

    /// Set a form-urlencoded body from field/value pairs
    pub fn form(
        mut self,
        fields: Vec<(
            impl Into<std::borrow::Cow<'static, str>>,
            impl Into<std::borrow::Cow<'static, str>>,
        )>,
    ) -> Self {
        self.body = Some(Body::form(fields));
        self
    }

    /// Set a plain text body
    pub fn text(mut self, content: impl Into<String>) -> Self {
        self.body = Some(Body::text(content));
        self
    }

    /// Set authentication for the request
    pub fn auth(mut self, auth: crate::Auth) -> Self {
        let header_value =
            HeaderValue::from_str(&auth.to_header_value()).expect("Invalid auth header value");
        self.headers.insert(header::AUTHORIZATION, header_value);
        self
    }

    /// Set progress callback
    pub fn progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(u64, Option<u64>) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
        self
    }


    /// Send the request and return the response
    pub async fn send(self) -> Result<crate::Response> {
        let request = Request {
            method: self.method,
            url: self.url,
            headers: self.headers,
            body: self.body,
            backend: self.backend,
            progress_callback: self.progress_callback,
        };
        request.send().await
    }
}
