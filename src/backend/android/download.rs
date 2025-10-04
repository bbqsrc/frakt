//! Android background downloads using DownloadManager

use crate::{Error, Result};
use jni::{JNIEnv, JavaVM, objects::GlobalRef, objects::JObject};
use std::path::PathBuf;
use std::sync::Arc;
use url::Url;

/// Execute a background download using Android's DownloadManager
pub async fn execute_background_download(
    jvm: &JavaVM,
    url: Url,
    file_path: PathBuf,
    session_identifier: Option<String>,
    headers: http::HeaderMap,
    _error_for_status: bool,
    progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
) -> Result<crate::client::download::DownloadResponse> {
    let mut env = jvm
        .attach_current_thread()
        .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

    // Get DownloadManager system service
    let download_manager_global = get_download_manager(&jvm)?;

    // Create download request
    let request_global = create_download_request(&jvm, &url, &file_path)?;

    // Configure the download request
    configure_download_request(&jvm, &request_global, &session_identifier)?;

    // Enqueue the download
    let download_id = env
        .call_method(
            download_manager_global.as_obj(),
            "enqueue",
            "(Landroid/app/DownloadManager$Request;)J",
            &[request_global.as_obj().into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to enqueue download: {}", e)))?
        .j()
        .map_err(|e| Error::Internal(format!("Failed to get download ID: {}", e)))?;

    // Create download response with monitoring
    create_download_response(
        jvm,
        download_manager_global,
        download_id,
        file_path,
        progress_callback,
    )
    .await
}

/// Get the DownloadManager system service
fn get_download_manager(jvm: &JavaVM) -> Result<GlobalRef> {
    let mut env = jvm
        .attach_current_thread()
        .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

    // Get application context inline to avoid borrowing issues
    let activity_thread_class = env
        .find_class("android/app/ActivityThread")
        .map_err(|e| Error::Internal(format!("Failed to find ActivityThread class: {}", e)))?;

    let current_activity_thread = env
        .call_static_method(
            activity_thread_class,
            "currentActivityThread",
            "()Landroid/app/ActivityThread;",
            &[],
        )
        .map_err(|e| Error::Internal(format!("Failed to get current ActivityThread: {}", e)))?;

    let context = env
        .call_method(
            current_activity_thread.l().unwrap(),
            "getApplication",
            "()Landroid/app/Application;",
            &[],
        )
        .map_err(|e| Error::Internal(format!("Failed to get application context: {}", e)))?;

    // Get the DownloadManager service
    let download_service_string = env
        .new_string("download")
        .map_err(|e| Error::Internal(format!("Failed to create download service string: {}", e)))?;

    let download_manager = env
        .call_method(
            context.l().unwrap(),
            "getSystemService",
            "(Ljava/lang/String;)Ljava/lang/Object;",
            &[(&download_service_string).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to get DownloadManager service: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to get DownloadManager object: {}", e)))?;

    env.new_global_ref(&download_manager).map_err(|e| {
        Error::Internal(format!(
            "Failed to create global ref for DownloadManager: {}",
            e
        ))
    })
}

/// Create a DownloadManager.Request
fn create_download_request(jvm: &JavaVM, url: &Url, file_path: &PathBuf) -> Result<GlobalRef> {
    let mut env = jvm
        .attach_current_thread()
        .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;
    // Create URI object for the URL
    let url_string = env
        .new_string(url.as_str())
        .map_err(|e| Error::Internal(format!("Failed to create URL string: {}", e)))?;

    let uri = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[(&url_string).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to parse URL: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to get URI object: {}", e)))?;

    // Create DownloadManager.Request
    let request_class = env
        .find_class("android/app/DownloadManager$Request")
        .map_err(|e| {
            Error::Internal(format!(
                "Failed to find DownloadManager.Request class: {}",
                e
            ))
        })?;

    let request = env
        .new_object(request_class, "(Landroid/net/Uri;)V", &[(&uri).into()])
        .map_err(|e| Error::Internal(format!("Failed to create DownloadManager.Request: {}", e)))?;

    // Ensure parent directory exists
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Internal(format!("Failed to create parent directory: {}", e)))?;
    }

    // Create file:// URI for the destination
    let file_uri_string = format!("file://{}", file_path.to_string_lossy());
    let destination_uri_string = env
        .new_string(&file_uri_string)
        .map_err(|e| Error::Internal(format!("Failed to create destination URI string: {}", e)))?;

    let destination_uri = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[(&destination_uri_string).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to parse destination URI: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to get destination URI object: {}", e)))?;

    env.call_method(
        &request,
        "setDestinationUri",
        "(Landroid/net/Uri;)Landroid/app/DownloadManager$Request;",
        &[(&destination_uri).into()],
    )
    .map_err(|e| Error::Internal(format!("Failed to set download destination: {}", e)))?;

    env.new_global_ref(&request).map_err(|e| {
        Error::Internal(format!(
            "Failed to create global ref for download request: {}",
            e
        ))
    })
}

/// Configure the download request with additional settings
fn configure_download_request(
    jvm: &JavaVM,
    request: &GlobalRef,
    session_identifier: &Option<String>,
) -> Result<()> {
    let mut env = jvm
        .attach_current_thread()
        .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;
    // Set notification visibility
    env.call_method(
        request.as_obj(),
        "setNotificationVisibility",
        "(I)Landroid/app/DownloadManager$Request;",
        &[1i32.into()], // VISIBILITY_VISIBLE
    )
    .map_err(|e| Error::Internal(format!("Failed to set notification visibility: {}", e)))?;

    // Allow download over mobile network
    env.call_method(
        request.as_obj(),
        "setAllowedNetworkTypes",
        "(I)Landroid/app/DownloadManager$Request;",
        &[3i32.into()], // NETWORK_WIFI | NETWORK_MOBILE
    )
    .map_err(|e| Error::Internal(format!("Failed to set allowed network types: {}", e)))?;

    // Set title and description
    if let Some(identifier) = session_identifier {
        let title = env
            .new_string(&format!("Download: {}", identifier))
            .map_err(|e| Error::Internal(format!("Failed to create title string: {}", e)))?;

        env.call_method(
            request.as_obj(),
            "setTitle",
            "(Ljava/lang/CharSequence;)Landroid/app/DownloadManager$Request;",
            &[(&title).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to set download title: {}", e)))?;
    }

    Ok(())
}

/// Create a download response that monitors the download progress
async fn create_download_response(
    jvm: &JavaVM,
    download_manager: GlobalRef,
    download_id: i64,
    file_path: PathBuf,
    progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
) -> Result<crate::client::download::DownloadResponse> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::time::{Duration, sleep};

    let completed = Arc::new(AtomicBool::new(false));
    let completed_clone = completed.clone();

    // Create global reference to download manager to prevent GC
    let mut env = jvm
        .attach_current_thread()
        .map_err(|e| Error::Internal(format!("Failed to attach to JVM thread: {}", e)))?;

    let dm_global = env.new_global_ref(&download_manager).map_err(|e| {
        Error::Internal(format!(
            "Failed to create global ref to DownloadManager: {}",
            e
        ))
    })?;

    // Spawn monitoring task
    let file_path_clone = file_path.clone();

    tokio::spawn(async move {
        let mut last_downloaded = 0u64;
        let mut last_total = None;

        while !completed_clone.load(Ordering::Relaxed) {
            if let Ok(jvm) = crate::backend::android::get_global_vm() {
                if let Ok(mut env) = jvm.attach_current_thread() {
                    match query_download_progress(&mut env, &dm_global, download_id) {
                        Ok((status, downloaded, total)) => {
                            // Update progress if callback is provided
                            if let Some(callback) = &progress_callback {
                                if downloaded != last_downloaded || total != last_total {
                                    callback(downloaded, total);
                                    last_downloaded = downloaded;
                                    last_total = total;
                                }
                            }

                            // Check if download is complete
                            match status {
                                DownloadStatus::Successful => {
                                    if let Some(callback) = &progress_callback {
                                        callback(downloaded, total);
                                    }
                                    completed_clone.store(true, Ordering::Relaxed);
                                    break;
                                }
                                DownloadStatus::Failed => {
                                    tracing::error!("Download failed for ID: {}", download_id);
                                    completed_clone.store(true, Ordering::Relaxed);
                                    break;
                                }
                                _ => {
                                    // Still in progress, continue monitoring
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to query download progress: {}", e);
                            // Continue trying - might be temporary issue
                        }
                    }
                }
            }

            // Wait before next progress check
            sleep(Duration::from_millis(500)).await;
        }
    });

    // TODO: Capture actual status and headers from DownloadManager
    Ok(crate::client::download::DownloadResponse {
        file_path,
        bytes_downloaded: 0, // TODO: Get actual bytes downloaded from DownloadManager
        status: http::StatusCode::OK,
        headers: http::HeaderMap::new(),
    })
}

/// Download status from DownloadManager
#[derive(Debug, Clone, Copy, PartialEq)]
enum DownloadStatus {
    Pending,
    Running,
    Paused,
    Successful,
    Failed,
}

impl From<i32> for DownloadStatus {
    fn from(status: i32) -> Self {
        match status {
            1 => DownloadStatus::Pending,
            2 => DownloadStatus::Running,
            4 => DownloadStatus::Paused,
            8 => DownloadStatus::Successful,
            16 => DownloadStatus::Failed,
            _ => DownloadStatus::Failed,
        }
    }
}

/// Query download progress from DownloadManager
fn query_download_progress(
    env: &mut JNIEnv,
    download_manager: &GlobalRef,
    download_id: i64,
) -> Result<(DownloadStatus, u64, Option<u64>)> {
    // Create query for this download
    let query_class = env
        .find_class("android/app/DownloadManager$Query")
        .map_err(|e| {
            Error::Internal(format!("Failed to find DownloadManager.Query class: {}", e))
        })?;

    let query = env
        .new_object(query_class, "()V", &[])
        .map_err(|e| Error::Internal(format!("Failed to create DownloadManager.Query: {}", e)))?;

    // Filter by download ID
    env.call_method(
        &query,
        "setFilterById",
        "(J)Landroid/app/DownloadManager$Query;",
        &[download_id.into()],
    )
    .map_err(|e| Error::Internal(format!("Failed to set filter by ID: {}", e)))?;

    // Execute query
    let cursor = env
        .call_method(
            download_manager.as_obj(),
            "query",
            "(Landroid/app/DownloadManager$Query;)Landroid/database/Cursor;",
            &[(&query).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to query download: {}", e)))?
        .l()
        .map_err(|e| Error::Internal(format!("Failed to get cursor: {}", e)))?;

    // Check if cursor has results
    let has_results = env
        .call_method(&cursor, "moveToFirst", "()Z", &[])
        .map_err(|e| Error::Internal(format!("Failed to move cursor to first: {}", e)))?
        .z()
        .map_err(|e| Error::Internal(format!("Failed to convert cursor result: {}", e)))?;

    if !has_results {
        return Err(Error::Internal("Download not found".to_string()));
    }

    // Get status column index
    let status_column = env
        .new_string("status")
        .map_err(|e| Error::Internal(format!("Failed to create status column string: {}", e)))?;
    let status_index = env
        .call_method(
            &cursor,
            "getColumnIndex",
            "(Ljava/lang/String;)I",
            &[(&status_column).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to get status column index: {}", e)))?
        .i()
        .map_err(|e| Error::Internal(format!("Failed to convert status index: {}", e)))?;

    // Get downloaded bytes column index
    let downloaded_column = env.new_string("bytes_so_far").map_err(|e| {
        Error::Internal(format!("Failed to create downloaded column string: {}", e))
    })?;
    let downloaded_index = env
        .call_method(
            &cursor,
            "getColumnIndex",
            "(Ljava/lang/String;)I",
            &[(&downloaded_column).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to get downloaded column index: {}", e)))?
        .i()
        .map_err(|e| Error::Internal(format!("Failed to convert downloaded index: {}", e)))?;

    // Get total bytes column index
    let total_column = env
        .new_string("total_size")
        .map_err(|e| Error::Internal(format!("Failed to create total column string: {}", e)))?;
    let total_index = env
        .call_method(
            &cursor,
            "getColumnIndex",
            "(Ljava/lang/String;)I",
            &[(&total_column).into()],
        )
        .map_err(|e| Error::Internal(format!("Failed to get total column index: {}", e)))?
        .i()
        .map_err(|e| Error::Internal(format!("Failed to convert total index: {}", e)))?;

    // Read values
    let status_value = env
        .call_method(&cursor, "getInt", "(I)I", &[status_index.into()])
        .map_err(|e| Error::Internal(format!("Failed to get status value: {}", e)))?
        .i()
        .map_err(|e| Error::Internal(format!("Failed to convert status value: {}", e)))?;

    let downloaded_bytes = env
        .call_method(&cursor, "getLong", "(I)J", &[downloaded_index.into()])
        .map_err(|e| Error::Internal(format!("Failed to get downloaded bytes: {}", e)))?
        .j()
        .map_err(|e| Error::Internal(format!("Failed to convert downloaded bytes: {}", e)))?
        as u64;

    let total_bytes = env
        .call_method(&cursor, "getLong", "(I)J", &[total_index.into()])
        .map_err(|e| Error::Internal(format!("Failed to get total bytes: {}", e)))?
        .j()
        .map_err(|e| Error::Internal(format!("Failed to convert total bytes: {}", e)))?
        as u64;

    // Close cursor
    env.call_method(&cursor, "close", "()V", &[])
        .map_err(|e| Error::Internal(format!("Failed to close cursor: {}", e)))?;

    let status = DownloadStatus::from(status_value);
    let total = if total_bytes > 0 {
        Some(total_bytes)
    } else {
        None
    };

    Ok((status, downloaded_bytes, total))
}

/// Get the Android application context
fn get_application_context<'a>(env: &'a mut JNIEnv<'a>) -> Result<JObject<'a>> {
    // Try to get the activity class
    let activity_thread_class = env
        .find_class("android/app/ActivityThread")
        .map_err(|e| Error::Internal(format!("Failed to find ActivityThread class: {}", e)))?;

    // Get current activity thread
    let current_activity_thread = env
        .call_static_method(
            activity_thread_class,
            "currentActivityThread",
            "()Landroid/app/ActivityThread;",
            &[],
        )
        .map_err(|e| Error::Internal(format!("Failed to get current ActivityThread: {}", e)))?;

    // Get application context
    let context = env
        .call_method(
            current_activity_thread.l().unwrap(),
            "getApplication",
            "()Landroid/app/Application;",
            &[],
        )
        .map_err(|e| Error::Internal(format!("Failed to get application context: {}", e)))?;

    Ok(context.l().unwrap())
}
