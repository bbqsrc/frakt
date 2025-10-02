//! Response handling for Cronet responses

use crate::{Error, Result};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use jni::{JNIEnv, objects::JObject};
use std::str::FromStr;

/// Extract response information from Cronet's UrlResponseInfo
pub struct ResponseInfo {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub was_cached: bool,
    pub negotiated_protocol: String,
    pub received_byte_count: i64,
}

impl ResponseInfo {
    /// Extract response info from Java UrlResponseInfo object
    pub fn from_cronet_response(env: &mut JNIEnv, response_info: &JObject) -> Result<Self> {
        // Get status code
        let status_code = env
            .call_method(response_info, "getHttpStatusCode", "()I", &[])
            .map_err(|e| Error::Internal(format!("Failed to get HTTP status code: {}", e)))?
            .i()
            .map_err(|e| Error::Internal(format!("Failed to convert status code: {}", e)))?;

        let status = StatusCode::from_u16(status_code as u16).map_err(|e| {
            Error::Internal(format!("Invalid HTTP status code {}: {}", status_code, e))
        })?;

        // Get headers
        let headers = Self::extract_headers(env, response_info)?;

        // Get cache status
        let was_cached = env
            .call_method(response_info, "wasCached", "()Z", &[])
            .map_err(|e| Error::Internal(format!("Failed to get cache status: {}", e)))?
            .z()
            .map_err(|e| Error::Internal(format!("Failed to convert cache status: {}", e)))?;

        // Get negotiated protocol
        let protocol = env
            .call_method(
                response_info,
                "getNegotiatedProtocol",
                "()Ljava/lang/String;",
                &[],
            )
            .map_err(|e| Error::Internal(format!("Failed to get negotiated protocol: {}", e)))?
            .l()
            .map_err(|e| Error::Internal(format!("Failed to get protocol string: {}", e)))?;

        let protocol_string: String = env
            .get_string(&protocol.into())
            .map_err(|e| Error::Internal(format!("Failed to convert protocol string: {}", e)))?
            .into();

        // Get received byte count
        let received_bytes = env
            .call_method(response_info, "getReceivedByteCount", "()J", &[])
            .map_err(|e| Error::Internal(format!("Failed to get received byte count: {}", e)))?
            .j()
            .map_err(|e| {
                Error::Internal(format!("Failed to convert received byte count: {}", e))
            })?;

        Ok(ResponseInfo {
            status,
            headers,
            was_cached,
            negotiated_protocol: protocol_string,
            received_byte_count: received_bytes,
        })
    }

    /// Extract headers from UrlResponseInfo
    fn extract_headers(env: &mut JNIEnv, response_info: &JObject) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();

        // Get all headers as a Map<String, List<String>>
        let headers_map = env
            .call_method(response_info, "getAllHeaders", "()Ljava/util/Map;", &[])
            .map_err(|e| Error::Internal(format!("Failed to get response headers: {}", e)))?
            .l()
            .map_err(|e| Error::Internal(format!("Failed to get headers map object: {}", e)))?;

        // Get the entry set
        let entry_set = env
            .call_method(&headers_map, "entrySet", "()Ljava/util/Set;", &[])
            .map_err(|e| Error::Internal(format!("Failed to get headers entry set: {}", e)))?
            .l()
            .map_err(|e| Error::Internal(format!("Failed to get entry set object: {}", e)))?;

        // Get iterator
        let iterator = env
            .call_method(&entry_set, "iterator", "()Ljava/util/Iterator;", &[])
            .map_err(|e| Error::Internal(format!("Failed to get headers iterator: {}", e)))?
            .l()
            .map_err(|e| Error::Internal(format!("Failed to get iterator object: {}", e)))?;

        // Iterate through headers
        loop {
            let has_next = env
                .call_method(&iterator, "hasNext", "()Z", &[])
                .map_err(|e| Error::Internal(format!("Failed to check iterator hasNext: {}", e)))?
                .z()
                .map_err(|e| Error::Internal(format!("Failed to convert hasNext: {}", e)))?;

            if !has_next {
                break;
            }

            let entry = env
                .call_method(&iterator, "next", "()Ljava/lang/Object;", &[])
                .map_err(|e| Error::Internal(format!("Failed to get next entry: {}", e)))?
                .l()
                .map_err(|e| Error::Internal(format!("Failed to get entry object: {}", e)))?;

            // Get key (header name)
            let key = env
                .call_method(&entry, "getKey", "()Ljava/lang/Object;", &[])
                .map_err(|e| Error::Internal(format!("Failed to get header key: {}", e)))?
                .l()
                .map_err(|e| Error::Internal(format!("Failed to get key object: {}", e)))?;

            let key_string: String = env
                .get_string(&key.into())
                .map_err(|e| Error::Internal(format!("Failed to convert header key: {}", e)))?
                .into();

            // Get value (List<String>)
            let value_list = env
                .call_method(&entry, "getValue", "()Ljava/lang/Object;", &[])
                .map_err(|e| Error::Internal(format!("Failed to get header value list: {}", e)))?
                .l()
                .map_err(|e| Error::Internal(format!("Failed to get value list object: {}", e)))?;

            // Iterate through values (headers can have multiple values)
            let value_iterator = env
                .call_method(&value_list, "iterator", "()Ljava/util/Iterator;", &[])
                .map_err(|e| Error::Internal(format!("Failed to get value iterator: {}", e)))?
                .l()
                .map_err(|e| {
                    Error::Internal(format!("Failed to get value iterator object: {}", e))
                })?;

            while env
                .call_method(&value_iterator, "hasNext", "()Z", &[])
                .map_err(|e| {
                    Error::Internal(format!("Failed to check value iterator hasNext: {}", e))
                })?
                .z()
                .map_err(|e| Error::Internal(format!("Failed to convert value hasNext: {}", e)))?
            {
                let value = env
                    .call_method(&value_iterator, "next", "()Ljava/lang/Object;", &[])
                    .map_err(|e| Error::Internal(format!("Failed to get next value: {}", e)))?
                    .l()
                    .map_err(|e| Error::Internal(format!("Failed to get value object: {}", e)))?;

                let value_string: String = env
                    .get_string(&value.into())
                    .map_err(|e| Error::Internal(format!("Failed to convert header value: {}", e)))?
                    .into();

                // Add header to map
                let header_name = HeaderName::from_str(&key_string).map_err(|e| {
                    Error::Internal(format!("Invalid header name '{}': {}", key_string, e))
                })?;

                let header_value = HeaderValue::from_str(&value_string).map_err(|e| {
                    Error::Internal(format!("Invalid header value for '{}': {}", key_string, e))
                })?;

                headers.append(header_name, header_value);
            }
        }

        Ok(headers)
    }
}

/// Utility functions for handling response data
pub mod utils {
    use super::*;

    /// Check if the response indicates a successful status code
    pub fn is_success_status(status: StatusCode) -> bool {
        status.is_success()
    }

    /// Check if the response indicates a redirect
    pub fn is_redirect_status(status: StatusCode) -> bool {
        status.is_redirection()
    }

    /// Get content type from headers
    pub fn get_content_type(headers: &HeaderMap) -> Option<String> {
        headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(|s| s.to_string())
    }

    /// Get content length from headers
    pub fn get_content_length(headers: &HeaderMap) -> Option<u64> {
        headers
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|s| s.parse().ok())
    }

    /// Get content encoding from headers
    pub fn get_content_encoding(headers: &HeaderMap) -> Option<String> {
        headers
            .get("content-encoding")
            .and_then(|value| value.to_str().ok())
            .map(|s| s.to_string())
    }

    /// Check if response is compressed
    pub fn is_compressed(headers: &HeaderMap) -> bool {
        get_content_encoding(headers)
            .map(|encoding| {
                let encoding = encoding.to_lowercase();
                encoding.contains("gzip") || encoding.contains("deflate") || encoding.contains("br")
            })
            .unwrap_or(false)
    }

    /// Extract filename from Content-Disposition header
    pub fn get_filename_from_content_disposition(headers: &HeaderMap) -> Option<String> {
        headers
            .get("content-disposition")
            .and_then(|value| value.to_str().ok())
            .and_then(|disposition| {
                // Simple extraction - in practice you'd want more robust parsing
                if let Some(filename_start) = disposition.find("filename=") {
                    let filename_part = &disposition[filename_start + 9..];
                    if filename_part.starts_with('"') && filename_part.len() > 1 {
                        // Quoted filename
                        filename_part[1..].split('"').next().map(|s| s.to_string())
                    } else {
                        // Unquoted filename
                        filename_part
                            .split(';')
                            .next()
                            .map(|s| s.trim().to_string())
                    }
                } else {
                    None
                }
            })
    }
}
