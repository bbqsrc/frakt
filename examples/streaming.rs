//! Streaming response example

use rsurlsession::Client;
use std::time::Duration;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating NSURLSession client...");

    // Create a client with a 30 second timeout
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("rsurlsession/0.1.0")
        .build()?;

    println!("Making streaming GET request...");

    // Make a request and get a streaming response
    let response = client
        .get("https://httpbin.org/bytes/8192") // Download 8KB
        .header("Accept", "application/octet-stream")
        .send()
        .await?;

    println!("Response status: {}", response.status());
    println!("Response headers: {:?}", response.headers());

    // Create a streaming reader
    let mut stream = response.stream();
    let mut total_bytes = 0;
    let mut buffer = [0u8; 1024]; // Read in 1KB chunks

    println!("Reading stream in chunks...");

    loop {
        let bytes_read = stream.read(&mut buffer).await?;
        if bytes_read == 0 {
            break; // EOF
        }

        total_bytes += bytes_read;
        println!("Read {} bytes (total: {})", bytes_read, total_bytes);
    }

    println!("Streaming complete! Total bytes read: {}", total_bytes);

    Ok(())
}
