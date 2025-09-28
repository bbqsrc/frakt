//! HTTP client utilities for Windows backend

use crate::backend::types::BackendResponse;
use crate::{Error, Result};
use tokio::sync::mpsc;
use windows::{
    core::{Interface, HSTRING},
    Storage::Streams::DataReader,
    Web::Http::{
        HttpResponseMessage, HttpStringContent, HttpMultipartFormDataContent,
        IHttpContent,
    },
};

/// Send-safe wrapper for IHttpContent
struct SendableHttpContent(*mut std::ffi::c_void);

unsafe impl Send for SendableHttpContent {}

impl SendableHttpContent {
    fn new(content: IHttpContent) -> Self {
        Self(content.as_raw() as *mut std::ffi::c_void)
    }

    fn to_interface(&self) -> Result<IHttpContent> {
        unsafe {
            let unknown = windows::core::IUnknown::from_raw(self.0);
            unknown.cast().map_err(|e| Error::Internal(format!("Failed to cast to IHttpContent: {}", e)))
        }
    }
}

/// Convert body to Windows HTTP content
pub fn convert_body_to_http_content(body: crate::body::Body) -> Result<IHttpContent> {
    match body {
        crate::body::Body::Empty => {
            // Create empty content
            let content = HttpStringContent::CreateFromString(&HSTRING::from(""))
                .map_err(|e| Error::Internal(format!("Failed to create empty content: {}", e)))?;
            Ok(content.cast().map_err(|e| Error::Internal(format!("Failed to cast content: {}", e)))?)
        }
        crate::body::Body::Bytes { content, content_type } => {
            // Create buffer content from bytes
            let content_str = String::from_utf8(content.to_vec())
                .map_err(|_| Error::Internal("Failed to convert bytes to string".to_string()))?;

            let http_content = HttpStringContent::CreateFromString(&HSTRING::from(content_str))
                .map_err(|e| Error::Internal(format!("Failed to create string content: {}", e)))?;

            // Set content type if provided
            if !content_type.is_empty() {
                let headers = http_content.Headers()
                    .map_err(|e| Error::Internal(format!("Failed to get content headers: {}", e)))?;

                if let Ok(content_type_header) = headers.ContentType() {
                    let _ = content_type_header.SetMediaType(&HSTRING::from(content_type));
                }
            }

            Ok(http_content.cast().map_err(|e| Error::Internal(format!("Failed to cast content: {}", e)))?)
        }
        crate::body::Body::Form { fields } => {
            // Encode form data
            let encoded = fields
                .iter()
                .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
                .collect::<Vec<_>>()
                .join("&");

            let content = HttpStringContent::CreateFromString(&HSTRING::from(encoded))
                .map_err(|e| Error::Internal(format!("Failed to create form content: {}", e)))?;

            // Set content type
            let headers = content.Headers()
                .map_err(|e| Error::Internal(format!("Failed to get content headers: {}", e)))?;

            if let Ok(content_type_header) = headers.ContentType() {
                let _ = content_type_header.SetMediaType(&HSTRING::from("application/x-www-form-urlencoded"));
            }

            Ok(content.cast().map_err(|e| Error::Internal(format!("Failed to cast content: {}", e)))?)
        }
        #[cfg(feature = "json")]
        crate::body::Body::Json { value } => {
            let json_string = serde_json::to_string(&value)
                .map_err(|e| Error::Internal(format!("Failed to serialize JSON: {}", e)))?;

            let content = HttpStringContent::CreateFromString(&HSTRING::from(json_string))
                .map_err(|e| Error::Internal(format!("Failed to create JSON content: {}", e)))?;

            // Set content type
            let headers = content.Headers()
                .map_err(|e| Error::Internal(format!("Failed to get content headers: {}", e)))?;

            if let Ok(content_type_header) = headers.ContentType() {
                let _ = content_type_header.SetMediaType(&HSTRING::from("application/json"));
            }

            Ok(content.cast().map_err(|e| Error::Internal(format!("Failed to cast content: {}", e)))?)
        }
        #[cfg(feature = "multipart")]
        crate::body::Body::Multipart { parts } => {
            // Create multipart form data content
            let multipart_content = HttpMultipartFormDataContent::new()
                .map_err(|e| Error::Internal(format!("Failed to create multipart content: {}", e)))?;

            for part in parts {
                // Convert bytes to string for Windows HttpStringContent
                let content_str = String::from_utf8(part.content.to_vec())
                    .map_err(|_| Error::Internal("Failed to convert bytes to string for multipart".to_string()))?;

                let part_content = HttpStringContent::CreateFromString(&HSTRING::from(content_str))
                    .map_err(|e| Error::Internal(format!("Failed to create part content: {}", e)))?;

                // Set content type if provided
                if let Some(ref content_type) = part.content_type {
                    if let Ok(headers) = part_content.Headers() {
                        if let Ok(content_type_header) = headers.ContentType() {
                            let _ = content_type_header.SetMediaType(&HSTRING::from(content_type.as_str()));
                        }
                    }
                }

                let part_content_interface = part_content.cast::<IHttpContent>()
                    .map_err(|e| Error::Internal(format!("Failed to cast part content: {}", e)))?;

                // Add part with name and optional filename
                if let Some(ref filename) = part.filename {
                    multipart_content.AddWithNameAndFileName(
                        &part_content_interface,
                        &HSTRING::from(part.name.as_str()),
                        &HSTRING::from(filename.as_str())
                    ).map_err(|e| Error::Internal(format!("Failed to add multipart part with filename: {}", e)))?;
                } else {
                    multipart_content.AddWithName(
                        &part_content_interface,
                        &HSTRING::from(part.name.as_str())
                    ).map_err(|e| Error::Internal(format!("Failed to add multipart part: {}", e)))?;
                }
            }

            Ok(multipart_content.cast()
                .map_err(|e| Error::Internal(format!("Failed to cast multipart content: {}", e)))?)
        }
    }
}

/// Convert Windows HTTP response to BackendResponse
pub async fn convert_http_response_to_backend_response(
    response: HttpResponseMessage,
) -> Result<BackendResponse> {
    // Get status code
    let status_code = response.StatusCode()
        .map_err(|e| Error::Internal(format!("Failed to get status code: {}", e)))?;
    let status = http::StatusCode::from_u16(status_code.0 as u16)
        .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);

    // Get headers
    let mut headers = http::HeaderMap::new();
    if let Ok(response_headers) = response.Headers() {
        // Iterate through Windows response headers and convert to http::HeaderMap
        for header_pair in response_headers {
            let key = header_pair.Key().unwrap_or_default().to_string();
            let value = header_pair.Value().unwrap_or_default().to_string();

            if let Ok(header_name) = http::HeaderName::from_bytes(key.as_bytes()) {
                if let Ok(header_value) = http::HeaderValue::from_str(&value) {
                    headers.insert(header_name, header_value);
                }
            }
        }
    }

    // Get content for streaming
    let content = response.Content()
        .map_err(|e| Error::Internal(format!("Failed to get response content: {}", e)))?;

    // Create channel for streaming body
    let (tx, rx) = mpsc::channel(32);

    // Stream content using Windows async operations with Send wrapper
    let sendable_content = SendableHttpContent::new(content);
    let tx_clone = tx.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async move {
            match sendable_content.to_interface() {
                Ok(content) => {
                    match stream_windows_content(content, tx_clone).await {
                        Ok(_) => {},
                        Err(e) => {
                            eprintln!("Windows content streaming error: {}", e);
                        }
                    }
                },
                Err(e) => {
                    eprintln!("Failed to convert sendable content: {}", e);
                }
            }
        });
    });

    Ok(BackendResponse {
        status,
        headers,
        body_receiver: rx,
    })
}

/// Stream Windows HTTP content to a tokio channel
async fn stream_windows_content(
    content: IHttpContent,
    tx: mpsc::Sender<Result<bytes::Bytes>>,
) -> Result<()> {
    // Read content as input stream
    let input_stream = content.ReadAsInputStreamAsync()
        .map_err(|e| Error::Internal(format!("Failed to get input stream: {}", e)))?
        .await
        .map_err(|e| super::error::map_windows_error(e))?;

    // Create data reader for the stream
    let data_reader = DataReader::CreateDataReader(&input_stream)
        .map_err(|e| Error::Internal(format!("Failed to create data reader: {}", e)))?;

    // Stream in chunks
    let chunk_size = 8192u32; // 8KB chunks
    loop {
        // Load data into the reader buffer
        let bytes_loaded = data_reader.LoadAsync(chunk_size)
            .map_err(|e| Error::Internal(format!("Failed to load data: {}", e)))?
            .await
            .map_err(|e| super::error::map_windows_error(e))?;

        if bytes_loaded == 0 {
            // End of stream
            break;
        }

        // Read the loaded bytes
        let mut buffer = vec![0u8; bytes_loaded as usize];
        data_reader.ReadBytes(&mut buffer)
            .map_err(|e| Error::Internal(format!("Failed to read bytes: {}", e)))?;

        // Send chunk through channel
        let chunk = bytes::Bytes::from(buffer);
        if tx.send(Ok(chunk)).await.is_err() {
            // Receiver has been dropped, stop streaming
            break;
        }
    }

    Ok(())
}