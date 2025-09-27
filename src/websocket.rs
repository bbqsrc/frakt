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

impl From<Vec<u8>> for Message {
    fn from(data: Vec<u8>) -> Self {
        Message::Binary(data)
    }
}

impl From<String> for Message {
    fn from(text: String) -> Self {
        Message::Text(text)
    }
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

/// WebSocket close codes.
///
/// These codes indicate the reason why a WebSocket connection was closed.
/// They correspond to the standard WebSocket close codes defined in RFC 6455.
///
/// # Examples
///
/// ```rust
/// use rsurlsession::{Client, CloseCode};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let websocket = client
///     .websocket()
///     .connect("wss://echo.websocket.org")
///     .await?;
///
/// // Close the connection normally
/// websocket.close(CloseCode::Normal, Some("Goodbye"));
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum CloseCode {
    /// Normal closure (1000).
    ///
    /// The connection was closed normally and the purpose for which it was opened has been fulfilled.
    Normal = 1000,
    /// Going away (1001).
    ///
    /// The endpoint is going away, such as a server going down or a browser navigating away.
    GoingAway = 1001,
    /// Protocol error (1002).
    ///
    /// The connection was terminated due to a protocol error.
    ProtocolError = 1002,
    /// Unsupported data (1003).
    ///
    /// The connection was terminated because the endpoint received data of a type it cannot accept.
    UnsupportedData = 1003,
    /// No status received (1005).
    ///
    /// No status code was actually present. This is a reserved value and MUST NOT be set as a status code.
    NoStatusReceived = 1005,
    /// Abnormal closure (1006).
    ///
    /// The connection was closed abnormally without a close frame being sent.
    AbnormalClosure = 1006,
    /// Invalid frame payload data (1007).
    ///
    /// The connection was terminated because the endpoint received data inconsistent with the type of the message.
    InvalidFramePayloadData = 1007,
    /// Policy violation (1008).
    ///
    /// The connection was terminated because the endpoint received a message that violates its policy.
    PolicyViolation = 1008,
    /// Message too big (1009).
    ///
    /// The connection was terminated because the endpoint received a message that is too big for it to process.
    MessageTooBig = 1009,
    /// Mandatory extension (1010).
    ///
    /// The connection was terminated because the client expected the server to negotiate one or more extensions.
    MandatoryExtension = 1010,
    /// Internal server error (1011).
    ///
    /// The connection was terminated because the server encountered an unexpected condition.
    InternalServerError = 1011,
    /// TLS handshake (1015).
    ///
    /// The connection was closed due to a failure to perform a TLS handshake.
    TlsHandshake = 1015,
}

impl From<CloseCode> for NSURLSessionWebSocketCloseCode {
    fn from(code: CloseCode) -> Self {
        NSURLSessionWebSocketCloseCode(code as isize)
    }
}

/// A WebSocket connection using NSURLSessionWebSocketTask.
///
/// This struct represents an active WebSocket connection that can send and receive messages.
/// It uses NSURLSession's WebSocket implementation for optimal performance and integration
/// with the Apple ecosystem.
///
/// # Examples
///
/// ```rust
/// use rsurlsession::{Client, Message, CloseCode};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let websocket = client
///     .websocket()
///     .maximum_message_size(1024 * 1024)
///     .connect("wss://echo.websocket.org")
///     .await?;
///
/// // Send a text message
/// websocket.send("Hello, WebSocket!").await?;
///
/// // Receive a message
/// let message = websocket.receive().await?;
/// match message {
///     Message::Text(text) => println!("Received text: {}", text),
///     Message::Binary(data) => println!("Received {} bytes", data.len()),
/// }
///
/// // Close the connection
/// websocket.close(CloseCode::Normal, Some("Goodbye"));
/// # Ok(())
/// # }
/// ```
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

    /// Send a message over the WebSocket connection.
    ///
    /// This method sends a message to the WebSocket server. The message can be either
    /// text or binary data, and will be automatically converted from types that implement
    /// `Into<Message>`.
    ///
    /// # Arguments
    ///
    /// * `message` - The message to send (String, &str, Vec<u8>, or Message)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::{Client, Message};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// // Send text directly
    /// websocket.send("Hello, World!").await?;
    ///
    /// // Send binary data
    /// websocket.send(vec![1, 2, 3, 4]).await?;
    ///
    /// // Send a Message explicitly
    /// websocket.send(Message::text("Explicit message")).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send(&self, message: impl Into<Message>) -> Result<()> {
        let message: Message = message.into();
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

    /// Receive a message from the WebSocket connection.
    ///
    /// This method waits for and returns the next message from the WebSocket server.
    /// The method will block until a message is received or an error occurs.
    ///
    /// # Returns
    ///
    /// Returns a [`Message`] which can be either text or binary data.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::{Client, Message};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// // Send a message first
    /// websocket.send("Hello").await?;
    ///
    /// // Receive the echo
    /// let message = websocket.receive().await?;
    /// match message {
    ///     Message::Text(text) => println!("Received text: {}", text),
    ///     Message::Binary(data) => println!("Received {} bytes of binary data", data.len()),
    /// }
    /// # Ok(())
    /// # }
    /// ```
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

    /// Close the WebSocket connection.
    ///
    /// This method closes the WebSocket connection with the specified close code and reason.
    /// The connection will be terminated and no further messages can be sent or received.
    ///
    /// # Arguments
    ///
    /// * `code` - The close code indicating the reason for closure
    /// * `reason` - Optional human-readable reason for closure
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::{Client, CloseCode};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// // Close normally
    /// websocket.close(CloseCode::Normal, Some("Session ended"));
    ///
    /// // Close due to policy violation
    /// websocket.close(CloseCode::PolicyViolation, None);
    /// # Ok(())
    /// # }
    /// ```
    pub fn close(&self, code: CloseCode, reason: Option<&str>) {
        unsafe {
            let reason_data = reason.map(|r| NSData::from_vec(r.as_bytes().to_vec()));
            self.task
                .cancelWithCloseCode_reason(code.into(), reason_data.as_deref());
        }
    }

    /// Get the current close code if the connection has been closed.
    ///
    /// Returns the close code that was used when the connection was closed,
    /// or `None` if the connection is still open.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::{Client, CloseCode};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// // Connection is open
    /// assert_eq!(websocket.close_code(), None);
    ///
    /// websocket.close(CloseCode::Normal, None);
    /// // After closing, code should be available
    /// // (Note: this might not be immediately available)
    /// # Ok(())
    /// # }
    /// ```
    pub fn close_code(&self) -> Option<isize> {
        unsafe {
            let code = self.task.closeCode();
            if code.0 == 0 { None } else { Some(code.0) }
        }
    }

    /// Get the close reason if the connection has been closed.
    ///
    /// Returns the human-readable reason that was provided when the connection
    /// was closed, or `None` if no reason was provided or the connection is still open.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::{Client, CloseCode};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// websocket.close(CloseCode::Normal, Some("User requested"));
    /// // After closing, reason should be available
    /// // (Note: this might not be immediately available)
    /// # Ok(())
    /// # }
    /// ```
    pub fn close_reason(&self) -> Option<String> {
        unsafe {
            self.task
                .closeReason()
                .map(|data| String::from_utf8_lossy(&data.to_vec()).to_string())
        }
    }

    /// Set the maximum message size for this WebSocket.
    ///
    /// This controls the maximum size of messages that can be sent or received.
    /// Messages larger than this size will be rejected.
    ///
    /// # Arguments
    ///
    /// * `size` - The maximum message size in bytes
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// // Set maximum message size to 1MB
    /// websocket.set_maximum_message_size(1024 * 1024);
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_maximum_message_size(&self, size: isize) {
        unsafe {
            self.task.setMaximumMessageSize(size);
        }
    }

    /// Get the current maximum message size.
    ///
    /// Returns the maximum size of messages that can be sent or received
    /// through this WebSocket connection.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// let max_size = websocket.maximum_message_size();
    /// println!("Maximum message size: {} bytes", max_size);
    /// # Ok(())
    /// # }
    /// ```
    pub fn maximum_message_size(&self) -> isize {
        unsafe { self.task.maximumMessageSize() }
    }
}

/// Builder for WebSocket connections.
///
/// This builder allows you to configure WebSocket connection parameters before
/// establishing the connection. It provides a fluent interface for setting options
/// like maximum message size.
///
/// # Examples
///
/// ```rust
/// use rsurlsession::Client;
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let websocket = client
///     .websocket()
///     .maximum_message_size(1024 * 1024)  // 1MB max messages
///     .connect("wss://echo.websocket.org")
///     .await?;
/// # Ok(())
/// # }
/// ```
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

    /// Set the maximum message size for the WebSocket connection.
    ///
    /// This sets the maximum size of messages that can be sent or received
    /// through the WebSocket connection. Messages larger than this size will be rejected.
    ///
    /// # Arguments
    ///
    /// * `size` - The maximum message size in bytes
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let websocket = client
    ///     .websocket()
    ///     .maximum_message_size(1024 * 1024)  // 1MB
    ///     .connect("wss://echo.websocket.org")
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn maximum_message_size(mut self, size: isize) -> Self {
        self.maximum_message_size = Some(size);
        self
    }

    /// Connect to the WebSocket server at the specified URL.
    ///
    /// This method establishes the WebSocket connection and returns a [`WebSocket`]
    /// instance that can be used to send and receive messages.
    ///
    /// # Arguments
    ///
    /// * `url` - The WebSocket URL (must use ws:// or wss:// scheme)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rsurlsession::Client;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let websocket = client
    ///     .websocket()
    ///     .connect("wss://echo.websocket.org")
    ///     .await?;
    ///
    /// // WebSocket is now ready for use
    /// websocket.send("Hello, WebSocket!").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(self, url: &str) -> Result<WebSocket> {
        let websocket = WebSocket::new(&self.session, url)?;

        if let Some(size) = self.maximum_message_size {
            websocket.set_maximum_message_size(size);
        }

        Ok(websocket)
    }
}
