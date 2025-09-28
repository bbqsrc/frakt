//! BITS (Background Intelligent Transfer Service) implementation for Windows
//!
//! BITS provides intelligent background downloads that survive reboots,
//! network interruptions, and user logoff/logon.

use crate::{Error, Result};
use std::path::PathBuf;
use url::Url;
use windows::{
    core::HSTRING,
    Win32::{
        Networking::BackgroundIntelligentTransferService::{
            BG_JOB_PRIORITY, BG_JOB_STATE, BG_JOB_TYPE,
            IBackgroundCopyManager, IBackgroundCopyJob,
            BackgroundCopyManager,
        },
        System::Com::{
            CoInitialize, CoCreateInstance, CLSCTX_LOCAL_SERVER,
        },
    },
};
use windows_sys::core::BSTR;
use windows::core::GUID;

/// BITS download manager for Windows background downloads
pub struct BitsDownloadManager {
    /// COM interface to BITS manager
    manager: IBackgroundCopyManager,
}

impl BitsDownloadManager {
    /// Create a new BITS download manager
    pub fn new() -> Result<Self> {
        unsafe {
            // Initialize COM
            let hr = CoInitialize(None);
            if hr.is_err() {
                return Err(Error::Internal(format!("Failed to initialize COM: {:?}", hr)));
            }

            // Create BITS manager instance
            let manager: IBackgroundCopyManager = CoCreateInstance(
                &BackgroundCopyManager,
                None,
                CLSCTX_LOCAL_SERVER,
            )
            .map_err(|e| Error::Internal(format!("Failed to create BITS manager: {}", e)))?;

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

        unsafe {
            // Create download job
            let mut job: Option<IBackgroundCopyJob> = None;
            let job_name_bstr = HSTRING::from(job_name.as_str());

            let mut job_id = GUID::zeroed();
            self.manager
                .CreateJob(
                    &job_name_bstr,
                    BG_JOB_TYPE(1), // BG_JOB_TYPE_DOWNLOAD
                    &mut job_id,
                    &mut job,
                )
                .map_err(|e| Error::Internal(format!("Failed to create BITS job: {}", e)))?;

            let job = job.ok_or_else(|| Error::Internal("BITS job creation returned null".to_string()))?;

            // Add file to download
            let remote_url = HSTRING::from(url.as_str());
            let local_path = HSTRING::from(file_path.to_string_lossy().as_ref());

            job.AddFile(&remote_url, &local_path)
                .map_err(|e| Error::Internal(format!("Failed to add file to BITS job: {}", e)))?;

            // Set job priority to normal
            job.SetPriority(BG_JOB_PRIORITY(2)) // BG_JOB_PRIORITY_NORMAL
                .map_err(|e| Error::Internal(format!("Failed to set job priority: {}", e)))?;

            // Resume the job to start download
            job.Resume()
                .map_err(|e| Error::Internal(format!("Failed to start BITS job: {}", e)))?;

            // Monitor progress if callback provided
            if let Some(callback) = progress_callback {
                let job_clone = job.clone();
                tokio::spawn(async move {
                    monitor_bits_progress(job_clone, callback).await;
                });
            }

            // Wait for completion
            let bytes_downloaded = wait_for_bits_completion(job).await?;

            Ok(crate::client::download::DownloadResponse {
                file_path,
                bytes_downloaded,
            })
        }
    }

    /// List all BITS jobs (for debugging/management)
    pub fn list_jobs(&self) -> Result<Vec<String>> {
        unsafe {
            let jobs = self.manager
                .EnumJobs(0) // 0 = all jobs for current user
                .map_err(|e| Error::Internal(format!("Failed to enumerate BITS jobs: {}", e)))?;

            let jobs = jobs.map_err(|e| Error::Internal(format!("Failed to enumerate jobs: {}", e)))?;

            let mut job_names = Vec::new();
            let mut count = 0u32;
            jobs.GetCount(&mut count)
                .map_err(|e| Error::Internal(format!("Failed to get job count: {}", e)))?;

            for i in 0..count {
                let mut job = None;
                if jobs.Next(1, &mut job, std::ptr::null_mut()).is_ok() {
                    if let Some(job) = job {
                        let mut display_name = BSTR::default();
                        if job.GetDisplayName(&mut display_name).is_ok() {
                            // Convert BSTR to String safely
                            if !display_name.0.is_null() {
                                if let Ok(name) = unsafe { std::ffi::OsString::from_wide(std::slice::from_raw_parts(display_name.0, display_name.len() as usize)) }.into_string() {
                                    job_names.push(name);
                                }
                            }
                        }
                    }
                }
            }

            Ok(job_names)
        }
    }
}

/// Wait for BITS job completion
async fn wait_for_bits_completion(job: IBackgroundCopyJob) -> Result<u64> {
    let mut bytes_downloaded = 0u64;

    loop {
        unsafe {
            let state = job.GetState()
                .map_err(|e| Error::Internal(format!("Failed to get job state: {}", e)))?;

            match state.0 {
                4 => { // BG_JOB_STATE_TRANSFERRED
                    // Job completed successfully
                    job.Complete()
                        .map_err(|e| Error::Internal(format!("Failed to complete job: {}", e)))?;

                    // Get final progress
                    let mut progress = std::mem::zeroed();
                    if job.GetProgress(&mut progress).is_ok() {
                        bytes_downloaded = progress.BytesTransferred;
                    }

                    break;
                }
                5 => { // BG_JOB_STATE_ACKNOWLEDGED
                    // Job acknowledged as complete
                    break;
                }
                6 => { // BG_JOB_STATE_CANCELLED
                    return Err(Error::Internal("BITS job was cancelled".to_string()));
                }
                7 => { // BG_JOB_STATE_ERROR
                    // Get error information
                    if let Ok(error) = job.GetError() {
                        if let Ok(error_desc) = error.GetErrorDescription(0) {
                            // Convert PWSTR to String safely
                            let error_msg = unsafe {
                                windows::core::PWSTR(error_desc.0).to_hstring().unwrap_or_default().to_string()
                            };
                            return Err(Error::Internal(format!("BITS job failed: {}", error_msg)));
                        }
                    }
                    return Err(Error::Internal("BITS job failed with unknown error".to_string()));
                }
                _ => {
                    // Job still in progress, wait a bit
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
    }

    Ok(bytes_downloaded)
}

/// Monitor BITS job progress and call callback
async fn monitor_bits_progress(
    job: IBackgroundCopyJob,
    callback: Box<dyn Fn(u64, Option<u64>) + Send + Sync + 'static>,
) {
    loop {
        unsafe {
            if let Ok(state) = job.GetState() {
                // Check if job is completed or failed
                if state.0 >= 4 { // BG_JOB_STATE_TRANSFERRED or higher
                    break;
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

            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }
    }
}