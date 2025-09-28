//! Download progress tracking example

use frakt::Client;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating NSURLSession client...");

    // Create a client with a 30 second timeout
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("frakt/0.1.0")
        .build()?;

    println!("Making GET request with progress tracking...");

    // Make a request with progress tracking
    let response = client
        .get("https://httpbin.org/bytes/5242880")? // Download 5MB for more granular progress
        .header("Accept", "application/octet-stream")?
        .progress(|downloaded, total| match total {
            Some(total_bytes) => {
                let percentage = (downloaded as f64 / total_bytes as f64) * 100.0;
                println!(
                    "Progress: {}/{} bytes ({:.1}%)",
                    downloaded, total_bytes, percentage
                );
            }
            None => {
                println!("Progress: {} bytes downloaded", downloaded);
            }
        })
        .send()
        .await?;

    println!("Response status: {}", response.status());
    println!("Response headers: {:?}", response.headers());

    // Get the response body
    let body = response.bytes().await?;
    println!("Final download: {} bytes", body.len());

    Ok(())
}
