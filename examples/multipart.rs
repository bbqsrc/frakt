//! Multipart form data example

use frakt::{Body, Client, MultipartPart};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating NSURLSession client...");

    // Create a client with a 30 second timeout
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("frakt/0.1.0")
        .build()?;

    println!("Creating multipart form data...");

    // Create multipart form data
    let parts = vec![
        MultipartPart::text("field1", "value1"),
        MultipartPart::text("field2", "value with spaces"),
        MultipartPart::file(
            "file_field",
            b"This is file content".to_vec(),
            "test.txt",
            Some("text/plain".to_string()),
        ),
    ];

    let multipart_body = Body::multipart(parts);

    println!("Sending multipart POST request to httpbin.org...");

    // Send multipart POST request
    let response = client
        .post("https://httpbin.org/post")
        .body(multipart_body)
        .send()
        .await?;

    println!("Response status: {}", response.status());
    println!("Response headers: {:?}", response.headers());

    // Get the response body
    let body = response.text().await?;
    println!("Response body:\n{}", body);

    Ok(())
}
