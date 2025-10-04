//! Request execution using Cronet UrlRequest

use super::callback::{
    CallbackEvent, CallbackHandler, create_callback_instance, register_callback_handler,
};
use super::jni_bindings::{HttpMethod, UrlRequestBuilder};
use crate::backend::types::{BackendRequest, BackendResponse};
use crate::{Error, Result};
use bytes::Bytes;
use http::Method;
use jni::{JavaVM, objects::GlobalRef};
use tokio::sync::mpsc;

/// Execute an HTTP request using Cronet
pub async fn execute_request(
    jvm: &JavaVM,
    cronet_engine: &GlobalRef,
    request: BackendRequest,
) -> Result<BackendResponse> {
    // Create callback handler first
    let (callback_handler, mut response_rx, mut body_rx) = CallbackHandler::new();
    let handler_id = register_callback_handler(callback_handler);

    // Save the URL and timeout before moving request
    let url = request.url.clone();
    let timeout = request.timeout;

    // Build and start request - each function creates its own env
    println!("🚀 Building and starting request to: {}", url);
    let url_request_global = build_and_start_request(jvm, cronet_engine, request, handler_id)?;
    println!("🚀 Request started, waiting for response...");

    // Wait for response to start and collect all events until completion
    println!("🚀 Waiting for response events...");

    let mut status = None;
    let mut headers = None;
    let mut body_chunks = Vec::new();
    let mut redirect_headers = Vec::new();

    // Create timeout future if timeout is specified
    let timeout_future = async {
        if let Some(duration) = timeout {
            tokio::time::sleep(duration).await;
            true
        } else {
            // Never complete if no timeout
            std::future::pending::<bool>().await
        }
    };

    tokio::pin!(timeout_future);

    // Process all events until we get Succeeded, Failed, or timeout
    let mut succeeded = false;
    let mut body_channel_closed = false;

    loop {
        // Exit when we've received Succeeded AND drained all body chunks
        if succeeded && body_channel_closed {
            break;
        }

        tokio::select! {
            // Handle timeout
            _ = &mut timeout_future => {
                println!("🚀 Request timed out, cancelling...");
                // Cancel the request
                if let Ok(mut env) = jvm.attach_current_thread() {
                    let _ = env.call_method(url_request_global.as_obj(), "cancel", "()V", &[]);
                }
                return Err(Error::Timeout);
            }
            // Handle response events (status, success, failure, redirects)
            event = response_rx.recv(), if !succeeded => {
                match event {
                    Some(CallbackEvent::ResponseStarted { status: s, headers: h }) => {
                        println!("🚀 Received ResponseStarted: {}", s);
                        status = Some(s);
                        headers = Some(h);
                    }
                    Some(CallbackEvent::Redirect { headers: h }) => {
                        println!("🚀 Received Redirect with headers");
                        redirect_headers.push(h);
                    }
                    Some(CallbackEvent::Succeeded) => {
                        println!("🚀 Received Succeeded, draining body channel...");
                        succeeded = true;
                    }
                    Some(CallbackEvent::Failed { error }) => {
                        println!("🚀 Received Failed: {:?}", error);
                        return Err(error);
                    }
                    Some(_) => {
                        return Err(Error::Internal("Unexpected callback event".to_string()));
                    }
                    None => {
                        return Err(Error::Internal("Response channel closed unexpectedly".to_string()));
                    }
                }
            }
            // Handle body chunks
            chunk = body_rx.recv(), if !body_channel_closed => {
                match chunk {
                    Some(Ok(data)) => {
                        println!("🚀 Received body chunk: {} bytes", data.len());
                        body_chunks.push(data);
                    }
                    Some(Err(e)) => {
                        println!("🚀 Received body error: {:?}", e);
                        return Err(e);
                    }
                    None => {
                        println!("🚀 Body channel closed");
                        body_channel_closed = true;
                    }
                }
            }
        }
    }

    let status = status.ok_or_else(|| Error::Internal("No status received".to_string()))?;
    let headers = headers.ok_or_else(|| Error::Internal("No headers received".to_string()))?;

    // Combine all body chunks into a single Bytes
    let total_size: usize = body_chunks.iter().map(|b| b.len()).sum();
    println!(
        "🚀 Combining {} chunks ({} total bytes)",
        body_chunks.len(),
        total_size
    );

    let mut body_buffer = Vec::with_capacity(total_size);
    for chunk in body_chunks {
        body_buffer.extend_from_slice(&chunk);
    }
    let complete_body = Bytes::from(body_buffer);

    // Create a channel with the complete body
    let (body_sender, body_receiver) = mpsc::channel(1);
    let _ = body_sender.send(Ok(complete_body)).await;
    drop(body_sender); // Close the channel after sending the complete body

    println!(
        "🚀 Request complete, returning response with {} redirect header sets",
        redirect_headers.len()
    );
    Ok(BackendResponse {
        status,
        headers,
        url,
        body_receiver,
        redirect_headers,
    })
}

/// Create a Java callback object that delegates to our Rust handler
fn create_rust_callback(jvm: &JavaVM, handler_id: i64) -> Result<GlobalRef> {
    let mut env = jvm
        .attach_current_thread()
        .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

    create_callback_instance(&mut env, handler_id)
}

/// Create an UploadDataProvider for request body
fn create_upload_data_provider(jvm: &JavaVM, body: crate::body::Body) -> Result<GlobalRef> {
    let mut env = jvm
        .attach_current_thread()
        .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

    let body_bytes = match body {
        crate::body::Body::Empty => {
            return Err(Error::Internal(
                "Empty body should not need upload provider".to_string(),
            ));
        }
        crate::body::Body::Bytes { content, .. } => content.to_vec(),
        crate::body::Body::Form { fields } => {
            // URL-encode form fields
            let encoded = fields
                .iter()
                .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
                .collect::<Vec<_>>()
                .join("&");
            encoded.into_bytes()
        }
        #[cfg(feature = "json")]
        crate::body::Body::Json { value } => {
            serde_json::to_vec(&value).map_err(|e| Error::Json(e.to_string()))?
        }
        #[cfg(feature = "multipart")]
        crate::body::Body::Multipart { .. } => {
            return Err(Error::Internal(
                "Multipart data not yet implemented for Android backend".to_string(),
            ));
        }
    };

    // Create a byte array from the body data
    let byte_array = env
        .byte_array_from_slice(&body_bytes)
        .map_err(|e| Error::Internal(format!("Failed to create byte array: {}", e)))?;

    // Use UploadDataProviders.create(byte[]) to create a provider from the byte array
    let providers_class = env
        .find_class("org/chromium/net/UploadDataProviders")
        .map_err(|e| Error::Internal(format!("Failed to find UploadDataProviders class: {}", e)))?;

    let provider = env
        .call_static_method(
            providers_class,
            "create",
            "([B)Lorg/chromium/net/UploadDataProvider;",
            &[(&byte_array).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to create UploadDataProvider: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to convert UploadDataProvider: {}", e)))?;

    env.new_global_ref(&provider).map_err(|e| {
        Error::Internal(format!(
            "Failed to create global ref for upload provider: {}",
            e
        ))
    })
}

/// Build and start a Cronet request
fn build_and_start_request(
    jvm: &JavaVM,
    cronet_engine: &GlobalRef,
    request: BackendRequest,
    handler_id: i64,
) -> Result<GlobalRef> {
    let mut env = jvm
        .attach_current_thread()
        .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

    // Convert HTTP method
    let method = match request.method {
        Method::GET => HttpMethod::Get,
        Method::POST => HttpMethod::Post,
        Method::PUT => HttpMethod::Put,
        Method::DELETE => HttpMethod::Delete,
        Method::HEAD => HttpMethod::Head,
        Method::PATCH => HttpMethod::Patch,
        _ => {
            return Err(Error::Internal(format!(
                "Unsupported HTTP method: {}",
                request.method
            )));
        }
    };

    // Create callback as GlobalRef
    let callback_global = create_rust_callback(jvm, handler_id)?;

    // Build the request (scope the builder to ensure it's dropped before using env again)
    let url_request = {
        let mut env = jvm
            .attach_current_thread()
            .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

        // Create UrlRequest.Builder
        let mut builder = UrlRequestBuilder::new(
            env,
            cronet_engine,
            request.url.as_str(),
            callback_global.as_obj(),
        )
        .map_err(|e| Error::Internal(format!("Failed to create UrlRequest.Builder: {}", e)))?;

        // Set HTTP method
        builder
            .set_http_method(method)
            .map_err(|e| Error::Internal(format!("Failed to set HTTP method: {}", e)))?;

        // Determine Content-Type from body if present
        let content_type = if let Some(ref body) = request.body {
            match body {
                crate::body::Body::Bytes { content_type, .. } => Some(content_type.clone()),
                crate::body::Body::Form { .. } => {
                    Some("application/x-www-form-urlencoded".to_string())
                }
                #[cfg(feature = "json")]
                crate::body::Body::Json { .. } => Some("application/json".to_string()),
                #[cfg(feature = "multipart")]
                crate::body::Body::Multipart { .. } => {
                    // Multipart needs a boundary, but since it's not implemented yet, use a placeholder
                    Some("multipart/form-data".to_string())
                }
                crate::body::Body::Empty => None,
            }
        } else {
            None
        };

        // Add Content-Type header if we have a body and it's not already set
        if let Some(ct) = content_type {
            if !request.headers.contains_key("content-type") {
                builder.add_header("Content-Type", &ct).map_err(|e| {
                    Error::Internal(format!("Failed to add Content-Type header: {}", e))
                })?;
            }
        }

        // Add other headers
        for (name, value) in &request.headers {
            let name_str = name.as_str();
            let value_str = std::str::from_utf8(value.as_bytes())
                .map_err(|e| Error::Internal(format!("Invalid header value: {}", e)))?;

            builder.add_header(name_str, value_str).map_err(|e| {
                Error::Internal(format!("Failed to add header {}: {}", name_str, e))
            })?;
        }

        // Note: Timeout is handled in execute_request() by cancelling the request
        // Cronet doesn't have a built-in setTimeout method

        // Handle request body if present
        if let Some(body) = request.body {
            let upload_global = create_upload_data_provider(jvm, body)?;
            builder
                .set_upload_data_provider(upload_global.as_obj())
                .map_err(|e| {
                    Error::Internal(format!("Failed to set upload data provider: {}", e))
                })?;
        }

        // Build the request
        builder
            .build()
            .map_err(|e| Error::Internal(format!("Failed to build UrlRequest: {}", e)))?
    };

    // Start the request
    println!("🚀 Calling request.start()...");
    env.call_method(&url_request, "start", "()V", &[])
        .map_err(|e| Error::Internal(format!("Failed to start request: {}", e)))?;
    println!("🚀 request.start() returned successfully");

    // Return global reference
    env.new_global_ref(&url_request)
        .map_err(|e| Error::Internal(format!("Failed to create global ref for request: {}", e)))
}
