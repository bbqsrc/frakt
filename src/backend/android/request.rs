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
    let (callback_handler, mut response_rx, _body_rx) = CallbackHandler::new();
    let handler_id = register_callback_handler(callback_handler);

    // Build and start request - each function creates its own env
    let _url_request_global = build_and_start_request(jvm, cronet_engine, request, handler_id)?;

    // Wait for response to start
    let (status, headers) = match response_rx.recv().await {
        Some(CallbackEvent::ResponseStarted { status, headers }) => (status, headers),
        Some(CallbackEvent::Failed { error }) => return Err(error),
        Some(_) => return Err(Error::Internal("Unexpected callback event".to_string())),
        None => {
            return Err(Error::Internal(
                "Callback channel closed unexpectedly".to_string(),
            ));
        }
    };

    // Create response body receiver
    let (body_sender, body_receiver) = mpsc::channel(16);

    // Spawn task to handle body streaming - don't hold AttachGuard across await
    tokio::spawn(async move {
        // TODO: Implement proper ByteBuffer creation for reading response body
        // For now, just send placeholder response body
        let _ = body_sender
            .send(Ok(Bytes::from("Placeholder response body")))
            .await;
    });

    // Wait for final success/failure
    tokio::spawn(async move {
        while let Some(event) = response_rx.recv().await {
            match event {
                CallbackEvent::Succeeded => {
                    break; // Request completed successfully
                }
                CallbackEvent::Failed { error } => {
                    tracing::error!("Request failed: {}", error);
                    break;
                }
                _ => {
                    // Ignore other events (ReadCompleted is handled in the body streaming task)
                }
            }
        }
    });

    Ok(BackendResponse {
        status,
        headers,
        body_receiver,
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
        crate::body::Body::Form { .. } => {
            return Err(Error::Internal(
                "Form data not yet implemented for Android backend".to_string(),
            ));
        }
        #[cfg(feature = "json")]
        crate::body::Body::Json { .. } => {
            return Err(Error::Internal(
                "JSON data not yet implemented for Android backend".to_string(),
            ));
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

    // Create UploadDataProvider (placeholder implementation)
    let provider_class = env
        .find_class("org/chromium/net/UploadDataProvider")
        .map_err(|e| Error::Internal(format!("Failed to find UploadDataProvider class: {}", e)))?;

    // This is a placeholder - in reality you'd create a custom UploadDataProvider
    let provider = env
        .new_object(provider_class, "()V", &[])
        .map_err(|e| Error::Internal(format!("Failed to create UploadDataProvider: {}", e)))?;

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

        // Add headers
        for (name, value) in &request.headers {
            let name_str = name.as_str();
            let value_str = std::str::from_utf8(value.as_bytes())
                .map_err(|e| Error::Internal(format!("Invalid header value: {}", e)))?;

            builder.add_header(name_str, value_str).map_err(|e| {
                Error::Internal(format!("Failed to add header {}: {}", name_str, e))
            })?;
        }

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
    env.call_method(&url_request, "start", "()V", &[])
        .map_err(|e| Error::Internal(format!("Failed to start request: {}", e)))?;

    // Return global reference
    env.new_global_ref(&url_request)
        .map_err(|e| Error::Internal(format!("Failed to create global ref for request: {}", e)))
}
