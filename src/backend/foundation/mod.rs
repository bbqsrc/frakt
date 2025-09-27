//! Foundation backend using NSURLSession

/// Foundation delegate implementations
pub mod delegate;

use crate::backend::types::{BackendRequest, BackendResponse};
use crate::{Error, Result};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{
    NSHTTPURLResponse, NSMutableURLRequest, NSString, NSURL, NSURLSession,
};
use std::str::FromStr;
use tokio::sync::mpsc;

/// Foundation backend using NSURLSession
#[derive(Clone)]
pub struct FoundationBackend {
    session: Retained<NSURLSession>,
    delegate: Retained<delegate::DataTaskDelegate>,
}

impl FoundationBackend {
    /// Create a new Foundation backend with default configuration
    pub fn new() -> Result<Self> {
        // Create delegate and session with delegate
        let delegate = delegate::DataTaskDelegate::new();
        let session = unsafe {
            NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &objc2_foundation::NSURLSessionConfiguration::defaultSessionConfiguration(),
                Some(ProtocolObject::from_ref(&*delegate)),
                None,
            )
        };

        Ok(Self { session, delegate })
    }

    /// Create a new Foundation backend with custom session configuration
    pub fn with_session(
        session: Retained<NSURLSession>,
        delegate: Retained<delegate::DataTaskDelegate>,
    ) -> Self {
        Self { session, delegate }
    }

    /// Create a new Foundation backend with configuration
    pub fn with_config(config: crate::backend::BackendConfig) -> Result<Self> {
        let delegate = delegate::DataTaskDelegate::new();

        let session_config =
            unsafe { objc2_foundation::NSURLSessionConfiguration::defaultSessionConfiguration() };

        // Apply timeout configuration
        if let Some(timeout) = config.timeout {
            unsafe {
                session_config.setTimeoutIntervalForRequest(timeout.as_secs_f64());
            }
        }

        let session = unsafe {
            NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &session_config,
                Some(ProtocolObject::from_ref(&*delegate)),
                None,
            )
        };

        Ok(Self { session, delegate })
    }

    /// Get the underlying NSURLSession (for WebSocket support)
    pub fn session(&self) -> &Retained<NSURLSession> {
        &self.session
    }

    /// Execute an HTTP request using NSURLSession
    pub async fn execute(&self, request: BackendRequest) -> Result<BackendResponse> {
        // Create NSURLRequest
        let nsurl = unsafe {
            NSURL::URLWithString(&NSString::from_str(&request.url)).ok_or(Error::InvalidUrl)?
        };

        let nsrequest = unsafe {
            let req = NSMutableURLRequest::requestWithURL(&nsurl);
            req.setHTTPMethod(&NSString::from_str(request.method.as_str()));

            // Set headers
            for (name, value) in &request.headers {
                req.setValue_forHTTPHeaderField(
                    Some(&NSString::from_str(
                        value.to_str().expect("Invalid header value"),
                    )),
                    &NSString::from_str(name.as_str()),
                );
            }

            // Set body
            if let Some(body) = &request.body {
                match body {
                    crate::body::Body::Empty => {}
                    crate::body::Body::Bytes {
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
                    crate::body::Body::Form { fields } => {
                        req.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str("application/x-www-form-urlencoded")),
                            &NSString::from_str("Content-Type"),
                        );
                        let encoded = encode_form_fields(fields);
                        let nsdata = objc2_foundation::NSData::from_vec(encoded.into_bytes());
                        req.setHTTPBody(Some(&nsdata));
                    }
                    #[cfg(feature = "json")]
                    crate::body::Body::Json { value } => {
                        req.setValue_forHTTPHeaderField(
                            Some(&NSString::from_str("application/json")),
                            &NSString::from_str("Content-Type"),
                        );
                        let json_bytes = serde_json::to_vec(value)?;
                        let nsdata = objc2_foundation::NSData::from_vec(json_bytes);
                        req.setHTTPBody(Some(&nsdata));
                    }
                    #[cfg(feature = "multipart")]
                    crate::body::Body::Multipart { parts } => {
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
        let task_context = std::sync::Arc::new(delegate::shared_context::TaskSharedContext::new());

        // Create data task
        let data_task = unsafe { self.session.dataTaskWithRequest(&nsrequest) };

        // Register the task context with the delegate using the task identifier
        let task_id = unsafe { data_task.taskIdentifier() } as usize;
        self.delegate.register_task(task_id, task_context.clone());

        // Create channel for response body
        let (tx, rx) = mpsc::channel(32);

        // Start task
        unsafe {
            data_task.resume();
        }

        // Wait for response headers
        while !task_context.is_completed() && task_context.response.load_full().is_none() {
            tokio::task::yield_now().await;
        }

        // Check for errors
        if let Some(error) = task_context.error.load_full() {
            return Err(Error::from_ns_error(&*error));
        }

        // Get response
        let response = task_context
            .response
            .load_full()
            .ok_or_else(|| Error::Internal("No response received".to_string()))?;

        let status = unsafe {
            // Try to cast to NSHTTPURLResponse to get status code
            let http_response: Option<&NSHTTPURLResponse> = response.as_ref().downcast_ref();
            if let Some(http_response) = http_response {
                http_response.statusCode() as u16
            } else {
                200
            }
        };
        let status_code = http::StatusCode::from_u16(status as u16).unwrap_or(http::StatusCode::OK);

        // Extract headers from NSHTTPURLResponse
        let mut headers = http::HeaderMap::new();

        unsafe {
            if let Some(http_response) = response.as_ref().downcast_ref::<NSHTTPURLResponse>() {
                let _all_headers = http_response.allHeaderFields();

                // TODO: Implement proper header extraction from NSDictionary
                // The objc2-foundation API for iterating NSDictionary is complex
                // For now, at least we extract the basic headers manually:

                // Add common headers that we can extract directly
                if let Some(content_type) = http_response.valueForHTTPHeaderField(&NSString::from_str("Content-Type")) {
                    if let Ok(ct_str) = content_type.to_string().parse::<http::HeaderValue>() {
                        headers.insert(http::header::CONTENT_TYPE, ct_str);
                    }
                }

                if let Some(content_length) = http_response.valueForHTTPHeaderField(&NSString::from_str("Content-Length")) {
                    if let Ok(cl_str) = content_length.to_string().parse::<http::HeaderValue>() {
                        headers.insert(http::header::CONTENT_LENGTH, cl_str);
                    }
                }
            }
        }

        // Spawn task to read body data
        let body_context = task_context.clone();
        tokio::spawn(async move {
            while !body_context.is_completed() {
                let data = body_context.response_buffer.lock().await.clone();
                if !data.is_empty() {
                    let bytes = bytes::Bytes::from(data);
                    if tx.send(Ok(bytes)).await.is_err() {
                        break;
                    }
                    // Clear the buffer
                    body_context.response_buffer.lock().await.clear();
                }
                tokio::task::yield_now().await;
            }

            // Send any remaining data when task completes
            let final_data = body_context.response_buffer.lock().await.clone();
            if !final_data.is_empty() {
                let bytes = bytes::Bytes::from(final_data);
                let _ = tx.send(Ok(bytes)).await;
            }
        });

        Ok(BackendResponse {
            status: status_code,
            headers,
            body_receiver: rx,
        })
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
