use crate::session::SessionConfigurationBuilder;
use crate::{Request, RequestBuilder, Result};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSURLSession;
use std::time::Duration;

/// Builder for downloading files
pub struct DownloadBuilder {
    url: String,
    session: Retained<NSURLSession>,
    destination: Option<std::path::PathBuf>,
    progress_callback: Option<std::sync::Arc<crate::delegate::shared_context::ProgressCallback>>,
    headers: std::collections::HashMap<String, String>,
}

impl DownloadBuilder {
    pub(crate) fn new(url: String, session: Retained<NSURLSession>) -> Self {
        Self {
            url,
            session,
            destination: None,
            progress_callback: None,
            headers: std::collections::HashMap::new(),
        }
    }

    /// Set the destination file path
    pub fn to_file<P: Into<std::path::PathBuf>>(mut self, path: P) -> Self {
        self.destination = Some(path.into());
        self
    }

    /// Set a progress callback
    pub fn progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(u64, Option<u64>) + Send + Sync + 'static,
    {
        self.progress_callback = Some(std::sync::Arc::new(callback));
        self
    }

    /// Add a header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set authentication for the download
    pub fn auth(mut self, auth: crate::Auth) -> Self {
        self.headers
            .insert("Authorization".to_string(), auth.to_header_value());
        self
    }

    /// Start the download
    pub async fn send(self) -> Result<DownloadResponse> {
        use crate::delegate::shared_context::ProgressCallback;
        use objc2::runtime::ProtocolObject;
        use objc2_foundation::{NSMutableURLRequest, NSString, NSURL};

        // Create NSURLRequest
        let nsurl = unsafe {
            NSURL::URLWithString(&NSString::from_str(&self.url)).ok_or(crate::Error::InvalidUrl)?
        };

        let nsrequest = unsafe {
            let req = NSMutableURLRequest::requestWithURL(&nsurl);

            // Set headers
            for (name, value) in &self.headers {
                req.setValue_forHTTPHeaderField(
                    Some(&NSString::from_str(value)),
                    &NSString::from_str(name),
                );
            }

            req
        };

        // Create download delegate and task context
        let download_delegate = crate::delegate::DownloadTaskDelegate::new();
        let task_context = std::sync::Arc::new(crate::delegate::TaskSharedContext::with_download(
            self.destination.clone(),
            self.progress_callback,
        ));

        // Create download session with delegate
        let download_session = unsafe {
            objc2_foundation::NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &self.session.configuration(),
                Some(ProtocolObject::from_ref(&*download_delegate)),
                None,
            )
        };

        // Create download task
        let download_task = unsafe { download_session.downloadTaskWithRequest(&nsrequest) };

        // Register the task context with the delegate
        let task_id = unsafe { download_task.taskIdentifier() } as usize;
        download_delegate.register_task(task_id, task_context.clone());

        // Create download future
        let download_future = DownloadFuture::new(download_task, task_context, self.destination);

        // Start the download
        unsafe {
            download_future.download_task.resume();
        }

        download_future.await
    }
}

/// Response from a download operation
pub struct DownloadResponse {
    /// The final file path where the download was saved
    pub file_path: std::path::PathBuf,
    /// Total bytes downloaded
    pub bytes_downloaded: u64,
}

/// Future for handling download completion
pub(super) struct DownloadFuture {
    pub(super) download_task: Retained<objc2_foundation::NSURLSessionDownloadTask>,
    pub(super) task_context: std::sync::Arc<crate::delegate::TaskSharedContext>,
    pub(super) destination: Option<std::path::PathBuf>,
}

impl DownloadFuture {
    pub(super) fn new(
        download_task: Retained<objc2_foundation::NSURLSessionDownloadTask>,
        task_context: std::sync::Arc<crate::delegate::TaskSharedContext>,
        destination: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            download_task,
            task_context,
            destination,
        }
    }
}

impl std::future::Future for DownloadFuture {
    type Output = Result<DownloadResponse>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.task_context.is_completed() {
            // Check for errors
            if let Some(error) = self.task_context.error.load_full() {
                return std::task::Poll::Ready(Err(crate::Error::from_ns_error(&*error)));
            }

            // Get the final file location (already copied by delegate)
            if let Some(download_context) = &self.task_context.download_context {
                if let Some(final_location) = download_context.final_location.load_full() {
                    let bytes_downloaded = self
                        .task_context
                        .bytes_downloaded
                        .load(std::sync::atomic::Ordering::Acquire);

                    return std::task::Poll::Ready(Ok(DownloadResponse {
                        file_path: (**final_location).to_path_buf(),
                        bytes_downloaded,
                    }));
                }
            }

            return std::task::Poll::Ready(Err(crate::Error::Internal(
                "No download location received".to_string(),
            )));
        }

        // Register waker
        let waker = cx.waker().clone();
        let task_context = self.task_context.clone();
        tokio::spawn(async move {
            task_context.waker.register(waker).await;
        });

        std::task::Poll::Pending
    }
}
