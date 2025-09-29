//! WinHTTP client utilities for Windows backend

use crate::backend::types::BackendResponse;
use crate::{Error, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use windows::{
    Win32::Foundation::{GetLastError, VARIANT_TRUE},
    Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS,
        WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE, WINHTTP_CALLBACK_STATUS_REDIRECT,
        WINHTTP_CALLBACK_STATUS_REQUEST_SENT, WINHTTP_CALLBACK_STATUS_RESPONSE_RECEIVED,
        WINHTTP_CALLBACK_STATUS_SENDING_REQUEST, WINHTTP_CALLBACK_STATUS_WRITE_COMPLETE,
        WINHTTP_DISABLE_COOKIES, WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE,
        WINHTTP_OPEN_REQUEST_FLAGS, WINHTTP_OPTION_CONNECT_TIMEOUT, WINHTTP_OPTION_CONTEXT_VALUE,
        WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_OPTION_RESOLVE_TIMEOUT,
        WINHTTP_OPTION_SEND_TIMEOUT, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_RAW_HEADERS_CRLF,
        WINHTTP_QUERY_SET_COOKIE, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
        WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders,
        WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
        WinHttpSetStatusCallback, WinHttpSetTimeouts,
    },
    core::{HSTRING, PCWSTR},
};

/// Context data passed to WinHTTP callback for cookie handling and progress tracking
#[repr(C)]
struct CallbackContext {
    cookie_storage: Option<super::cookies::WindowsCookieStorage>,
    request_url: url::Url,
    progress_callback: Option<Arc<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
    total_body_size: u64,
    bytes_sent: Arc<std::sync::atomic::AtomicU64>,
}

/// WinHTTP callback function to handle redirects, capture Set-Cookie headers, and track upload progress
unsafe extern "system" fn winhttp_callback(
    h_internet: *mut std::ffi::c_void,
    dw_context: usize,
    dw_internet_status: u32,
    lpv_status_information: *mut std::ffi::c_void,
    dw_status_information_length: u32,
) {
    eprintln!(
        "CALLBACK: Status: {} (redirect = {}, headers = {}, sending = {}, sent = {}, write_complete = {})",
        dw_internet_status,
        WINHTTP_CALLBACK_STATUS_REDIRECT,
        WINHTTP_CALLBACK_STATUS_HEADERS_AVAILABLE,
        WINHTTP_CALLBACK_STATUS_SENDING_REQUEST,
        WINHTTP_CALLBACK_STATUS_REQUEST_SENT,
        WINHTTP_CALLBACK_STATUS_WRITE_COMPLETE
    );

    // Get context data once for all handlers
    if dw_context != 0 {
        eprintln!("CALLBACK: Context pointer: 0x{:x}", dw_context);
        let context = unsafe { &*(dw_context as *const CallbackContext) };
        eprintln!(
            "CALLBACK: Context dereferenced, progress_callback: {:?}, body_size: {}",
            context.progress_callback.is_some(),
            context.total_body_size
        );

        // Handle upload progress events
        if dw_internet_status == WINHTTP_CALLBACK_STATUS_SENDING_REQUEST {
            eprintln!(
                "CALLBACK: Starting to send request, progress_callback: {:?}",
                context.progress_callback.is_some()
            );
            if let Some(ref progress_callback) = context.progress_callback {
                eprintln!(
                    "CALLBACK: Calling progress callback with (0, {})",
                    context.total_body_size
                );
                progress_callback(0, Some(context.total_body_size));
                eprintln!("CALLBACK: Progress callback called successfully");
            }
        } else if dw_internet_status == WINHTTP_CALLBACK_STATUS_REQUEST_SENT {
            eprintln!(
                "CALLBACK: Request headers sent, progress_callback: {:?}",
                context.progress_callback.is_some()
            );
            if let Some(ref progress_callback) = context.progress_callback {
                eprintln!(
                    "CALLBACK: Calling progress callback with (0, {})",
                    context.total_body_size
                );
                // Request headers sent, but body data might still be sending
                progress_callback(0, Some(context.total_body_size));
                eprintln!("CALLBACK: Progress callback called successfully");
            }
        } else if dw_internet_status == WINHTTP_CALLBACK_STATUS_WRITE_COMPLETE {
            eprintln!(
                "CALLBACK: Write complete, bytes sent: {}, progress_callback: {:?}",
                dw_status_information_length,
                context.progress_callback.is_some()
            );
            // Track cumulative bytes sent
            let bytes_sent = context.bytes_sent.fetch_add(
                dw_status_information_length as u64,
                std::sync::atomic::Ordering::Relaxed,
            ) + dw_status_information_length as u64;
            if let Some(ref progress_callback) = context.progress_callback {
                eprintln!(
                    "CALLBACK: Calling progress callback with ({}, {})",
                    bytes_sent, context.total_body_size
                );
                progress_callback(bytes_sent, Some(context.total_body_size));
                eprintln!("CALLBACK: Progress callback called successfully");
            }
        } else if dw_internet_status == WINHTTP_CALLBACK_STATUS_RESPONSE_RECEIVED {
            eprintln!(
                "CALLBACK: Response received, upload complete, progress_callback: {:?}",
                context.progress_callback.is_some()
            );
            if let Some(ref progress_callback) = context.progress_callback {
                eprintln!(
                    "CALLBACK: Calling progress callback with ({}, {})",
                    context.total_body_size, context.total_body_size
                );
                // Upload complete
                progress_callback(context.total_body_size, Some(context.total_body_size));
                eprintln!("CALLBACK: Progress callback called successfully");
            }
        }
    }

    // Handle both redirect and headers available callbacks (status 131072)
    if true {
        eprintln!(
            "CALLBACK: Headers/Redirect callback triggered for status: {}",
            dw_internet_status
        );
        // Get context data
        if dw_context != 0 {
            eprintln!("CALLBACK: Context available, processing...");
            unsafe {
                let context = &*(dw_context as *const CallbackContext);
                eprintln!("CALLBACK: Context dereferenced successfully");

                if let Some(ref cookie_storage) = context.cookie_storage {
                    eprintln!("CALLBACK: Cookie storage available, querying headers...");
                    // Query Set-Cookie headers from the redirect response
                    let mut header_buffer_size: u32 = 0;

                    // First call to get buffer size for Set-Cookie headers
                    // let result = WinHttpQueryHeaders(
                    //     h_internet,
                    //     WINHTTP_QUERY_SET_COOKIE,
                    //     PCWSTR::null(),
                    //     None,
                    //     &mut header_buffer_size,
                    //     std::ptr::null_mut(),
                    // );

                    // eprintln!(
                    //     "CALLBACK: Query Set-Cookie headers result: {:?}, size: {}",
                    //     result, header_buffer_size
                    // );
                    // eprintln!(
                    //     "CALLBACK: Error code if failed: {:?}",
                    //     if result.is_err() {
                    //         Some(GetLastError())
                    //     } else {
                    //         None
                    //     }
                    // );

                    // Also try to get all headers to see what's available
                    let mut all_header_buffer_size: u32 = 0;
                    let _ = WinHttpQueryHeaders(
                        h_internet,
                        WINHTTP_QUERY_RAW_HEADERS_CRLF,
                        PCWSTR::null(),
                        None,
                        &mut all_header_buffer_size,
                        std::ptr::null_mut(),
                    );

                    if all_header_buffer_size > 0 {
                        let mut all_header_buffer =
                            vec![0u16; (all_header_buffer_size / 2) as usize];
                        let result = WinHttpQueryHeaders(
                            h_internet,
                            WINHTTP_QUERY_RAW_HEADERS_CRLF,
                            PCWSTR::null(),
                            Some(all_header_buffer.as_mut_ptr() as *mut _),
                            &mut all_header_buffer_size,
                            std::ptr::null_mut(),
                        );

                        if result.is_ok() {
                            let all_headers = String::from_utf16_lossy(&all_header_buffer);
                            eprintln!("CALLBACK: All headers during redirect: {}", all_headers);
                        }
                    } else {
                        eprintln!("CALLBACK: No headers available during redirect");
                    }
                }
            }
        }
    }
}

/// Execute an HTTP request using WinHTTP
pub async fn execute_winhttp_request(
    request: crate::backend::types::BackendRequest,
    user_agent: &str,
    default_headers: &Option<http::HeaderMap>,
    timeout: &Option<Duration>,
    cookie_storage: &Option<super::cookies::WindowsCookieStorage>,
) -> Result<BackendResponse> {
    let url = request.url;
    let method = request.method;
    let headers = request.headers;
    let body = request.body;
    let progress_callback = request.progress_callback;

    // Parse URL components and clone data for the blocking task
    let scheme = url.scheme().to_string();
    let host = url.host_str().ok_or(Error::InvalidUrl)?.to_string();
    let port = url
        .port()
        .unwrap_or(if scheme == "https" { 443 } else { 80 });
    let path = url.path().to_string();
    let query = url.query().map(|q| format!("?{}", q)).unwrap_or_default();
    let full_path = format!("{}{}", path, query);
    let user_agent = user_agent.to_string();
    let default_headers = default_headers.clone();
    let timeout = timeout.clone();
    let cookie_storage = cookie_storage.clone();
    let request_url = url.clone();

    tokio::task::spawn_blocking(move || {
        unsafe {
            // Open WinHTTP session
            let user_agent_wide = HSTRING::from(user_agent);
            let session = WinHttpOpen(
                &user_agent_wide,
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            );

            if session.is_null() {
                return Err(Error::Internal(format!(
                    "Failed to open WinHTTP session: {}",
                    GetLastError().0
                )));
            }

            // Set timeout if configured
            if let Some(timeout_duration) = timeout {
                let timeout_ms = timeout_duration.as_millis() as i32;
                WinHttpSetTimeouts(
                    session, timeout_ms, // Resolve timeout
                    timeout_ms, // Connect timeout
                    timeout_ms, // Send timeout
                    timeout_ms, // Receive timeout
                )
                .map_err(|e| Error::Internal(format!("Failed to set timeouts: {}", e)))?;
            }

            // Connect to server
            let host_wide = HSTRING::from(&host);
            let connection = WinHttpConnect(session, &host_wide, port, 0);

            if connection.is_null() {
                let _ = WinHttpCloseHandle(session);
                return Err(Error::Internal(format!(
                    "Failed to connect to server: {}",
                    GetLastError().0
                )));
            }

            // Open request
            let method_wide = HSTRING::from(method.as_str());
            let path_wide = HSTRING::from(&full_path);
            let mut flags = WINHTTP_OPEN_REQUEST_FLAGS(0);
            if scheme == "https" {
                flags = WINHTTP_FLAG_SECURE;
            }

            let request_handle = WinHttpOpenRequest(
                connection,
                &method_wide,
                &path_wide,
                PCWSTR::null(),   // HTTP version (default)
                PCWSTR::null(),   // Referrer
                std::ptr::null(), // Accept types
                flags,
            );

            if request_handle.is_null() {
                let _ = WinHttpCloseHandle(connection);
                let _ = WinHttpCloseHandle(session);
                return Err(Error::Internal(format!(
                    "Failed to open request: {}",
                    GetLastError().0
                )));
            }

            // Add headers
            let mut header_string = String::new();

            // Add default headers
            if let Some(ref default_headers) = default_headers {
                for (name, value) in default_headers {
                    header_string.push_str(&format!(
                        "{}: {}\r\n",
                        name.as_str(),
                        value.to_str().unwrap_or("")
                    ));
                }
            }

            // Add request headers
            for (name, value) in &headers {
                header_string.push_str(&format!(
                    "{}: {}\r\n",
                    name.as_str(),
                    value.to_str().unwrap_or("")
                ));
            }

            // Add cookies if available
            if let Some(cookie_storage) = &cookie_storage {
                if let Ok(cookie_headers) = cookie_storage.get_cookies_for_url(&request_url) {
                    for (name, value) in &cookie_headers {
                        header_string.push_str(&format!(
                            "{}: {}\r\n",
                            name.as_str(),
                            value.to_str().unwrap_or("")
                        ));
                    }
                }
            }

            // Prepare body data first
            let body_bytes = if let Some(body_data) = body {
                match body_data {
                    crate::body::Body::Empty => Vec::new(),
                    crate::body::Body::Bytes { content, .. } => content.to_vec(),
                    crate::body::Body::Form { fields } => {
                        // Add Content-Type header for form data
                        header_string
                            .push_str("Content-Type: application/x-www-form-urlencoded\r\n");

                        let encoded = fields
                            .iter()
                            .map(|(k, v)| {
                                format!("{}={}", urlencoding::encode(k), urlencoding::encode(v))
                            })
                            .collect::<Vec<_>>()
                            .join("&");
                        encoded.into_bytes()
                    }
                    #[cfg(feature = "json")]
                    crate::body::Body::Json { value } => {
                        // Add Content-Type header for JSON
                        header_string.push_str("Content-Type: application/json\r\n");

                        serde_json::to_string(&value)
                            .map_err(|e| {
                                Error::Internal(format!("Failed to serialize JSON: {}", e))
                            })?
                            .into_bytes()
                    }
                    #[cfg(feature = "multipart")]
                    crate::body::Body::Multipart { .. } => {
                        return Err(Error::Internal(
                            "Multipart not yet supported with WinHTTP".to_string(),
                        ));
                    }
                }
            } else {
                Vec::new()
            };

            // Set up callback for capturing cookies and tracking progress
            eprintln!(
                "SETUP: Cookie storage: {:?}, Progress callback: {:?}, Body size: {}",
                cookie_storage.is_some(),
                progress_callback.is_some(),
                body_bytes.len()
            );

            // Always use all notifications - this includes progress events
            let callback_flags = WINHTTP_CALLBACK_FLAG_ALL_NOTIFICATIONS;

            let _ =
                WinHttpSetStatusCallback(request_handle, Some(winhttp_callback), callback_flags, 0);

            let callback_context = Arc::new(CallbackContext {
                cookie_storage: cookie_storage.clone(),
                request_url: request_url.clone(),
                progress_callback: progress_callback.clone(),
                total_body_size: body_bytes.len() as u64,
                bytes_sent: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            });
            let callback_ctx_ptr = Arc::into_raw(callback_context);

            if let Some(timeout_duration) = timeout {
                WinHttpSetOption(
                    Some(request_handle),
                    WINHTTP_OPTION_RESOLVE_TIMEOUT,
                    Some(&(timeout_duration.as_millis() as u64).to_ne_bytes()),
                )
                .map_err(|e| {
                    let _ = WinHttpCloseHandle(request_handle);
                    let _ = WinHttpCloseHandle(connection);
                    let _ = WinHttpCloseHandle(session);
                    Error::Internal(format!("Failed to disable features: {}", e))
                })?;
                WinHttpSetOption(
                    Some(request_handle),
                    WINHTTP_OPTION_CONNECT_TIMEOUT,
                    Some(&(timeout_duration.as_millis() as u64).to_ne_bytes()),
                )
                .map_err(|e| {
                    let _ = WinHttpCloseHandle(request_handle);
                    let _ = WinHttpCloseHandle(connection);
                    let _ = WinHttpCloseHandle(session);
                    Error::Internal(format!("Failed to disable features: {}", e))
                })?;
                WinHttpSetOption(
                    Some(request_handle),
                    WINHTTP_OPTION_SEND_TIMEOUT,
                    Some(&(timeout_duration.as_millis() as u64).to_ne_bytes()),
                )
                .map_err(|e| {
                    let _ = WinHttpCloseHandle(request_handle);
                    let _ = WinHttpCloseHandle(connection);
                    let _ = WinHttpCloseHandle(session);
                    Error::Internal(format!("Failed to disable features: {}", e))
                })?;
            }
            WinHttpSetOption(
                Some(request_handle),
                WINHTTP_OPTION_DISABLE_FEATURE,
                Some(&WINHTTP_DISABLE_COOKIES.to_ne_bytes()),
            )
            .map_err(|e| {
                let _ = WinHttpCloseHandle(request_handle);
                let _ = WinHttpCloseHandle(connection);
                let _ = WinHttpCloseHandle(session);
                Error::Internal(format!("Failed to disable features: {}", e))
            })?;

            WinHttpSetOption(
                Some(request_handle),
                WINHTTP_OPTION_DISABLE_FEATURE,
                Some(&WINHTTP_DISABLE_REDIRECTS.to_ne_bytes()),
            )
            .map_err(|e| {
                let _ = WinHttpCloseHandle(request_handle);
                let _ = WinHttpCloseHandle(connection);
                let _ = WinHttpCloseHandle(session);
                Error::Internal(format!("Failed to disable features: {}", e))
            })?;

            WinHttpSetOption(
                Some(request_handle),
                WINHTTP_OPTION_CONTEXT_VALUE,
                Some(&(callback_ctx_ptr as usize).to_ne_bytes()),
            )
            .map_err(|e| {
                let _ = WinHttpCloseHandle(request_handle);
                let _ = WinHttpCloseHandle(connection);
                let _ = WinHttpCloseHandle(session);
                Error::Internal(format!("Failed to set callback context: {}", e))
            })?;

            // Send request with headers and body in a single call
            let headers_wide = if !header_string.is_empty() {
                Some(
                    header_string
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect::<Vec<u16>>(),
                )
            } else {
                None
            };

            let (headers_ptr, body_ptr, body_len, total_len) =
                if let Some(ref headers) = headers_wide {
                    let headers_slice = &headers[..headers.len() - 1]; // Exclude null terminator

                    if !body_bytes.is_empty() {
                        (
                            Some(headers_slice),
                            Some(body_bytes.as_ptr() as *const _),
                            body_bytes.len() as u32,
                            body_bytes.len() as u32,
                        )
                    } else {
                        (Some(headers_slice), None, 0, 0)
                    }
                } else {
                    if !body_bytes.is_empty() {
                        (
                            None,
                            Some(body_bytes.as_ptr() as *const _),
                            body_bytes.len() as u32,
                            body_bytes.len() as u32,
                        )
                    } else {
                        (None, None, 0, 0)
                    }
                };

            WinHttpSendRequest(
                request_handle,
                headers_ptr,
                body_ptr,
                body_len,
                total_len,
                0,
            )
            .map_err(|e| Error::Internal(format!("Failed to send request: {}", e)))?;

            // Receive response
            WinHttpReceiveResponse(request_handle, std::ptr::null_mut()).map_err(|e| {
                let _ = WinHttpCloseHandle(request_handle);
                let _ = WinHttpCloseHandle(connection);
                let _ = WinHttpCloseHandle(session);
                Error::Internal(format!("Failed to receive response: {}", e))
            })?;

            // Get status code
            let mut status_code: u32 = 0;
            let mut status_code_size = std::mem::size_of::<u32>() as u32;
            WinHttpQueryHeaders(
                request_handle,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut status_code as *mut _ as *mut _),
                &mut status_code_size,
                std::ptr::null_mut(),
            )
            .unwrap_or(());

            let status = http::StatusCode::from_u16(status_code as u16)
                .unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);

            // Get headers
            let mut headers = http::HeaderMap::new();
            let mut header_buffer_size: u32 = 0;

            // First call to get buffer size
            let _ = WinHttpQueryHeaders(
                request_handle,
                WINHTTP_QUERY_RAW_HEADERS_CRLF,
                PCWSTR::null(),
                None,
                &mut header_buffer_size,
                std::ptr::null_mut(),
            );

            if header_buffer_size > 0 {
                let mut header_buffer = vec![0u16; (header_buffer_size / 2) as usize];
                let result = WinHttpQueryHeaders(
                    request_handle,
                    WINHTTP_QUERY_RAW_HEADERS_CRLF,
                    PCWSTR::null(),
                    Some(header_buffer.as_mut_ptr() as *mut _),
                    &mut header_buffer_size,
                    std::ptr::null_mut(),
                );

                if result.is_ok() {
                    let header_string = String::from_utf16_lossy(&header_buffer);
                    eprintln!("MAIN RESPONSE: Got headers: {}", header_string);
                    parse_headers_into_map(&header_string, &mut headers);

                    // Store cookies if we have cookie storage
                    if let Some(cookie_storage) = &cookie_storage {
                        eprintln!("MAIN RESPONSE: Processing cookies from headers");
                        store_cookies_from_headers(&header_string, &request_url, cookie_storage);
                    }
                }
            }

            // Read all response data at once (simplified approach for now)
            let mut response_data = Vec::new();
            let mut buffer = [0u8; 8192];

            loop {
                let mut bytes_available: u32 = 0;
                let result = WinHttpQueryDataAvailable(request_handle, &mut bytes_available);

                if result.is_err() || bytes_available == 0 {
                    break;
                }

                let bytes_to_read = std::cmp::min(bytes_available as usize, buffer.len());
                let mut bytes_read: u32 = 0;

                let result = WinHttpReadData(
                    request_handle,
                    buffer.as_mut_ptr() as *mut _,
                    bytes_to_read as u32,
                    &mut bytes_read,
                );

                if result.is_err() || bytes_read == 0 {
                    break;
                }

                response_data.extend_from_slice(&buffer[..bytes_read as usize]);
            }

            drop(Arc::from_raw(callback_ctx_ptr));

            // Clean up handles
            let _ = WinHttpCloseHandle(request_handle);
            let _ = WinHttpCloseHandle(connection);
            let _ = WinHttpCloseHandle(session);

            // Create channel and send all data at once
            let (tx, rx) = mpsc::channel(1);
            if !response_data.is_empty() {
                let _ = tx.try_send(Ok(bytes::Bytes::from(response_data)));
            }

            Ok(BackendResponse {
                status,
                headers,
                body_receiver: rx,
            })
        }
    })
    .await
    .map_err(|e| Error::Internal(format!("WinHTTP task failed: {}", e)))?
}

/// Parse raw HTTP headers into HeaderMap
fn parse_headers_into_map(header_string: &str, headers: &mut http::HeaderMap) {
    for line in header_string.lines() {
        if let Some(colon_pos) = line.find(':') {
            let name = line[..colon_pos].trim();
            let value = line[colon_pos + 1..].trim();

            if let (Ok(header_name), Ok(header_value)) = (
                http::HeaderName::from_bytes(name.as_bytes()),
                http::HeaderValue::from_str(value),
            ) {
                headers.insert(header_name, header_value);
            }
        }
    }
}

/// Store cookies from response headers
fn store_cookies_from_headers(
    header_string: &str,
    url: &url::Url,
    cookie_storage: &super::cookies::WindowsCookieStorage,
) {
    // Convert the header string into an http::HeaderMap to use with cookie_store
    let mut headers = http::HeaderMap::new();

    for line in header_string.lines() {
        if let Some(colon_pos) = line.find(':') {
            let name = line[..colon_pos].trim();
            let value = line[colon_pos + 1..].trim();

            if name.eq_ignore_ascii_case("set-cookie") {
                eprintln!("STORE COOKIES: Found Set-Cookie header: {}", value);
                // Add each Set-Cookie header to the HeaderMap
                if let Ok(header_value) = http::HeaderValue::from_str(value) {
                    let header_name = http::HeaderName::from_static("set-cookie");
                    headers.append(header_name, header_value);
                }
            }
        }
    }

    // Process all Set-Cookie headers using the new cookie_store integration
    let _ = cookie_storage.process_response_headers(url, &headers);
}
