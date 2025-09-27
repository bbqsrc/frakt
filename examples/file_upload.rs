//! File upload example

use rsurlsession::{Body, Client, MultipartPart};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating NSURLSession client...");

    // Create a client with a 30 second timeout
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("rsurlsession/0.1.0")
        .build()?;

    // First, create a test file to upload
    let test_file_content = "Hello, this is a test file content!\nLine 2 of the file.\n";
    tokio::fs::write("test_upload.txt", test_file_content).await?;

    println!("Testing simple file upload...");

    // Test 1: Simple file upload
    let body = Body::from_file("test_upload.txt", Some("text/plain".to_string())).await?;
    let response = client
        .post("https://httpbin.org/post")
        .body(body)
        .send()
        .await?;

    println!("Simple upload response status: {}", response.status());

    println!("Testing multipart file upload...");

    // Test 2: Multipart form with file upload
    let mut parts = vec![
        MultipartPart::text("description", "This is a test file upload"),
        MultipartPart::text("type", "text"),
    ];

    // Add file from filesystem
    let file_part =
        MultipartPart::from_file("file", "test_upload.txt", Some("text/plain".to_string())).await?;
    parts.push(file_part);

    let multipart_body = Body::multipart(parts);

    let response = client
        .post("https://httpbin.org/post")
        .body(multipart_body)
        .send()
        .await?;

    println!("Multipart upload response status: {}", response.status());

    // Parse and display the response
    let response_text = response.text().await?;
    println!("Response body:\n{}", response_text);

    // Clean up
    tokio::fs::remove_file("test_upload.txt").await?;

    Ok(())
}
