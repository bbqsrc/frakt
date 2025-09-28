//! Windows WebSocket implementation using MessageWebSocket

use crate::{
    Error, Result,
    websocket::{CloseCode, Message},
};
use std::sync::{Arc, atomic::{AtomicI32, Ordering}, RwLock};
use tokio::sync::mpsc;
use windows::{
    core::HSTRING,
    Foundation::{TypedEventHandler, Uri},
    Networking::Sockets::{
        MessageWebSocket, MessageWebSocketMessageReceivedEventArgs, SocketMessageType,
    },
    Storage::Streams::DataWriter,
};

/// Convert library Message to Windows socket message type and content
fn convert_message_for_sending(message: &Message) -> Result<(SocketMessageType, Vec<u8>)> {
    match message {
        Message::Text(text) => {
            Ok((SocketMessageType::Utf8, text.as_bytes().to_vec()))
        }
        Message::Binary(data) => {
            Ok((SocketMessageType::Binary, data.clone()))
        }
    }
}

/// Convert Windows message to library Message
fn convert_received_message(
    message_type: SocketMessageType,
    data: &[u8],
) -> Result<Message> {
    match message_type {
        SocketMessageType::Utf8 => {
            let text = String::from_utf8(data.to_vec())
                .map_err(|e| Error::Internal(format!("Invalid UTF-8 in text message: {}", e)))?;
            Ok(Message::Text(text))
        }
        SocketMessageType::Binary => {
            Ok(Message::Binary(data.to_vec()))
        }
        _ => Err(Error::Internal("Unknown message type".to_string())),
    }
}

/// Windows WebSocket implementation using MessageWebSocket
pub struct WindowsWebSocket {
    /// The underlying MessageWebSocket
    socket: MessageWebSocket,
    /// Data writer for sending messages
    writer: DataWriter,
    /// Receiver for incoming messages
    message_receiver: mpsc::Receiver<Result<Message>>,
    /// Close code
    close_code: Arc<AtomicI32>,
    /// Close reason
    close_reason: Arc<RwLock<Option<String>>>,
    /// Flag to track if connection is closed
    closed: bool,
}

impl WindowsWebSocket {
    /// Create a new Windows WebSocket connection
    pub async fn new(url: &str) -> Result<Self> {
        let socket = MessageWebSocket::new()
            .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to create MessageWebSocket"))?;

        // Configure the socket for UTF-8 messages by default
        let control = socket.Control()
            .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to get socket control"))?;
        control.SetMessageType(SocketMessageType::Utf8)
            .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to set message type"))?;

        // Parse and validate URL
        let uri = Uri::CreateUri(&HSTRING::from(url))
            .map_err(|_| Error::InvalidUrl)?;

        // Set up message receiver
        let (message_sender, message_receiver) = mpsc::channel(32);
        let close_code = Arc::new(AtomicI32::new(0));
        let close_reason = Arc::new(RwLock::new(None));

        // TODO: Implement Windows WebSocket message handling and connection
        // For now, return an error as this is a placeholder
        return Err(Error::Internal("Windows WebSocket implementation not yet complete".to_string()));

        // Create placeholder data writer - will be implemented properly later
        let writer = DataWriter::new()
            .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to create data writer"))?;

        Ok(Self {
            socket,
            writer,
            message_receiver,
            close_code,
            close_reason,
            closed: false,
        })
    }

    /// Send a message
    pub async fn send(&mut self, message: Message) -> Result<()> {
        if self.closed {
            return Err(Error::Internal("WebSocket connection is closed".to_string()));
        }

        let (message_type, data) = convert_message_for_sending(&message)?;

        // Set the message type
        self.socket.Control()
            .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to get socket control"))?
            .SetMessageType(message_type)
            .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to set message type"))?;

        // Write the data
        match message_type {
            SocketMessageType::Utf8 => {
                let text = String::from_utf8(data)
                    .map_err(|e| Error::Internal(format!("Invalid UTF-8 data: {}", e)))?;
                self.writer.WriteString(&HSTRING::from(text))
                    .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to write string"))?;
            }
            SocketMessageType::Binary => {
                self.writer.WriteBytes(&data)
                    .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to write bytes"))?;
            }
            _ => return Err(Error::Internal("Unsupported message type".to_string())),
        }

        // Store the data (send it)
        let store_operation = self.writer.StoreAsync()
            .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to initiate store operation"))?;

        // TODO: Properly await the store operation
        let _ = store_operation; // Suppress unused variable warning

        Ok(())
    }

    /// Receive a message
    pub async fn receive(&mut self) -> Result<Message> {
        if self.closed {
            return Err(Error::Internal("WebSocket connection is closed".to_string()));
        }

        self.message_receiver.recv().await
            .ok_or_else(|| Error::Internal("WebSocket message channel closed".to_string()))?
    }

    /// Close the WebSocket connection
    pub async fn close(&mut self, code: CloseCode, reason: Option<&str>) -> Result<()> {
        if self.closed {
            return Ok(());
        }

        // Store close information
        self.close_code.store(code as i32, Ordering::Relaxed);
        if let Some(reason_str) = reason {
            *self.close_reason.write().unwrap() = Some(reason_str.to_string());
        }

        // Close the socket - Windows MessageWebSocket.Close() takes no parameters
        self.socket.Close()
            .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to close WebSocket"))?;

        self.closed = true;
        Ok(())
    }

    /// Get the current close code if the connection has been closed
    pub fn close_code(&self) -> Option<isize> {
        let code = self.close_code.load(Ordering::Relaxed);
        if code == 0 { None } else { Some(code as isize) }
    }

    /// Get the close reason if the connection has been closed
    pub fn close_reason(&self) -> Option<String> {
        self.close_reason.read().unwrap().clone()
    }

    /// Set the maximum message size for this WebSocket
    pub fn set_maximum_message_size(&self, size: isize) {
        if let Ok(control) = self.socket.Control() {
            let _ = control.SetMaxMessageSize(size as u32);
        }
    }

    /// Get the current maximum message size
    pub fn maximum_message_size(&self) -> isize {
        self.socket.Control()
            .and_then(|control| control.MaxMessageSize())
            .unwrap_or(1024 * 1024) as isize
    }
}

/// Process a received message from Windows
fn process_received_message(
    args: &MessageWebSocketMessageReceivedEventArgs,
) -> Result<Message> {
    let message_type = args.MessageType()
        .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to get message type"))?;

    let reader = args.GetDataReader()
        .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to get data reader"))?;

    let length = reader.UnconsumedBufferLength()
        .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to get buffer length"))?;

    match message_type {
        SocketMessageType::Utf8 => {
            let text = reader.ReadString(length)
                .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to read string"))?;
            Ok(Message::Text(text.to_string()))
        }
        SocketMessageType::Binary => {
            let mut buffer = vec![0u8; length as usize];
            reader.ReadBytes(&mut buffer)
                .map_err(|e| super::error::map_windows_error_with_context(e, "Failed to read bytes"))?;
            Ok(Message::Binary(buffer))
        }
        _ => Err(Error::Internal("Unknown message type received".to_string())),
    }
}

/// Windows WebSocket builder
pub struct WindowsWebSocketBuilder {
    /// Maximum message size for the WebSocket connection
    max_message_size: Option<isize>,
}

impl WindowsWebSocketBuilder {
    /// Create a new Windows WebSocket builder
    pub fn new() -> Self {
        Self {
            max_message_size: None,
        }
    }

    /// Set the maximum message size for the WebSocket connection
    pub fn maximum_message_size(mut self, size: isize) -> Self {
        self.max_message_size = Some(size);
        self
    }

    /// Connect to a WebSocket URL
    pub async fn connect(self, url: &str) -> Result<WindowsWebSocket> {
        let websocket = WindowsWebSocket::new(url).await?;

        // Set maximum message size if specified
        if let Some(size) = self.max_message_size {
            websocket.set_maximum_message_size(size);
        }

        Ok(websocket)
    }
}