//! Background session delegate implementation

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{
    NSCopying, NSError, NSObject, NSObjectProtocol, NSURL, NSURLSession, NSURLSessionDelegate,
    NSURLSessionDownloadDelegate, NSURLSessionDownloadTask, NSURLSessionTask,
    NSURLSessionTaskDelegate,
};

use super::TaskSharedContext;

/// Background session completion handler
pub type BackgroundCompletionHandler = DynBlock<dyn Fn()>;

/// Instance variables for the BackgroundSessionDelegate
pub struct BackgroundSessionDelegateIvars {
    /// Map of task identifiers to their shared contexts
    pub task_contexts: Mutex<HashMap<usize, Arc<TaskSharedContext>>>,
    /// Background completion handlers by session identifier
    pub completion_handlers: Mutex<HashMap<String, Retained<BackgroundCompletionHandler>>>,
}

define_class!(
    /// NSURLSessionDelegate implementation for background sessions
    #[unsafe(super = NSObject)]
    #[name = "fraktBackgroundSessionDelegate"]
    #[ivars = BackgroundSessionDelegateIvars]
    pub struct BackgroundSessionDelegate;

    unsafe impl NSObjectProtocol for BackgroundSessionDelegate {}

    unsafe impl NSURLSessionDelegate for BackgroundSessionDelegate {
        #[unsafe(method(URLSessionDidFinishEventsForBackgroundURLSession:))]
        fn URLSessionDidFinishEventsForBackgroundURLSession(&self, session: &NSURLSession) {
            let ivars = self.ivars();

            // Get the session identifier
            let session_id = unsafe {
                objc2::rc::autoreleasepool(|pool| {
                    session
                        .configuration()
                        .identifier()
                        .unwrap()
                        .to_str(pool)
                        .to_string()
                })
            };

            // Call the completion handler if we have one
            if let Ok(mut handlers) = ivars.completion_handlers.lock() {
                if let Some(handler) = handlers.remove(&session_id) {
                    handler.call(());
                }
            }
        }
    }

    unsafe impl NSURLSessionTaskDelegate for BackgroundSessionDelegate {
        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn URLSession_task_didCompleteWithError(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionTask,
            error: Option<&NSError>,
        ) {
            let ivars = self.ivars();
            let task_id = unsafe { task.taskIdentifier() } as usize;

            if let Ok(mut contexts) = ivars.task_contexts.lock() {
                if let Some(shared_context) = contexts.remove(&task_id) {
                    if let Some(error) = error {
                        // Set error in shared context
                        shared_context.set_error(error.copy());
                    } else {
                        // Mark task as completed successfully
                        shared_context.mark_completed();
                    }
                }
            }
        }
    }

    unsafe impl NSURLSessionDownloadDelegate for BackgroundSessionDelegate {
        #[unsafe(method(URLSession:downloadTask:didFinishDownloadingToURL:))]
        fn URLSession_downloadTask_didFinishDownloadingToURL(
            &self,
            _session: &NSURLSession,
            download_task: &NSURLSessionDownloadTask,
            location: &NSURL,
        ) {
            let ivars = self.ivars();
            let task_id = unsafe { download_task.taskIdentifier() } as usize;

            if let Ok(contexts) = ivars.task_contexts.lock() {
                if let Some(shared_context) = contexts.get(&task_id) {
                    // We must copy the file immediately as the temp file will be deleted after this method returns
                    if let Some(download_context) = shared_context.download_context.as_ref() {
                        let temp_path = unsafe {
                            objc2::rc::autoreleasepool(|pool| {
                                location.path().unwrap().to_str(pool).to_string()
                            })
                        };

                        // Get the destination path from context
                        if let Some(dest_path) = download_context.destination_path.clone() {
                            // Copy the file immediately
                            if let Err(_e) = std::fs::copy(&temp_path, &dest_path) {
                                // Set error if copy fails
                                let error_msg = format!(
                                    "Failed to copy downloaded file from {} to {:?}",
                                    temp_path, dest_path
                                );
                                shared_context.set_error_from_string(error_msg);
                                return;
                            }
                            // Store the final destination path
                            download_context.set_final_location(dest_path);
                        } else {
                            // Generate default filename if no destination specified
                            let default_path = std::path::PathBuf::from(format!(
                                "download_{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs()
                            ));

                            if let Err(_e) = std::fs::copy(&temp_path, &default_path) {
                                let error_msg = format!(
                                    "Failed to copy downloaded file from {} to {:?}",
                                    temp_path, default_path
                                );
                                shared_context.set_error_from_string(error_msg);
                                return;
                            }
                            download_context.set_final_location(default_path);
                        }
                    }
                }
            }
        }

        #[unsafe(method(URLSession:downloadTask:didWriteData:totalBytesWritten:totalBytesExpectedToWrite:))]
        fn URLSession_downloadTask_didWriteData_totalBytesWritten_totalBytesExpectedToWrite(
            &self,
            _session: &NSURLSession,
            download_task: &NSURLSessionDownloadTask,
            _bytes_written: i64,
            total_bytes_written: i64,
            total_bytes_expected_to_write: i64,
        ) {
            let ivars = self.ivars();
            let task_id = unsafe { download_task.taskIdentifier() } as usize;

            if let Ok(contexts) = ivars.task_contexts.lock() {
                if let Some(shared_context) = contexts.get(&task_id) {
                    // Update progress tracking
                    if total_bytes_expected_to_write > 0 {
                        shared_context
                            .set_total_bytes_expected(total_bytes_expected_to_write as u64);
                    }

                    // Set current progress (this will trigger callbacks)
                    let previous_bytes = shared_context
                        .bytes_downloaded
                        .load(std::sync::atomic::Ordering::Acquire);
                    let additional = (total_bytes_written as u64).saturating_sub(previous_bytes);
                    if additional > 0 {
                        shared_context.update_progress(additional);
                    }
                }
            }
        }
    }
);

impl BackgroundSessionDelegate {
    /// Create a new background session delegate
    pub fn new() -> Retained<Self> {
        let delegate = Self::alloc().set_ivars(BackgroundSessionDelegateIvars {
            task_contexts: Mutex::new(HashMap::new()),
            completion_handlers: Mutex::new(HashMap::new()),
        });

        // Initialize the NSObject
        unsafe { msg_send![super(delegate), init] }
    }

    /// Register a task context for a specific task
    pub fn register_task(&self, task_id: usize, context: Arc<TaskSharedContext>) {
        if let Ok(mut contexts) = self.ivars().task_contexts.lock() {
            contexts.insert(task_id, context);
        }
    }

    /// Register a background completion handler for a session
    pub fn register_background_completion_handler(
        &self,
        session_identifier: String,
        completion_handler: Retained<BackgroundCompletionHandler>,
    ) {
        if let Ok(mut handlers) = self.ivars().completion_handlers.lock() {
            handlers.insert(session_identifier, completion_handler);
        }
    }
}
