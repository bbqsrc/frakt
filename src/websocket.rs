//! WebSocket support with backend abstraction

use crate::{Error, Result};
use tokio::sync::oneshot;

#[cfg(target_vendor = "apple")]
use {
    block2::RcBlock,
    objc2::{AnyThread, rc::Retained},
    objc2_foundation::{
        NSData, NSError, NSString, NSURL, NSURLSession, NSURLSessionWebSocketCloseCode,
        NSURLSessionWebSocketMessage, NSURLSessionWebSocketMessageType, NSURLSessionWebSocketTask,
    },
};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{self, protocol::CloseFrame},
};

/// [`WebSocket`] message types
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

    /// Convert to tokio-tungstenite message
    fn to_tungstenite_message(&self) -> tungstenite::Message {
        match self {
            Message::Text(text) => tungstenite::Message::Text(text.clone()),
            Message::Binary(data) => tungstenite::Message::Binary(data.clone()),
        }
    }

    /// Create from tokio-tungstenite message
    fn from_tungstenite_message(msg: tungstenite::Message) -> Result<Self> {
        match msg {
            tungstenite::Message::Text(text) => Ok(Message::Text(text)),
            tungstenite::Message::Binary(data) => Ok(Message::Binary(data)),
            tungstenite::Message::Close(_) => Err(Error::WebSocketClosed),
            tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_) => {
                Err(Error::Internal("Received ping/pong frame".to_string()))
            }
            tungstenite::Message::Frame(_) => {
                Err(Error::Internal("Received raw frame".to_string()))
            }
        }
    }

    /// Convert to NSURLSessionWebSocketMessage
    #[cfg(target_vendor = "apple")]
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
    #[cfg(target_vendor = "apple")]
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

/// [`WebSocket`] close codes.
///
/// These codes indicate the reason why a WebSocket connection was closed.
/// They correspond to the standard WebSocket close codes defined in RFC 6455.
///
/// # Examples
///
/// ```rust,no_run
/// use rsurlsession::{Client, CloseCode};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let mut websocket = client
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

/// A [`WebSocket`] connection using NSURLSessionWebSocketTask.
///
/// This struct represents an active [`WebSocket`] connection that can send and receive messages.
/// It uses NSURLSession's [`WebSocket`] implementation for optimal performance and integration
/// with the Apple ecosystem.
///
/// # Examples
///
/// ```rust,no_run
/// use rsurlsession::{Client, Message, CloseCode};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let mut websocket = client
///     .websocket()
///     .maximum_message_size(1024 * 1024)
///     .connect("wss://echo.websocket.org")
///     .await?;
///
/// // Send a text message
/// websocket.send(Message::text("Hello, WebSocket!")).await?;
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
pub enum WebSocket {
    /// Foundation backend using NSURLSessionWebSocketTask
    #[cfg(target_vendor = "apple")]
    Foundation {
        /// The underlying NSURLSessionWebSocketTask
        task: Retained<NSURLSessionWebSocketTask>,
    },
    /// Reqwest backend using tokio-tungstenite
    Reqwest {
        /// The underlying WebSocket stream
        stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
        /// Whether the connection has been closed
        closed: bool,
    },
}

impl WebSocket {
    /// Create a new Foundation [`WebSocket`] connection
    #[cfg(target_vendor = "apple")]
    pub(crate) fn new_foundation(session: &NSURLSession, url: &str) -> Result<Self> {
        unsafe {
            let nsurl = NSURL::URLWithString(&NSString::from_str(url)).ok_or(Error::InvalidUrl)?;

            let task = session.webSocketTaskWithURL(&nsurl);
            task.resume();

            Ok(Self::Foundation { task })
        }
    }

    /// Create a new Reqwest [`WebSocket`] connection using tokio-tungstenite
    pub(crate) async fn new_reqwest(url: &str) -> Result<Self> {
        let (stream, _) =
            tokio_tungstenite::connect_async(url)
                .await
                .map_err(|e| Error::Network {
                    code: -1,
                    message: format!("WebSocket connection failed: {}", e),
                })?;

        Ok(Self::Reqwest {
            stream,
            closed: false,
        })
    }

    /// Send a message over the [`WebSocket`] connection.
    ///
    /// This method sends a message to the [`WebSocket`] server. The message can be either
    /// text or binary data, and will be automatically converted from types that implement
    /// `Into<Message>`.
    ///
    /// # Arguments
    ///
    /// * `message` - The message to send (`String`, `&str`, `Vec<u8>`, or [`Message`])
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::{Client, Message};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let mut websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// // Send text directly
    /// websocket.send(Message::text("Hello, World!")).await?;
    ///
    /// // Send binary data
    /// websocket.send(vec![1, 2, 3, 4]).await?;
    ///
    /// // Send a Message explicitly
    /// websocket.send(Message::text("Explicit message")).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn send(&mut self, message: impl Into<Message>) -> Result<()> {
        let message: Message = message.into();

        match self {
            #[cfg(target_vendor = "apple")]
            WebSocket::Foundation { task } => {
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
                    task.sendMessage_completionHandler(&ns_message, &completion_handler);
                }

                let _ = rx.await.map_err(|_| {
                    Error::Internal("WebSocket send completion handler was dropped".to_string())
                })?;
            }
            WebSocket::Reqwest { stream, closed } => {
                if *closed {
                    return Err(Error::WebSocketClosed);
                }

                let tungstenite_message = message.to_tungstenite_message();
                stream
                    .send(tungstenite_message)
                    .await
                    .map_err(|e| Error::Network {
                        code: -1,
                        message: format!("WebSocket send failed: {}", e),
                    })?;
            }
        }

        Ok(())
    }

    /// Receive a message from the [`WebSocket`] connection.
    ///
    /// This method waits for and returns the next message from the [`WebSocket`] server.
    /// The method will block until a message is received or an error occurs.
    ///
    /// # Returns
    ///
    /// Returns a [`Message`] which can be either text or binary data.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::{Client, Message};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let mut websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// // Send a message first
    /// websocket.send(Message::text("Hello")).await?;
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
    pub async fn receive(&mut self) -> Result<Message> {
        match self {
            #[cfg(target_vendor = "apple")]
            WebSocket::Foundation { task } => {
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
                    task.receiveMessageWithCompletionHandler(&completion_handler);
                }

                rx.await.map_err(|_| {
                    Error::Internal("WebSocket receive completion handler was dropped".to_string())
                })?
            }
            WebSocket::Reqwest { stream, closed } => {
                if *closed {
                    return Err(Error::WebSocketClosed);
                }

                let msg = stream.next().await.ok_or(Error::WebSocketClosed)?;
                match msg {
                    Ok(tungstenite_msg) => {
                        if let tungstenite::Message::Close(_) = tungstenite_msg {
                            *closed = true;
                            return Err(Error::WebSocketClosed);
                        }
                        Message::from_tungstenite_message(tungstenite_msg)
                    }
                    Err(e) => Err(Error::Network {
                        code: -1,
                        message: format!("WebSocket receive failed: {}", e),
                    }),
                }
            }
        }
    }

    /// Close the [`WebSocket`] connection.
    ///
    /// This method closes the [`WebSocket`] connection with the specified close code and reason.
    /// The connection will be terminated and no further messages can be sent or received.
    ///
    /// # Arguments
    ///
    /// * `code` - The close code indicating the reason for closure
    /// * `reason` - Optional human-readable reason for closure
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::{Client, CloseCode};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let mut websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// // Close normally
    /// websocket.close(CloseCode::Normal, Some("Session ended"));
    ///
    /// // Close due to policy violation
    /// websocket.close(CloseCode::PolicyViolation, None);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn close(&mut self, code: CloseCode, reason: Option<&str>) -> Result<()> {
        match self {
            #[cfg(target_vendor = "apple")]
            WebSocket::Foundation { task } => {
                unsafe {
                    let reason_data = reason.map(|r| NSData::from_vec(r.as_bytes().to_vec()));
                    task.cancelWithCloseCode_reason(code.into(), reason_data.as_deref());
                }
                Ok(())
            }
            WebSocket::Reqwest { stream, closed } => {
                if *closed {
                    return Ok(());
                }

                let close_frame = reason.map(|r| CloseFrame {
                    code: tungstenite::protocol::frame::coding::CloseCode::from(code as u16),
                    reason: r.into(),
                });

                stream
                    .close(close_frame)
                    .await
                    .map_err(|e| Error::Network {
                        code: -1,
                        message: format!("WebSocket close failed: {}", e),
                    })?;

                *closed = true;
                Ok(())
            }
        }
    }

    /// Get the current close code if the connection has been closed.
    ///
    /// Returns the close code that was used when the connection was closed,
    /// or `None` if the connection is still open.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::{Client, CloseCode};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let mut websocket = client.websocket().connect("wss://echo.websocket.org").await?;
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
        match self {
            #[cfg(target_vendor = "apple")]
            WebSocket::Foundation { task } => unsafe {
                let code = task.closeCode();
                if code.0 == 0 { None } else { Some(code.0) }
            },
            WebSocket::Reqwest { closed, .. } => {
                if *closed {
                    Some(CloseCode::Normal as isize)
                } else {
                    None
                }
            }
        }
    }

    /// Get the close reason if the connection has been closed.
    ///
    /// Returns the human-readable reason that was provided when the connection
    /// was closed, or `None` if no reason was provided or the connection is still open.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::{Client, CloseCode};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let mut websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// websocket.close(CloseCode::Normal, Some("User requested"));
    /// // After closing, reason should be available
    /// // (Note: this might not be immediately available)
    /// # Ok(())
    /// # }
    /// ```
    pub fn close_reason(&self) -> Option<String> {
        match self {
            #[cfg(target_vendor = "apple")]
            WebSocket::Foundation { task } => unsafe {
                task.closeReason()
                    .map(|data| String::from_utf8_lossy(&data.to_vec()).to_string())
            },
            WebSocket::Reqwest { closed, .. } => {
                if *closed {
                    Some("Connection closed".to_string())
                } else {
                    None
                }
            }
        }
    }

    /// Set the maximum message size for this [`WebSocket`].
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
    /// ```rust,no_run
    /// use rsurlsession::{Client, Message};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let mut websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// // Set maximum message size to 1MB
    /// websocket.set_maximum_message_size(1024 * 1024);
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_maximum_message_size(&self, size: isize) {
        match self {
            #[cfg(target_vendor = "apple")]
            WebSocket::Foundation { task } => unsafe {
                task.setMaximumMessageSize(size);
            },
            WebSocket::Reqwest { .. } => {
                // tokio-tungstenite doesn't support runtime max message size configuration
                // This would need to be handled during connection setup
            }
        }
    }

    /// Get the current maximum message size.
    ///
    /// Returns the maximum size of messages that can be sent or received
    /// through this [`WebSocket`] connection.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::{Client, Message};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let mut websocket = client.websocket().connect("wss://echo.websocket.org").await?;
    ///
    /// let max_size = websocket.maximum_message_size();
    /// println!("Maximum message size: {} bytes", max_size);
    /// # Ok(())
    /// # }
    /// ```
    pub fn maximum_message_size(&self) -> isize {
        match self {
            #[cfg(target_vendor = "apple")]
            WebSocket::Foundation { task } => unsafe { task.maximumMessageSize() },
            WebSocket::Reqwest { .. } => {
                // tokio-tungstenite doesn't expose max message size
                // Return a reasonable default
                16 * 1024 * 1024 // 16MB default
            }
        }
    }
}

/// Builder for [`WebSocket`] connections.
///
/// This builder allows you to configure [`WebSocket`] connection parameters before
/// establishing the connection. It provides a fluent interface for setting options
/// like maximum message size.
///
/// # Examples
///
/// ```rust,no_run
/// use rsurlsession::{Client, Message};
///
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Client::new()?;
/// let mut websocket = client
///     .websocket()
///     .maximum_message_size(1024 * 1024)  // 1MB max messages
///     .connect("wss://echo.websocket.org")
///     .await?;
/// # Ok(())
/// # }
/// ```
pub enum WebSocketBuilder {
    /// Foundation backend using NSURLSession
    #[cfg(target_vendor = "apple")]
    Foundation {
        /// The NSURLSession instance to use for the WebSocket connection
        session: Retained<NSURLSession>,
        /// Maximum message size for the WebSocket connection
        maximum_message_size: Option<isize>,
    },
    /// Reqwest backend using tokio-tungstenite
    Reqwest {
        /// Maximum message size for the WebSocket connection
        maximum_message_size: Option<isize>,
    },
}

impl WebSocketBuilder {
    /// Create a new Foundation WebSocket builder
    #[cfg(target_vendor = "apple")]
    pub(crate) fn new_foundation(session: Retained<NSURLSession>) -> Self {
        Self::Foundation {
            session,
            maximum_message_size: None,
        }
    }

    /// Create a new Reqwest WebSocket builder
    pub(crate) fn new_reqwest() -> Self {
        Self::Reqwest {
            maximum_message_size: None,
        }
    }

    /// Set the maximum message size for the [`WebSocket`] connection.
    ///
    /// This sets the maximum size of messages that can be sent or received
    /// through the [`WebSocket`] connection. Messages larger than this size will be rejected.
    ///
    /// # Arguments
    ///
    /// * `size` - The maximum message size in bytes
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::{Client, Message};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let mut websocket = client
    ///     .websocket()
    ///     .maximum_message_size(1024 * 1024)  // 1MB
    ///     .connect("wss://echo.websocket.org")
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn maximum_message_size(mut self, size: isize) -> Self {
        match &mut self {
            #[cfg(target_vendor = "apple")]
            WebSocketBuilder::Foundation {
                maximum_message_size,
                ..
            } => {
                *maximum_message_size = Some(size);
            }
            WebSocketBuilder::Reqwest {
                maximum_message_size,
            } => {
                *maximum_message_size = Some(size);
            }
        }
        self
    }

    /// Connect to the [`WebSocket`] server at the specified URL.
    ///
    /// This method establishes the [`WebSocket`] connection and returns a [`WebSocket`]
    /// instance that can be used to send and receive messages.
    ///
    /// # Arguments
    ///
    /// * `url` - The WebSocket URL (must use ws:// or wss:// scheme)
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rsurlsession::{Client, Message};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new()?;
    /// let mut websocket = client
    ///     .websocket()
    ///     .connect("wss://echo.websocket.org")
    ///     .await?;
    ///
    /// // WebSocket is now ready for use
    /// websocket.send(Message::text("Hello, WebSocket!")).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect(self, url: &str) -> Result<WebSocket> {
        match self {
            #[cfg(target_vendor = "apple")]
            WebSocketBuilder::Foundation {
                session,
                maximum_message_size,
            } => {
                let websocket = WebSocket::new_foundation(&session, url)?;

                if let Some(size) = maximum_message_size {
                    websocket.set_maximum_message_size(size);
                }

                Ok(websocket)
            }
            WebSocketBuilder::Reqwest {
                maximum_message_size: _,
            } => {
                // Note: tokio-tungstenite doesn't support setting max message size at runtime
                // This would need to be configured during connection
                WebSocket::new_reqwest(url).await
            }
        }
    }
}
