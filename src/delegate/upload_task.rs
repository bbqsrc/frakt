//! Upload task delegate implementation

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use objc2::rc::Retained;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{
    NSCopying, NSError, NSObject, NSObjectProtocol, NSURLResponse, NSURLSession,
    NSURLSessionDataDelegate, NSURLSessionDelegate, NSURLSessionTask, NSURLSessionTaskDelegate,
};

use super::TaskSharedContext;

/// Instance variables for the UploadTaskDelegate
pub struct UploadTaskDelegateIvars {
    /// Map of task identifiers to their shared contexts
    pub task_contexts: Mutex<HashMap<usize, Arc<TaskSharedContext>>>,
}

define_class!(
    /// NSURLSessionUploadDelegate implementation
    #[unsafe(super = NSObject)]
    #[name = "fraktUploadTaskDelegate"]
    #[ivars = UploadTaskDelegateIvars]
    pub struct UploadTaskDelegate;

    unsafe impl NSObjectProtocol for UploadTaskDelegate {}

    unsafe impl NSURLSessionDelegate for UploadTaskDelegate {}

    unsafe impl NSURLSessionTaskDelegate for UploadTaskDelegate {
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

        #[unsafe(method(URLSession:task:didSendBodyData:totalBytesSent:totalBytesExpectedToSend:))]
        fn URLSession_task_didSendBodyData_totalBytesSent_totalBytesExpectedToSend(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionTask,
            _bytes_sent: i64,
            total_bytes_sent: i64,
            total_bytes_expected_to_send: i64,
        ) {
            let ivars = self.ivars();
            let task_id = unsafe { task.taskIdentifier() } as usize;

            if let Ok(contexts) = ivars.task_contexts.lock() {
                if let Some(shared_context) = contexts.get(&task_id) {
                    // Update progress tracking
                    if total_bytes_expected_to_send > 0 {
                        shared_context
                            .set_total_bytes_expected(total_bytes_expected_to_send as u64);
                    }

                    // Set current progress (this will trigger callbacks)
                    let previous_bytes = shared_context
                        .bytes_downloaded
                        .load(std::sync::atomic::Ordering::Acquire);
                    let additional = (total_bytes_sent as u64).saturating_sub(previous_bytes);
                    if additional > 0 {
                        shared_context.update_progress(additional);
                    }
                }
            }
        }
    }

    unsafe impl NSURLSessionDataDelegate for UploadTaskDelegate {
        #[unsafe(method(URLSession:dataTask:didReceiveResponse:completionHandler:))]
        fn URLSession_dataTask_didReceiveResponse_completionHandler(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionTask,
            response: &NSURLResponse,
            completion_handler: &block2::Block<
                dyn Fn(objc2_foundation::NSURLSessionResponseDisposition),
            >,
        ) {
            let ivars = self.ivars();
            let task_id = unsafe { task.taskIdentifier() } as usize;

            if let Ok(contexts) = ivars.task_contexts.lock() {
                if let Some(shared_context) = contexts.get(&task_id) {
                    // Store the response
                    shared_context
                        .response
                        .store(Some(Arc::new(response.copy())));
                }
            }

            // Continue with the upload
            completion_handler.call((objc2_foundation::NSURLSessionResponseDisposition::Allow,));
        }

        #[unsafe(method(URLSession:dataTask:didReceiveData:))]
        fn URLSession_dataTask_didReceiveData(
            &self,
            _session: &NSURLSession,
            task: &NSURLSessionTask,
            data: &objc2_foundation::NSData,
        ) {
            let ivars = self.ivars();
            let task_id = unsafe { task.taskIdentifier() } as usize;

            if let Ok(contexts) = ivars.task_contexts.lock() {
                if let Some(shared_context) = contexts.get(&task_id) {
                    // Convert NSData to bytes
                    let bytes = data.to_vec();

                    // We need to handle this synchronously since we're not in a tokio context
                    if let Ok(mut buffer) = shared_context.response_buffer.try_lock() {
                        let max_size = shared_context
                            .max_response_buffer_size
                            .load(std::sync::atomic::Ordering::Acquire);
                        if buffer.len() as u64 + bytes.len() as u64 <= max_size {
                            buffer.extend_from_slice(&bytes);
                        }
                    }
                }
            }
        }
    }
);

impl UploadTaskDelegate {
    /// Create a new upload task delegate
    pub fn new() -> Retained<Self> {
        let delegate = Self::alloc().set_ivars(UploadTaskDelegateIvars {
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
