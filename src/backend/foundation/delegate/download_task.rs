//! Download task delegate implementation

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{
    NSCopying, NSError, NSObject, NSObjectProtocol, NSURL, NSURLAuthenticationChallenge,
    NSURLAuthenticationMethodServerTrust, NSURLCredential, NSURLSession,
    NSURLSessionAuthChallengeDisposition, NSURLSessionDelegate, NSURLSessionDownloadDelegate,
    NSURLSessionDownloadTask, NSURLSessionTask, NSURLSessionTaskDelegate,
};

use super::TaskSharedContext;

/// Instance variables for the DownloadTaskDelegate
pub struct DownloadTaskDelegateIvars {
    /// Map of task identifiers to their shared contexts
    /// This allows one delegate to handle multiple tasks
    pub task_contexts: Mutex<HashMap<usize, Arc<TaskSharedContext>>>,
}

define_class!(
    /// NSURLSessionDownloadDelegate implementation
    #[unsafe(super = NSObject)]
    #[name = "fraktDownloadTaskDelegate"]
    #[ivars = DownloadTaskDelegateIvars]
    pub struct DownloadTaskDelegate;

    unsafe impl NSObjectProtocol for DownloadTaskDelegate {}

    unsafe impl NSURLSessionDelegate for DownloadTaskDelegate {}

    unsafe impl NSURLSessionTaskDelegate for DownloadTaskDelegate {
        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn URLSession_task_didCompleteWithError(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionTask,
            error: Option<&NSError>,
        ) {
            let ivars = self.ivars();
            let task_id = task.taskIdentifier() as usize;

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

        #[unsafe(method(URLSession:task:didReceiveChallenge:completionHandler:))]
        fn URLSession_task_didReceiveChallenge_completionHandler(
            &self,
            _session: &NSURLSession,
            _task: &NSURLSessionTask,
            challenge: &NSURLAuthenticationChallenge,
            completion_handler: &DynBlock<
                dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential),
            >,
        ) {
            unsafe {
                let protection_space = challenge.protectionSpace();
                let auth_method = protection_space.authenticationMethod();

                // Check if this is a server trust challenge
                if auth_method.isEqualToString(&NSURLAuthenticationMethodServerTrust) {
                    // Use default handling which respects the session configuration
                    completion_handler.call((
                        NSURLSessionAuthChallengeDisposition::PerformDefaultHandling,
                        std::ptr::null_mut(),
                    ));
                } else {
                    // For other types of challenges (HTTP auth, etc.), use default handling
                    completion_handler.call((
                        NSURLSessionAuthChallengeDisposition::PerformDefaultHandling,
                        std::ptr::null_mut(),
                    ));
                }
            }
        }
    }

    unsafe impl NSURLSessionDownloadDelegate for DownloadTaskDelegate {
        #[unsafe(method(URLSession:downloadTask:didFinishDownloadingToURL:))]
        fn URLSession_downloadTask_didFinishDownloadingToURL(
            &self,
            _session: &NSURLSession,
            download_task: &NSURLSessionDownloadTask,
            location: &NSURL,
        ) {
            let ivars = self.ivars();
            let task_id = download_task.taskIdentifier() as usize;

            if let Ok(contexts) = ivars.task_contexts.lock() {
                if let Some(shared_context) = contexts.get(&task_id) {
                    // Store the download location in shared context
                    if let Some(download_context) = shared_context.download_context.as_ref() {
                        download_context.set_download_location(location.copy());
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
            let task_id = download_task.taskIdentifier() as usize;

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

impl DownloadTaskDelegate {
    /// Create a new download task delegate that can handle multiple tasks
    pub fn new() -> Retained<Self> {
        let delegate = Self::alloc().set_ivars(DownloadTaskDelegateIvars {
            task_contexts: Mutex::new(HashMap::new()),
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
}
