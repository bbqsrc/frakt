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
        // Extract status code
        let status_code = env
            .call_method(response_info, "getHttpStatusCode", "()I", &[])
            .map_err(|e| Error::Internal(format!("Failed to get status code: {}", e)))?
            .i()
            .map_err(|e| Error::Internal(format!("Failed to convert status code: {}", e)))?;

        let status = StatusCode::from_u16(status_code as u16)
            .map_err(|e| Error::Internal(format!("Invalid status code {}: {}", status_code, e)))?;

        // Extract headers (simplified for now)
        let headers = HeaderMap::new();
        // TODO: Implement proper header extraction from response_info.getAllHeaders()

        if let Some(sender) = &self.response_sender {
            let _ = sender.send(CallbackEvent::ResponseStarted { status, headers });
        }

        Ok(())
    }

    /// Handle onReadCompleted callback
    pub fn on_read_completed(&mut self, env: &mut JNIEnv, byte_buffer: &JObject) -> Result<()> {
        // Extract data from ByteBuffer
        let data = self.extract_byte_buffer_data(env, byte_buffer)?;

        if let Some(sender) = &self.body_sender {
            let _ = sender.send(Ok(data));
        }

        Ok(())
    }

    /// Handle onSucceeded callback
    pub fn on_succeeded(&mut self) {
        if let Some(sender) = &self.response_sender {
            let _ = sender.send(CallbackEvent::Succeeded);
        }

        // Close the body channel
        self.body_sender = None;
    }

    /// Handle onFailed callback
    pub fn on_failed(&mut self, error: Error) {
        if let Some(sender) = &self.response_sender {
            let _ = sender.send(CallbackEvent::Failed {
                error: error.clone(),
            });
        }

        if let Some(sender) = &self.body_sender {
            let _ = sender.send(Err(error));
        }

        // Close channels
        self.response_sender = None;
        self.body_sender = None;
    }

    /// Extract data from a Java ByteBuffer
    fn extract_byte_buffer_data(&self, env: &mut JNIEnv, byte_buffer: &JObject) -> Result<Bytes> {
        // Get the position and limit of the ByteBuffer
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

        let data = if has_array {
            // Get the backing array
            let array = env
                .call_method(byte_buffer, "array", "()[B", &[])
                .map_err(|e| Error::Internal(format!("Failed to get ByteBuffer array: {}", e)))?
                .l()
                .map_err(|e| Error::Internal(format!("Failed to get array object: {}", e)))?;

            let array_offset = env
                .call_method(byte_buffer, "arrayOffset", "()I", &[])
                .map_err(|e| Error::Internal(format!("Failed to get array offset: {}", e)))?
                .i()
                .map_err(|e| Error::Internal(format!("Failed to convert array offset: {}", e)))?
                as usize;

            // Get bytes from the array
            let start_index = (array_offset + position) as i32;
            let byte_array = unsafe {
                jni::objects::JByteArray::from_raw(array.into_raw() as jni::sys::jbyteArray)
            };

            let mut buffer = vec![0i8; length];
            env.get_byte_array_region(&byte_array, start_index, &mut buffer)
                .map_err(|e| Error::Internal(format!("Failed to get byte array region: {}", e)))?;

            buffer.as_ptr() as *const u8
        } else {
            // Handle direct ByteBuffer - this is more complex and requires unsafe code
            return Err(Error::Internal(
                "Direct ByteBuffers not yet supported".to_string(),
            ));
        };

        // Convert to Bytes (this is simplified - in practice you'd need proper memory management)
        let vec_data = unsafe { std::slice::from_raw_parts(data as *const u8, length).to_vec() };

        Ok(Bytes::from(vec_data))
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
// These need to be exported and match the expected signatures

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_chromium_net_UrlRequest_00024Callback_onResponseStarted(
    mut env: JNIEnv,
    _class: JClass,
    handler_id: jlong,
    _request: JObject,
    response_info: JObject,
) {
    if let Some(mut handler) = get_callback_handler(handler_id) {
        if let Err(e) = handler.on_response_started(&mut env, &response_info) {
            tracing::error!("Error in onResponseStarted: {}", e);
        }
        // Re-register the handler
        register_callback_handler(*handler);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_chromium_net_UrlRequest_00024Callback_onReadCompleted(
    mut env: JNIEnv,
    _class: JClass,
    handler_id: jlong,
    _request: JObject,
    _response_info: JObject,
    byte_buffer: JObject,
) {
    if let Some(mut handler) = get_callback_handler(handler_id) {
        if let Err(e) = handler.on_read_completed(&mut env, &byte_buffer) {
            tracing::error!("Error in onReadCompleted: {}", e);
        }
        // Re-register the handler
        register_callback_handler(*handler);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_chromium_net_UrlRequest_00024Callback_onSucceeded(
    _env: JNIEnv,
    _class: JClass,
    handler_id: jlong,
    _request: JObject,
    _response_info: JObject,
) {
    if let Some(mut handler) = get_callback_handler(handler_id) {
        handler.on_succeeded();
    }
    // Handler is already removed by get_callback_handler
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_chromium_net_UrlRequest_00024Callback_onFailed(
    _env: JNIEnv,
    _class: JClass,
    handler_id: jlong,
    _request: JObject,
    _response_info: JObject,
    _error: JObject,
) {
    let error_msg = "Request failed".to_string(); // TODO: Extract actual error from CronetException
    let rust_error = Error::Internal(error_msg);

    if let Some(mut handler) = get_callback_handler(handler_id) {
        handler.on_failed(rust_error);
    }
    // Handler is already removed by get_callback_handler
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

    // Store the class as a global reference
    let class_ref = env
        .new_global_ref(&loaded_class)
        .map_err(|e| Error::Internal(format!("Failed to create global ref for class: {}", e)))?;

    // Store in OnceCell (ignore if another thread already stored it)
    let _ = CALLBACK_CLASS.set(class_ref);

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

// JNI callback functions called by RustUrlRequestCallback native methods

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_se_brendan_frakt_RustUrlRequestCallback_onRedirectReceived(
    mut env: JNIEnv,
    this: JObject,
    _request: JObject,
    _info: JObject,
    _new_location: JObject,
) {
    // Get handler_id from the callback object
    let handler_id = match env.call_method(&this, "getHandlerId", "()J", &[]) {
        Ok(val) => match val.j() {
            Ok(id) => id,
            Err(_) => return,
        },
        Err(_) => return,
    };

    // For now, automatically follow redirects
    // TODO: Implement proper redirect handling
    tracing::debug!("onRedirectReceived for handler {}", handler_id);
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_se_brendan_frakt_RustUrlRequestCallback_onResponseStarted(
    mut env: JNIEnv,
    this: JObject,
    _request: JObject,
    response_info: JObject,
) {
    // Get handler_id from the callback object
    let handler_id = match env.call_method(&this, "getHandlerId", "()J", &[]) {
        Ok(val) => match val.j() {
            Ok(id) => id,
            Err(_) => return,
        },
        Err(_) => return,
    };

    if let Some(mut handler) = get_callback_handler(handler_id) {
        if let Err(e) = handler.on_response_started(&mut env, &response_info) {
            tracing::error!("Error in onResponseStarted: {}", e);
        }
        // Re-register the handler
        register_callback_handler(*handler);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_se_brendan_frakt_RustUrlRequestCallback_onReadCompleted(
    mut env: JNIEnv,
    this: JObject,
    _request: JObject,
    _response_info: JObject,
    byte_buffer: JObject,
) {
    // Get handler_id from the callback object
    let handler_id = match env.call_method(&this, "getHandlerId", "()J", &[]) {
        Ok(val) => match val.j() {
            Ok(id) => id,
            Err(_) => return,
        },
        Err(_) => return,
    };

    if let Some(mut handler) = get_callback_handler(handler_id) {
        if let Err(e) = handler.on_read_completed(&mut env, &byte_buffer) {
            tracing::error!("Error in onReadCompleted: {}", e);
        }
        // Re-register the handler
        register_callback_handler(*handler);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_se_brendan_frakt_RustUrlRequestCallback_onSucceeded(
    mut env: JNIEnv,
    this: JObject,
    _request: JObject,
    _response_info: JObject,
) {
    // Get handler_id from the callback object
    let handler_id = match env.call_method(&this, "getHandlerId", "()J", &[]) {
        Ok(val) => match val.j() {
            Ok(id) => id,
            Err(_) => return,
        },
        Err(_) => return,
    };

    if let Some(mut handler) = get_callback_handler(handler_id) {
        handler.on_succeeded();
    }
    // Handler is already removed by get_callback_handler
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_se_brendan_frakt_RustUrlRequestCallback_onFailed(
    mut env: JNIEnv,
    this: JObject,
    _request: JObject,
    _response_info: JObject,
    _error: JObject,
) {
    // Get handler_id from the callback object
    let handler_id = match env.call_method(&this, "getHandlerId", "()J", &[]) {
        Ok(val) => match val.j() {
            Ok(id) => id,
            Err(_) => return,
        },
        Err(_) => return,
    };

    let error_msg = "Request failed".to_string(); // TODO: Extract actual error from CronetException
    let rust_error = Error::Internal(error_msg);

    if let Some(mut handler) = get_callback_handler(handler_id) {
        handler.on_failed(rust_error);
    }
    // Handler is already removed by get_callback_handler
}
