//! Generic waker implementation

use std::sync::Arc;
use std::task::Waker;
use tokio::sync::Mutex;

/// Generic waker that can be used across different async contexts
pub struct GenericWaker {
    waker: Arc<Mutex<Option<Waker>>>,
}

impl GenericWaker {
    /// Create a new generic waker
    pub fn new() -> Self {
        Self {
            waker: Arc::new(Mutex::new(None)),
        }
    }

    /// Register a waker
    pub async fn register(&self, waker: Waker) {
        let mut guard = self.waker.lock().await;
        *guard = Some(waker);
    }

    /// Wake the registered waker
    pub fn wake(&self) {
        if let Ok(guard) = self.waker.try_lock() {
            if let Some(waker) = guard.as_ref() {
                waker.wake_by_ref();
            }
        }
    }
}