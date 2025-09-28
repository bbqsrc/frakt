//! Reqwest WebSocket implementation using tokio-tungstenite

use crate::{
    Error, Result,
    websocket::{CloseCode, Message},
};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{self, protocol::CloseFrame},
};


/// Reqwest WebSocket implementation
pub struct ReqwestWebSocket {
    /// The underlying WebSocket stream
    pub stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    /// Flag to track if connection is closed
    pub closed: bool,
    /// Maximum message size for this WebSocket connection
    pub max_message_size: isize,
}

impl ReqwestWebSocket {
    /// Create a new Reqwest WebSocket connection
    pub async fn new(url: &str) -> Result<Self> {
        let (stream, _) =
            tokio_tungstenite::connect_async(url)
                .await
                .map_err(|e| Error::Network {
                    code: -1,
                    message: format!("WebSocket connection failed: {}", e),
                })?;

        Ok(Self {
            stream,
            closed: false,
            max_message_size: 1024 * 1024, // Default 1 MB
        })
    }

    /// Create a new Reqwest WebSocket connection with configuration
    pub async fn new_with_config(url: &str, max_message_size: Option<isize>) -> Result<Self> {
        use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

        let config = if let Some(size) = max_message_size {
            let mut config = WebSocketConfig::default();
            config.max_message_size = Some(size as usize);
            Some(config)
        } else {
            None
        };

        let (stream, _) = if let Some(config) = config {
            tokio_tungstenite::connect_async_with_config(url, Some(config), false)
                .await
                .map_err(|e| Error::Network {
                    code: -1,
                    message: format!("WebSocket connection failed: {}", e),
                })?
        } else {
            tokio_tungstenite::connect_async(url)
                .await
                .map_err(|e| Error::Network {
                    code: -1,
                    message: format!("WebSocket connection failed: {}", e),
                })?
        };

        Ok(Self {
            stream,
            closed: false,
            max_message_size: max_message_size.unwrap_or(1024 * 1024),
        })
    }

    /// Send a message
    pub async fn send(&mut self, message: Message) -> Result<()> {
        if self.closed {
            return Err(Error::Internal(
                "WebSocket connection is closed".to_string(),
            ));
        }

        let tungstenite_message = match message {
            Message::Text(text) => tungstenite::Message::Text(text.into()),
            Message::Binary(data) => tungstenite::Message::Binary(data.into()),
        };

        self.stream
            .send(tungstenite_message)
            .await
            .map_err(|e| Error::Network {
                code: -1,
                message: format!("Failed to send WebSocket message: {}", e),
            })
    }

    /// Receive a message
    pub async fn receive(&mut self) -> Result<Message> {
        if self.closed {
            return Err(Error::Internal(
                "WebSocket connection is closed".to_string(),
            ));
        }

        loop {
            match self.stream.next().await {
                Some(Ok(tungstenite::Message::Text(text))) => {
                    return Ok(Message::Text(text.to_string()));
                }
                Some(Ok(tungstenite::Message::Binary(data))) => {
                    return Ok(Message::Binary(data.to_vec()));
                }
                Some(Ok(tungstenite::Message::Close(_))) => {
                    self.closed = true;
                    return Err(Error::Internal(
                        "WebSocket connection closed by server".to_string(),
                    ));
                }
                Some(Ok(tungstenite::Message::Ping(_))) => {
                    // Pings are automatically handled by tungstenite
                    continue;
                }
                Some(Ok(tungstenite::Message::Pong(_))) => {
                    // Pongs are automatically handled by tungstenite
                    continue;
                }
                Some(Ok(tungstenite::Message::Frame(_))) => {
                    // Raw frames are not exposed at this level
                    continue;
                }
                Some(Err(e)) => {
                    self.closed = true;
                    return Err(Error::Network {
                        code: -1,
                        message: format!("WebSocket error: {}", e),
                    });
                }
                None => {
                    self.closed = true;
                    return Err(Error::Internal("WebSocket stream ended".to_string()));
                }
            }
        }
    }

    /// Close the WebSocket connection
    pub async fn close(&mut self, code: CloseCode, reason: Option<&str>) -> Result<()> {
        if self.closed {
            return Ok(());
        }

        let close_frame = reason.map(|r| CloseFrame {
            code: tungstenite::protocol::frame::coding::CloseCode::from(code as u16),
            reason: r.to_string().into(),
        });

        let close_message = tungstenite::Message::Close(close_frame);

        let result = self.stream.send(close_message).await;
        self.closed = true;

        result.map_err(|e| Error::Network {
            code: -1,
            message: format!("Failed to close WebSocket: {}", e),
        })
    }

    /// Get the current close code if the connection has been closed
    pub fn close_code(&self) -> Option<isize> {
        if self.closed {
            Some(CloseCode::Normal as isize)
        } else {
            None
        }
    }

    /// Get the close reason if the connection has been closed
    pub fn close_reason(&self) -> Option<String> {
        if self.closed {
            Some("Connection closed".to_string())
        } else {
            None
        }
    }

    /// Set the maximum message size for this WebSocket
    pub fn set_maximum_message_size(&mut self, size: isize) {
        self.max_message_size = size;
    }

    /// Get the current maximum message size
    pub fn maximum_message_size(&self) -> isize {
        self.max_message_size
    }
}

/// Reqwest WebSocket builder
pub struct ReqwestWebSocketBuilder {
    /// Maximum message size for the WebSocket connection
    pub max_message_size: Option<isize>,
}

impl ReqwestWebSocketBuilder {
    /// Create a new Reqwest WebSocket builder
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
    pub async fn connect(self, url: &str) -> Result<ReqwestWebSocket> {
        ReqwestWebSocket::new_with_config(url, self.max_message_size).await
    }
}
