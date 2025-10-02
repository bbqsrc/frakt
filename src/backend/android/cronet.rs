//! Cronet engine creation and configuration

use crate::backend::BackendConfig;
use crate::{Error, Result};
use jni::{JNIEnv, JavaVM, objects::GlobalRef};

/// Create a Cronet engine with default configuration
pub fn create_cronet_engine(jvm: &JavaVM) -> Result<GlobalRef> {
    let config = BackendConfig::default();
    create_cronet_engine_with_config(jvm, &config)
}

/// Create a Cronet engine with custom configuration
pub fn create_cronet_engine_with_config(jvm: &JavaVM, config: &BackendConfig) -> Result<GlobalRef> {
    let mut env = jvm
        .attach_current_thread()
        .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

    // Get Android application context
    let context = get_application_context()?;

    // Create CronetEngine.Builder
    let builder_class = env
        .find_class("org/chromium/net/CronetEngine$Builder")
        .map_err(|e| {
            Error::Internal(format!("Failed to find CronetEngine.Builder class: {}", e))
        })?;

    let builder = env
        .new_object(
            builder_class,
            "(Landroid/content/Context;)V",
            &[(context.as_obj()).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to create CronetEngine.Builder: {}", e)))?;

    // Configure the builder
    configure_cronet_builder(&mut env, &builder, config)?;

    // Build the engine
    let engine = env
        .call_method(&builder, "build", "()Lorg/chromium/net/CronetEngine;", &[])
        .map_err(|e| Error::Internal(format!("Failed to build CronetEngine: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to convert CronetEngine: {}", e)))?;

    env.new_global_ref(&engine)
        .map_err(|e| Error::Internal(format!("Failed to create global reference: {}", e)))
}

/// Configure the Cronet engine builder with our settings
fn configure_cronet_builder(
    env: &mut JNIEnv,
    builder: &jni::objects::JObject,
    config: &BackendConfig,
) -> Result<()> {
    // Enable HTTP/2
    env.call_method(
        builder,
        "enableHttp2",
        "(Z)Lorg/chromium/net/CronetEngine$Builder;",
        &[true.into()],
    )
    .map_err(|e| Error::Internal(format!("Failed to enable HTTP/2: {}", e)))?;

    // Enable QUIC (HTTP/3)
    env.call_method(
        builder,
        "enableQuic",
        "(Z)Lorg/chromium/net/CronetEngine$Builder;",
        &[true.into()],
    )
    .map_err(|e| Error::Internal(format!("Failed to enable QUIC: {}", e)))?;

    // Enable Brotli compression
    env.call_method(
        builder,
        "enableBrotli",
        "(Z)Lorg/chromium/net/CronetEngine$Builder;",
        &[true.into()],
    )
    .map_err(|e| Error::Internal(format!("Failed to enable Brotli: {}", e)))?;

    // Set user agent if provided
    if let Some(user_agent) = &config.user_agent {
        let user_agent_jstring = env
            .new_string(user_agent)
            .map_err(|e| Error::Internal(format!("Failed to create user agent string: {}", e)))?;

        env.call_method(
            builder,
            "setUserAgent",
            "(Ljava/lang/String;)Lorg/chromium/net/CronetEngine$Builder;",
            &[(&user_agent_jstring).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to set user agent: {}", e)))?;
    }

    // Configure cache if needed (using default cache directory for now)
    env.call_method(
        builder,
        "setStoragePath",
        "(Ljava/lang/String;)Lorg/chromium/net/CronetEngine$Builder;",
        &[(&env
            .new_string("/data/data/com.example.app/cache/cronet")
            .map_err(|e| Error::Internal(format!("Failed to create cache path string: {}", e)))?)
            .into()],
    )
    .map_err(|e| Error::Internal(format!("Failed to set storage path: {}", e)))?;

    env.call_method(
        builder,
        "enableHttpCache",
        "(IJ)Lorg/chromium/net/CronetEngine$Builder;",
        &[
            2i32.into(),                  // HTTP_CACHE_DISK
            (50 * 1024 * 1024i64).into(), // 50MB cache size
        ],
    )
    .map_err(|e| Error::Internal(format!("Failed to enable HTTP cache: {}", e)))?;

    Ok(())
}

/// Get the Android application context using ActivityThread
fn get_application_context() -> Result<GlobalRef> {
    // We need to get the JavaVM to attach and get the context
    let jvm = super::get_global_vm()?;
    let mut env = jvm
        .attach_current_thread()
        .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

    // Get ActivityThread class
    let activity_thread_class = env
        .find_class("android/app/ActivityThread")
        .map_err(|e| Error::Internal(format!("Failed to find ActivityThread class: {}", e)))?;

    // Get current ActivityThread
    let current_activity_thread = env
        .call_static_method(
            activity_thread_class,
            "currentActivityThread",
            "()Landroid/app/ActivityThread;",
            &[],
        )
        .map_err(|e| Error::Internal(format!("Failed to get current ActivityThread: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to convert ActivityThread: {}", e)))?;

    // Get Application (which is a Context)
    let application = env
        .call_method(
            &current_activity_thread,
            "getApplication",
            "()Landroid/app/Application;",
            &[],
        )
        .map_err(|e| Error::Internal(format!("Failed to get Application: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to convert Application: {}", e)))?;

    // Create a global reference so it lives long enough
    env.new_global_ref(&application)
        .map_err(|e| Error::Internal(format!("Failed to create global ref for context: {}", e)))
}
