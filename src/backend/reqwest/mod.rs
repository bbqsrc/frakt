//! Reqwest backend for cross-platform HTTP support

use crate::backend::types::{BackendRequest, BackendResponse};
use crate::{Error, Result};
use futures_util::StreamExt;
use tokio::sync::mpsc;

/// Reqwest backend for cross-platform HTTP
#[derive(Clone)]
pub struct ReqwestBackend {
    client: reqwest::Client,
}

impl ReqwestBackend {
    /// Create a new Reqwest backend
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| Error::Internal(format!("Failed to create reqwest client: {}", e)))?;

        Ok(Self { client })
    }

    /// Create a new Reqwest backend with configuration
    pub fn with_config(config: crate::backend::BackendConfig) -> Result<Self> {
        let mut builder = reqwest::Client::builder();

        // Apply timeout configuration
        if let Some(timeout) = config.timeout {
            builder = builder.timeout(timeout);
        }

        // Apply user agent configuration
        if let Some(user_agent) = config.user_agent {
            builder = builder.user_agent(user_agent);
        }

        let client = builder
            .build()
            .map_err(|e| Error::Internal(format!("Failed to create reqwest client: {}", e)))?;

        Ok(Self { client })
    }

    /// Execute an HTTP request using reqwest
    pub async fn execute(&self, request: BackendRequest) -> Result<BackendResponse> {
        // Convert method
        let method = match request.method {
            http::Method::GET => reqwest::Method::GET,
            http::Method::POST => reqwest::Method::POST,
            http::Method::PUT => reqwest::Method::PUT,
            http::Method::DELETE => reqwest::Method::DELETE,
            http::Method::HEAD => reqwest::Method::HEAD,
            http::Method::PATCH => reqwest::Method::PATCH,
            _ => {
                return Err(Error::Internal(format!(
                    "Unsupported method: {}",
                    request.method
                )));
            }
        };

        // Build request
        let mut req_builder = self.client.request(method, &request.url);

        // Add headers
        for (name, value) in &request.headers {
            req_builder = req_builder.header(name, value);
        }

        // Add body
        if let Some(body) = request.body {
            match &body {
                #[cfg(feature = "multipart")]
                crate::body::Body::Multipart { parts } => {
                    let mut form = reqwest::multipart::Form::new();
                    for part in parts {
                        let mut part_builder =
                            reqwest::multipart::Part::bytes(part.content.to_vec());

                        if let Some(filename) = &part.filename {
                            part_builder = part_builder.file_name(filename.clone());
                        }

                        if let Some(content_type) = &part.content_type {
                            part_builder = part_builder.mime_str(content_type).map_err(|e| {
                                Error::Internal(format!("Invalid content type: {}", e))
                            })?;
                        }

                        form = form.part(part.name.clone(), part_builder);
                    }
                    req_builder = req_builder.multipart(form);
                }
                _ => {
                    req_builder = req_builder.body(self.convert_body(body)?);
                }
            }
        }

        // Send request
        let response = req_builder.send().await.map_err(|e| Error::Network {
            code: -1,
            message: format!("Request failed: {}", e),
        })?;

        // Extract status and headers
        let status = response.status();
        let headers = response.headers().clone();

        // Convert headers to http::HeaderMap
        let mut header_map = http::HeaderMap::new();
        for (name, value) in headers.iter() {
            if let Ok(header_name) = http::HeaderName::from_bytes(name.as_str().as_bytes()) {
                if let Ok(header_value) = http::HeaderValue::from_bytes(value.as_bytes()) {
                    header_map.insert(header_name, header_value);
                }
            }
        }

        // Create channel for streaming body
        let (tx, rx) = mpsc::channel(32);

        // Stream response body
        tokio::spawn(async move {
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        if tx.send(Ok(bytes::Bytes::from(bytes))).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Err(Error::Network {
                                code: -1,
                                message: format!("Stream error: {}", e),
                            }))
                            .await;
                        break;
                    }
                }
            }
        });

        Ok(BackendResponse {
            status,
            headers: header_map,
            body_receiver: rx,
        })
    }

    fn convert_body(&self, body: crate::body::Body) -> Result<reqwest::Body> {
        match body {
            crate::body::Body::Empty => Ok(reqwest::Body::from("")),
            crate::body::Body::Bytes { content, .. } => Ok(reqwest::Body::from(content)),
            crate::body::Body::Form { fields } => {
                let form_data = fields
                    .iter()
                    .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
                    .collect::<Vec<_>>()
                    .join("&");
                Ok(reqwest::Body::from(form_data))
            }
            #[cfg(feature = "json")]
            crate::body::Body::Json { value } => {
                let json_bytes = serde_json::to_vec(&value)?;
                Ok(reqwest::Body::from(json_bytes))
            }
            #[cfg(feature = "multipart")]
            crate::body::Body::Multipart { .. } => {
                // Multipart is handled separately in the execute function
                Ok(reqwest::Body::from(""))
            }
        }
    }
}
