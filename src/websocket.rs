//! WebSocket support using NSURLSessionWebSocketTask

use crate::{Error, Result};
use block2::RcBlock;
use objc2::{AnyThread, rc::Retained};
use objc2_foundation::{
    NSData, NSError, NSString, NSURL, NSURLSession, NSURLSessionWebSocketCloseCode,
    NSURLSessionWebSocketMessage, NSURLSessionWebSocketMessageType, NSURLSessionWebSocketTask,
};
use tokio::sync::oneshot;

/// WebSocket message types
#[derive(Debug, Clone)]
pub enum Message {
    /// Text message
    Text(String),
    /// Binary data message
    Binary(Vec<u8>),
}

impl Message {
    /// Create a text message
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Create a binary message
    pub fn binary(data: impl Into<Vec<u8>>) -> Self {
        Self::Binary(data.into())
    }

    /// Convert to NSURLSessionWebSocketMessage
    fn to_ns_message(&self) -> Result<Retained<NSURLSessionWebSocketMessage>> {
        unsafe {
            match self {
                Message::Text(text) => {
                    let ns_string = NSString::from_str(text);
                    Ok(NSURLSessionWebSocketMessage::initWithString(
                        NSURLSessionWebSocketMessage::alloc(),
                        &ns_string,
                    ))
                }
                Message::Binary(data) => {
                    let ns_data = NSData::from_vec(data.clone());
                    Ok(NSURLSessionWebSocketMessage::initWithData(
                        NSURLSessionWebSocketMessage::alloc(),
                        &ns_data,
                    ))
                }
            }
        }
    }

    /// Create from NSURLSessionWebSocketMessage
    fn from_ns_message(ns_message: &NSURLSessionWebSocketMessage) -> Result<Self> {
        unsafe {
            let message_type = ns_message.r#type();

            if message_type == NSURLSessionWebSocketMessageType::String {
                if let Some(string) = ns_message.string() {
                    Ok(Message::Text(string.to_string()))
                } else {
                    Err(Error::Internal(
                        "WebSocket string message had no string content".to_string(),
                    ))
                }
            } else if message_type == NSURLSessionWebSocketMessageType::Data {
                if let Some(data) = ns_message.data() {
                    Ok(Message::Binary(data.to_vec()))
                } else {
                    Err(Error::Internal(
                        "WebSocket data message had no data content".to_string(),
                    ))
                }
            } else {
                Err(Error::Internal(format!(
                    "Unknown WebSocket message type: {:?}",
                    message_type
                )))
            }
        }
    }
}

/// WebSocket close codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum CloseCode {
    /// Normal closure
    Normal = 1000,
    /// Going away
    GoingAway = 1001,
    /// Protocol error
    ProtocolError = 1002,
    /// Unsupported data
    UnsupportedData = 1003,
    /// No status received
    NoStatusReceived = 1005,
    /// Abnormal closure
    AbnormalClosure = 1006,
    /// Invalid frame payload data
    InvalidFramePayloadData = 1007,
    /// Policy violation
    PolicyViolation = 1008,
    /// Message too big
    MessageTooBig = 1009,
    /// Mandatory extension
    MandatoryExtension = 1010,
    /// Internal server error
    InternalServerError = 1011,
    /// TLS handshake
    TlsHandshake = 1015,
}

impl From<CloseCode> for NSURLSessionWebSocketCloseCode {
    fn from(code: CloseCode) -> Self {
        NSURLSessionWebSocketCloseCode(code as isize)
    }
}

/// A WebSocket connection using NSURLSessionWebSocketTask
pub struct WebSocket {
    task: Retained<NSURLSessionWebSocketTask>,
}

impl WebSocket {
    /// Create a new WebSocket connection
    pub(crate) fn new(session: &NSURLSession, url: &str) -> Result<Self> {
        unsafe {
            let nsurl = NSURL::URLWithString(&NSString::from_str(url)).ok_or(Error::InvalidUrl)?;

            let task = session.webSocketTaskWithURL(&nsurl);
            task.resume();

            Ok(Self { task })
        }
    }

    /// Send a message
    pub async fn send(&self, message: Message) -> Result<()> {
        let ns_message = message.to_ns_message()?;

        let (tx, rx) = oneshot::channel();

        // Create a completion handler that can be called multiple times
        let completion_handler = {
            let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
            RcBlock::new(move |error: *mut NSError| {
                let result = if error.is_null() {
                    Ok(())
                } else {
                    unsafe {
                        let ns_error = &*error;
                        Err(Error::from_ns_error(ns_error))
                    }
                };
                // Only send once - ignore subsequent calls
                if let Ok(mut tx_guard) = tx.lock() {
                    if let Some(tx) = tx_guard.take() {
                        let _ = tx.send(result);
                    }
                }
            })
        };

        unsafe {
            self.task
                .sendMessage_completionHandler(&ns_message, &completion_handler);
        }

        rx.await.map_err(|_| {
            Error::Internal("WebSocket send completion handler was dropped".to_string())
        })?
    }

    /// Receive a message
    pub async fn receive(&self) -> Result<Message> {
        let (tx, rx) = oneshot::channel();

        // Create a completion handler that can be called multiple times
        let completion_handler = {
            let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
            RcBlock::new(
                move |message: *mut NSURLSessionWebSocketMessage, error: *mut NSError| {
                    let result = if error.is_null() && !message.is_null() {
                        unsafe {
                            let ns_message = &*message;
                            Message::from_ns_message(ns_message)
                        }
                    } else if !error.is_null() {
                        unsafe {
                            let ns_error = &*error;
                            Err(Error::from_ns_error(ns_error))
                        }
                    } else {
                        Err(Error::Internal(
                            "WebSocket receive got null message and null error".to_string(),
                        ))
                    };
                    // Only send once - ignore subsequent calls
                    if let Ok(mut tx_guard) = tx.lock() {
                        if let Some(tx) = tx_guard.take() {
                            let _ = tx.send(result);
                        }
                    }
                },
            )
        };

        unsafe {
            self.task
                .receiveMessageWithCompletionHandler(&completion_handler);
        }

        rx.await.map_err(|_| {
            Error::Internal("WebSocket receive completion handler was dropped".to_string())
        })?
    }

    /// Close the WebSocket connection
    pub fn close(&self, code: CloseCode, reason: Option<&str>) {
        unsafe {
            let reason_data = reason.map(|r| NSData::from_vec(r.as_bytes().to_vec()));
            self.task
                .cancelWithCloseCode_reason(code.into(), reason_data.as_deref());
        }
    }

    /// Get the current close code (if closed)
    pub fn close_code(&self) -> Option<isize> {
        unsafe {
            let code = self.task.closeCode();
            if code.0 == 0 { None } else { Some(code.0) }
        }
    }

    /// Get the close reason (if closed)
    pub fn close_reason(&self) -> Option<String> {
        unsafe {
            self.task
                .closeReason()
                .map(|data| String::from_utf8_lossy(&data.to_vec()).to_string())
        }
    }

    /// Set maximum message size
    pub fn set_maximum_message_size(&self, size: isize) {
        unsafe {
            self.task.setMaximumMessageSize(size);
        }
    }

    /// Get maximum message size
    pub fn maximum_message_size(&self) -> isize {
        unsafe { self.task.maximumMessageSize() }
    }
}

/// Builder for WebSocket connections
pub struct WebSocketBuilder {
    session: Retained<NSURLSession>,
    maximum_message_size: Option<isize>,
}

impl WebSocketBuilder {
    /// Create a new WebSocket builder
    pub(crate) fn new(session: Retained<NSURLSession>) -> Self {
        Self {
            session,
            maximum_message_size: None,
        }
    }

    /// Set maximum message size
    pub fn maximum_message_size(mut self, size: isize) -> Self {
        self.maximum_message_size = Some(size);
        self
    }

    /// Connect to the WebSocket URL
    pub async fn connect(self, url: &str) -> Result<WebSocket> {
        let websocket = WebSocket::new(&self.session, url)?;

        if let Some(size) = self.maximum_message_size {
            websocket.set_maximum_message_size(size);
        }

        Ok(websocket)
    }
}
