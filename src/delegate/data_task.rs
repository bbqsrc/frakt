//! Data task delegate implementation

use std::sync::Arc;

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{
    NSCopying, NSData, NSError, NSObject, NSObjectProtocol, NSURLAuthenticationChallenge,
    NSURLAuthenticationMethodServerTrust, NSURLCredential, NSURLResponse, NSURLSession,
    NSURLSessionAuthChallengeDisposition, NSURLSessionDataDelegate, NSURLSessionDataTask,
    NSURLSessionDelegate, NSURLSessionResponseDisposition, NSURLSessionTask,
    NSURLSessionTaskDelegate,
};

use super::TaskSharedContext;

use std::collections::HashMap;
use std::sync::Mutex;

/// Instance variables for the DataTaskDelegate
pub struct DataTaskDelegateIvars {
    /// Map of task identifiers to their shared contexts
    /// This allows one delegate to handle multiple tasks
    pub task_contexts: Mutex<HashMap<usize, Arc<TaskSharedContext>>>,
}

define_class!(
    /// NSURLSessionDataDelegate implementation
    #[unsafe(super = NSObject)]
    #[name = "RsUrlSessionDataTaskDelegate"]
    #[ivars = DataTaskDelegateIvars]
    pub struct DataTaskDelegate;

    unsafe impl NSObjectProtocol for DataTaskDelegate {}

    unsafe impl NSURLSessionDelegate for DataTaskDelegate {}

    unsafe impl NSURLSessionTaskDelegate for DataTaskDelegate {
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

        #[unsafe(method(URLSession:task:didReceiveChallenge:completionHandler:))]
        fn URLSession_task_didReceiveChallenge_completionHandler(
            &self,
            session: &NSURLSession,
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
                    // Check if we should ignore certificate errors from session config
                    let config = session.configuration();

                    // For now, we'll use the default handling which respects the session configuration
                    // In the future, this could be expanded to allow custom certificate validation
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

    unsafe impl NSURLSessionDataDelegate for DataTaskDelegate {
        #[unsafe(method(URLSession:dataTask:didReceiveResponse:completionHandler:))]
        fn URLSession_dataTask_didReceiveResponse_completionHandler(
            &self,
            _session: &NSURLSession,
            data_task: &NSURLSessionDataTask,
            response: &NSURLResponse,
            completion_handler: &DynBlock<dyn Fn(NSURLSessionResponseDisposition)>,
        ) {
            let ivars = self.ivars();
            let task_id = unsafe { data_task.taskIdentifier() } as usize;

            if let Ok(contexts) = ivars.task_contexts.lock() {
                if let Some(shared_context) = contexts.get(&task_id) {
                    // Store the response in shared context
                    shared_context
                        .response
                        .store(Some(Arc::new(response.copy())));

                    // Set expected content length for progress tracking
                    let expected_length = unsafe { response.expectedContentLength() };
                    if expected_length > 0 {
                        shared_context.set_total_bytes_expected(expected_length as u64);
                    }
                }
            }

            // Allow the response to continue
            completion_handler.call((NSURLSessionResponseDisposition::Allow,));
        }

        #[unsafe(method(URLSession:dataTask:didReceiveData:))]
        fn URLSession_dataTask_didReceiveData(
            &self,
            _session: &NSURLSession,
            data_task: &NSURLSessionDataTask,
            data: &NSData,
        ) {
            let ivars = self.ivars();
            let task_id = unsafe { data_task.taskIdentifier() } as usize;

            if let Ok(contexts) = ivars.task_contexts.lock() {
                if let Some(shared_context) = contexts.get(&task_id) {
                    // Convert NSData to bytes and append to buffer
                    // NSData implements Deref<Target=[u8]>
                    let bytes = data.to_vec();

                    // We need to handle this synchronously since we're not in a tokio context
                    // Instead of async append_data, we'll access the buffer directly
                    if let Ok(mut buffer) = shared_context.response_buffer.try_lock() {
                        let max_size = shared_context
                            .max_response_buffer_size
                            .load(std::sync::atomic::Ordering::Acquire);
                        if buffer.len() as u64 + bytes.len() as u64 <= max_size {
                            buffer.extend_from_slice(&bytes);

                            // Update progress tracking
                            shared_context.update_progress(bytes.len() as u64);
                        }
                        // If buffer would be too large, we silently ignore (could improve this)
                    }
                }
            }
        }
    }
);

impl DataTaskDelegate {
    /// Create a new data task delegate that can handle multiple tasks
    pub fn new() -> Retained<Self> {
        let delegate = Self::alloc().set_ivars(DataTaskDelegateIvars {
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
