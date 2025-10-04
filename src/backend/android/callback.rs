//! UrlRequest.Callback implementation for bridging Cronet to Rust async

use crate::{Error, Result};
use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use jni::sys::jlong;
use jni::{
    JNIEnv,
    objects::{GlobalRef, JClass, JObject},
};
use tokio::sync::mpsc;

/// Rust-side callback handler that receives Cronet callbacks
pub struct CallbackHandler {
    response_sender: Option<mpsc::UnboundedSender<CallbackEvent>>,
    body_sender: Option<mpsc::UnboundedSender<Result<Bytes>>>,
}

/// Events from Cronet callbacks
#[derive(Debug)]
pub enum CallbackEvent {
    ResponseStarted {
        status: StatusCode,
        headers: HeaderMap,
    },
    Redirect {
        headers: HeaderMap,
    },
    ReadCompleted {
        data: Bytes,
    },
    Succeeded,
    Failed {
        error: Error,
    },
}

impl CallbackHandler {
    /// Create a new callback handler
    pub fn new() -> (
        Self,
        mpsc::UnboundedReceiver<CallbackEvent>,
        mpsc::UnboundedReceiver<Result<Bytes>>,
    ) {
        let (response_tx, response_rx) = mpsc::unbounded_channel();
        let (body_tx, body_rx) = mpsc::unbounded_channel();

        let handler = Self {
            response_sender: Some(response_tx),
            body_sender: Some(body_tx),
        };

        (handler, response_rx, body_rx)
    }

    /// Handle onResponseStarted callback
    pub fn on_response_started(&mut self, env: &mut JNIEnv, response_info: &JObject) -> Result<()> {
        println!("📡 onResponseStarted called");

        // Extract status code
        let status_code = env
            .call_method(response_info, "getHttpStatusCode", "()I", &[])
            .map_err(|e| Error::Internal(format!("Failed to get status code: {}", e)))?
            .i()
            .map_err(|e| Error::Internal(format!("Failed to convert status code: {}", e)))?;

        println!("📡 Status code: {}", status_code);

        let status = StatusCode::from_u16(status_code as u16)
            .map_err(|e| Error::Internal(format!("Invalid status code {}: {}", status_code, e)))?;

        // Extract headers from getAllHeaders()
        let headers = self.extract_headers(env, response_info)?;
        println!("📡 Extracted {} headers", headers.len());

        if let Some(sender) = &self.response_sender {
            println!("📡 Sending ResponseStarted event");
            let _ = sender.send(CallbackEvent::ResponseStarted { status, headers });
        }

        Ok(())
    }

    /// Handle onReadCompleted callback
    pub fn on_read_completed(&mut self, env: &mut JNIEnv, byte_buffer: &JObject) -> Result<()> {
        println!("📡 onReadCompleted called");

        // Extract data from ByteBuffer
        let data = self.extract_byte_buffer_data(env, byte_buffer)?;
        println!("📡 Read {} bytes", data.len());

        if let Some(sender) = &self.body_sender {
            println!("📡 Sending {} bytes to body channel", data.len());
            let _ = sender.send(Ok(data));
            println!("📡 Sent to body channel");
        } else {
            println!("📡 WARNING: body_sender is None, cannot send data!");
        }

        Ok(())
    }

    /// Handle onSucceeded callback
    pub fn on_succeeded(&mut self) {
        println!("📡 onSucceeded called");

        if let Some(sender) = &self.response_sender {
            let _ = sender.send(CallbackEvent::Succeeded);
        }

        // Close the body channel
        self.body_sender = None;
    }

    /// Handle onFailed callback
    pub fn on_failed(&mut self, error: Error) {
        println!("📡 onFailed called: {:?}", error);
        // Send error to response_sender if it still exists (early failure before response started)
        // Otherwise send to body_sender (failure during body streaming)
        // We can only send to one since Error is no longer Clone (due to HttpError containing Response)
        if let Some(sender) = self.response_sender.take() {
            let _ = sender.send(CallbackEvent::Failed { error });
        } else if let Some(sender) = self.body_sender.take() {
            let _ = sender.send(Err(error));
        }

        // Close remaining channels
        self.response_sender = None;
        self.body_sender = None;
    }

    /// Extract headers from UrlResponseInfo.getAllHeaders()
    fn extract_headers(&self, env: &mut JNIEnv, response_info: &JObject) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();

        // Call getAllHeaders() which returns Map<String, List<String>>
        let headers_map = env
            .call_method(response_info, "getAllHeaders", "()Ljava/util/Map;", &[])
            .map_err(|e| Error::Internal(format!("Failed to get headers map: {}", e)))?
            .l()
            .map_err(|e| Error::Internal(format!("Failed to convert headers map: {}", e)))?;

        // Get entrySet() to iterate over the map
        let entry_set = env
            .call_method(&headers_map, "entrySet", "()Ljava/util/Set;", &[])
            .map_err(|e| Error::Internal(format!("Failed to get entry set: {}", e)))?
            .l()
            .map_err(|e| Error::Internal(format!("Failed to convert entry set: {}", e)))?;

        // Get iterator
        let iterator = env
            .call_method(&entry_set, "iterator", "()Ljava/util/Iterator;", &[])
            .map_err(|e| Error::Internal(format!("Failed to get iterator: {}", e)))?
            .l()
            .map_err(|e| Error::Internal(format!("Failed to convert iterator: {}", e)))?;

        // Iterate over entries
        loop {
            let has_next = env
                .call_method(&iterator, "hasNext", "()Z", &[])
                .map_err(|e| Error::Internal(format!("Failed to call hasNext: {}", e)))?
                .z()
                .map_err(|e| Error::Internal(format!("Failed to convert hasNext: {}", e)))?;

            if !has_next {
                break;
            }

            let entry = env
                .call_method(&iterator, "next", "()Ljava/lang/Object;", &[])
                .map_err(|e| Error::Internal(format!("Failed to get next entry: {}", e)))?
                .l()
                .map_err(|e| Error::Internal(format!("Failed to convert entry: {}", e)))?;

            // Get key (header name)
            let key_obj = env
                .call_method(&entry, "getKey", "()Ljava/lang/Object;", &[])
                .map_err(|e| Error::Internal(format!("Failed to get key: {}", e)))?
                .l()
                .map_err(|e| Error::Internal(format!("Failed to convert key: {}", e)))?;

            let key_jstring = unsafe { jni::objects::JString::from_raw(key_obj.as_raw()) };
            let key: String = env
                .get_string(&key_jstring)
                .map_err(|e| Error::Internal(format!("Failed to get key string: {}", e)))?
                .into();

            // Get value (List<String>)
            let value_list = env
                .call_method(&entry, "getValue", "()Ljava/lang/Object;", &[])
                .map_err(|e| Error::Internal(format!("Failed to get value: {}", e)))?
                .l()
                .map_err(|e| Error::Internal(format!("Failed to convert value: {}", e)))?;

            // Convert List to array to iterate
            let value_array = env
                .call_method(&value_list, "toArray", "()[Ljava/lang/Object;", &[])
                .map_err(|e| Error::Internal(format!("Failed to convert list to array: {}", e)))?
                .l()
                .map_err(|e| Error::Internal(format!("Failed to get array: {}", e)))?;

            let value_jarray =
                unsafe { jni::objects::JObjectArray::from_raw(value_array.as_raw()) };
            let array_len = env
                .get_array_length(&value_jarray)
                .map_err(|e| Error::Internal(format!("Failed to get array length: {}", e)))?;

            // HTTP allows multiple values for the same header
            for i in 0..array_len {
                let value_obj = env
                    .get_object_array_element(&value_jarray, i)
                    .map_err(|e| Error::Internal(format!("Failed to get array element: {}", e)))?;

                let value_jstring = unsafe { jni::objects::JString::from_raw(value_obj.as_raw()) };
                let value: String = env
                    .get_string(&value_jstring)
                    .map_err(|e| Error::Internal(format!("Failed to get value string: {}", e)))?
                    .into();

                // Insert into HeaderMap
                if let Ok(header_name) = http::header::HeaderName::from_bytes(key.as_bytes()) {
                    if let Ok(header_value) = http::header::HeaderValue::from_str(&value) {
                        headers.append(header_name, header_value);
                    }
                }
            }
        }

        Ok(headers)
    }

    /// Extract data from a Java ByteBuffer
    fn extract_byte_buffer_data(&self, env: &mut JNIEnv, byte_buffer: &JObject) -> Result<Bytes> {
        // IMPORTANT: When Cronet writes to the ByteBuffer, it advances the position.
        // The data is from position 0 to current position.
        // We need to flip the buffer to prepare it for reading.
        env.call_method(byte_buffer, "flip", "()Ljava/nio/Buffer;", &[])
            .map_err(|e| Error::Internal(format!("Failed to flip ByteBuffer: {}", e)))?;

        // Get the position and limit of the ByteBuffer AFTER flipping
        let position = env
            .call_method(byte_buffer, "position", "()I", &[])
            .map_err(|e| Error::Internal(format!("Failed to get ByteBuffer position: {}", e)))?
            .i()
            .map_err(|e| Error::Internal(format!("Failed to convert position: {}", e)))?
            as usize;

        let limit = env
            .call_method(byte_buffer, "limit", "()I", &[])
            .map_err(|e| Error::Internal(format!("Failed to get ByteBuffer limit: {}", e)))?
            .i()
            .map_err(|e| Error::Internal(format!("Failed to convert limit: {}", e)))?
            as usize;

        let length = limit - position;

        if length == 0 {
            return Ok(Bytes::new());
        }

        // Create a byte array to hold the data
        let byte_array = env
            .new_byte_array(length as i32)
            .map_err(|e| Error::Internal(format!("Failed to create byte array: {}", e)))?;

        // Copy data from ByteBuffer to byte array
        // This is a bit complex in JNI - we need to get the buffer's array or use direct buffer access

        // For now, let's use a simpler approach with array() method if available
        let has_array = env
            .call_method(byte_buffer, "hasArray", "()Z", &[])
            .map_err(|e| {
                Error::Internal(format!("Failed to check if ByteBuffer has array: {}", e))
            })?
            .z()
            .map_err(|e| Error::Internal(format!("Failed to convert hasArray result: {}", e)))?;

        // Use ByteBuffer.get(byte[]) to read data into our array
        // This handles both direct and array-backed buffers
        env.call_method(
            byte_buffer,
            "get",
            "([B)Ljava/nio/ByteBuffer;",
            &[(&byte_array).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to get bytes from ByteBuffer: {}", e)))?;

        // Copy data from JNI array into our buffer
        let mut buffer = vec![0i8; length];
        env.get_byte_array_region(&byte_array, 0, &mut buffer)
            .map_err(|e| Error::Internal(format!("Failed to get byte array region: {}", e)))?;

        // Convert i8 to u8 and return
        let u8_vec: Vec<u8> = buffer.iter().map(|&b| b as u8).collect();
        Ok(Bytes::from(u8_vec))
    }
}

use once_cell::sync::OnceCell;
use std::sync::LazyLock;

/// Global storage for callback handlers
/// In practice, you'd want a more sophisticated system for managing these
static CALLBACK_HANDLERS: LazyLock<
    std::sync::Mutex<std::collections::HashMap<jlong, Box<CallbackHandler>>>,
> = LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
static CALLBACK_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

/// Global storage for the loaded callback class
static CALLBACK_CLASS: OnceCell<GlobalRef> = OnceCell::new();

/// Register a callback handler and return its ID
pub fn register_callback_handler(handler: CallbackHandler) -> jlong {
    let id = CALLBACK_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut handlers) = CALLBACK_HANDLERS.lock() {
        handlers.insert(id, Box::new(handler));
    }
    id
}

/// Re-insert a callback handler with the same ID
pub fn reinsert_callback_handler(id: jlong, handler: CallbackHandler) {
    if let Ok(mut handlers) = CALLBACK_HANDLERS.lock() {
        handlers.insert(id, Box::new(handler));
    }
}

/// Get a callback handler by ID
pub fn get_callback_handler(id: jlong) -> Option<Box<CallbackHandler>> {
    if let Ok(mut handlers) = CALLBACK_HANDLERS.lock() {
        handlers.remove(&id)
    } else {
        None
    }
}

/// Remove a callback handler
pub fn unregister_callback_handler(id: jlong) {
    if let Ok(mut handlers) = CALLBACK_HANDLERS.lock() {
        handlers.remove(&id);
    }
}

// JNI callback functions that will be called from Java
// These need to be exported and match the expected signatures for se.brendan.frakt.RustUrlRequestCallback

// NetworkException error codes from Android Cronet API
const ERROR_HOSTNAME_NOT_RESOLVED: i32 = 6;
const ERROR_INTERNET_DISCONNECTED: i32 = 7;
const ERROR_NETWORK_CHANGED: i32 = 8;
const ERROR_TIMED_OUT: i32 = 9;
const ERROR_CONNECTION_CLOSED: i32 = 10;
const ERROR_CONNECTION_TIMED_OUT: i32 = 11;
const ERROR_CONNECTION_REFUSED: i32 = 12;
const ERROR_CONNECTION_RESET: i32 = 13;
const ERROR_ADDRESS_UNREACHABLE: i32 = 14;
const ERROR_QUIC_PROTOCOL_FAILED: i32 = 15;
const ERROR_OTHER: i32 = 16;

/// Extract error information from CronetException
fn extract_cronet_error(env: &mut JNIEnv, error: &JObject) -> Error {
    // Get error message from getMessage()
    let message = match env.call_method(error, "getMessage", "()Ljava/lang/String;", &[]) {
        Ok(result) => match result.l() {
            Ok(jstring) => {
                let java_str = unsafe { jni::objects::JString::from_raw(jstring.as_raw()) };
                match env.get_string(&java_str) {
                    Ok(s) => s.into(),
                    Err(_) => "Unknown error".to_string(),
                }
            }
            Err(_) => "Unknown error".to_string(),
        },
        Err(_) => "Unknown error".to_string(),
    };

    // Check if it's a NetworkException to get error code
    let network_exception_class = match env.find_class("org/chromium/net/NetworkException") {
        Ok(cls) => cls,
        Err(e) => {
            tracing::warn!("Failed to find NetworkException class: {}", e);
            return Error::Network { code: -1, message };
        }
    };

    let is_network_exception = match env.is_instance_of(error, &network_exception_class) {
        Ok(result) => result,
        Err(e) => {
            tracing::warn!("Failed to check instance type: {}", e);
            return Error::Network { code: -1, message };
        }
    };

    if !is_network_exception {
        // Generic CronetException, not a NetworkException
        return Error::Network { code: -1, message };
    }

    // Get error code from NetworkException.getErrorCode()
    let error_code = match env.call_method(error, "getErrorCode", "()I", &[]) {
        Ok(result) => match result.i() {
            Ok(code) => code,
            Err(e) => {
                tracing::warn!("Failed to convert error code: {}", e);
                -1
            }
        },
        Err(e) => {
            tracing::warn!("Failed to get error code: {}", e);
            -1
        }
    };

    tracing::error!("Cronet error code {}: {}", error_code, message);

    // Map error codes to appropriate Error variants
    match error_code {
        ERROR_CONNECTION_TIMED_OUT | ERROR_TIMED_OUT => Error::Timeout,
        ERROR_HOSTNAME_NOT_RESOLVED => Error::Network {
            code: error_code as i64,
            message: format!("Hostname not resolved: {}", message),
        },
        ERROR_INTERNET_DISCONNECTED => Error::Network {
            code: error_code as i64,
            message: format!("No internet connection: {}", message),
        },
        ERROR_CONNECTION_REFUSED => Error::Network {
            code: error_code as i64,
            message: format!("Connection refused: {}", message),
        },
        ERROR_CONNECTION_RESET => Error::Network {
            code: error_code as i64,
            message: format!("Connection reset: {}", message),
        },
        ERROR_ADDRESS_UNREACHABLE => Error::Network {
            code: error_code as i64,
            message: format!("Address unreachable: {}", message),
        },
        _ => Error::Network {
            code: error_code as i64,
            message,
        },
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_se_brendan_frakt_RustUrlRequestCallback_nativeOnRedirectReceived(
    mut env: JNIEnv,
    this: JObject,
    request: JObject,
    response_info: JObject,
    new_location: JObject,
) {
    println!("🔵 JNI nativeOnRedirectReceived called");

    // Get the redirect URL for logging
    let url_jstring = unsafe { jni::objects::JString::from_raw(new_location.as_raw()) };
    if let Ok(url) = env.get_string(&url_jstring) {
        println!("🔄 Following redirect to: {}", url.to_string_lossy());
    }

    // Get handler ID and send redirect headers (for cookie processing)
    if let Ok(handler_id_long) = env.get_field(&this, "handlerId", "J") {
        if let Ok(handler_id) = handler_id_long.j() {
            if let Some(mut handler) = get_callback_handler(handler_id) {
                // Extract headers from redirect response (which may contain Set-Cookie)
                if let Ok(headers) = handler.extract_headers(&mut env, &response_info) {
                    println!(
                        "🔄 Extracted {} headers from redirect response",
                        headers.len()
                    );
                    // Send redirect headers so cookies can be processed
                    if let Some(ref sender) = handler.response_sender {
                        let _ = sender.send(CallbackEvent::Redirect { headers });
                    }
                }
                // Put handler back
                reinsert_callback_handler(handler_id, *handler);
            }
        }
    }

    // Auto-follow redirects by calling request.followRedirect()
    if let Err(e) = env.call_method(&request, "followRedirect", "()V", &[]) {
        println!("❌ Failed to follow redirect: {}", e);
        tracing::error!("Failed to follow redirect: {}", e);
    } else {
        println!("✅ Redirect followed successfully");
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_se_brendan_frakt_RustUrlRequestCallback_nativeOnResponseStarted(
    mut env: JNIEnv,
    this: JObject,
    request: JObject,
    response_info: JObject,
) {
    println!("🔵 JNI nativeOnResponseStarted called");

    let handler_id = match env.call_method(&this, "getHandlerId", "()J", &[]) {
        Ok(result) => match result.j() {
            Ok(id) => id,
            Err(e) => {
                println!("❌ Failed to convert handler ID: {}", e);
                tracing::error!("Failed to convert handler ID: {}", e);
                return;
            }
        },
        Err(e) => {
            tracing::error!("Failed to get handler ID: {}", e);
            return;
        }
    };

    if let Some(mut handler) = get_callback_handler(handler_id) {
        if let Err(e) = handler.on_response_started(&mut env, &response_info) {
            tracing::error!("Error in onResponseStarted: {}", e);
        }
        // Re-insert the handler with the same ID
        reinsert_callback_handler(handler_id, *handler);
    }

    // Allocate ByteBuffer and start reading (32KB buffer)
    let byte_buffer = match env.call_static_method(
        "java/nio/ByteBuffer",
        "allocateDirect",
        "(I)Ljava/nio/ByteBuffer;",
        &[32768i32.into()],
    ) {
        Ok(result) => match result.l() {
            Ok(buf) => buf,
            Err(e) => {
                tracing::error!("Failed to convert ByteBuffer: {}", e);
                return;
            }
        },
        Err(e) => {
            tracing::error!("Failed to allocate ByteBuffer: {}", e);
            return;
        }
    };

    if let Err(e) = env.call_method(
        &request,
        "read",
        "(Ljava/nio/ByteBuffer;)V",
        &[(&byte_buffer).into()],
    ) {
        tracing::error!("Failed to start reading response: {}", e);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_se_brendan_frakt_RustUrlRequestCallback_nativeOnReadCompleted(
    mut env: JNIEnv,
    this: JObject,
    request: JObject,
    _response_info: JObject,
    byte_buffer: JObject,
) {
    println!("🔵 JNI nativeOnReadCompleted called");

    let handler_id = match env.call_method(&this, "getHandlerId", "()J", &[]) {
        Ok(result) => match result.j() {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Failed to convert handler ID: {}", e);
                return;
            }
        },
        Err(e) => {
            tracing::error!("Failed to get handler ID: {}", e);
            return;
        }
    };

    if let Some(mut handler) = get_callback_handler(handler_id) {
        if let Err(e) = handler.on_read_completed(&mut env, &byte_buffer) {
            tracing::error!("Error in onReadCompleted: {}", e);
        }
        // Re-insert the handler with the same ID
        reinsert_callback_handler(handler_id, *handler);
    }

    // Continue reading - clear buffer and read again
    if let Err(e) = env.call_method(&byte_buffer, "clear", "()Ljava/nio/Buffer;", &[]) {
        tracing::error!("Failed to clear ByteBuffer: {}", e);
        return;
    }

    if let Err(e) = env.call_method(
        &request,
        "read",
        "(Ljava/nio/ByteBuffer;)V",
        &[(&byte_buffer).into()],
    ) {
        tracing::error!("Failed to continue reading response: {}", e);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_se_brendan_frakt_RustUrlRequestCallback_nativeOnSucceeded(
    mut env: JNIEnv,
    this: JObject,
    _request: JObject,
    _response_info: JObject,
) {
    println!("🔵 JNI nativeOnSucceeded called");

    let handler_id = match env.call_method(&this, "getHandlerId", "()J", &[]) {
        Ok(result) => match result.j() {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Failed to convert handler ID: {}", e);
                return;
            }
        },
        Err(e) => {
            tracing::error!("Failed to get handler ID: {}", e);
            return;
        }
    };

    if let Some(mut handler) = get_callback_handler(handler_id) {
        handler.on_succeeded();
    }
    // Handler is already removed by get_callback_handler
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_se_brendan_frakt_RustUrlRequestCallback_nativeOnFailed(
    mut env: JNIEnv,
    this: JObject,
    _request: JObject,
    _response_info: JObject,
    error: JObject,
) {
    println!("🔵 JNI nativeOnFailed called");

    let handler_id = match env.call_method(&this, "getHandlerId", "()J", &[]) {
        Ok(result) => match result.j() {
            Ok(id) => id,
            Err(e) => {
                tracing::error!("Failed to convert handler ID: {}", e);
                return;
            }
        },
        Err(e) => {
            tracing::error!("Failed to get handler ID: {}", e);
            return;
        }
    };

    // Extract error details from CronetException
    let rust_error = extract_cronet_error(&mut env, &error);

    if let Some(mut handler) = get_callback_handler(handler_id) {
        handler.on_failed(rust_error);
    }
}

// RustUrlRequestCallback Java class loading
// This class is compiled at build time and embedded in the binary

/// Embedded DEX bytecode for RustUrlRequestCallback
/// Compiled from java/se/brendan/frakt/RustUrlRequestCallback.java and converted to DEX by build.rs
const RUST_CALLBACK_DEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/classes.dex"));

/// Ensure the RustUrlRequestCallback class is loaded
fn ensure_callback_class_loaded(env: &mut JNIEnv) -> Result<()> {
    // Check if we've already loaded the class
    if CALLBACK_CLASS.get().is_some() {
        return Ok(());
    }

    // Load the DEX file using InMemoryDexClassLoader (API 26+)
    // For older APIs, this will fail and we'd need DexClassLoader with file writing

    // Create a ByteBuffer from the DEX bytes
    // We need to create a mutable copy since new_direct_byte_buffer requires *mut u8
    let mut dex_bytes = RUST_CALLBACK_DEX.to_vec();
    let byte_buffer = unsafe {
        env.new_direct_byte_buffer(dex_bytes.as_mut_ptr(), dex_bytes.len())
            .map_err(|e| Error::Internal(format!("Failed to create ByteBuffer from DEX: {}", e)))?
    };

    // Get parent class loader - use the context class loader which has access to Cronet
    // The context class loader is the app's class loader, not the system class loader
    let current_thread = env
        .call_static_method(
            "java/lang/Thread",
            "currentThread",
            "()Ljava/lang/Thread;",
            &[],
        )
        .map_err(|e| Error::Internal(format!("Failed to get current thread: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to convert current thread: {}", e)))?;

    let parent_loader = env
        .call_method(
            &current_thread,
            "getContextClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .map_err(|e| Error::Internal(format!("Failed to get context class loader: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to convert class loader: {}", e)))?;

    // Create InMemoryDexClassLoader
    let dex_class_loader_class = env
        .find_class("dalvik/system/InMemoryDexClassLoader")
        .map_err(|e| {
            Error::Internal(format!(
                "Failed to find InMemoryDexClassLoader class (requires Android 8.0+): {}",
                e
            ))
        })?;

    let dex_class_loader = env
        .new_object(
            dex_class_loader_class,
            "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
            &[(&byte_buffer).into(), (&parent_loader).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to create InMemoryDexClassLoader: {}", e)))?;

    // Load the class from the DEX class loader
    let class_name = env
        .new_string("se.brendan.frakt.RustUrlRequestCallback")
        .map_err(|e| Error::Internal(format!("Failed to create class name string: {}", e)))?;

    let loaded_class = env
        .call_method(
            &dex_class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&class_name).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to load class from DEX: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to convert loaded class: {}", e)))?;

    // Register native methods manually since InMemoryDexClassLoader doesn't support automatic JNI resolution
    register_native_methods(env, &loaded_class)?;

    // Store the class as a global reference
    let class_ref = env
        .new_global_ref(&loaded_class)
        .map_err(|e| Error::Internal(format!("Failed to create global ref for class: {}", e)))?;

    // Store in OnceCell (ignore if another thread already stored it)
    let _ = CALLBACK_CLASS.set(class_ref);

    Ok(())
}

/// Register native methods with the RustUrlRequestCallback class
fn register_native_methods(env: &mut JNIEnv, class: &JObject) -> Result<()> {
    use jni::NativeMethod;
    use jni::objects::JClass;

    // Convert JObject to JClass without moving
    let jclass = unsafe { JClass::from_raw(class.as_raw()) };

    let native_methods = [
        NativeMethod {
            name: "nativeOnRedirectReceived".into(),
            sig: "(Lorg/chromium/net/UrlRequest;Lorg/chromium/net/UrlResponseInfo;Ljava/lang/String;)V".into(),
            fn_ptr: Java_se_brendan_frakt_RustUrlRequestCallback_nativeOnRedirectReceived as *mut std::ffi::c_void,
        },
        NativeMethod {
            name: "nativeOnResponseStarted".into(),
            sig: "(Lorg/chromium/net/UrlRequest;Lorg/chromium/net/UrlResponseInfo;)V".into(),
            fn_ptr: Java_se_brendan_frakt_RustUrlRequestCallback_nativeOnResponseStarted as *mut std::ffi::c_void,
        },
        NativeMethod {
            name: "nativeOnReadCompleted".into(),
            sig: "(Lorg/chromium/net/UrlRequest;Lorg/chromium/net/UrlResponseInfo;Ljava/nio/ByteBuffer;)V".into(),
            fn_ptr: Java_se_brendan_frakt_RustUrlRequestCallback_nativeOnReadCompleted as *mut std::ffi::c_void,
        },
        NativeMethod {
            name: "nativeOnSucceeded".into(),
            sig: "(Lorg/chromium/net/UrlRequest;Lorg/chromium/net/UrlResponseInfo;)V".into(),
            fn_ptr: Java_se_brendan_frakt_RustUrlRequestCallback_nativeOnSucceeded as *mut std::ffi::c_void,
        },
        NativeMethod {
            name: "nativeOnFailed".into(),
            sig: "(Lorg/chromium/net/UrlRequest;Lorg/chromium/net/UrlResponseInfo;Lorg/chromium/net/CronetException;)V".into(),
            fn_ptr: Java_se_brendan_frakt_RustUrlRequestCallback_nativeOnFailed as *mut std::ffi::c_void,
        },
    ];

    env.register_native_methods(jclass, &native_methods)
        .map_err(|e| Error::Internal(format!("Failed to register native methods: {}", e)))?;

    Ok(())
}

/// Create a RustUrlRequestCallback instance
pub fn create_callback_instance(env: &mut JNIEnv, handler_id: jlong) -> Result<GlobalRef> {
    // Ensure the RustUrlRequestCallback class is loaded
    ensure_callback_class_loaded(env)?;

    // Get the stored class reference
    let callback_class = CALLBACK_CLASS
        .get()
        .ok_or_else(|| Error::Internal("RustUrlRequestCallback class not loaded".to_string()))?;

    // Create instance using the stored class
    // Convert GlobalRef to JClass for new_object
    let class_obj = unsafe { JClass::from_raw(callback_class.as_obj().as_raw()) };
    let callback_object = env
        .new_object(class_obj, "(J)V", &[handler_id.into()])
        .map_err(|e| Error::Internal(format!("Failed to create RustUrlRequestCallback: {}", e)))?;

    env.new_global_ref(&callback_object)
        .map_err(|e| Error::Internal(format!("Failed to create global ref for callback: {}", e)))
}
