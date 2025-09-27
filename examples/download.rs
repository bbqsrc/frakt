//! Simple file download example

use rsurlsession::Client;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating NSURLSession client...");

    // Create a client with a 30 second timeout
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("rsurlsession/0.1.0")
        .build()?;

    println!("Making GET request to httpbin.org...");

    // Make a simple GET request
    let response = client
        .get("https://httpbin.org/json")
        .header("Accept", "application/json")
        .send()
        .await?;

    println!("Response status: {}", response.status());
    println!("Response headers: {:?}", response.headers());

    // Get the response body
    let body = response.bytes().await?;
    println!("Downloaded {} bytes", body.len());

    // Convert to text and print first 200 characters
    let text = String::from_utf8_lossy(&body);
    let preview = if text.len() > 200 {
        &text[..200]
    } else {
        &text
    };
    println!("Response preview:\n{}", preview);

    println!("\n--- Testing file download ---");

    // Download a small file
    let response = client.get("https://httpbin.org/robots.txt").send().await?;

    println!("File download status: {}", response.status());

    let file_content = response.text().await?;
    println!("Downloaded file content:\n{}", file_content);

    Ok(())
}
