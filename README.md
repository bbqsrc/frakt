# rsurlsession

High-performance, async-first NSURLSession bindings for Rust. Native macOS/iOS HTTP client with zero-overhead Objective-C interop.

[![Crates.io](https://img.shields.io/crates/v/rsurlsession.svg)](https://crates.io/crates/rsurlsession)
[![Documentation](https://docs.rs/rsurlsession/badge.svg)](https://docs.rs/rsurlsession)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

## Features

- **Async/await with tokio** - Full async support, no blocking APIs
- **All HTTP methods** - GET, POST, PUT, DELETE, PATCH, HEAD
- **WebSocket support** - NSURLSessionWebSocketTask integration
- **File operations** - Uploads/downloads with progress tracking
- **Background sessions** - Downloads that survive app suspension (iOS)
- **Cookie management** - NSHTTPCookieStorage integration
- **Proxy configuration** - HTTP/HTTPS/SOCKS proxy support
- **Authentication** - Basic, Bearer, and Custom authentication
- **TLS/Certificate handling** - Server trust challenge support
- **Streaming responses** - AsyncRead for memory-efficient large downloads
- **Multipart form data** - File uploads with form fields
- **Zero-overhead** - Direct objc2 bindings with minimal abstractions

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
rsurlsession = "0.1"
```

## Platform Support

- **macOS** 10.15+ (Catalina)
- **iOS** 13.0+
- **Rust** 1.89+

Only Apple platforms are supported as this library provides direct NSURLSession bindings.

## Quick Start

```rust
use rsurlsession::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client
    let client = Client::builder()
        .user_agent("MyApp/1.0")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    // Make a GET request
    let response = client
        .get("https://httpbin.org/json")
        .header("Accept", "application/json")
        .send()
        .await?;

    println!("Status: {}", response.status());
    let json: serde_json::Value = response.json().await?;
    println!("Response: {}", json);

    Ok(())
}
```

## Examples

### Basic HTTP Requests

```rust
// GET request
let response = client.get("https://api.example.com/data").send().await?;

// POST with JSON
let response = client
    .post("https://api.example.com/users")
    .header("Content-Type", "application/json")
    .body(r#"{"name": "John", "email": "john@example.com"}"#)
    .send()
    .await?;
```

### Authentication

```rust
use rsurlsession::Auth;

// Basic authentication
let response = client
    .get("https://api.example.com/protected")
    .auth(Auth::Basic {
        username: "user".to_string(),
        password: "pass".to_string(),
    })
    .send()
    .await?;

// Bearer token
let response = client
    .get("https://api.example.com/protected")
    .auth(Auth::Bearer {
        token: "your-token".to_string(),
    })
    .send()
    .await?;
```

### File Downloads with Progress

```rust
let response = client
    .get("https://example.com/large-file.zip")
    .progress(|downloaded, total| {
        if let Some(total) = total {
            let percent = (downloaded as f64 / total as f64) * 100.0;
            println!("Progress: {:.1}%", percent);
        }
    })
    .send()
    .await?;

// Download directly to file
client
    .download("https://example.com/file.zip")
    .to_file("./downloads/file.zip")
    .progress(|downloaded, total| {
        println!("Downloaded: {} / {:?} bytes", downloaded, total);
    })
    .send()
    .await?;
```

### WebSocket

```rust
use rsurlsession::{Message, CloseCode};

let websocket = client
    .websocket()
    .maximum_message_size(1024 * 1024)
    .connect("wss://echo.websocket.org")
    .await?;

// Send message
websocket.send(Message::text("Hello, WebSocket!")).await?;

// Receive message
let message = websocket.receive().await?;
match message {
    Message::Text(text) => println!("Received: {}", text),
    Message::Binary(data) => println!("Received {} bytes", data.len()),
}

// Close connection
websocket.close(CloseCode::Normal, Some("Goodbye"));
```

### Cookies

```rust
let client = Client::builder()
    .use_cookies(true)
    .build()?;

// Cookies are automatically managed
let response = client.get("https://httpbin.org/cookies/set/session/abc123").send().await?;
let response = client.get("https://httpbin.org/cookies").send().await?; // Cookie sent automatically
```

### Proxy Configuration

```rust
let client = Client::builder()
    .http_proxy("proxy.example.com", 8080)
    .proxy_auth("username", "password")
    .build()?;
```

### Streaming Large Responses

```rust
use tokio::io::AsyncReadExt;

let response = client.get("https://example.com/large-file").send().await?;
let mut stream = response.stream();
let mut buffer = [0u8; 8192];

while let bytes_read = stream.read(&mut buffer).await? {
    if bytes_read == 0 { break; }
    // Process chunk
    process_chunk(&buffer[..bytes_read]);
}
```

## Available Examples

Run examples with `cargo run --example <name>`:

- **`auth`** - Authentication methods (Basic, Bearer, Custom)
- **`cookies`** - Cookie management and automatic handling
- **`download`** - Basic file downloads
- **`file_download`** - Direct-to-file downloads with progress
- **`file_upload`** - File uploads using Body::from_file()
- **`multipart`** - Multipart form data uploads
- **`progress`** - Progress tracking for downloads
- **`proxy`** - Proxy configuration (HTTP/HTTPS/SOCKS)
- **`streaming`** - Streaming large responses with AsyncRead
- **`upload_task`** - Upload tasks with progress tracking
- **`websocket`** - WebSocket client usage
- **`background_download`** - Background downloads (iOS)

## Architecture

This library provides direct Rust bindings to NSURLSession using [objc2](https://github.com/madsmtm/objc2). Key design principles:

- **Async-only**: Built for tokio, no blocking APIs
- **Zero-overhead**: Direct objc2 calls with minimal abstraction
- **Memory efficient**: Uses NSData references where possible
- **Type safe**: All unsafe objc2 calls are wrapped in safe APIs
- **Rusty**: Builder patterns and ergonomic error handling

## Error Handling

All NSURLSession errors are mapped to Rust's `Result` type:

```rust
use rsurlsession::Error;

match client.get("https://invalid-url").send().await {
    Ok(response) => println!("Success: {}", response.status()),
    Err(Error::InvalidUrl) => println!("Invalid URL"),
    Err(Error::Network(msg)) => println!("Network error: {}", msg),
    Err(Error::Timeout) => println!("Request timed out"),
    Err(e) => println!("Other error: {}", e),
}
```

## Performance

rsurlsession leverages NSURLSession's native performance optimizations:

- **HTTP/2 and HTTP/3** support (when available)
- **Connection pooling** and keep-alive
- **Automatic compression** (gzip, deflate, br)
- **Native TLS** implementation
- **Background processing** capabilities

## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.