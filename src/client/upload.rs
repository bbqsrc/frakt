//! Upload task implementation using NSURLSessionUploadTask

use crate::Result;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSCopying, NSURLSession};
use std::sync::Arc;

/// Builder for uploading files using NSURLSessionUploadTask.
///
/// `UploadBuilder` provides a convenient interface for uploading files or data using
/// NSURLSessionUploadTask, which provides efficient streaming uploads with progress tracking.
/// You can upload either from a file on disk or from data in memory.
///
/// # Examples
///
/// Upload from a file:
/// ```rust
/// use rsurlsession::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let response = client
///     .upload("https://httpbin.org/post")
///     .from_file("./upload.txt")
///     .progress(|uploaded, total| {
///         if let Some(total) = total {
///             let percent = (uploaded as f64 / total as f64) * 100.0;
///             println!("Upload progress: {:.1}%", percent);
///         }
///     })
///     .send()
///     .await?;
/// # Ok(())
/// # }
/// ```
///
/// Upload from data:
/// ```rust
/// use rsurlsession::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let data = b"Hello, World!".to_vec();
/// let response = client
///     .upload("https://httpbin.org/post")
///     .from_data(data)
///     .header("Content-Type", "text/plain")
///     .send()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct UploadBuilder {
    url: String,
    session: Retained<NSURLSession>,
    file_path: Option<std::path::PathBuf>,
    data: Option<Vec<u8>>,
    progress_callback: Option<Arc<crate::delegate::shared_context::ProgressCallback>>,
    headers: std::collections::HashMap<String, String>,
}

impl UploadBuilder {
    pub(crate) fn new(url: String, session: Retained<NSURLSession>) -> Self {
        Self {
            url,
            session,
            file_path: None,
            data: None,
            progress_callback: None,
            headers: std::collections::HashMap::new(),
        }
    }

    /// Set the file to upload from disk.
    ///
    /// This method configures the upload to read data from a file on disk.
    /// The file will be streamed during upload, making this efficient for large files.
    /// If both `from_file()` and `from_data()` are called, the last one wins.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to upload
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .upload("https://httpbin.org/post")
    ///     .from_file("./document.pdf")
    ///     .header("Content-Type", "application/pdf")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_file<P: Into<std::path::PathBuf>>(mut self, path: P) -> Self {
        self.file_path = Some(path.into());
        self.data = None; // Clear data if file is set
        self
    }

    /// Set the data to upload from memory.
    ///
    /// This method configures the upload to send data that's already in memory.
    /// This is convenient for small uploads or when the data is generated programmatically.
    /// If both `from_file()` and `from_data()` are called, the last one wins.
    ///
    /// # Arguments
    ///
    /// * `data` - The data to upload as bytes
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let json_data = r#"{"message": "Hello, World!"}"#;
    /// let response = client
    ///     .upload("https://httpbin.org/post")
    ///     .from_data(json_data.as_bytes().to_vec())
    ///     .header("Content-Type", "application/json")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_data(mut self, data: Vec<u8>) -> Self {
        self.data = Some(data);
        self.file_path = None; // Clear file if data is set
        self
    }

    /// Set a progress callback to track upload progress.
    ///
    /// The callback will be called periodically during the upload with the number
    /// of bytes uploaded so far and the total size (if known).
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that takes (uploaded_bytes, total_bytes) parameters
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .upload("https://httpbin.org/post")
    ///     .from_file("./large-file.zip")
    ///     .progress(|uploaded, total| {
    ///         if let Some(total) = total {
    ///             let percent = (uploaded as f64 / total as f64) * 100.0;
    ///             println!("Upload progress: {:.1}%", percent);
    ///         } else {
    ///             println!("Uploaded: {} bytes", uploaded);
    ///         }
    ///     })
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(u64, Option<u64>) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
        self
    }

    /// Add a header to the upload request.
    ///
    /// This allows you to add custom headers to the upload request, such as
    /// content type, authentication headers, or custom API keys.
    ///
    /// # Arguments
    ///
    /// * `name` - The header name
    /// * `value` - The header value
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .upload("https://api.example.com/files")
    ///     .from_file("./image.jpg")
    ///     .header("Content-Type", "image/jpeg")
    ///     .header("X-Upload-Source", "mobile-app")
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set authentication for the upload request.
    ///
    /// This adds the appropriate `Authorization` header based on the authentication
    /// method provided.
    ///
    /// # Arguments
    ///
    /// * `auth` - The authentication method to use
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::{Client, Auth};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .upload("https://api.example.com/upload")
    ///     .from_file("./document.pdf")
    ///     .auth(Auth::bearer("your-token"))
    ///     .send()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn auth(mut self, auth: crate::Auth) -> Self {
        self.headers
            .insert("Authorization".to_string(), auth.to_header_value());
        self
    }

    /// Start the upload and return the response.
    ///
    /// This method initiates the upload using NSURLSessionUploadTask and returns
    /// a future that resolves when the upload is complete. The upload method (file vs data)
    /// depends on which method was called: `from_file()` or `from_data()`.
    ///
    /// # Returns
    ///
    /// Returns a [`Response`] containing the server's response to the upload.
    ///
    /// # Errors
    ///
    /// This method can fail with various errors including:
    /// - Network connectivity issues
    /// - Invalid URLs
    /// - File system errors when reading the source file
    /// - Authentication failures
    /// - No upload source specified (neither file nor data)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let response = client
    ///     .upload("https://httpbin.org/post")
    ///     .from_data(b"Hello, Server!".to_vec())
    ///     .header("Content-Type", "text/plain")
    ///     .send()
    ///     .await?;
    ///
    /// println!("Upload status: {}", response.status());
    /// let body = response.text().await?;
    /// println!("Server response: {}", body);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`Response`]: crate::Response
    pub async fn send(self) -> Result<crate::Response> {
        use objc2_foundation::{NSData, NSMutableURLRequest, NSString, NSURL};

        // Create NSURLRequest
        let nsurl = unsafe {
            NSURL::URLWithString(&NSString::from_str(&self.url)).ok_or(crate::Error::InvalidUrl)?
        };

        let nsrequest = unsafe {
            let req = NSMutableURLRequest::requestWithURL(&nsurl);
            req.setHTTPMethod(&NSString::from_str("POST"));

            // Set headers
            for (name, value) in &self.headers {
                req.setValue_forHTTPHeaderField(
                    Some(&NSString::from_str(value)),
                    &NSString::from_str(name),
                );
            }

            req
        };

        // Create upload delegate and task context
        let upload_delegate = crate::delegate::UploadTaskDelegate::new();
        let task_context = if let Some(callback) = self.progress_callback {
            Arc::new(crate::delegate::TaskSharedContext::with_progress_callback(
                callback,
            ))
        } else {
            Arc::new(crate::delegate::TaskSharedContext::new())
        };

        // Create upload session with delegate
        let upload_session = unsafe {
            NSURLSession::sessionWithConfiguration_delegate_delegateQueue(
                &self.session.configuration(),
                Some(ProtocolObject::from_ref(&*upload_delegate)),
                None,
            )
        };

        // Create upload task based on data source
        let upload_task = if let Some(file_path) = &self.file_path {
            // Upload from file
            let nsurl_file = unsafe {
                NSURL::fileURLWithPath(&NSString::from_str(&file_path.to_string_lossy()))
            };
            unsafe { upload_session.uploadTaskWithRequest_fromFile(&nsrequest, &nsurl_file) }
        } else if let Some(data) = &self.data {
            // Upload from data
            let nsdata =
                unsafe { NSData::dataWithBytes_length(data.as_ptr() as *const _, data.len()) };
            unsafe { upload_session.uploadTaskWithRequest_fromData(&nsrequest, &nsdata) }
        } else {
            return Err(crate::Error::Internal(
                "No file or data specified for upload".to_string(),
            ));
        };

        // Register the task context with the delegate
        let task_id = unsafe { upload_task.taskIdentifier() } as usize;
        upload_delegate.register_task(task_id, task_context.clone());

        // Create upload future
        let upload_future = UploadFuture::new(upload_task, task_context);

        // Start the upload
        unsafe {
            upload_future.upload_task.resume();
        }

        upload_future.await
    }
}

/// Future for handling upload completion
struct UploadFuture {
    upload_task: Retained<objc2_foundation::NSURLSessionUploadTask>,
    task_context: Arc<crate::delegate::TaskSharedContext>,
}

impl UploadFuture {
    fn new(
        upload_task: Retained<objc2_foundation::NSURLSessionUploadTask>,
        task_context: Arc<crate::delegate::TaskSharedContext>,
    ) -> Self {
        Self {
            upload_task,
            task_context,
        }
    }
}

impl std::future::Future for UploadFuture {
    type Output = Result<crate::Response>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.task_context.is_completed() {
            // Check for errors
            if let Some(error) = self.task_context.error.load_full() {
                return std::task::Poll::Ready(Err(crate::Error::from_ns_error(&*error)));
            }

            // Get the response
            if let Some(response) = self.task_context.response.load_full() {
                return std::task::Poll::Ready(Ok(crate::Response::new(
                    (**response).copy(),
                    self.task_context.clone(),
                )));
            }

            return std::task::Poll::Ready(Err(crate::Error::Internal(
                "No response received".to_string(),
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
