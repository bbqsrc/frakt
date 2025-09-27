//! Request types and builders

use crate::delegate::shared_context::ProgressCallback;
use crate::{Error, Result, body::Body};
use objc2::rc::Retained;
use objc2_foundation::{NSMutableURLRequest, NSString, NSURL, NSURLSession};
use std::collections::HashMap;

/// HTTP methods
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    /// GET method
    GET,
    /// POST method
    POST,
    /// PUT method
    PUT,
    /// DELETE method
    DELETE,
    /// PATCH method
    PATCH,
    /// HEAD method
    HEAD,
    /// Custom method
    Custom(String),
}

impl Method {
    fn as_str(&self) -> &str {
        match self {
            Method::GET => "GET",
            Method::POST => "POST",
            Method::PUT => "PUT",
            Method::DELETE => "DELETE",
            Method::PATCH => "PATCH",
            Method::HEAD => "HEAD",
            Method::Custom(method) => method,
        }
    }
}

/// HTTP request
pub struct Request {
    pub(crate) method: Method,
    pub(crate) url: String,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) body: Option<Body>,
    pub(crate) session: Retained<NSURLSession>,
    pub(crate) delegate: Retained<crate::delegate::DataTaskDelegate>,
    pub(crate) progress_callback: Option<std::sync::Arc<ProgressCallback>>,
}

impl Request {
    /// Send the request and get a response
    pub async fn send(self) -> Result<crate::Response> {
        // Create NSURLRequest
        let nsurl = unsafe {
            NSURL::URLWithString(&NSString::from_str(&self.url)).ok_or(Error::InvalidUrl)?
        };

        let nsrequest = unsafe {
            let req = NSMutableURLRequest::requestWithURL(&nsurl);
            req.setHTTPMethod(&NSString::from_str(self.method.as_str()));

            // Set headers
            for (name, value) in &self.headers {
                req.setValue_forHTTPHeaderField(
                    Some(&NSString::from_str(value)),
                    &NSString::from_str(name),
                );
            }

            // Set body
            if let Some(body) = &self.body {
                match body {
                    Body::Empty => {}
                    Body::Bytes {
                        content,
                        content_type,
                    } => {
                        req.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str(content_type)),
                            &NSString::from_str("Content-Type"),
                        );
                        let nsdata = objc2_foundation::NSData::from_vec(content.to_vec());
                        req.setHTTPBody(Some(&nsdata));
                    }
                    Body::Form { fields } => {
                        req.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str("application/x-www-form-urlencoded")),
                            &NSString::from_str("Content-Type"),
                        );
                        let encoded = encode_form_fields(fields);
                        let nsdata = objc2_foundation::NSData::from_vec(encoded.into_bytes());
                        req.setHTTPBody(Some(&nsdata));
                    }
                    #[cfg(feature = "json")]
                    Body::Json { value } => {
                        req.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str("application/json")),
                            &NSString::from_str("Content-Type"),
                        );
                        let json_bytes = serde_json::to_vec(value)?;
                        let nsdata = objc2_foundation::NSData::from_vec(json_bytes);
                        req.setHTTPBody(Some(&nsdata));
                    }
                    #[cfg(feature = "multipart")]
                    Body::Multipart { parts } => {
                        let boundary = generate_boundary();
                        let content_type = format!("multipart/form-data; boundary={}", boundary);
                        req.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str(&content_type)),
                            &NSString::from_str("Content-Type"),
                        );
                        let multipart_data = encode_multipart_data(&boundary, parts)?;
                        let nsdata = objc2_foundation::NSData::from_vec(multipart_data);
                        req.setHTTPBody(Some(&nsdata));
                    }
                }
            }

            req
        };

        // Create task context
        let task_context = if let Some(progress_callback) = self.progress_callback {
            std::sync::Arc::new(crate::delegate::TaskSharedContext::with_progress_callback(
                progress_callback,
            ))
        } else {
            std::sync::Arc::new(crate::delegate::TaskSharedContext::new())
        };

        // Create data task
        let data_task = unsafe { self.session.dataTaskWithRequest(&nsrequest) };

        // Register the task context with the delegate using the task identifier
        let task_id = unsafe { data_task.taskIdentifier() } as usize;
        self.delegate.register_task(task_id, task_context.clone());

        // Create response future
        let response_future = ResponseFuture::new(data_task, task_context, self.delegate);

        unsafe {
            response_future.data_task.resume();
        }

        response_future.await
    }
}

/// Request builder
pub struct RequestBuilder {
    method: Method,
    url: String,
    headers: HashMap<String, String>,
    body: Option<Body>,
    session: Retained<NSURLSession>,
    delegate: Retained<crate::delegate::DataTaskDelegate>,
    progress_callback: Option<std::sync::Arc<ProgressCallback>>,
}

impl RequestBuilder {
    pub(crate) fn new(
        method: Method,
        url: String,
        session: Retained<NSURLSession>,
        delegate: Retained<crate::delegate::DataTaskDelegate>,
    ) -> Self {
        Self {
            method,
            url,
            headers: HashMap::new(),
            body: None,
            session,
            delegate,
            progress_callback: None,
        }
    }

    /// Add a header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set the request body
    pub fn body(mut self, body: Body) -> Self {
        self.body = Some(body);
        self
    }

    /// Set a JSON body
    #[cfg(feature = "json")]
    pub fn json(mut self, value: impl serde::Serialize) -> Result<Self> {
        self.body = Some(Body::json(value)?);
        Ok(self)
    }

    /// Set a form body
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

    /// Set a text body
    pub fn text(mut self, content: impl Into<String>) -> Self {
        self.body = Some(Body::text(content));
        self
    }

    /// Set a progress callback for tracking download progress
    pub fn progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(u64, Option<u64>) + Send + Sync + 'static,
    {
        self.progress_callback = Some(std::sync::Arc::new(callback));
        self
    }

    /// Send the request
    pub async fn send(self) -> Result<crate::Response> {
        let request = Request {
            method: self.method,
            url: self.url,
            headers: self.headers,
            body: self.body,
            session: self.session,
            delegate: self.delegate,
            progress_callback: self.progress_callback,
        };
        request.send().await
    }
}

/// Future for handling response
struct ResponseFuture {
    data_task: Retained<objc2_foundation::NSURLSessionDataTask>,
    task_context: std::sync::Arc<crate::delegate::TaskSharedContext>,
    _delegate: Retained<crate::delegate::DataTaskDelegate>,
}

impl ResponseFuture {
    fn new(
        data_task: Retained<objc2_foundation::NSURLSessionDataTask>,
        task_context: std::sync::Arc<crate::delegate::TaskSharedContext>,
        delegate: Retained<crate::delegate::DataTaskDelegate>,
    ) -> Self {
        Self {
            data_task,
            task_context,
            _delegate: delegate,
        }
    }
}

impl std::future::Future for ResponseFuture {
    type Output = Result<crate::Response>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.task_context.is_completed() {
            // Check for errors
            if let Some(error) = self.task_context.error.load_full() {
                return std::task::Poll::Ready(Err(Error::from_ns_error(&*error)));
            }

            // Get response
            if let Some(response) = self.task_context.response.load_full() {
                let response = crate::Response::new((*response).clone(), self.task_context.clone());
                return std::task::Poll::Ready(Ok(response));
            }

            return std::task::Poll::Ready(Err(Error::Internal(
                "No response received".to_string(),
            )));
        }

        // Register waker
        let waker = cx.waker().clone();
        let task_context = self.task_context.clone();
        tokio::spawn(async move {
            task_context.waker.register(waker).await;
        });

        std::task::Poll::Pending
    }
}

fn encode_form_fields(
    fields: &[(
        std::borrow::Cow<'static, str>,
        std::borrow::Cow<'static, str>,
    )],
) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(feature = "multipart")]
fn generate_boundary() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("----formdata-rsurlsession-{}", timestamp)
}

#[cfg(feature = "multipart")]
fn encode_multipart_data(boundary: &str, parts: &[crate::body::MultipartPart]) -> Result<Vec<u8>> {
    let mut data = Vec::new();

    for part in parts {
        data.extend_from_slice(format!("\r\n--{}\r\n", boundary).as_bytes());

        let mut disposition = format!("Content-Disposition: form-data; name=\"{}\"", part.name);
        if let Some(filename) = &part.filename {
            disposition.push_str(&format!("; filename=\"{}\"", filename));
        }
        data.extend_from_slice(disposition.as_bytes());
        data.extend_from_slice(b"\r\n");

        if let Some(content_type) = &part.content_type {
            data.extend_from_slice(format!("Content-Type: {}\r\n", content_type).as_bytes());
        }

        data.extend_from_slice(b"\r\n");
        data.extend_from_slice(&part.content);
    }

    data.extend_from_slice(format!("\r\n--{}--\r\n", boundary).as_bytes());
    Ok(data)
}
