//! File download example

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

    println!("Starting file download...");

    // Download a file with progress tracking
    let download_response = client
        .download("https://httpbin.org/bytes/10240") // Download 10KB
        .to_file("downloaded_file.bin")
        .progress(|downloaded, total| match total {
            Some(total_bytes) => {
                let percentage = (downloaded as f64 / total_bytes as f64) * 100.0;
                println!(
                    "Download Progress: {}/{} bytes ({:.1}%)",
                    downloaded, total_bytes, percentage
                );
            }
            None => {
                println!("Download Progress: {} bytes downloaded", downloaded);
            }
        })
        .send()
        .await?;

    println!("Download completed!");
    println!("File saved to: {:?}", download_response.file_path);
    println!(
        "Total bytes downloaded: {}",
        download_response.bytes_downloaded
    );

    // Verify the file exists
    let metadata = std::fs::metadata(&download_response.file_path)?;
    println!("File size on disk: {} bytes", metadata.len());

    // Clean up
    std::fs::remove_file(&download_response.file_path)?;
    println!("Cleaned up downloaded file");

    Ok(())
}
