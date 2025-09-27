use super::download::{DownloadFuture, DownloadResponse};
use crate::Result;

/// Builder for downloading files in background sessions
pub struct BackgroundDownloadBuilder {
    url: String,
    destination: Option<std::path::PathBuf>,
    progress_callback: Option<std::sync::Arc<crate::delegate::shared_context::ProgressCallback>>,
    headers: std::collections::HashMap<String, String>,
    session_identifier: Option<String>,
    background_completion_handler: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl BackgroundDownloadBuilder {
    pub(crate) fn new(url: String) -> Self {
        Self {
            url,
            destination: None,
            progress_callback: None,
            headers: std::collections::HashMap::new(),
            session_identifier: None,
            background_completion_handler: None,
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

    /// Set authentication for the background download
    pub fn auth(mut self, auth: crate::Auth) -> Self {
        self.headers
            .insert("Authorization".to_string(), auth.to_header_value());
        self
    }

    /// Set the background session identifier (required for background downloads)
    pub fn session_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.session_identifier = Some(identifier.into());
        self
    }

    /// Set a completion handler that's called when all background events finish
    pub fn on_background_completion<F>(mut self, handler: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.background_completion_handler = Some(std::sync::Arc::new(handler));
        self
    }

    /// Start the background download
    pub async fn send(self) -> Result<DownloadResponse> {
        use objc2::runtime::ProtocolObject;
        use objc2_foundation::{NSMutableURLRequest, NSString, NSURL};

        // Background downloads require a session identifier
        let session_identifier = self.session_identifier.ok_or_else(|| {
            crate::Error::Internal("Background downloads require a session identifier".to_string())
        })?;

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

        // Create background session configuration
        let session_config = crate::session::SessionConfigurationBuilder::new()
            .background_session(&session_identifier)
            .build()?;

        // Create background delegate and task context
        let background_delegate = crate::delegate::BackgroundSessionDelegate::new();
        let task_context = std::sync::Arc::new(crate::delegate::TaskSharedContext::with_download(
            self.destination.clone(),
            self.progress_callback,
        ));

        // Create background session with delegate
        let background_session = unsafe {
            objc2_foundation::NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &session_config,
                Some(ProtocolObject::from_ref(&*background_delegate)),
                None,
            )
        };

        // TODO: Register background completion handler if provided
        // Note: This is complex due to block2/objc2 type conversions
        // For now, focusing on basic background download functionality
        if let Some(_handler) = self.background_completion_handler {
            // Will implement completion handler registration in a future iteration
            eprintln!("Warning: Background completion handler registration not yet implemented");
        }

        // Create download task
        let download_task = unsafe { background_session.downloadTaskWithRequest(&nsrequest) };

        // Register the task context with the delegate
        let task_id = unsafe { download_task.taskIdentifier() } as usize;
        background_delegate.register_task(task_id, task_context.clone());

        // Create download future
        let download_future = DownloadFuture::new(download_task, task_context, self.destination);

        // Start the download
        unsafe {
            download_future.download_task.resume();
        }

        download_future.await
    }
}
