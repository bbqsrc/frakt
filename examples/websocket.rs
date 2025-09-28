//! Example demonstrating WebSocket support

use rsurlsession::{Client, Message, Result};

#[tokio::main]
async fn main() -> Result<()> {
    println!("WebSocket Example - Connecting to echo server...");

    // Create a client
    let client = Client::builder()
        .user_agent("rsurlsession-websocket-example/1.0")
        .build()?;

    // Create WebSocket connection to echo server
    let mut websocket = client
        .websocket()
        .maximum_message_size(1024 * 1024) // 1MB max message size
        .connect("wss://echo.websocket.org")
        .await?;

    println!("Connected to WebSocket server!");

    // Send a text message
    println!("Sending text message...");
    let text_message = Message::text("Hello, WebSocket!");
    websocket.send(text_message).await?;

    // Receive the echo
    println!("Waiting for echo...");
    let received = websocket.receive().await?;
    match received {
        Message::Text(text) => println!("Received text: {}", text),
        Message::Binary(data) => println!("Received binary data: {} bytes", data.len()),
    }

    // Send a binary message
    println!("Sending binary message...");
    let binary_message = Message::binary(b"Binary data test".to_vec());
    websocket.send(binary_message).await?;

    // Receive the binary echo
    println!("Waiting for binary echo...");
    let received = websocket.receive().await?;
    match received {
        Message::Text(text) => println!("Received text: {}", text),
        Message::Binary(data) => {
            println!("Received binary data: {} bytes", data.len());
            println!("Content: {}", String::from_utf8_lossy(&data));
        }
    }

    // Test sending multiple messages
    println!("Sending multiple messages...");
    for i in 1..=3 {
        let msg = Message::text(format!("Message {}", i));
        websocket.send(msg).await?;

        let received = websocket.receive().await?;
        if let Message::Text(text) = received {
            println!("Echo {}: {}", i, text);
        }
    }

    // Get WebSocket info
    println!("Maximum message size: {}", websocket.maximum_message_size());

    // Close the connection
    println!("Closing WebSocket connection...");
    websocket
        .close(rsurlsession::CloseCode::Normal, Some("Example completed"))
        .await?;

    // Check close info
    if let Some(code) = websocket.close_code() {
        println!("WebSocket closed with code: {}", code);
    }
    if let Some(reason) = websocket.close_reason() {
        println!("Close reason: {}", reason);
    }

    println!("WebSocket example completed!");
    Ok(())
}
