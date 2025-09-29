//! BITS (Background Intelligent Transfer Service) implementation for Windows
//!
//! BITS provides intelligent background downloads that survive reboots,
//! network interruptions, and user logoff/logon.

use crate::{Error, Result};
use std::path::PathBuf;
use url::Url;
use windows::core::GUID;
use windows::{
    Win32::{
        Networking::BackgroundIntelligentTransferService::{
            BG_JOB_PRIORITY, BG_JOB_TYPE, BackgroundCopyManager, IBackgroundCopyJob,
            IBackgroundCopyManager,
        },
        System::Com::{
            CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        },
    },
    core::{HSTRING, Interface},
};

/// Send-safe wrapper for IBackgroundCopyJob
struct SendableBitsJob(*mut std::ffi::c_void);

unsafe impl Send for SendableBitsJob {}

impl SendableBitsJob {
    fn new(job: IBackgroundCopyJob) -> Self {
        Self(job.as_raw() as *mut std::ffi::c_void)
    }

    fn to_interface(&self) -> Result<IBackgroundCopyJob> {
        unsafe {
            let unknown = windows::core::IUnknown::from_raw(self.0);
            unknown.cast().map_err(|e| {
                Error::Internal(format!("Failed to cast to IBackgroundCopyJob: {}", e))
            })
        }
    }
}

/// Send-safe wrapper for IBackgroundCopyManager
struct SendableBitsManager(*mut std::ffi::c_void);

unsafe impl Send for SendableBitsManager {}

impl SendableBitsManager {
    fn new(manager: IBackgroundCopyManager) -> Self {
        Self(manager.as_raw() as *mut std::ffi::c_void)
    }

    fn to_interface(&self) -> Result<IBackgroundCopyManager> {
        unsafe {
            let unknown = windows::core::IUnknown::from_raw(self.0);
            unknown.cast().map_err(|e| {
                Error::Internal(format!("Failed to cast to IBackgroundCopyManager: {}", e))
            })
        }
    }
}

/// BITS download manager for Windows background downloads
pub struct BitsDownloadManager {
    /// COM interface to BITS manager
    manager: IBackgroundCopyManager,
}

impl BitsDownloadManager {
    /// Create a new BITS download manager
    pub fn new() -> Result<Self> {
        unsafe {
            // Initialize COM with multithreaded apartment
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr.is_err() {
                return Err(Error::Internal(format!(
                    "Failed to initialize COM: {:?}",
                    hr
                )));
            }

            // Create BITS manager instance
            let manager: IBackgroundCopyManager =
                CoCreateInstance(&BackgroundCopyManager, None, CLSCTX_LOCAL_SERVER).map_err(
                    |e| Error::Internal(format!("Failed to create BITS manager: {}", e)),
                )?;

            Ok(Self { manager })
        }
    }

    /// Start a background download using BITS
    pub async fn start_background_download(
        &self,
        url: Url,
        file_path: PathBuf,
        session_identifier: Option<String>,
        progress_callback: Option<Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>>,
    ) -> Result<crate::client::download::DownloadResponse> {
        let job_name = session_identifier.unwrap_or_else(|| {
            format!(
                "frakt-download-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            )
        });

        let sendable_manager = SendableBitsManager::new(self.manager.clone());
        let url_str = url.to_string();
        let file_path_str = file_path.to_string_lossy().to_string();

        // Create and start the BITS job in spawn_blocking
        let sendable_job = tokio::task::spawn_blocking(move || {
            // Initialize COM for this thread
            unsafe {
                let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
                if hr.is_err() {
                    return Err(Error::Internal(format!(
                        "Failed to initialize COM in worker thread: {:?}",
                        hr
                    )));
                }

                let manager = sendable_manager.to_interface()?;

                // Create download job
                let mut job: Option<IBackgroundCopyJob> = None;
                let job_name_bstr = HSTRING::from(job_name.as_str());

                let mut job_id = GUID::zeroed();
                manager
                    .CreateJob(
                        &job_name_bstr,
                        BG_JOB_TYPE(1), // BG_JOB_TYPE_DOWNLOAD
                        &mut job_id,
                        &mut job,
                    )
                    .map_err(|e| Error::Internal(format!("Failed to create BITS job: {}", e)))?;

                let job = job.ok_or_else(|| {
                    Error::Internal("BITS job creation returned null".to_string())
                })?;

                // Add file to download
                let remote_url = HSTRING::from(url_str.as_str());
                let local_path = HSTRING::from(file_path_str.as_str());

                job.AddFile(&remote_url, &local_path).map_err(|e| {
                    Error::Internal(format!("Failed to add file to BITS job: {}", e))
                })?;

                // Set job priority to normal
                job.SetPriority(BG_JOB_PRIORITY(2)) // BG_JOB_PRIORITY_NORMAL
                    .map_err(|e| Error::Internal(format!("Failed to set job priority: {}", e)))?;

                // Resume the job to start download
                job.Resume()
                    .map_err(|e| Error::Internal(format!("Failed to start BITS job: {}", e)))?;

                Ok(SendableBitsJob::new(job))
            }
        })
        .await
        .map_err(|e| Error::Internal(format!("Failed to create BITS job: {}", e)))??;

        // Monitor progress if callback provided
        if let Some(callback) = progress_callback {
            let sendable_job_clone = SendableBitsJob(sendable_job.0);
            tokio::task::spawn_blocking(move || {
                let rt = tokio::runtime::Handle::current();
                rt.block_on(async move {
                    monitor_bits_progress(sendable_job_clone, callback).await;
                });
            });
        }

        // Wait for completion
        let bytes_downloaded = wait_for_bits_completion(sendable_job).await?;

        Ok(crate::client::download::DownloadResponse {
            file_path,
            bytes_downloaded,
        })
    }

    /// List all BITS jobs (for debugging/management)
    pub fn list_jobs(&self) -> Result<Vec<String>> {
        // TODO: Implement proper BITS job enumeration
        // For now, return empty list to avoid compilation complexity
        Ok(Vec::new())
    }
}

/// Wait for BITS job completion
async fn wait_for_bits_completion(sendable_job: SendableBitsJob) -> Result<u64> {
    let bytes_downloaded;

    loop {
        let (state, should_break, error_result) = tokio::task::spawn_blocking({
            let sendable_job = SendableBitsJob(sendable_job.0);
            move || {
                unsafe {
                    // Initialize COM for this thread
                    let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
                    if hr.is_err() {
                        return (
                            0,
                            true,
                            Some(Err(Error::Internal(format!(
                                "Failed to initialize COM: {:?}",
                                hr
                            )))),
                        );
                    }

                    let job = match sendable_job.to_interface() {
                        Ok(job) => job,
                        Err(e) => return (0, true, Some(Err(e))),
                    };

                    let state = match job.GetState() {
                        Ok(state) => state,
                        Err(e) => {
                            return (
                                0,
                                true,
                                Some(Err(Error::Internal(format!(
                                    "Failed to get job state: {}",
                                    e
                                )))),
                            );
                        }
                    };

                    match state.0 {
                        4 => {
                            // BG_JOB_STATE_TRANSFERRED
                            // Job completed successfully
                            if let Err(e) = job.Complete() {
                                return (
                                    0,
                                    true,
                                    Some(Err(Error::Internal(format!(
                                        "Failed to complete job: {}",
                                        e
                                    )))),
                                );
                            }

                            // Get final progress
                            let mut progress = std::mem::zeroed();
                            let bytes_downloaded = if job.GetProgress(&mut progress).is_ok() {
                                progress.BytesTransferred
                            } else {
                                0
                            };

                            (bytes_downloaded, true, None)
                        }
                        5 => {
                            // BG_JOB_STATE_ACKNOWLEDGED
                            // Job acknowledged as complete
                            (0, true, None)
                        }
                        6 => {
                            // BG_JOB_STATE_CANCELLED
                            (
                                0,
                                true,
                                Some(Err(Error::Internal("BITS job was cancelled".to_string()))),
                            )
                        }
                        7 => {
                            // BG_JOB_STATE_ERROR
                            // Get error information
                            let error_msg = "BITS job failed with error".to_string();
                            (
                                0,
                                true,
                                Some(Err(Error::Internal(format!(
                                    "BITS job failed: {}",
                                    error_msg
                                )))),
                            )
                        }
                        _ => {
                            // Job still in progress
                            (0, false, None)
                        }
                    }
                }
            }
        })
        .await
        .map_err(|e| Error::Internal(format!("Failed to check BITS job state: {}", e)))?;

        if let Some(result) = error_result {
            return result;
        }

        if should_break {
            bytes_downloaded = state;
            break;
        }

        // Job still in progress, wait a bit
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    Ok(bytes_downloaded)
}

/// Monitor BITS job progress and call callback
async fn monitor_bits_progress(
    sendable_job: SendableBitsJob,
    callback: Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>,
) {
    let callback = std::sync::Arc::new(callback);
    loop {
        let should_break = tokio::task::spawn_blocking({
            let sendable_job = SendableBitsJob(sendable_job.0);
            let callback = callback.clone();
            move || {
                unsafe {
                    // Initialize COM for this thread
                    let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
                    if hr.is_err() {
                        return true; // Exit on COM error
                    }

                    let job = match sendable_job.to_interface() {
                        Ok(job) => job,
                        Err(_) => return true, // Exit on interface error
                    };

                    if let Ok(state) = job.GetState() {
                        // Check if job is completed or failed
                        if state.0 >= 4 {
                            // BG_JOB_STATE_TRANSFERRED or higher
                            return true;
                        }

                        // Get current progress
                        let mut progress = std::mem::zeroed();
                        if job.GetProgress(&mut progress).is_ok() {
                            let total = if progress.BytesTotal == u64::MAX {
                                None // Unknown total size
                            } else {
                                Some(progress.BytesTotal)
                            };

                            callback(progress.BytesTransferred, total);
                        }
                    }

                    false // Continue monitoring
                }
            }
        })
        .await
        .unwrap_or(true);

        if should_break {
            break;
        }

        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
}
