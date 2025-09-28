//! Simple HTTP example to test basic functionality

use rsurlsession::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing basic HTTP functionality...");

    // Test client builder with basic configuration
    let client = Client::builder()
        .user_agent("rsurlsession-test/1.0")
        .header("X-Test-Header", "test-value")?
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    println!("✓ Client created successfully");

    // Test GET request
    let response = client
        .get("https://httpbin.org/get")?
        .header("Accept", "application/json")?
        .send()
        .await?;

    println!("✓ GET request completed");
    println!("  Status: {}", response.status());
    println!("  Headers: {} entries", response.headers().len());

    let body = response.text().await?;
    println!("  Body length: {} bytes", body.len());

    // Test POST request
    let response = client
        .post("https://httpbin.org/post")?
        .header("Content-Type", "application/json")?
        .text(r#"{"test": "data"}"#)
        .send()
        .await?;

    println!("✓ POST request completed");
    println!("  Status: {}", response.status());

    let body = response.text().await?;
    println!("  Response body length: {} bytes", body.len());

    println!("✅ All tests passed!");

    Ok(())
}
